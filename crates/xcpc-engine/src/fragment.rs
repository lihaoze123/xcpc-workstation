//! Standalone, pure-memory Typst fragment renderer for untrusted plugin snippets.
//!
//! Strictly forbids disk reads (`FileError::AccessDenied` for all non-builtin virtual paths),
//! enforces source byte length, wall-clock compilation timeout, and rendered pixel dimensions,
//! and outputs either crisp vector SVG (`typst-svg`) or transparent PNG (`typst-render`).

use std::sync::{Arc, OnceLock, mpsc};
use std::time::Duration;

use typst::diag::{FileError, FileResult};
use typst::foundations::{Bytes, Datetime, Duration as TypstDuration};
use typst::syntax::{FileId, Source, VirtualRoot};
use typst::text::{Font, FontBook};
use typst::utils::LazyHash;
use typst::{Library, LibraryExt, World};
use typst_kit::fonts::FontStore;
use typst_layout::PagedDocument;

use crate::{FONTS_PATH, FontOptions, main_id, project_id, resolve_cjk_fonts, template};

/// Maximum input source byte length for a plugin Typst / math snippet (64 KiB).
pub const MAX_FRAGMENT_SOURCE_BYTES: usize = 64 * 1024;
/// Maximum wall-clock compilation timeout for a plugin Typst / math snippet (2000 ms).
pub const FRAGMENT_TIMEOUT_MS: u64 = 2_000;
/// Maximum pixel width or height for rendered PNG output.
pub const MAX_FRAGMENT_PIXEL_DIM: u32 = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FragmentFormat {
    #[default]
    Svg,
    Png,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FragmentTheme {
    #[default]
    Light,
    Dark,
}

#[derive(Debug, Clone)]
pub struct FragmentRenderOptions {
    pub format: FragmentFormat,
    pub scale: f32,
    pub width_pt: Option<f64>,
    pub theme: FragmentTheme,
}

impl Default for FragmentRenderOptions {
    fn default() -> Self {
        Self {
            format: FragmentFormat::Svg,
            scale: 2.0,
            width_pt: None,
            theme: FragmentTheme::Light,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RenderedFragment {
    pub svg: Option<String>,
    pub png_bytes: Option<Vec<u8>>,
    pub width: f64,
    pub height: f64,
}

struct SharedFragmentRuntime {
    library: Arc<LazyHash<Library>>,
    fonts: Arc<FontStore>,
    fonts_text: String,
}

fn shared_runtime() -> &'static SharedFragmentRuntime {
    static RT: OnceLock<SharedFragmentRuntime> = OnceLock::new();
    RT.get_or_init(|| {
        let opts = FontOptions::from_env();
        let mut store = FontStore::new();
        for dir in &opts.dirs {
            store.extend(typst_kit::fonts::scan(dir));
        }
        #[cfg(any(not(debug_assertions), feature = "bundled-fonts"))]
        store.extend(crate::bundled_fonts());
        if opts.system {
            #[cfg(windows)]
            crate::load_windows_cjk_fonts(&mut store);
            #[cfg(not(windows))]
            store.extend(typst_kit::fonts::system());
        }
        let (fonts_text, _) = resolve_cjk_fonts(store.book());
        SharedFragmentRuntime {
            library: Arc::new(LazyHash::new(Library::default())),
            fonts: Arc::new(store),
            fonts_text,
        }
    })
}

/// Strictly pure-memory Typst [`World`] that refuses all host/workspace disk reads and
/// only serves `/main.typ`, `/xcpc/fonts.typ`, `/xcpc/defaults.typ`, `/xcpc/markdown.typ`,
/// `/xcpc/mono.tmTheme`, and `/xcpc/mitex/*`.
struct MemoryFragmentWorld {
    library: Arc<LazyHash<Library>>,
    fonts: Arc<FontStore>,
    main: Source,
    fonts_source: Source,
}

fn allowed_builtin_template(id: FileId) -> Option<&'static str> {
    if !matches!(id.root(), VirtualRoot::Project) {
        return None;
    }
    let vpath = id.vpath().get_with_slash();
    let mapped = match vpath {
        "/templates/defaults.typ" => "/xcpc/defaults.typ",
        "/templates/markdown.typ" => "/xcpc/markdown.typ",
        "/templates/mono.tmTheme" => "/xcpc/mono.tmTheme",
        other => other,
    };
    if mapped == "/xcpc/defaults.typ"
        || mapped == "/xcpc/markdown.typ"
        || mapped == "/xcpc/mono.tmTheme"
        || mapped.starts_with("/xcpc/mitex/")
    {
        return template(project_id(mapped));
    }
    None
}

impl World for MemoryFragmentWorld {
    fn library(&self) -> &LazyHash<Library> {
        &self.library
    }

    fn book(&self) -> &LazyHash<FontBook> {
        self.fonts.book()
    }

    fn main(&self) -> FileId {
        self.main.id()
    }

    fn source(&self, id: FileId) -> FileResult<Source> {
        if id == self.main.id() {
            return Ok(self.main.clone());
        }
        if id == self.fonts_source.id()
            || (matches!(id.root(), VirtualRoot::Project)
                && id.vpath().get_with_slash() == "/templates/fonts.typ")
        {
            return Ok(self.fonts_source.clone());
        }
        if let Some(text) = allowed_builtin_template(id) {
            return Ok(Source::new(id, text.into()));
        }
        Err(FileError::AccessDenied)
    }

    fn file(&self, id: FileId) -> FileResult<Bytes> {
        if id == self.fonts_source.id()
            || (matches!(id.root(), VirtualRoot::Project)
                && id.vpath().get_with_slash() == "/templates/fonts.typ")
        {
            return Ok(Bytes::from_string(self.fonts_source.text().to_owned()));
        }
        if let Some(text) = allowed_builtin_template(id) {
            return Ok(Bytes::from_string(text));
        }
        Err(FileError::AccessDenied)
    }

    fn font(&self, index: usize) -> Option<Font> {
        self.fonts.font(index)
    }

    fn today(&self, _offset: Option<TypstDuration>) -> Option<Datetime> {
        None
    }
}

/// Renders an untrusted Typst snippet inside an isolated pure-memory `World` with strict
/// size, timeout, and pixel-dimension guardrails.
pub fn render_typst_fragment(
    source: &str,
    opts: &FragmentRenderOptions,
) -> Result<RenderedFragment, String> {
    if source.len() > MAX_FRAGMENT_SOURCE_BYTES {
        return Err(format!(
            "source exceeds maximum length of {MAX_FRAGMENT_SOURCE_BYTES} bytes"
        ));
    }
    if !opts.scale.is_finite() || opts.scale <= 0.0 {
        return Err("scale must be a positive finite number".into());
    }
    let scale = opts.scale.clamp(0.5, 4.0);
    let width_decl = match opts.width_pt {
        Some(w) if w.is_finite() && w > 0.0 => format!("{:.2}pt", w.clamp(40.0, 2000.0)),
        Some(_) => return Err("widthPt must be a positive finite number".into()),
        None => "auto".to_owned(),
    };
    let fg = match opts.theme {
        FragmentTheme::Light => "#09090B",
        FragmentTheme::Dark => "#FAFAFA",
    };

    let wrapped = format!(
        "#import \"/xcpc/fonts.typ\": cjk-song, lm-roman-10\n\
         #import \"/xcpc/markdown.typ\": m, dm\n\
         #set page(width: {width_decl}, height: auto, margin: 6pt, fill: none)\n\
         #set text(font: (lm-roman-10, cjk-song), size: 11pt, fill: rgb(\"{fg}\"))\n\
         #show math.equation: set text(font: \"Latin Modern Math\")\n\
         {source}\n"
    );

    let rt = shared_runtime();
    let world = MemoryFragmentWorld {
        library: Arc::clone(&rt.library),
        fonts: Arc::clone(&rt.fonts),
        main: Source::new(main_id(), wrapped),
        fonts_source: Source::new(project_id(FONTS_PATH), rt.fonts_text.clone()),
    };
    let format = opts.format;

    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("xcpc-fragment-render".into())
        .spawn(move || {
            let res = compile_and_export_fragment(&world, format, scale);
            let _ = tx.send(res);
        })
        .map_err(|e| format!("failed to spawn render worker: {e}"))?;

    match rx.recv_timeout(Duration::from_millis(FRAGMENT_TIMEOUT_MS)) {
        Ok(res) => res,
        Err(mpsc::RecvTimeoutError::Timeout) => Err(format!(
            "Typst fragment rendering exceeded {FRAGMENT_TIMEOUT_MS}ms timeout"
        )),
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            Err("Typst fragment render worker terminated unexpectedly".into())
        }
    }
}

fn compile_and_export_fragment(
    world: &MemoryFragmentWorld,
    format: FragmentFormat,
    scale: f32,
) -> Result<RenderedFragment, String> {
    let warned = typst::compile::<PagedDocument>(world);
    comemo::evict(2);
    let doc = warned.output.map_err(|errs| {
        errs.first()
            .map(|e| e.message.to_string())
            .unwrap_or_else(|| "Typst compilation failed".into())
    })?;
    let Some(page) = doc.pages().first() else {
        return Err("rendered document produced no pages".into());
    };
    let size = page.frame.size();
    let width_pt = size.x.to_pt().max(1.0);
    let height_pt = size.y.to_pt().max(1.0);

    match format {
        FragmentFormat::Svg => {
            let svg = typst_svg::svg(page, &typst_svg::SvgOptions::default());
            Ok(RenderedFragment {
                svg: Some(svg),
                png_bytes: None,
                width: width_pt,
                height: height_pt,
            })
        }
        FragmentFormat::Png => {
            let px_w = (width_pt * f64::from(scale)).ceil();
            let px_h = (height_pt * f64::from(scale)).ceil();
            if px_w > f64::from(MAX_FRAGMENT_PIXEL_DIM) || px_h > f64::from(MAX_FRAGMENT_PIXEL_DIM)
            {
                return Err(format!(
                    "rendered PNG dimensions ({px_w:.0}x{px_h:.0}) exceed limit ({MAX_FRAGMENT_PIXEL_DIM}x{MAX_FRAGMENT_PIXEL_DIM})"
                ));
            }
            let options = typst_render::RenderOptions {
                pixel_per_pt: f64::from(scale).into(),
                ..Default::default()
            };
            let pix = typst_render::render(page, &options);
            let w = f64::from(pix.width());
            let h = f64::from(pix.height());
            let png = pix
                .encode_png()
                .map_err(|e| format!("PNG encoding failed: {e}"))?;
            Ok(RenderedFragment {
                svg: None,
                png_bytes: Some(png),
                width: w,
                height: h,
            })
        }
    }
}

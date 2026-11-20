//! Native Typst typesetting for XCPC Workstation.
//!
//! An [`Engine`] owns one Typst [`World`]: the bundled `olymp-classic` template, a font
//! store and an optional project root for assets. The main source is replaced in place
//! ([`Engine::set_main`]), so repeated compilations reuse `comemo`'s memoized work.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use typst::diag::{FileError, FileResult, Severity, SourceDiagnostic};
use typst::foundations::{Bytes, Datetime, Duration as TypstDuration};
use typst::syntax::{FileId, RootedPath, Source, VirtualPath, VirtualRoot};
use typst::text::{Font, FontBook};
use typst::utils::LazyHash;
use typst::{Library, LibraryExt, World, WorldExt};
use typst_kit::fonts::FontStore;
use typst_layout::PagedDocument;

mod fingerprint;
mod overflow;

pub use typst_render::render as render_page_raw;

/// The bundled templates, addressable from Typst as `/xcpc/<name>`: `olymp-classic`,
/// what the Markdown converter emits calls (`markdown.typ`), MiTeX's scope for it, and
/// the editorial's code theme (`mono.tmTheme`).
const TEMPLATES: &[(&str, &str)] = &[
    (
        "/xcpc/olymp-classic.typ",
        include_str!("../templates/olymp-classic.typ"),
    ),
    (
        "/xcpc/markdown.typ",
        include_str!("../templates/markdown.typ"),
    ),
    (
        "/xcpc/mono.tmTheme",
        include_str!("../templates/mono.tmTheme"),
    ),
    (
        "/xcpc/mitex/mod.typ",
        include_str!("../templates/mitex/mod.typ"),
    ),
    (
        "/xcpc/mitex/prelude.typ",
        include_str!("../templates/mitex/prelude.typ"),
    ),
    (
        "/xcpc/mitex/latex/standard.typ",
        include_str!("../templates/mitex/latex/standard.typ"),
    ),
];

/// Path of the main source inside the virtual project.
const MAIN_PATH: &str = "/main.typ";

#[cfg(any(not(debug_assertions), feature = "bundled-fonts"))]
const BUNDLED_FONT_BYTES: &[&[u8]] = &[
    include_bytes!(concat!(env!("OUT_DIR"), "/fonts/lmroman10-regular.otf")),
    include_bytes!(concat!(env!("OUT_DIR"), "/fonts/lmroman10-bold.otf")),
    include_bytes!(concat!(env!("OUT_DIR"), "/fonts/lmroman10-italic.otf")),
    include_bytes!(concat!(env!("OUT_DIR"), "/fonts/lmroman10-bolditalic.otf")),
    include_bytes!(concat!(env!("OUT_DIR"), "/fonts/lmroman12-regular.otf")),
    include_bytes!(concat!(env!("OUT_DIR"), "/fonts/lmroman12-bold.otf")),
    include_bytes!(concat!(env!("OUT_DIR"), "/fonts/lmroman12-italic.otf")),
    include_bytes!(concat!(env!("OUT_DIR"), "/fonts/lmroman17-regular.otf")),
    include_bytes!(concat!(env!("OUT_DIR"), "/fonts/lmsans10-regular.otf")),
    include_bytes!(concat!(env!("OUT_DIR"), "/fonts/lmsans10-bold.otf")),
    include_bytes!(concat!(env!("OUT_DIR"), "/fonts/lmsans10-oblique.otf")),
    include_bytes!(concat!(env!("OUT_DIR"), "/fonts/lmsans10-boldoblique.otf")),
    include_bytes!(concat!(env!("OUT_DIR"), "/fonts/lmsans12-regular.otf")),
    include_bytes!(concat!(env!("OUT_DIR"), "/fonts/lmsans12-oblique.otf")),
    include_bytes!(concat!(env!("OUT_DIR"), "/fonts/lmsans17-regular.otf")),
    include_bytes!(concat!(env!("OUT_DIR"), "/fonts/lmsans17-oblique.otf")),
    include_bytes!(concat!(env!("OUT_DIR"), "/fonts/lmmono10-regular.otf")),
    include_bytes!(concat!(env!("OUT_DIR"), "/fonts/lmmono10-italic.otf")),
    include_bytes!(concat!(env!("OUT_DIR"), "/fonts/lmmono12-regular.otf")),
    include_bytes!(concat!(env!("OUT_DIR"), "/fonts/latinmodern-math.otf")),
    include_bytes!(concat!(env!("OUT_DIR"), "/fonts/JetBrainsMono-Regular.otf")),
    include_bytes!(concat!(env!("OUT_DIR"), "/fonts/JetBrainsMono-Italic.otf")),
    include_bytes!(concat!(env!("OUT_DIR"), "/fonts/JetBrainsMono-Bold.otf")),
    include_bytes!(concat!(
        env!("OUT_DIR"),
        "/fonts/JetBrainsMono-BoldItalic.otf"
    )),
];

#[cfg(any(not(debug_assertions), feature = "bundled-fonts"))]
fn bundled_fonts() -> impl Iterator<Item = (Font, typst::text::FontInfo)> {
    BUNDLED_FONT_BYTES.iter().flat_map(|&data| {
        Font::iter(Bytes::new(data)).map(|font| {
            let info = font.info().clone();
            (font, info)
        })
    })
}

/// Where fonts come from.
#[derive(Debug, Clone, Default)]
pub struct FontOptions {
    /// Directories scanned first, in order (`XCPC_FONT_DIRS` in the dev shell).
    pub dirs: Vec<PathBuf>,
    /// Also scan the operating system's fonts, after `dirs`.
    pub system: bool,
}

impl FontOptions {
    /// `XCPC_FONT_DIRS` (colon-separated) and no system fonts: deterministic output.
    pub fn from_env() -> Self {
        let dirs = std::env::var_os("XCPC_FONT_DIRS")
            .map(|v| std::env::split_paths(&v).collect())
            .unwrap_or_default();
        Self {
            dirs,
            system: false,
        }
    }
}

/// A diagnostic in a form the CLI, MCP and GUI can all show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub error: bool,
    pub message: String,
    /// The file the diagnostic points into, as a virtual path (`/main.typ`).
    pub file: Option<String>,
    /// 1-based line and column in that file, and the byte offset.
    pub line: Option<usize>,
    pub column: Option<usize>,
    pub offset: Option<usize>,
    /// The page (1-based, physical) a layout warning is about.
    pub page: Option<usize>,
    pub hints: Vec<String>,
}

/// The result of a successful compilation.
pub struct Compiled {
    pub document: PagedDocument,
    pub warnings: Vec<Diagnostic>,
    pub duration: Duration,
}

impl Compiled {
    pub fn page_count(&self) -> usize {
        self.document.pages().len()
    }

    /// The physical page (1-based) each element labelled `label` (`xcpc-problem`,
    /// `xcpc-tutorial`) starts on, with its metadata's `label` field: where each problem
    /// begins.
    pub fn starts(&self, label: &str) -> Vec<(String, usize)> {
        use typst::introspection::Introspector;
        let Some(label) = typst::foundations::Label::new(typst::utils::PicoStr::intern(label))
        else {
            return Vec::new();
        };
        let introspector = self.document.introspector();
        introspector
            .query(&typst::foundations::Selector::Label(label))
            .iter()
            .filter_map(|c| {
                let page = introspector.page(c.location()?)?.get();
                let value = c.get_by_name("value").ok()?;
                let name = match value {
                    typst::foundations::Value::Dict(d) => match d.get("label").ok()? {
                        typst::foundations::Value::Str(s) => s.to_string(),
                        _ => String::new(),
                    },
                    _ => String::new(),
                };
                Some((name, page))
            })
            .collect()
    }

    /// A fingerprint of what one page (0-based) draws, blind to source positions:
    /// equal fingerprints render the same, so a preview can keep the pages an edit
    /// did not change.
    pub fn page_fingerprint(&self, page: usize) -> Option<u128> {
        self.document.pages().get(page).map(fingerprint::page)
    }

    /// Fingerprints of regions `[x, y, width, height]` (pixels at `pixel_per_pt`) of one
    /// page (0-based), as [`Self::render_region`] takes them: equal fingerprints render the
    /// same pixels, and an edit changes only those of the regions it touches.
    pub fn region_fingerprints(
        &self,
        page: usize,
        pixel_per_pt: f32,
        regions: &[[u32; 4]],
    ) -> Option<Vec<u128>> {
        let page = self.document.pages().get(page)?;
        Some(fingerprint::regions(page, pixel_per_pt, regions))
    }

    /// One page's size in points (0-based).
    pub fn page_size(&self, page: usize) -> Option<(f64, f64)> {
        let size = self.document.pages().get(page)?.frame.size();
        Some((size.x.to_pt(), size.y.to_pt()))
    }

    /// Renders one page (0-based) at `pixel_per_pt` (2.0 ≈ 144 dpi).
    pub fn render(&self, page: usize, pixel_per_pt: f32) -> Option<tiny_skia::Pixmap> {
        let page = self.document.pages().get(page)?;
        let options = typst_render::RenderOptions {
            pixel_per_pt: (pixel_per_pt as f64).into(),
            ..Default::default()
        };
        Some(typst_render::render(page, &options))
    }

    /// Renders the pixels `x..x + width`, `y..y + height` of one page (0-based) at
    /// `pixel_per_pt`: a tile of what [`Self::render`] gives, the same pixels.
    pub fn render_region(
        &self,
        page: usize,
        pixel_per_pt: f32,
        [x, y, width, height]: [u32; 4],
    ) -> Option<tiny_skia::Pixmap> {
        let page = self.document.pages().get(page)?;
        let pt = |px: u32| typst::layout::Abs::pt(f64::from(px) / f64::from(pixel_per_pt));
        let mut frame = typst::layout::Frame::hard(typst::layout::Size::new(pt(width), pt(height)));
        frame.push_frame(
            typst::layout::Point::new(-pt(x), -pt(y)),
            page.frame.clone(),
        );
        let tile = typst_layout::Page {
            frame,
            bleed: Default::default(),
            ..page.clone()
        };
        let options = typst_render::RenderOptions {
            pixel_per_pt: (pixel_per_pt as f64).into(),
            ..Default::default()
        };
        Some(typst_render::render(&tile, &options))
    }

    /// Renders one page (0-based) as an SVG document whose units are points.
    pub fn render_svg(&self, page: usize) -> Option<String> {
        let page = self.document.pages().get(page)?;
        Some(typst_svg::svg(page, &typst_svg::SvgOptions::default()))
    }

    /// Renders one page as PNG bytes.
    pub fn render_png(&self, page: usize, pixel_per_pt: f32) -> Option<Vec<u8>> {
        self.render(page, pixel_per_pt)?.encode_png().ok()
    }

    /// Exports the whole document as PDF.
    pub fn pdf(&self) -> Result<Vec<u8>, Vec<Diagnostic>> {
        typst_pdf::pdf(&self.document, &typst_pdf::PdfOptions::default())
            .map_err(|errors| errors.iter().map(plain_diagnostic).collect())
    }
}

/// Compilation failed.
#[derive(Debug, Clone, thiserror::Error)]
#[error("{} error(s), first: {}", .errors.len(), .errors.first().map(|e| e.message.as_str()).unwrap_or(""))]
pub struct CompileError {
    pub errors: Vec<Diagnostic>,
    pub warnings: Vec<Diagnostic>,
}

/// A long-lived Typst world.
pub struct Engine {
    library: LazyHash<Library>,
    fonts: FontStore,
    root: Option<PathBuf>,
    main: Source,
    /// Disk files read so far, keyed by their virtual id.
    files: Mutex<HashMap<FileId, FileResult<Bytes>>>,
}

impl Engine {
    /// An engine whose relative paths resolve against `root` (a problem or contest directory).
    pub fn new(fonts: &FontOptions, root: Option<PathBuf>) -> Self {
        let mut store = FontStore::new();
        for dir in &fonts.dirs {
            store.extend(typst_kit::fonts::scan(dir));
        }
        #[cfg(any(not(debug_assertions), feature = "bundled-fonts"))]
        store.extend(bundled_fonts());
        if fonts.system {
            store.extend(typst_kit::fonts::system());
        }
        Self {
            library: LazyHash::new(Library::default()),
            fonts: store,
            root,
            main: Source::new(main_id(), String::new()),
            files: Mutex::new(HashMap::new()),
        }
    }

    /// Font families known to the engine, with the number of faces in each.
    pub fn font_families(&self) -> Vec<(String, usize)> {
        self.fonts
            .book()
            .families()
            .map(|(name, infos)| (name.to_string(), infos.count()))
            .collect()
    }

    /// Replaces the main source. Unchanged parts keep their memoized layout.
    pub fn set_main(&mut self, text: &str) {
        if self.main.text() != text {
            self.main.replace(text);
        }
    }

    /// Resolves the project's files against `root` from now on.
    pub fn set_root(&mut self, root: Option<PathBuf>) {
        self.root = root;
        self.reset_files();
    }

    /// Forgets cached disk files (call after they change on disk).
    pub fn reset_files(&mut self) {
        self.files.lock().unwrap().clear();
    }

    /// Compiles the main source. The warnings include content that overflows the text
    /// area the template declares.
    pub fn compile(&self) -> Result<Compiled, CompileError> {
        let start = Instant::now();
        let warned = typst::compile::<PagedDocument>(self);
        let mut warnings = self.diagnostics(&warned.warnings);
        if let Ok(document) = &warned.output {
            warnings.extend(overflow::check(document).into_iter().map(|o| {
                let mut d = self.locate(o.span);
                d.message = o.message;
                d.page = Some(o.page);
                d
            }));
        }
        match warned.output {
            Ok(document) => Ok(Compiled {
                document,
                warnings,
                duration: start.elapsed(),
            }),
            Err(errors) => Err(CompileError {
                errors: self.diagnostics(&errors),
                warnings,
            }),
        }
    }

    fn diagnostics(&self, list: &[SourceDiagnostic]) -> Vec<Diagnostic> {
        list.iter()
            .map(|d| {
                // In a bundled template (a helper the document called), point at the
                // call in the main source instead: that is the user's code.
                let main = self.main.id();
                let span = if d
                    .span
                    .id()
                    .is_some_and(|id| id != main && template(id).is_some())
                {
                    d.trace
                        .iter()
                        .map(|t| typst::syntax::DiagSpan::from(t.span))
                        .find(|s| s.id() == Some(main))
                        .unwrap_or(d.span)
                } else {
                    d.span
                };
                let mut out = self.locate(span);
                let plain = plain_diagnostic(d);
                out.error = plain.error;
                out.message = plain.message;
                out.hints = plain.hints;
                out
            })
            .collect()
    }

    /// A diagnostic without a message at `span`.
    fn locate(&self, span: impl Into<typst::syntax::DiagSpan>) -> Diagnostic {
        let span = span.into();
        let mut out = Diagnostic {
            error: false,
            message: String::new(),
            file: None,
            line: None,
            column: None,
            offset: None,
            page: None,
            hints: Vec::new(),
        };
        if let Some(id) = span.id() {
            out.file = Some(id.vpath().get_with_slash().to_string());
            if let (Some(range), Ok(source)) = (self.range(span), self.source(id)) {
                let lines = source.lines();
                out.offset = Some(range.start);
                out.line = lines.byte_to_line(range.start).map(|l| l + 1);
                out.column = lines.byte_to_column(range.start).map(|c| c + 1);
            }
        }
        out
    }

    /// A file of the project, if it is inside the root, links followed.
    fn read_disk(&self, id: FileId) -> FileResult<Bytes> {
        let root = self.root.as_deref().ok_or_else(|| not_found(id))?;
        let path = id
            .vpath()
            .realize(root)
            .map_err(|_| FileError::AccessDenied)?;
        let real = path
            .canonicalize()
            .map_err(|e| FileError::from_io(e, &path))?;
        let real_root = root
            .canonicalize()
            .map_err(|e| FileError::from_io(e, root))?;
        if !real.starts_with(&real_root) {
            return Err(FileError::AccessDenied);
        }
        std::fs::read(&real)
            .map(Bytes::new)
            .map_err(|e| FileError::from_io(e, &path))
    }
}

fn plain_diagnostic(d: &SourceDiagnostic) -> Diagnostic {
    Diagnostic {
        error: d.severity == Severity::Error,
        message: d.message.to_string(),
        file: None,
        line: None,
        column: None,
        offset: None,
        page: None,
        hints: d.hints.iter().map(|h| h.v.to_string()).collect(),
    }
}

fn main_id() -> FileId {
    project_id(MAIN_PATH)
}

fn project_id(path: &str) -> FileId {
    RootedPath::new(
        VirtualRoot::Project,
        VirtualPath::new(path).expect("valid virtual path"),
    )
    .intern()
}

fn not_found(id: FileId) -> FileError {
    FileError::NotFound(Path::new(id.vpath().get_with_slash()).to_path_buf())
}

fn template(id: FileId) -> Option<&'static str> {
    if !matches!(id.root(), VirtualRoot::Project) {
        return None;
    }
    let path = id.vpath().get_with_slash();
    TEMPLATES
        .iter()
        .find(|(p, _)| *p == path)
        .map(|(_, text)| *text)
}

impl World for Engine {
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
        static TEMPLATE_SOURCES: OnceLock<Mutex<HashMap<FileId, Source>>> = OnceLock::new();
        if let Some(text) = template(id) {
            let cache = TEMPLATE_SOURCES.get_or_init(Default::default);
            let mut cache = cache.lock().unwrap();
            return Ok(cache
                .entry(id)
                .or_insert_with(|| Source::new(id, text.into()))
                .clone());
        }
        let bytes = self.file(id)?;
        let text = std::str::from_utf8(&bytes).map_err(|_| FileError::InvalidUtf8)?;
        Ok(Source::new(id, text.into()))
    }

    fn file(&self, id: FileId) -> FileResult<Bytes> {
        if let Some(text) = template(id) {
            return Ok(Bytes::from_string(text));
        }
        if !matches!(id.root(), VirtualRoot::Project) {
            return Err(FileError::Other(Some("packages are not available".into())));
        }
        let mut files = self.files.lock().unwrap();
        files
            .entry(id)
            .or_insert_with(|| self.read_disk(id))
            .clone()
    }

    fn font(&self, index: usize) -> Option<Font> {
        self.fonts.font(index)
    }

    fn today(&self, _offset: Option<TypstDuration>) -> Option<Datetime> {
        None
    }
}

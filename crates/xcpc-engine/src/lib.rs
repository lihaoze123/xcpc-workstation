//! Native Typst typesetting for XCPC Workstation.
//!
//! An [`Engine`] owns one Typst [`World`]: the bundled `olymp-classic` template, a font
//! store and an optional project root for assets. The main source is replaced in place
//! ([`Engine::set_main`]), so repeated compilations reuse `comemo`'s memoized work.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
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
pub mod fragment;
mod locate;
mod overflow;
mod packages;
mod schema;

pub use fragment::{
    FRAGMENT_TIMEOUT_MS, FragmentFormat, FragmentRenderOptions, FragmentTheme,
    MAX_FRAGMENT_PIXEL_DIM, MAX_FRAGMENT_SOURCE_BYTES, RenderedFragment, render_typst_fragment,
};

pub use typst_render::render as render_page_raw;
pub use xcpc_domain::{PRESETS, PresetSeed, list_presets, list_presets_by_kind, preset_source};

/// The bundled runtime modules, addressable from Typst as `/xcpc/<name>`: default
/// statement/editorial hooks (`defaults.typ`), what the Markdown converter emits calls
/// (`markdown.typ`), MiTeX's scope for it, and the editorial's code theme (`mono.tmTheme`).
const TEMPLATES: &[(&str, &str)] = &[
    (
        "/xcpc/defaults.typ",
        include_str!("../templates/defaults.typ"),
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
    (
        "/xcpc/trd-compact.typ",
        include_str!("../templates/trd-compact.typ"),
    ),
    (
        "/xcpc/trd-book.typ",
        include_str!("../templates/trd-book.typ"),
    ),
];

/// Path of the main source inside the virtual project.
const MAIN_PATH: &str = "/main.typ";

/// Per-engine virtual module exposing the resolved CJK font family names.
const FONTS_PATH: &str = "/xcpc/fonts.typ";

/// Maximum file size allowed for a single font file inside a template plugin (25 MiB).
pub const MAX_PLUGIN_FONT_FILE_BYTES: u64 = 25 * 1024 * 1024;

/// Metadata for a single template declared by an enabled template plugin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginTemplateEntry {
    pub id: String,
    pub entry: String,
    pub kind: Option<xcpc_domain::TemplateKind>,
    pub name: Option<String>,
    pub description: Option<String>,
}

/// Read-only virtual mount for an enabled template plugin at `/plugins/<plugin_id>/...`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginMount {
    pub plugin_id: String,
    pub plugin_name: String,
    pub version: String,
    pub root: PathBuf,
    pub font_dirs: Vec<PathBuf>,
    pub templates: Vec<PluginTemplateEntry>,
}

#[derive(Debug, Clone)]
struct ResolvedPluginMount {
    mount: PluginMount,
    canonical_root: PathBuf,
    font_fingerprint: u64,
}

struct LayeredFontStore {
    base: Arc<FontStore>,
    base_len: usize,
    extra: FontStore,
    combined_book: LazyHash<FontBook>,
}

impl LayeredFontStore {
    fn new(base: Arc<FontStore>, extra: FontStore) -> Self {
        let mut book = FontBook::new();
        let mut base_len = 0usize;
        while let Some(info) = base.book().info(base_len) {
            book.push(info.clone());
            base_len += 1;
        }
        let mut extra_idx = 0usize;
        while let Some(info) = extra.book().info(extra_idx) {
            book.push(info.clone());
            extra_idx += 1;
        }
        Self {
            base,
            base_len,
            extra,
            combined_book: LazyHash::new(book),
        }
    }

    fn book(&self) -> &LazyHash<FontBook> {
        &self.combined_book
    }

    fn font(&self, index: usize) -> Option<Font> {
        if index < self.base_len {
            self.base.font(index)
        } else {
            self.extra.font(index - self.base_len)
        }
    }
}

/// Scans `.ttf` / `.otf` / `.ttc` font files inside `dirs` (strictly within `allowed_root`
/// when provided, rejecting symlinks and files exceeding `MAX_PLUGIN_FONT_FILE_BYTES`).
fn scan_bounded_font_dirs(dirs: &[PathBuf], allowed_root: Option<&Path>) -> FontStore {
    let mut store = FontStore::new();
    let mut stack: Vec<PathBuf> = dirs.to_vec();
    while let Some(dir) = stack.pop() {
        let Ok(meta) = std::fs::symlink_metadata(&dir) else {
            continue;
        };
        if meta.file_type().is_symlink() || !meta.is_dir() {
            continue;
        }
        let Ok(canon_dir) = dir.canonicalize() else {
            continue;
        };
        if let Some(root) = allowed_root
            && !canon_dir.starts_with(root)
        {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&canon_dir) else {
            continue;
        };
        let mut children: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
        children.sort();
        for path in children {
            let Ok(child_meta) = std::fs::symlink_metadata(&path) else {
                continue;
            };
            if child_meta.file_type().is_symlink() {
                continue;
            }
            if child_meta.is_dir() {
                stack.push(path);
                continue;
            }
            if !child_meta.is_file() || child_meta.len() > MAX_PLUGIN_FONT_FILE_BYTES {
                continue;
            }
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .map(|s| s.to_ascii_lowercase());
            if !matches!(ext.as_deref(), Some("ttf" | "otf" | "ttc")) {
                continue;
            }
            let Ok(canon_file) = path.canonicalize() else {
                continue;
            };
            if let Some(root) = allowed_root
                && !canon_file.starts_with(root)
            {
                continue;
            }
            let Ok(bytes) = std::fs::read(&canon_file) else {
                continue;
            };
            for (index, info) in typst::text::FontInfo::iter(&bytes).enumerate() {
                store.push((
                    typst_kit::fonts::FontPath {
                        path: canon_file.clone(),
                        index: index as u32,
                    },
                    info,
                ));
            }
        }
    }
    store
}

fn fingerprint_font_dirs(dirs: &[PathBuf]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    let mut stack: Vec<PathBuf> = dirs.to_vec();
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut children: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
        children.sort();
        for path in children {
            let Ok(meta) = std::fs::symlink_metadata(&path) else {
                continue;
            };
            if meta.file_type().is_symlink() {
                continue;
            }
            if meta.is_dir() {
                stack.push(path);
                continue;
            }
            path.to_string_lossy().hash(&mut hasher);
            meta.len().hash(&mut hasher);
            if let Ok(mtime) = meta.modified()
                && let Ok(dur) = mtime.duration_since(std::time::UNIX_EPOCH)
            {
                dur.as_secs().hash(&mut hasher);
                dur.subsec_nanos().hash(&mut hasher);
            }
        }
    }
    hasher.finish()
}

/// Candidate CJK font families in preference order for each template role.
/// The first entry is the pinned FounderType family; later entries serve as
/// standalone/cross-platform fallbacks when FounderType is unavailable.
const CJK_ROLES: &[(&str, &[&str])] = &[
    (
        "cjk-song",
        &[
            "FZShuSong-Z01",
            "Source Han Serif SC",
            "Noto Serif CJK SC",
            "SimSun",
            "NSimSun",
            "Songti SC",
            "STSong",
        ],
    ),
    (
        "cjk-hei",
        &[
            "FZHei-B01",
            "Source Han Sans SC",
            "Noto Sans CJK SC",
            "SimHei",
            "Microsoft YaHei",
            "PingFang SC",
            "Hiragino Sans GB",
            "Heiti SC",
            "STHeiti",
            "Songti SC",
            "STSong",
        ],
    ),
    (
        "cjk-kai",
        &[
            "FZKai-Z03",
            "KaiTi",
            "STKaiti",
            "Kaiti SC",
            "Source Han Serif SC",
            "Noto Serif CJK SC",
            "SimSun",
            "Songti SC",
            "STSong",
        ],
    ),
    (
        "cjk-fangsong",
        &[
            "FZFangSong-Z02",
            "FangSong",
            "STFangsong",
            "Source Han Serif SC",
            "Noto Serif CJK SC",
            "SimSun",
            "Songti SC",
            "STSong",
        ],
    ),
];

/// Whether Latin Modern and JetBrains Mono fonts are embedded directly in this binary.
pub const HAS_BUNDLED_FONTS: bool = cfg!(any(not(debug_assertions), feature = "bundled-fonts"));

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
    include_bytes!(concat!(env!("OUT_DIR"), "/fonts/JetBrainsMono-Regular.ttf")),
    include_bytes!(concat!(env!("OUT_DIR"), "/fonts/JetBrainsMono-Italic.ttf")),
    include_bytes!(concat!(env!("OUT_DIR"), "/fonts/JetBrainsMono-Bold.ttf")),
    include_bytes!(concat!(
        env!("OUT_DIR"),
        "/fonts/JetBrainsMono-BoldItalic.ttf"
    )),
];

#[cfg(any(not(debug_assertions), feature = "bundled-fonts"))]
struct BundledFontSource {
    data: &'static [u8],
    index: u32,
}

#[cfg(any(not(debug_assertions), feature = "bundled-fonts"))]
impl typst_kit::fonts::FontSource for BundledFontSource {
    fn load(&self) -> Option<Font> {
        Font::new(Bytes::new(self.data), self.index)
    }
}

#[cfg(any(not(debug_assertions), feature = "bundled-fonts"))]
fn bundled_fonts() -> impl Iterator<Item = (BundledFontSource, typst::text::FontInfo)> {
    BUNDLED_FONT_BYTES.iter().flat_map(|&data| {
        typst::text::FontInfo::iter(data)
            .enumerate()
            .map(move |(index, info)| {
                (
                    BundledFontSource {
                        data,
                        index: index as u32,
                    },
                    info,
                )
            })
    })
}

/// Releases all globally memoized Typst AST, layout, and glyph-shaping caches.
pub fn evict_typst_cache() {
    comemo::evict(0);
}

/// Scans a single font file on disk into `store` without keeping its raw bytes in memory.
#[cfg(windows)]
fn push_font_file(store: &mut FontStore, path: &Path) -> bool {
    let Ok(bytes) = std::fs::read(path) else {
        return false;
    };
    let mut added = false;
    for (index, info) in typst::text::FontInfo::iter(&bytes).enumerate() {
        store.push((
            typst_kit::fonts::FontPath {
                path: path.to_path_buf(),
                index: index as u32,
            },
            info,
        ));
        added = true;
    }
    added
}

/// Loads only the CJK fonts needed by `CJK_ROLES` from standard Windows font directories,
/// avoiding a full 500+-file scan of `C:\Windows\Fonts` (~180–220 MB RAM & ~250ms startup).
/// If a custom template later references another system font, [`Engine::ensure_full_system_fonts`]
/// transparently falls back to scanning the full system font database.
#[cfg(windows)]
fn load_windows_cjk_fonts(store: &mut FontStore) {
    let mut dirs = Vec::new();
    if let Some(windir) = std::env::var_os("WINDIR") {
        let sys_fonts = PathBuf::from(windir).join("Fonts");
        if sys_fonts.is_dir() {
            dirs.push(sys_fonts);
        }
    }
    if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
        let user_fonts = PathBuf::from(local_app_data)
            .join("Microsoft")
            .join("Windows")
            .join("Fonts");
        if user_fonts.is_dir() {
            dirs.push(user_fonts);
        }
    }
    if dirs.is_empty() {
        return;
    }

    let mut loaded_paths = std::collections::HashSet::new();

    // 1. Prefer user-installed FounderType fonts if present.
    for dir in &dirs {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            let lower = name.to_ascii_lowercase();
            let is_font =
                lower.ends_with(".ttf") || lower.ends_with(".otf") || lower.ends_with(".ttc");
            if !is_font {
                continue;
            }
            let is_foundertype = matches!(
                lower.as_str(),
                "fzssk.ttf" | "fzhtk.ttf" | "fzktk.ttf" | "fzfsk.ttf"
            ) || lower.starts_with("fzshusong")
                || lower.starts_with("fzhei")
                || lower.starts_with("fzkai")
                || lower.starts_with("fzfangsong");
            if is_foundertype && loaded_paths.insert(path.clone()) {
                push_font_file(store, &path);
            }
        }
    }

    // 2. For each CJK role still missing a primary/fallback family, load only the first
    //    available built-in Windows CJK font file (~4 files instead of 500+).
    let mut try_load = |store: &mut FontStore, filename: &str| -> bool {
        for dir in &dirs {
            let candidate = dir.join(filename);
            if candidate.is_file()
                && loaded_paths.insert(candidate.clone())
                && push_font_file(store, &candidate)
            {
                return true;
            }
        }
        false
    };
    const WINDOWS_ROLE_FILES: &[(&[&str], &[&str])] = &[
        (
            &[
                "FZShuSong-Z01",
                "Source Han Serif SC",
                "Noto Serif CJK SC",
                "SimSun",
            ],
            &["simsun.ttc", "NotoSerifSC-VF.ttf"],
        ),
        (
            &[
                "FZHei-B01",
                "Source Han Sans SC",
                "Noto Sans CJK SC",
                "SimHei",
                "Microsoft YaHei",
            ],
            &["simhei.ttf", "msyh.ttc", "NotoSansSC-VF.ttf"],
        ),
        (
            &["FZKai-Z03", "KaiTi", "STKaiti"],
            &["simkai.ttf", "STKAITI.TTF"],
        ),
        (
            &["FZFangSong-Z02", "FangSong", "STFangsong"],
            &["simfang.ttf", "STFANGSO.TTF"],
        ),
    ];

    for &(families, filenames) in WINDOWS_ROLE_FILES {
        let already_have = families.iter().any(|name| {
            store
                .book()
                .select_family(&name.to_lowercase())
                .next()
                .is_some()
        });
        if already_have {
            continue;
        }
        for &filename in filenames {
            if try_load(store, filename) {
                break;
            }
        }
    }
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
    /// Reads `XCPC_FONT_DIRS` (deterministic, no system fonts). When unset or empty,
    /// scans a sibling `fonts/` directory next to the executable (if present) and
    /// enables system font discovery for standalone builds.
    pub fn from_env() -> Self {
        let exe = std::env::current_exe().ok();
        Self::from_env_with(
            std::env::var_os("XCPC_FONT_DIRS").as_deref(),
            exe.as_deref().and_then(Path::parent),
        )
    }

    fn from_env_with(env_val: Option<&OsStr>, exe_dir: Option<&Path>) -> Self {
        if let Some(v) = env_val.filter(|v| !v.is_empty()) {
            return Self {
                dirs: std::env::split_paths(v).collect(),
                system: false,
            };
        }
        let mut dirs = Vec::new();
        if let Some(dir) = exe_dir {
            let sibling = dir.join("fonts");
            if sibling.is_dir() {
                dirs.push(sibling);
            }
            // macOS `.app` bundles place executables in `Contents/MacOS/` and resources in
            // `Contents/Resources/`.
            if dir.file_name().and_then(|n| n.to_str()) == Some("MacOS")
                && let Some(contents) = dir.parent()
                && contents.file_name().and_then(|n| n.to_str()) == Some("Contents")
            {
                let res_fonts = contents.join("Resources").join("fonts");
                if res_fonts.is_dir() {
                    dirs.push(res_fonts);
                }
            }
        }
        Self { dirs, system: true }
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
        let _span = xcpc_trace::span_with(
            "engine",
            "render_page",
            || serde_json::json!({ "page": page, "scale": pixel_per_pt }),
        );
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
        let _span = xcpc_trace::span_with(
            "engine",
            "render_region",
            || serde_json::json!({ "page": page, "scale": pixel_per_pt, "w": width, "h": height }),
        );
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

    /// Renders one page as PNG bytes.
    pub fn render_png(&self, page: usize, pixel_per_pt: f32) -> Option<Vec<u8>> {
        let pix = self.render(page, pixel_per_pt)?;
        let _span = xcpc_trace::span_with(
            "engine",
            "encode_png",
            || serde_json::json!({ "page": page, "w": pix.width(), "h": pix.height() }),
        );
        pix.encode_png().ok()
    }

    /// Exports the whole document as PDF.
    pub fn pdf(&self) -> Result<Vec<u8>, Vec<Diagnostic>> {
        let _span = xcpc_trace::span_with(
            "engine",
            "export_pdf",
            || serde_json::json!({ "pages": self.document.pages().len() }),
        );
        let options = typst_pdf::PdfOptions {
            creator: typst::foundations::Smart::Custom(Some(format!(
                "XCPC Workstation {}",
                env!("CARGO_PKG_VERSION")
            ))),
            ..Default::default()
        };
        typst_pdf::pdf(&self.document, &options)
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
    library: Arc<LazyHash<Library>>,
    fonts: Arc<FontStore>,
    active_extra_fonts: Option<Arc<LayeredFontStore>>,
    active_extra_font_key: Option<String>,
    extra_font_cache: HashMap<String, (Arc<LayeredFontStore>, String, Option<String>)>,
    base_fonts_source: Source,
    base_font_fallback_warning: Option<String>,
    font_options: FontOptions,
    full_system_fonts_loaded: bool,
    root: Option<PathBuf>,
    canonical_root: Option<PathBuf>,
    plugin_mounts: HashMap<String, ResolvedPluginMount>,
    main: Source,
    fonts_source: Source,
    fonts_used: AtomicBool,
    font_fallback_warning: Option<String>,
    /// Disk files read so far, keyed by their virtual id.
    files: Mutex<HashMap<FileId, FileResult<Bytes>>>,
    /// Parsed workspace `.typ` sources cached across compiles until `reset_files()`.
    sources: Mutex<HashMap<FileId, FileResult<Source>>>,
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
        let full_system_fonts_loaded = if fonts.system {
            #[cfg(windows)]
            {
                load_windows_cjk_fonts(&mut store);
                false
            }
            #[cfg(not(windows))]
            {
                store.extend(typst_kit::fonts::system());
                true
            }
        } else {
            true
        };
        let (fonts_text, font_fallback_warning) = resolve_cjk_fonts(store.book());
        let canonical_root = root.as_deref().and_then(|r| r.canonicalize().ok());
        let base_fonts_source = Source::new(project_id(FONTS_PATH), fonts_text.clone());
        Self {
            library: Arc::new(LazyHash::new(Library::default())),
            fonts: Arc::new(store),
            active_extra_fonts: None,
            active_extra_font_key: None,
            extra_font_cache: HashMap::new(),
            base_fonts_source: base_fonts_source.clone(),
            base_font_fallback_warning: font_fallback_warning.clone(),
            font_options: fonts.clone(),
            full_system_fonts_loaded,
            root,
            canonical_root,
            plugin_mounts: HashMap::new(),
            main: Source::new(main_id(), String::new()),
            fonts_source: base_fonts_source,
            fonts_used: AtomicBool::new(false),
            font_fallback_warning,
            files: Mutex::new(HashMap::new()),
            sources: Mutex::new(HashMap::new()),
        }
    }

    /// Creates an independent engine that shares this engine's already-scanned font store
    /// and standard library, avoiding duplicate system-font scans and memory usage.
    pub fn share_fonts(&self, root: Option<PathBuf>) -> Self {
        let canonical_root = root.as_deref().and_then(|r| r.canonicalize().ok());
        let base_fonts_source = Source::new(
            project_id(FONTS_PATH),
            self.base_fonts_source.text().to_owned(),
        );
        Self {
            library: Arc::clone(&self.library),
            fonts: Arc::clone(&self.fonts),
            active_extra_fonts: None,
            active_extra_font_key: None,
            extra_font_cache: HashMap::new(),
            base_fonts_source: base_fonts_source.clone(),
            base_font_fallback_warning: self.base_font_fallback_warning.clone(),
            font_options: self.font_options.clone(),
            full_system_fonts_loaded: self.full_system_fonts_loaded,
            root,
            canonical_root,
            plugin_mounts: self.plugin_mounts.clone(),
            main: Source::new(main_id(), String::new()),
            fonts_source: base_fonts_source,
            fonts_used: AtomicBool::new(false),
            font_fallback_warning: self.base_font_fallback_warning.clone(),
            files: Mutex::new(HashMap::new()),
            sources: Mutex::new(HashMap::new()),
        }
    }

    /// Loads the full OS system font database on demand when a custom template references
    /// a system font outside the bundled + targeted CJK set. Returns `true` if new system
    /// fonts were loaded and the caller should re-compile.
    pub fn ensure_full_system_fonts(&mut self) -> bool {
        if !self.font_options.system || self.full_system_fonts_loaded {
            return false;
        }
        static FULL_SYSTEM_STORE: OnceLock<Arc<FontStore>> = OnceLock::new();
        let store = if self.font_options.dirs.is_empty() {
            Arc::clone(FULL_SYSTEM_STORE.get_or_init(|| {
                let mut s = FontStore::new();
                #[cfg(any(not(debug_assertions), feature = "bundled-fonts"))]
                s.extend(bundled_fonts());
                s.extend(typst_kit::fonts::system());
                Arc::new(s)
            }))
        } else {
            let mut s = FontStore::new();
            for dir in &self.font_options.dirs {
                s.extend(typst_kit::fonts::scan(dir));
            }
            #[cfg(any(not(debug_assertions), feature = "bundled-fonts"))]
            s.extend(bundled_fonts());
            s.extend(typst_kit::fonts::system());
            Arc::new(s)
        };
        let (fonts_text, font_fallback_warning) = resolve_cjk_fonts(store.book());
        let base_src = Source::new(project_id(FONTS_PATH), fonts_text);
        self.fonts = store;
        self.base_fonts_source = base_src.clone();
        self.base_font_fallback_warning = font_fallback_warning.clone();
        self.active_extra_fonts = None;
        self.active_extra_font_key = None;
        self.extra_font_cache.clear();
        self.fonts_source = base_src;
        self.font_fallback_warning = font_fallback_warning;
        self.full_system_fonts_loaded = true;
        true
    }

    /// Replaces the set of enabled template plugin virtual mounts (`/plugins/<plugin_id>/...`).
    /// Invalidates cached virtual files and stale plugin font caches when mounts change.
    pub fn set_plugin_mounts(&mut self, mounts: Vec<PluginMount>) {
        let mut next = HashMap::with_capacity(mounts.len());
        let mut valid_font_keys = std::collections::HashSet::new();
        for mount in mounts {
            let Ok(canonical_root) = mount.root.canonicalize() else {
                continue;
            };
            let font_fingerprint = fingerprint_font_dirs(&mount.font_dirs);
            let font_key = format!(
                "plugin:{}:{}:{font_fingerprint}",
                mount.plugin_id, mount.version
            );
            valid_font_keys.insert(font_key);
            next.insert(
                mount.plugin_id.clone(),
                ResolvedPluginMount {
                    mount,
                    canonical_root,
                    font_fingerprint,
                },
            );
        }
        self.plugin_mounts = next;
        self.extra_font_cache
            .retain(|k, _| !k.starts_with("plugin:") || valid_font_keys.contains(k));
        if let Some(active_key) = self.active_extra_font_key.as_deref()
            && active_key.starts_with("plugin:")
            && !valid_font_keys.contains(active_key)
        {
            self.activate_plugin_fonts(None);
        }
        self.reset_files();
    }

    /// Returns all currently registered plugin mounts sorted by `plugin_id`.
    pub fn plugin_mounts(&self) -> Vec<&PluginMount> {
        let mut list: Vec<&PluginMount> = self.plugin_mounts.values().map(|r| &r.mount).collect();
        list.sort_by(|a, b| a.plugin_id.cmp(&b.plugin_id));
        list
    }

    /// Looks up a mounted plugin template by `(plugin_id, template_id)`.
    pub fn find_plugin_template(
        &self,
        plugin_id: &str,
        template_id: &str,
    ) -> Option<(&PluginMount, &PluginTemplateEntry)> {
        let resolved = self.plugin_mounts.get(plugin_id)?;
        let entry = resolved
            .mount
            .templates
            .iter()
            .find(|t| t.id == template_id)?;
        Some((&resolved.mount, entry))
    }

    /// Activates on-demand fonts for the given mounted plugin (`Some(plugin_id)`), or
    /// restores the base font set (`None`). Results are cached by `(plugin_id, version, mtime)`.
    pub fn activate_plugin_fonts(&mut self, plugin_id: Option<&str>) {
        let Some(pid) = plugin_id else {
            if self.active_extra_fonts.is_some() {
                self.active_extra_fonts = None;
                self.active_extra_font_key = None;
                self.fonts_source = self.base_fonts_source.clone();
                self.font_fallback_warning = self.base_font_fallback_warning.clone();
            }
            return;
        };
        let Some(resolved) = self.plugin_mounts.get(pid) else {
            self.activate_plugin_fonts(None);
            return;
        };
        if resolved.mount.font_dirs.is_empty() {
            self.activate_plugin_fonts(None);
            return;
        }
        let key = format!(
            "plugin:{}:{}:{}",
            resolved.mount.plugin_id, resolved.mount.version, resolved.font_fingerprint
        );
        if self.active_extra_font_key.as_deref() == Some(key.as_str()) {
            return;
        }
        let (layered, fonts_text, warn) = if let Some(hit) = self.extra_font_cache.get(&key) {
            hit.clone()
        } else {
            let extra =
                scan_bounded_font_dirs(&resolved.mount.font_dirs, Some(&resolved.canonical_root));
            let layered = Arc::new(LayeredFontStore::new(Arc::clone(&self.fonts), extra));
            let (fonts_text, warn) = resolve_cjk_fonts(layered.book());
            self.extra_font_cache.insert(
                key.clone(),
                (Arc::clone(&layered), fonts_text.clone(), warn.clone()),
            );
            (layered, fonts_text, warn)
        };
        self.active_extra_fonts = Some(layered);
        self.active_extra_font_key = Some(key);
        self.fonts_source = Source::new(project_id(FONTS_PATH), fonts_text);
        self.font_fallback_warning = warn;
    }

    /// Activates extra on-demand font directories (e.g. from a frozen contest snapshot's
    /// archived plugin template fonts) keyed by `cache_key`.
    pub fn activate_extra_font_dirs(&mut self, cache_key: &str, dirs: &[PathBuf]) {
        if dirs.is_empty() {
            self.activate_plugin_fonts(None);
            return;
        }
        let fp = fingerprint_font_dirs(dirs);
        let key = format!("extra:{cache_key}:{fp}");
        if self.active_extra_font_key.as_deref() == Some(key.as_str()) {
            return;
        }
        let (layered, fonts_text, warn) = if let Some(hit) = self.extra_font_cache.get(&key) {
            hit.clone()
        } else {
            let extra = scan_bounded_font_dirs(dirs, self.canonical_root.as_deref());
            let layered = Arc::new(LayeredFontStore::new(Arc::clone(&self.fonts), extra));
            let (fonts_text, warn) = resolve_cjk_fonts(layered.book());
            self.extra_font_cache.insert(
                key.clone(),
                (Arc::clone(&layered), fonts_text.clone(), warn.clone()),
            );
            (layered, fonts_text, warn)
        };
        self.active_extra_fonts = Some(layered);
        self.active_extra_font_key = Some(key);
        self.fonts_source = Source::new(project_id(FONTS_PATH), fonts_text);
        self.font_fallback_warning = warn;
    }

    /// Font families known to the engine, with the number of faces in each.
    pub fn font_families(&self) -> Vec<(String, usize)> {
        self.book()
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
        self.canonical_root = root.as_deref().and_then(|r| r.canonicalize().ok());
        self.root = root;
        self.reset_files();
    }

    /// Forgets cached disk files (call after they change on disk).
    pub fn reset_files(&mut self) {
        self.files.lock().unwrap().clear();
        self.sources.lock().unwrap().clear();
    }

    /// Compiles the main source. The warnings include content that overflows the text
    /// area the template declares.
    pub fn compile(&self) -> Result<Compiled, CompileError> {
        let _span = xcpc_trace::span("engine", "compile");
        let start = Instant::now();
        let warned = {
            let _typst = xcpc_trace::span("engine", "typst_compile");
            let out = typst::compile::<PagedDocument>(self);
            // Evict stale comemo memo entries older than 2 compiles so switching away from
            // heavy CeTZ/WASM/contest documents immediately frees their AST & frame graphs
            // while live editing of the active document keeps 100% incremental cache hits.
            comemo::evict(2);
            out
        };
        let mut warnings = self.diagnostics(&warned.warnings);
        if let Ok(document) = &warned.output {
            let _overflow = xcpc_trace::span("engine", "overflow_check");
            warnings.extend(overflow::check(document).into_iter().map(|o| {
                let mut d = self.locate(o.span);
                d.message = o.message;
                d.page = Some(o.page);
                d
            }));
        }
        if self.fonts_used.load(Ordering::Relaxed)
            && let Some(message) = &self.font_fallback_warning
        {
            warnings.push(Diagnostic {
                error: false,
                message: message.clone(),
                file: None,
                line: None,
                column: None,
                offset: None,
                page: None,
                hints: Vec::new(),
            });
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

    /// Evaluates `import_path` (e.g. `"/xcpc/olymp-classic.typ"`, `"/templates/my.typ"`,
    /// or `"/plugins/my-pack/templates/ec.typ"`) and returns its exported `#let schema = (...)`,
    /// or an empty schema if absent.
    pub fn inspect_template_schema(&self, import_path: &str) -> xcpc_domain::TemplateSchema {
        schema::inspect(self, import_path)
    }

    /// Validates that `import_path` compiles cleanly and exports `#let render(doc)`,
    /// returning its parsed `#let schema` on success or Typst diagnostics on failure.
    pub fn validate_template_module(
        &self,
        import_path: &str,
    ) -> Result<xcpc_domain::TemplateSchema, Vec<Diagnostic>> {
        schema::validate_module(self, import_path)
    }

    /// Resolves a point `(x_pt, y_pt)` (typographic points from the top-left of 0-based
    /// `page`) to the corresponding byte offset in the main source (`/main.typ`).
    pub fn locate_point(
        &self,
        compiled: &Compiled,
        page: usize,
        x_pt: f64,
        y_pt: f64,
    ) -> Option<usize> {
        locate::point(&self.main, compiled, page, x_pt, y_pt)
    }

    fn diagnostics(&self, list: &[SourceDiagnostic]) -> Vec<Diagnostic> {
        list.iter()
            .map(|d| {
                // In a bundled template (a helper the document called), point at the
                // call in the main source instead: that is the user's code.
                let main = self.main.id();
                let span = if d.span.id().is_some_and(|id| {
                    id != main
                        && (id == self.fonts_source.id()
                            || template(id).is_some()
                            || matches!(id.root(), VirtualRoot::Package(_)))
                }) {
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
            out.file = Some(match id.root() {
                VirtualRoot::Project => id.vpath().get_with_slash().to_string(),
                VirtualRoot::Package(spec) => {
                    format!("{spec}{}", id.vpath().get_with_slash())
                }
            });
            if let (Some(range), Ok(source)) = (self.range(span), self.source(id)) {
                let lines = source.lines();
                out.offset = Some(range.start);
                out.line = lines.byte_to_line(range.start).map(|l| l + 1);
                out.column = lines.byte_to_column(range.start).map(|c| c + 1);
            }
        }
        out
    }

    /// A file of the project, if it is inside the root, or inside a mounted plugin at
    /// `/plugins/<plugin_id>/...` (strictly isolated to that plugin's canonical root).
    fn read_disk(&self, id: FileId) -> FileResult<Bytes> {
        let vpath = id.vpath().get_with_slash();
        if let Some(rest) = vpath.strip_prefix("/plugins/") {
            let Some((plugin_id, rel)) = rest.split_once('/') else {
                return Err(FileError::AccessDenied);
            };
            if plugin_id.is_empty() || rel.is_empty() || rel.contains('\\') {
                return Err(FileError::AccessDenied);
            }
            let Some(resolved) = self.plugin_mounts.get(plugin_id) else {
                return Err(not_found(id));
            };
            let rel_path = Path::new(rel);
            if rel_path.is_absolute() {
                return Err(FileError::AccessDenied);
            }
            for comp in rel_path.components() {
                if matches!(
                    comp,
                    std::path::Component::ParentDir
                        | std::path::Component::RootDir
                        | std::path::Component::Prefix(_)
                ) {
                    return Err(FileError::AccessDenied);
                }
            }
            let full = resolved.canonical_root.join(rel_path);
            let real = full
                .canonicalize()
                .map_err(|e| FileError::from_io(e, &full))?;
            if !real.starts_with(&resolved.canonical_root) {
                return Err(FileError::AccessDenied);
            }
            return std::fs::read(&real)
                .map(Bytes::new)
                .map_err(|e| FileError::from_io(e, &full));
        }

        let disk_res = (|| -> FileResult<Bytes> {
            let root = self.root.as_deref().ok_or_else(|| not_found(id))?;
            let path = id
                .vpath()
                .realize(root)
                .map_err(|_| FileError::AccessDenied)?;
            let real = path
                .canonicalize()
                .map_err(|e| FileError::from_io(e, &path))?;
            let owned_real_root;
            let real_root = match self.canonical_root.as_deref() {
                Some(r) => r,
                None => {
                    owned_real_root = root
                        .canonicalize()
                        .map_err(|e| FileError::from_io(e, root))?;
                    &owned_real_root
                }
            };
            if !real.starts_with(real_root) {
                return Err(FileError::AccessDenied);
            }
            std::fs::read(&real)
                .map(Bytes::new)
                .map_err(|e| FileError::from_io(e, &path))
        })();

        // Allow plugin templates to import built-in helper aliases like `/templates/defaults.typ`
        // or `/templates/markdown.typ` when the workspace doesn't shadow them on disk.
        if let Err(FileError::NotFound(_)) = &disk_res {
            if let Some(builtin_text) = builtin_template_alias(vpath) {
                return Ok(Bytes::from_string(builtin_text));
            }
            if vpath == "/templates/fonts.typ" {
                self.fonts_used.store(true, Ordering::Relaxed);
                return Ok(Bytes::from_string(self.fonts_source.text().to_owned()));
            }
        }
        disk_res
    }
}

fn builtin_template_alias(vpath: &str) -> Option<&'static str> {
    let mapped = match vpath {
        "/templates/defaults.typ" => "/xcpc/defaults.typ",
        "/templates/markdown.typ" => "/xcpc/markdown.typ",
        "/templates/mono.tmTheme" => "/xcpc/mono.tmTheme",
        _ => return None,
    };
    TEMPLATES
        .iter()
        .find(|(p, _)| *p == mapped)
        .map(|(_, text)| *text)
}

/// Picks the first available family for each CJK role and generates `/xcpc/fonts.typ`.
/// Selecting a single family per role avoids Typst's per-entry `unknown font family`
/// warnings for platform fallbacks that are not installed on the current host.
fn resolve_cjk_fonts(book: &FontBook) -> (String, Option<String>) {
    let mut source = String::new();
    let mut fallbacks = Vec::new();
    for &(var, candidates) in CJK_ROLES {
        let primary = candidates[0];
        let chosen = candidates
            .iter()
            .copied()
            .find(|name| book.select_family(&name.to_lowercase()).next().is_some());
        let family = match chosen {
            Some(name) => {
                if name != primary {
                    fallbacks.push(format!("{primary} -> {name}"));
                }
                name
            }
            None => primary,
        };
        source.push_str(&format!("#let {var} = \"{family}\"\n"));
    }
    // Latin Modern 2.004 (Nix / Debian `lmodern`) names the 10pt optical size
    // `"Latin Modern Roman"` / `"Latin Modern Sans"`, whereas Latin Modern 2.006
    // (CTAN `lm.zip`, bundled in release builds) names it `"Latin Modern Roman 10"` /
    // `"Latin Modern Sans 10"`. Export the resolved 10pt families via `/xcpc/fonts.typ`.
    for &(var, candidates) in &[
        (
            "lm-roman-10",
            &["Latin Modern Roman", "Latin Modern Roman 10"][..],
        ),
        (
            "lm-sans-10",
            &["Latin Modern Sans", "Latin Modern Sans 10"][..],
        ),
    ] {
        let family = candidates
            .iter()
            .copied()
            .find(|name| book.select_family(&name.to_lowercase()).next().is_some())
            .unwrap_or(candidates[0]);
        source.push_str(&format!("#let {var} = \"{family}\"\n"));
    }
    let warning = (!fallbacks.is_empty()).then(|| {
        format!(
            "FounderType CJK font(s) missing; using fallback: {}",
            fallbacks.join(", ")
        )
    });
    (source, warning)
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
        if let Some(layered) = &self.active_extra_fonts {
            layered.book()
        } else {
            self.fonts.book()
        }
    }

    fn main(&self) -> FileId {
        self.main.id()
    }

    fn source(&self, id: FileId) -> FileResult<Source> {
        if id == self.main.id() {
            return Ok(self.main.clone());
        }
        if id == self.fonts_source.id() {
            self.fonts_used.store(true, Ordering::Relaxed);
            return Ok(self.fonts_source.clone());
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
        if let VirtualRoot::Package(spec) = id.root() {
            return packages::load_package_source(id, spec);
        }
        if let Some(cached) = self.sources.lock().unwrap().get(&id).cloned() {
            return cached;
        }
        let loaded = self.file(id).and_then(|bytes| {
            let text = std::str::from_utf8(&bytes).map_err(|_| FileError::InvalidUtf8)?;
            Ok(Source::new(id, text.into()))
        });
        self.sources.lock().unwrap().insert(id, loaded.clone());
        loaded
    }

    fn file(&self, id: FileId) -> FileResult<Bytes> {
        if id == self.fonts_source.id() {
            self.fonts_used.store(true, Ordering::Relaxed);
            return Ok(Bytes::from_string(self.fonts_source.text().to_owned()));
        }
        if let Some(text) = template(id) {
            return Ok(Bytes::from_string(text));
        }
        if let VirtualRoot::Package(spec) = id.root() {
            return packages::load_package_file(spec, id.vpath());
        }
        let mut files = self.files.lock().unwrap();
        files
            .entry(id)
            .or_insert_with(|| self.read_disk(id))
            .clone()
    }

    fn font(&self, index: usize) -> Option<Font> {
        if let Some(layered) = &self.active_extra_fonts {
            layered.font(index)
        } else {
            self.fonts.font(index)
        }
    }

    fn today(&self, offset: Option<TypstDuration>) -> Option<Datetime> {
        current_datetime(offset)
    }
}

/// Computes the current civil date from `SystemTime::now()` shifted by Typst's optional
/// UTC offset (pure std, zero external crates). Required by packages such as
/// `@preview/touying` that evaluate `datetime.today()` at module import time.
pub(crate) fn current_datetime(offset: Option<TypstDuration>) -> Option<Datetime> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs() as i64;
    let shift_secs = offset.map(|d| d.seconds().round() as i64).unwrap_or(0);
    let secs = now.saturating_add(shift_secs);
    let days = secs.div_euclid(86_400);
    // Howard Hinnant's civil_from_days algorithm (valid for all proleptic Gregorian dates).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = (yoe as i64) + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u8;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u8;
    let year = (y + i64::from(m <= 2)) as i32;
    Datetime::from_ymd(year, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use typst::text::{Coverage, FontFlags, FontInfo, FontVariant};

    fn fake_book(families: &[&str]) -> FontBook {
        FontBook::from_infos(families.iter().map(|family| FontInfo {
            family: (*family).to_string(),
            variant: FontVariant::new(Default::default(), Default::default(), Default::default()),
            flags: FontFlags::empty(),
            axes: Vec::new(),
            coverage: Coverage::from_vec(Vec::new()),
        }))
    }

    #[test]
    fn from_env_uses_env_dirs_without_system_fonts() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("fonts")).unwrap();
        let a = dir.path().join("a");
        let b = dir.path().join("b");
        let joined = std::env::join_paths([&a, &b]).unwrap();
        let opts = FontOptions::from_env_with(Some(&joined), Some(dir.path()));
        assert_eq!(opts.dirs, vec![a, b]);
        assert!(!opts.system);
    }

    #[test]
    fn from_env_falls_back_to_sibling_fonts_and_system() {
        let dir = tempfile::tempdir().unwrap();
        let without = FontOptions::from_env_with(None, Some(dir.path()));
        assert!(without.dirs.is_empty());
        assert!(without.system);

        let sibling = dir.path().join("fonts");
        std::fs::create_dir(&sibling).unwrap();
        let from_empty_env = FontOptions::from_env_with(Some(OsStr::new("")), Some(dir.path()));
        assert_eq!(from_empty_env.dirs, vec![sibling]);
        assert!(from_empty_env.system);
    }

    #[test]
    fn from_env_discovers_macos_bundle_resources_fonts() {
        let dir = tempfile::tempdir().unwrap();
        let macos_dir = dir.path().join("App.app/Contents/MacOS");
        let res_fonts = dir.path().join("App.app/Contents/Resources/fonts");
        std::fs::create_dir_all(&macos_dir).unwrap();
        std::fs::create_dir_all(&res_fonts).unwrap();

        let opts = FontOptions::from_env_with(None, Some(&macos_dir));
        assert_eq!(opts.dirs, vec![res_fonts]);
        assert!(opts.system);
    }

    #[test]
    fn resolve_cjk_fonts_exports_latin_modern_10pt_for_2004_and_2006() {
        let book_2004 = fake_book(&["Latin Modern Roman", "Latin Modern Sans", "Songti SC"]);
        let (src_2004, _) = resolve_cjk_fonts(&book_2004);
        assert!(src_2004.contains("#let lm-roman-10 = \"Latin Modern Roman\""));
        assert!(src_2004.contains("#let lm-sans-10 = \"Latin Modern Sans\""));

        let book_2006 = fake_book(&["Latin Modern Roman 10", "Latin Modern Sans 10", "Songti SC"]);
        let (src_2006, _) = resolve_cjk_fonts(&book_2006);
        assert!(src_2006.contains("#let lm-roman-10 = \"Latin Modern Roman 10\""));
        assert!(src_2006.contains("#let lm-sans-10 = \"Latin Modern Sans 10\""));
    }

    #[test]
    fn resolve_cjk_fonts_prefers_foundertype_and_falls_back_cleanly() {
        let founder = fake_book(&[
            "FZShuSong-Z01",
            "FZHei-B01",
            "FZKai-Z03",
            "FZFangSong-Z02",
            "SimSun",
        ]);
        let (src, warn) = resolve_cjk_fonts(&founder);
        assert!(warn.is_none());
        assert!(src.contains(r#"#let cjk-song = "FZShuSong-Z01""#));
        assert!(src.contains(r#"#let cjk-hei = "FZHei-B01""#));
        assert!(src.contains(r#"#let cjk-kai = "FZKai-Z03""#));
        assert!(src.contains(r#"#let cjk-fangsong = "FZFangSong-Z02""#));

        let windows = fake_book(&["SimSun", "SimHei", "KaiTi", "FangSong"]);
        let (src, warn) = resolve_cjk_fonts(&windows);
        let warn = warn.expect("fallback warning expected");
        assert!(warn.contains("FZShuSong-Z01 -> SimSun"));
        assert!(warn.contains("FZHei-B01 -> SimHei"));
        assert!(warn.contains("FZKai-Z03 -> KaiTi"));
        assert!(warn.contains("FZFangSong-Z02 -> FangSong"));
        assert!(src.contains(r#"#let cjk-song = "SimSun""#));
        assert!(src.contains(r#"#let cjk-hei = "SimHei""#));
        assert!(src.contains(r#"#let cjk-kai = "KaiTi""#));
        assert!(src.contains(r#"#let cjk-fangsong = "FangSong""#));

        let source_han = fake_book(&["Source Han Serif SC", "Source Han Sans SC"]);
        let (src, warn) = resolve_cjk_fonts(&source_han);
        assert!(warn.is_some());
        assert!(src.contains(r#"#let cjk-song = "Source Han Serif SC""#));
        assert!(src.contains(r#"#let cjk-hei = "Source Han Sans SC""#));
        assert!(src.contains(r#"#let cjk-kai = "Source Han Serif SC""#));
        assert!(src.contains(r#"#let cjk-fangsong = "Source Han Serif SC""#));

        let empty = fake_book(&[]);
        let (src, warn) = resolve_cjk_fonts(&empty);
        assert!(warn.is_none());
        assert!(src.contains(r#"#let cjk-song = "FZShuSong-Z01""#));
    }

    fn seeded_presets_dir() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let tpl_dir = dir.path().join("templates");
        std::fs::create_dir_all(&tpl_dir).unwrap();
        for p in PRESETS {
            std::fs::write(tpl_dir.join(format!("{}.typ", p.id)), p.source).unwrap();
        }
        dir
    }

    #[test]
    fn fallback_warning_only_emits_when_olymp_fonts_are_imported() {
        let dir = seeded_presets_dir();
        let mut engine = Engine::new(&FontOptions::from_env(), Some(dir.path().to_path_buf()));
        engine.font_fallback_warning =
            Some("FounderType CJK font(s) missing; using fallback: FZShuSong-Z01 -> SimSun".into());
        engine.set_main("#set page(width: 100pt, height: 100pt)\nHello");
        let plain = engine.compile().unwrap();
        assert!(
            !plain
                .warnings
                .iter()
                .any(|w| w.message.contains("using fallback")),
            "unexpected fallback warning without olymp-classic: {:?}",
            plain.warnings
        );

        engine.set_main(
            r#"#import "/xcpc/defaults.typ": section, samples, subsection, runin
#import "/templates/olymp-classic.typ" as __xcpc_tpl
#import "/templates/olymp-classic.typ": *
#__xcpc_tpl.render((
  name: "Test",
  language: "chinese",
  book: "statements",
  single: true,
  options: (:),
  problems: ((id: "a", label: "A", title: "Title", body: [测试]),),
))
"#,
        );
        let with_template = engine.compile().unwrap();
        assert!(
            with_template
                .warnings
                .iter()
                .any(|w| w.message.contains("using fallback")),
            "expected fallback warning when olymp-classic is used: {:?}",
            with_template.warnings
        );
        assert!(
            !with_template
                .warnings
                .iter()
                .any(|w| w.message.contains("unknown font family")),
            "unexpected unknown font family noise: {:?}",
            with_template.warnings
        );
    }

    #[test]
    fn olymp_classic_compiles_with_env_fonts_without_fallback_warning() {
        let dir = seeded_presets_dir();
        let mut engine = Engine::new(&FontOptions::from_env(), Some(dir.path().to_path_buf()));
        let has_foundertype = engine.font_fallback_warning.is_none();
        engine.set_main(
            r#"#import "/xcpc/defaults.typ": section, samples, subsection, runin
#import "/templates/olymp-classic.typ" as __xcpc_tpl
#import "/templates/olymp-classic.typ": *
#__xcpc_tpl.render((
  name: "Test",
  language: "chinese",
  book: "statements",
  single: true,
  options: (:),
  problems: ((id: "a", label: "A", title: "Title", body: [中文 *加粗* _斜体_ `代码`]),),
))
"#,
        );
        let compiled = engine.compile().unwrap();
        if has_foundertype {
            assert!(
                compiled.warnings.is_empty(),
                "unexpected warnings: {:?}",
                compiled.warnings
            );
        } else {
            let unexpected: Vec<_> = compiled
                .warnings
                .iter()
                .filter(|w| !w.message.starts_with("FounderType CJK font(s) missing;"))
                .collect();
            assert!(
                unexpected.is_empty(),
                "unexpected non-fallback warnings: {unexpected:?}"
            );
        }
    }

    #[test]
    fn bundled_preview_packages_compile_offline() {
        let dir = seeded_presets_dir();
        let mut engine = Engine::new(&FontOptions::from_env(), Some(dir.path().to_path_buf()));
        let has_foundertype = engine.font_fallback_warning.is_none();
        engine.set_main(
            r#"#import "/xcpc/defaults.typ": section, samples, subsection, runin
#import "/templates/olymp-classic.typ" as __xcpc_tpl
#import "/templates/olymp-classic.typ": *
#import "@preview/cetz:0.5.3"
#import "@preview/pinit:0.2.2": *
#import "@preview/mannot:0.4.0": annot, markul
#__xcpc_tpl.render((
  name: "Package Test",
  language: "chinese",
  book: "statements",
  single: true,
  options: (:),
  problems: ((id: "a", label: "A", title: "Demo", body: [
#cetz.canvas({
  import cetz.draw: *
  circle((0, 0), radius: 0.6, stroke: blue)
  line((0.6, 0), (2, 0), mark: (end: ">"))
})

$ #pin(1) a_i #pin(2) + k $
#pinit-highlight(1, 2)

$ r = markul(|G| - 1, #<len>) #annot(<len>, pos: right, dx: 1em)[len] $
]),),
))
"#,
        );
        let compiled = engine.compile().unwrap();
        assert_eq!(compiled.page_count(), 1);
        if has_foundertype {
            assert!(
                compiled.warnings.is_empty(),
                "unexpected warnings: {:?}",
                compiled.warnings
            );
        }
    }

    #[test]
    fn bundled_touying_and_pinit_compile_offline() {
        let dir = seeded_presets_dir();
        let mut engine = Engine::new(&FontOptions::from_env(), Some(dir.path().to_path_buf()));
        engine.set_main(
            r#"#import "/xcpc/fonts.typ": lm-sans-10
#import "@preview/touying:0.8.0": *
#import "@preview/pinit:0.2.2": *
#import themes.simple: *

#set text(font: lm-sans-10)
#show math.equation: set text(font: "Latin Modern Math")
#show: simple-theme.with(aspect-ratio: "16-9")

= A. Demo

== Slide One

$ #pin(1) a + b #pin(2) $
#pause
#pinit-highlight(1, 2)
"#,
        );
        let compiled = engine.compile().unwrap();
        assert_eq!(compiled.page_count(), 3);
    }

    #[test]
    fn preset_templates_export_declarative_schemas_and_render_single_entry() {
        let dir = seeded_presets_dir();
        let mut engine = Engine::new(&FontOptions::from_env(), Some(dir.path().to_path_buf()));

        let olymp = engine.inspect_template_schema("/templates/olymp-classic.typ");
        assert_eq!(
            olymp.name.as_ref().map(|t| t.resolve("zh-CN")),
            Some("XCPC 经典")
        );
        assert_eq!(
            olymp.name.as_ref().map(|t| t.resolve("en")),
            Some("XCPC Classic")
        );
        assert_eq!(
            olymp.description.as_ref().map(|t| t.resolve("zh-CN")),
            Some("对齐 olymp.sty 的经典 ICPC / XCPC 版式")
        );
        assert_eq!(
            olymp.description.as_ref().map(|t| t.resolve("en")),
            Some("Classic ICPC / XCPC layout aligned with olymp.sty")
        );
        assert!(olymp.has_book_options());
        assert!(olymp.has_problem_options());
        assert!(olymp.fields.iter().any(|f| f.key == "cover"));
        assert!(
            olymp.fields.iter().any(|f| {
                f.key == "logo" && f.field_type == xcpc_domain::TemplateFieldType::Image
            })
        );
        assert!(olymp.fields.iter().any(|f| f.key == "blank-pages"));
        assert!(olymp.fields.iter().any(|f| f.key == "sample-layout"));

        let cnoi = engine.inspect_template_schema("/templates/cnoi-classic.typ");
        assert_eq!(
            cnoi.name.as_ref().map(|t| t.resolve("zh-CN")),
            Some("OI 风格")
        );
        assert_eq!(
            cnoi.name.as_ref().map(|t| t.resolve("en")),
            Some("OI Classic")
        );
        assert!(cnoi.has_book_options());
        assert!(cnoi.has_problem_options());
        assert!(cnoi.fields.iter().any(|f| f.key == "cover"));
        assert!(
            cnoi.fields.iter().any(|f| {
                f.key == "logo" && f.field_type == xcpc_domain::TemplateFieldType::Image
            })
        );
        assert!(cnoi.fields.iter().any(|f| f.key == "compile-opts"));
        assert!(cnoi.fields.iter().any(|f| f.key == "notices"));
        assert!(cnoi.fields.iter().any(|f| f.key == "prob-type"));

        let minimal = engine.inspect_template_schema("/templates/minimal.typ");
        assert_eq!(
            minimal.name.as_ref().map(|t| t.resolve("zh-CN")),
            Some("极简空白")
        );
        assert_eq!(
            minimal.name.as_ref().map(|t| t.resolve("en")),
            Some("Minimal Blank")
        );
        assert!(minimal.has_book_options());
        assert!(minimal.fields.iter().any(|f| f.key == "show-cover"));

        // Verify olymp-classic wide sample layout compiles cleanly via render(doc).
        engine.set_main(
            r#"#import "/xcpc/defaults.typ": section, samples, subsection, runin
#import "/templates/olymp-classic.typ" as __xcpc_tpl
#import "/templates/olymp-classic.typ": *
#__xcpc_tpl.render((
  name: "Test",
  language: "chinese",
  book: "statements",
  single: true,
  options: ("body-size": "11pt", "margin-x": "20mm"),
  problems: ((
    id: "a",
    label: "A",
    title: "Wide",
    options: ("sample-layout": "wide"),
    body: [
      正文内容。
      #samples(("1 2\n", "3\n"),)
    ],
  ),),
))
"#,
        );
        let compiled = engine.compile().unwrap();
        assert_eq!(compiled.page_count(), 1);

        // Verify cnoi-classic options compile cleanly via render(doc).
        engine.set_main(
            r##"#import "/xcpc/defaults.typ": section, samples, subsection, runin
#import "/templates/cnoi-classic.typ" as __xcpc_tpl
#import "/templates/cnoi-classic.typ": *
#__xcpc_tpl.render((
  name: "NOI 模拟赛",
  language: "chinese",
  book: "statements",
  single: false,
  options: (
    "cover": "on",
    "compile-opts": "-O2 -std=c++20 -static",
    "sample-border-color": "#1166aa",
    "sample-line-numbers": false,
    "dotted-emphasis": false,
    "notices": ("测试说明 1", "测试说明 2"),
  ),
  problems: ((
    id: "a",
    label: "A",
    title: "交互题",
    options: ("prob-type": "交互型", "source-file": "inter"),
    body: [
      给定 *重点* 内容。
      #samples(("1\n", "2\n"),)
    ],
  ),),
))
"##,
        );
        let compiled_cnoi = engine.compile().unwrap();
        assert!(compiled_cnoi.page_count() >= 2);

        // Verify minimal template uses /xcpc/defaults.typ hooks cleanly.
        engine.set_main(
            r#"#import "/xcpc/defaults.typ": section, samples, subsection, runin
#import "/templates/minimal.typ" as __xcpc_tpl
#import "/templates/minimal.typ": *
#__xcpc_tpl.render((
  name: "Minimal Round",
  date: "2026-11-22",
  language: "chinese",
  book: "statements",
  single: false,
  pdf-title: "Minimal Round",
  pdf-author: none,
  options: ("show-cover": true),
  problems: ((
    id: "a",
    label: "A",
    title: "Hello",
    body: [
      #section("input")
      输入两个整数。
      #samples(("1 2\n", "3\n"),)
    ],
  ),),
))
"#,
        );
        let compiled_min = engine.compile().unwrap();
        assert_eq!(compiled_min.page_count(), 2);
    }
}

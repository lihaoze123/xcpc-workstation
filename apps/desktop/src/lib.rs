//! The desktop shell's Rust side: what `App.tsx` imports from `xcpc-workstation/native`.
//! Their TypeScript declarations are generated into `.nsh/types/xcpc-workstation/native.d.ts`.
//!
//! Only glue lives here: the workspace, index and file watcher are `xcpc-storage`'s,
//! publishing and preflight lint are `xcpc-convert`'s, typesetting is `xcpc-engine`'s.
//!
//! Pages go to the screen as pixel buffers ([`ImageSrc::rgba`]), not encoded files:
//! rasterizing a page takes a fraction of what encoding it as PNG would. A page shows as
//! layers ([`PreviewLayer`]): the whole page at the scale the app asks for, up to
//! `BASE_SCALE`; sharper, the whole page at that scale and over it tiles of 512 pixels at
//! the scale asked for, covering the part in view. Pixels are kept in a 96 MiB LRU cache
//! preferring base layers over tiles, and stale render tasks abort immediately via an
//! `AtomicU64` generation counter.

mod cache;
mod linux_chrome;
mod page_image;
mod plugins;
mod prefs;
mod snapshot;
mod state;
mod theme_config;
mod types;
mod vim_bridge;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use nsh_base as _;
use nsh_core::element::JsError;
use nsh_core::file_dialog::{FileFilter, OpenDialog, SaveDialog};
use nsh_core::prelude::*;
use xcpc_storage::Workspace;

use crate::cache::CancelToken;
use crate::state::State;

pub use crate::page_image::{PageImage, PageImageProps};
pub use crate::theme_config::{ThemeColorEntryJs, ThemeOverridesJs};
pub use crate::types::{
    AboutInfoJs, ActionResult, AddedTestResultJs, AdoptSolutionResultJs, AppPrefsJs,
    AssetReferenceJs, BatchDeleteItemJs, BatchDeleteResultJs, BatchExportRequestJs,
    BatchItemResultJs, BatchMutationResultJs, BatchStartResultJs, BatchTaskEventJs,
    BatchTaskItemJs, BatchTaskSnapshotJs, CleanUnusedAssetsResultJs, ColdStartSnapshotJs,
    ContestDetailJs, ContestItemDetailJs, ContestMetaPatchJs, ContestStatsJs,
    CreateProblemFromUrlResultJs, DetectedCompilerJs, DialogLabelsJs, EditorialListResultJs,
    EditorialSummaryJs, ExportBookStatusJs, ExportOjResultJs, ExportPluginResultJs, FreezeResultJs,
    FuzzyHitJs, GroupSummaryJs, IngestProblemRequestJs, IngestProblemResultJs,
    InstallPluginResultJs, JsonReportResultJs, LimitsJs, ManualLogResultJs, MarkdownSpanJs,
    MathPreviewJs, NormalizeDirtyResultJs, OverridePatchJs, PackageBuildStatusJs,
    PackageCreateResultJs, PackageFileContentJs, PackageInitResultJs, PackageInspectionResultJs,
    PackageWarningJs, PdfFreshnessJs, PickedFolder, PickedPathsJs, PlatformSyncReportJs,
    PluginCredentialFieldJs, PluginRenderPngResultJs, PluginRepoConfigJs, PluginSummaryJs,
    PluginUiSourceResultJs, PluginsOverviewJs, PolygonImportResultJs, PreviewDiagnostic,
    PreviewLayer, PreviewLocationJs, PreviewPage, PreviewRequest, PreviewResult, PreviewStart,
    PreviewView, ProblemAssetJs, ProblemDetailJs, ProblemMetaPatchJs, ProblemSearchQuery,
    ProblemSourceJs, ProblemSummaryJs, RatingBucketJs, RepoCatalogResultJs, RepoPluginEntryJs,
    RepoPluginPermissionJs, ResolvedProblemUrlJs, SamplePairJs, SavePackageFileResultJs,
    SaveReportResultJs, SaveResult, SourceLocationJs, SyncReportJs, TagCountJs, TestCasePreviewJs,
    ToolchainStatusJs, TraceExportResultJs, TraceStatusJs, TrainingSubmissionsQueryJs,
    UnfreezeDiffResultJs, UnfreezeProblemDiffJs, VerifyPluginResultJs, WatchEventJs,
    WorkspaceContest, WorkspaceFacets, WorkspaceFile, WorkspaceInfo, WorkspaceItem,
    WorkspaceProblem,
};
pub use crate::vim_bridge::{VimKeyBridge, VimKeyBridgeEvent, VimKeyBridgeProps};

/// The crate's UI set (`nsh.json` `"rust"`).
#[native_module("xcpc-workstation/native")]
fn native(registry: &mut Registry) {
    registry.class::<Preview>();
    registry.register::<PageImage>();
    registry.register::<VimKeyBridge>();
}

/// The workspace bridge and preview engine: reads and saves files with SHA-256 conflict
/// detection, watches external changes, maintains the in-memory workspace index, exposes
/// Phase 2 semantic operations, compiles with `publish::check` off the UI thread, and
/// rasterizes visible pages for [`PageImage`].
#[class]
pub struct Preview {
    state: Arc<State>,
    /// Generation counter incremented on every `Preview::compile` (before taking `compiler`)
    /// so in-flight tile rasterization aborts as soon as a newer request arrives.
    generation: Arc<AtomicU64>,
}

impl Drop for Preview {
    fn drop(&mut self) {
        self.generation.fetch_add(1, Ordering::AcqRel);
        // Stop the watcher and any running batch task before dropping our `Arc<State>` so
        // any pending `next_change` / `next_batch_event` blocking on `mpsc::Receiver::recv()`
        // wakes up immediately and never leaks.
        self.state.stop_batch_task();
        let old_watcher = self.state.stop_watcher();
        drop(old_watcher);
    }
}

#[class]
impl Preview {
    #[allow(clippy::new_without_default)] // the JS constructor
    /// Opens the workspace named by `XCPC_WORKSPACE`, else the one containing the current
    /// directory, else the most recent workspace recorded in `<dataDir>/prefs.json` (or
    /// legacy `last-workspace`). Pass `process.dataDir` for persistence; `null` disables it.
    pub fn new(data_dir: Option<String>) -> Self {
        linux_chrome::strip_titlebar_icon_once();
        let data_dir = data_dir.map(|passed| {
            std::env::var_os("XCPC_DATA_DIR")
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(passed))
        });
        Preview {
            state: Arc::new(State::new(data_dir)),
            generation: Arc::new(AtomicU64::new(0)),
        }
    }

    /// The workspace of the test fixture `contest-zh`.
    pub fn fixture() -> String {
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/fixtures/olymp-ref/contest-zh/workspace"
        )
        .to_owned()
    }

    /// Returns raw font file bytes for JetBrains Mono (Regular, Medium, Bold, Italic,
    /// BoldItalic) discovered from `XCPC_FONT_DIRS` and the executable's sibling `fonts/`
    /// directory, ready to pass to `registerFont(bytes)`.
    pub fn ui_fonts() -> Vec<Vec<u8>> {
        let mut dirs = Vec::new();
        if let Some(env_dirs) = std::env::var_os("XCPC_FONT_DIRS").filter(|v| !v.is_empty()) {
            dirs.extend(std::env::split_paths(&env_dirs));
        }
        if let Ok(exe) = std::env::current_exe()
            && let Some(parent) = exe.parent()
        {
            let sibling = parent.join("fonts");
            if sibling.is_dir() {
                dirs.push(sibling);
            }
        }
        load_ui_fonts_from(&dirs)
    }

    /// Returns build and runtime diagnostic info for the About view.
    pub fn about_info() -> AboutInfoJs {
        let font_source =
            if let Some(env_dirs) = std::env::var_os("XCPC_FONT_DIRS").filter(|v| !v.is_empty()) {
                let count = std::env::split_paths(&env_dirs).count();
                format!("环境变量 XCPC_FONT_DIRS（{count} 个目录）")
            } else if std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|d| d.join("fonts").is_dir()))
                .unwrap_or(false)
            {
                "应用内置字体目录 + 系统字体".to_owned()
            } else if xcpc_engine::HAS_BUNDLED_FONTS {
                "内嵌字体 + 系统字体".to_owned()
            } else {
                "系统字体".to_owned()
            };
        AboutInfoJs {
            version: env!("CARGO_PKG_VERSION").to_owned(),
            commit: env!("XCPC_GIT_COMMIT").to_owned(),
            build_date: env!("XCPC_BUILD_DATE").to_owned(),
            is_dev: cfg!(debug_assertions),
            os: std::env::consts::OS.to_owned(),
            arch: std::env::consts::ARCH.to_owned(),
            font_source,
        }
    }

    /// Returns current offline performance trace status and Rust monotonic clock reading.
    pub fn trace_status() -> TraceStatusJs {
        let s = xcpc_trace::status();
        TraceStatusJs {
            enabled: s.enabled,
            event_count: s.event_count as u32,
            dropped_count: s.dropped_count as u32,
            rust_now_us: s.rust_now_us,
        }
    }

    /// Starts (or restarts) an offline performance trace recording session.
    pub fn trace_start() -> TraceStatusJs {
        let s = xcpc_trace::start();
        TraceStatusJs {
            enabled: s.enabled,
            event_count: s.event_count as u32,
            dropped_count: s.dropped_count as u32,
            rust_now_us: s.rust_now_us,
        }
    }

    /// Stops offline performance trace recording while keeping buffered events for export.
    pub fn trace_stop() -> TraceStatusJs {
        let s = xcpc_trace::stop();
        TraceStatusJs {
            enabled: s.enabled,
            event_count: s.event_count as u32,
            dropped_count: s.dropped_count as u32,
            rust_now_us: s.rust_now_us,
        }
    }

    /// Prompts for a save location and writes a merged Rust + JS Chrome Trace JSON file
    /// (compatible with Perfetto UI and `chrome://tracing`).
    pub async fn trace_export_dialog(
        js_events_json: String,
        js_clock_offset_us: f64,
        title: String,
        button: String,
    ) -> TraceExportResultJs {
        let js_events: Vec<xcpc_trace::JsTraceEvent> =
            serde_json::from_str(&js_events_json).unwrap_or_default();
        let dialog = SaveDialog::new()
            .title(title)
            .button_label(button)
            .file_name("xcpc-trace.json")
            .filter(FileFilter::new("Chrome Trace JSON", ["json"]));
        let picked = match pick_save_path(dialog).await {
            Ok(Some(path)) => path,
            Ok(None) => {
                return TraceExportResultJs {
                    ok: false,
                    cancelled: true,
                    path: None,
                    bytes: 0,
                    error: None,
                };
            }
            Err(err) => {
                return TraceExportResultJs {
                    ok: false,
                    cancelled: false,
                    path: None,
                    bytes: 0,
                    error: Some(err),
                };
            }
        };
        match xcpc_trace::export_to_file(&picked, &js_events, js_clock_offset_us) {
            Ok(bytes) => TraceExportResultJs {
                ok: true,
                cancelled: false,
                path: Some(picked.display().to_string()),
                bytes: bytes as u32,
                error: None,
            },
            Err(err) => TraceExportResultJs {
                ok: false,
                cancelled: false,
                path: None,
                bytes: 0,
                error: Some(err.to_string()),
            },
        }
    }

    // ---- Workspace & settings ----------------------------------------------------------

    /// Opens the workspace at `path` instead.
    pub fn open_workspace(&self, path: String, read_only: bool) -> WorkspaceInfo {
        match Workspace::open(&path, read_only) {
            Ok(ws) => self.switch(ws),
            Err(e) => WorkspaceInfo {
                error: Some(e.human_message()),
                ..Default::default()
            },
        }
    }

    /// Initializes a new XCPC workspace at `path` (creating `workspace.toml`, `problems/`,
    /// `contests/`, `.gitignore`, and a static `AGENTS.md` if absent) and switches to it.
    pub fn init_workspace(&self, path: String, template: Option<String>) -> WorkspaceInfo {
        match xcpc_storage::init_workspace_with_template(&path, template.as_deref()) {
            Ok(ws) => self.switch(ws),
            Err(e) => WorkspaceInfo {
                error: Some(e.human_message()),
                ..self.workspace()
            },
        }
    }

    /// Initializes the built-in example workspace at `dir` (3 curated problems + 2 contests)
    /// in the requested language (`"en"` / `"english"` or `"zh"` / `"chinese"`, default Chinese)
    /// and switches to it.
    pub fn init_example_workspace(
        &self,
        dir: String,
        template: Option<String>,
        language: Option<String>,
    ) -> WorkspaceInfo {
        let lang = parse_example_language(language.as_deref());
        match xcpc_storage::init_example_workspace_with_options(&dir, template.as_deref(), lang) {
            Ok(ws) => self.switch(ws),
            Err(e) => WorkspaceInfo {
                error: Some(e.human_message()),
                ..self.workspace()
            },
        }
    }

    /// Opens the system folder picker, releases the built-in example workspace into the
    /// chosen folder, switches to it, and returns the new workspace info (or leaves the
    /// current workspace unchanged when cancelled).
    pub async fn pick_and_init_example_workspace(
        this: Instance<Self>,
        template: Option<String>,
        labels: Option<DialogLabelsJs>,
        language: Option<String>,
    ) -> WorkspaceInfo {
        if this.is_disposed() {
            return WorkspaceInfo::default();
        }
        let start = this.with(|p| p.state.workspace_root());
        let title = label_or(
            labels.as_ref().and_then(|l| l.title.as_deref()),
            "选择示例工作区存放目录",
        );
        let button = label_or(
            labels.as_ref().and_then(|l| l.button_label.as_deref()),
            "创建示例",
        );
        let mut dialog = OpenDialog::folders().title(title).button_label(button);
        if let Some(dir) = start.as_ref().and_then(|r| r.parent()) {
            dialog = dialog.directory(dir);
        }
        let picked = pick_paths(dialog).await;
        if this.is_disposed() {
            return WorkspaceInfo::default();
        }
        let lang = parse_example_language(language.as_deref());
        this.with(|p| {
            let dir = match picked {
                Ok(Some(paths)) => paths.into_iter().next(),
                Ok(None) => None,
                Err(e) => {
                    return WorkspaceInfo {
                        error: Some(e),
                        ..p.workspace()
                    };
                }
            };
            let Some(dir) = dir else {
                return p.workspace();
            };
            let target_dir = resolve_example_target_dir(&dir);
            match xcpc_storage::init_example_workspace_with_options(
                &target_dir,
                template.as_deref(),
                lang,
            ) {
                Ok(ws) => p.switch(ws),
                Err(e) => WorkspaceInfo {
                    error: Some(e.human_message()),
                    ..p.workspace()
                },
            }
        })
    }

    /// Opens the system folder picker and reports whether the selected directory belongs to
    /// an existing workspace (`isWorkspace`, `workspaceRoot`), allowing the GUI to offer
    /// initializing a new workspace when `isWorkspace` is false.
    pub async fn pick_folder(this: Instance<Self>, labels: Option<DialogLabelsJs>) -> PickedFolder {
        if this.is_disposed() {
            return PickedFolder {
                cancelled: true,
                ..Default::default()
            };
        }
        let start = this.with(|p| p.state.workspace_root());
        let title = label_or(
            labels.as_ref().and_then(|l| l.title.as_deref()),
            "选择工作区目录",
        );
        let button = label_or(
            labels.as_ref().and_then(|l| l.button_label.as_deref()),
            "选择",
        );
        let mut dialog = OpenDialog::folders().title(title).button_label(button);
        if let Some(dir) = start.as_ref().and_then(|r| r.parent()) {
            dialog = dialog.directory(dir);
        }
        match pick_paths(dialog).await {
            Ok(Some(paths)) => {
                let Some(dir) = paths.into_iter().next() else {
                    return PickedFolder {
                        cancelled: true,
                        ..Default::default()
                    };
                };
                let path = dir.display().to_string();
                match Workspace::discover(&dir, false) {
                    Ok(ws) => PickedFolder {
                        cancelled: false,
                        path,
                        workspace_root: Some(ws.root().display().to_string()),
                        is_workspace: true,
                        error: None,
                    },
                    Err(_) => PickedFolder {
                        cancelled: false,
                        path,
                        workspace_root: None,
                        is_workspace: false,
                        error: None,
                    },
                }
            }
            Ok(None) => PickedFolder {
                cancelled: true,
                ..Default::default()
            },
            Err(e) => PickedFolder {
                cancelled: false,
                error: Some(e),
                ..Default::default()
            },
        }
    }

    /// Asks for a folder with the system's dialog and opens the workspace it is in
    /// (it, or the nearest folder above it with a `workspace.toml`). Cancelled, the
    /// workspace stays as it was; a folder in no workspace leaves it too, with the
    /// error said.
    pub async fn pick_workspace(
        this: Instance<Self>,
        labels: Option<DialogLabelsJs>,
    ) -> WorkspaceInfo {
        if this.is_disposed() {
            return WorkspaceInfo::default();
        }
        let start = this.with(|p| p.state.workspace_root());
        let title = label_or(
            labels.as_ref().and_then(|l| l.title.as_deref()),
            "打开工作区",
        );
        let button = label_or(
            labels.as_ref().and_then(|l| l.button_label.as_deref()),
            "打开",
        );
        let mut dialog = OpenDialog::folders().title(title).button_label(button);
        if let Some(dir) = start.as_ref().and_then(|r| r.parent()) {
            dialog = dialog.directory(dir);
        }
        let picked = pick_paths(dialog).await;
        if this.is_disposed() {
            return WorkspaceInfo::default();
        }
        this.with(|p| {
            let dir = match picked {
                Ok(Some(paths)) => paths.into_iter().next(),
                Ok(None) => None,
                Err(e) => {
                    return WorkspaceInfo {
                        error: Some(e),
                        ..p.workspace()
                    };
                }
            };
            let Some(dir) = dir else {
                return p.workspace();
            };
            match Workspace::discover(&dir, false) {
                Ok(ws) => p.switch(ws),
                Err(_) => WorkspaceInfo {
                    error: Some(format!(
                        "{} 不在工作区里（没有 workspace.toml）",
                        dir.display()
                    )),
                    ..p.workspace()
                },
            }
        })
    }

    /// What the workspace has.
    pub fn workspace(&self) -> WorkspaceInfo {
        self.state.info()
    }

    /// Recently opened workspace directories, most recent first.
    pub fn recent_workspaces(&self) -> Vec<String> {
        self.state.recent_workspaces()
    }

    /// Generates the ready-to-paste JSON snippet for configuring `xcpc mcp` against the
    /// currently open workspace.
    pub fn mcp_config_snippet(&self) -> String {
        self.state.mcp_config_snippet()
    }

    // ---- File read / save & live watcher -----------------------------------------------

    /// Reads a file of the workspace for the editor together with its SHA-256 content hash
    /// (empty string when the file does not exist yet).
    pub fn read(&self, path: String) -> WorkspaceFile {
        self.state.read(path)
    }

    /// Atomically saves a workspace file, checking `expectedHash` when provided.
    ///
    /// Returns `{ conflict: true }` without overwriting when the file changed on disk since
    /// `expectedHash`. Successful writes are recorded in `SelfWriteTracker` so they are not
    /// echoed back by `nextChange()`, incrementally refresh the index, and preserve the
    /// compiled preview cache when the saved Markdown matches the already-compiled state.
    pub fn save(&self, path: String, text: String, expected_hash: Option<String>) -> SaveResult {
        self.state.save(path, text, expected_hash)
    }

    /// Long-polling async notification of external filesystem changes inside the active
    /// workspace. Blocks off the UI thread with zero polling until an external change
    /// arrives or the watcher stops (workspace switch or `Preview` disposal).
    ///
    /// When `"changed"` or `"rescan"` resolves, the in-memory `WorkspaceIndex` and compiled
    /// preview cache have already been updated. When `"closed"` resolves, callers should stop
    /// the watch loop (and start a fresh one after switching workspaces).
    pub async fn next_change(this: Instance<Self>) -> WatchEventJs {
        if this.is_disposed() {
            return WatchEventJs {
                kind: "closed".into(),
                paths: Vec::new(),
            };
        }
        let (weak_state, session) = this.with(|p| {
            let session = p.state.watch_session();
            (Arc::downgrade(&p.state), session)
        });
        let Some(session) = session else {
            return WatchEventJs {
                kind: "closed".into(),
                paths: Vec::new(),
            };
        };
        let session_id = session.id;
        let rx = session.rx;
        blocking(move || {
            let event = {
                let guard = match rx.lock() {
                    Ok(g) => g,
                    Err(_) => {
                        return WatchEventJs {
                            kind: "closed".into(),
                            paths: Vec::new(),
                        };
                    }
                };
                match guard.recv() {
                    Ok(ev) => ev,
                    Err(_) => {
                        return WatchEventJs {
                            kind: "closed".into(),
                            paths: Vec::new(),
                        };
                    }
                }
            };
            let Some(state) = weak_state.upgrade() else {
                return WatchEventJs {
                    kind: "closed".into(),
                    paths: Vec::new(),
                };
            };
            state.apply_watch_event(session_id, event)
        })
        .await
        .unwrap_or_else(|_| WatchEventJs {
            kind: "closed".into(),
            paths: Vec::new(),
        })
    }

    // ---- Preview compile, bidirectional locate & export --------------------------------

    /// Compiles `request` via `publish::check` (including `xcpc-lint` diagnostics) and
    /// rasterizes the pages in `view` for `scale` pixels per point. View/scale-only changes
    /// reuse the cached compilation with zero recompile cost.
    pub async fn compile(
        this: Instance<Self>,
        request: PreviewRequest,
        scale: f64,
        view: Vec<PreviewView>,
    ) -> PreviewResult {
        if this.is_disposed() {
            return PreviewResult::default();
        }
        let (state, generation) = this.with(|p| (p.state.clone(), p.generation.clone()));
        let my_gen = generation.fetch_add(1, Ordering::AcqRel).wrapping_add(1);
        blocking(move || {
            let cancel = CancelToken {
                generation: my_gen,
                latest: &generation,
            };
            if cancel.is_cancelled() {
                return PreviewResult::default();
            }
            state.run(&request, scale as f32, &view, cancel)
        })
        .await
        .unwrap_or_else(|e| PreviewResult {
            error: Some(format!("preview task failed: {e}")),
            ..Default::default()
        })
    }

    /// Speculatively pre-warms adjacent pages at base scale into the pixel cache.
    /// Does not increment `generation` (never cancels an active compile) and yields
    /// immediately whenever any new compile supersedes `expected_gen`.
    pub async fn prewarm_neighbors(this: Instance<Self>, scale: f64, pages: Vec<u32>) -> u32 {
        if this.is_disposed() || pages.is_empty() {
            return 0;
        }
        let (state, generation) = this.with(|p| (p.state.clone(), p.generation.clone()));
        let expected_gen = generation.load(Ordering::Acquire);
        blocking(move || {
            let cancel = CancelToken {
                generation: expected_gen,
                latest: &generation,
            };
            state.prewarm_neighbors(scale as f32, &pages, cancel)
        })
        .await
        .unwrap_or(0)
    }

    /// Compiles and rasterizes a single LaTeX math expression off the UI thread for the
    /// editor's floating mini math preview (`PageImage` reads its pixels by `key`).
    pub async fn render_math_preview(
        this: Instance<Self>,
        latex: String,
        display: bool,
        dark: bool,
        scale: f64,
    ) -> MathPreviewJs {
        if this.is_disposed() {
            return MathPreviewJs::default();
        }
        let state = this.with(|p| p.state.clone());
        blocking(move || state.render_math_preview(&latex, display, dark, scale as f32))
            .await
            .unwrap_or_default()
    }

    /// Maps a click at `(xPt, yPt)` (typographic points from the top-left of 0-based `page`)
    /// on the currently cached preview back to its 1-based Markdown source location.
    /// Returns `null` immediately without blocking if a compilation is in flight.
    pub fn locate_click(&self, page: u32, x_pt: f64, y_pt: f64) -> Option<SourceLocationJs> {
        self.state.locate_click(page, x_pt, y_pt)
    }

    /// Maps a 1-based `line` in workspace-relative `file` (e.g. `"problems/a/statement.md"`)
    /// to its position `(page, yPt)` in the currently cached preview. Never blocks on an
    /// active compilation; falls back to the problem's first page when exact anchors are
    /// unavailable.
    pub fn locate_source(&self, file: String, line: u32) -> Option<PreviewLocationJs> {
        self.state.locate_source(&file, line)
    }

    /// Where `text`'s Markdown structure lies (headings, emphasis, math, code, markup), as
    /// the conversion reads it, for the editor to style its source.
    pub fn markdown_spans(text: String) -> Vec<MarkdownSpanJs> {
        use xcpc_convert::SpanKind as K;
        // Byte offset → (line, UTF-16 column), walking the text once in span order.
        let mut starts = vec![0usize];
        starts.extend(text.match_indices('\n').map(|(i, _)| i + 1));
        let at = |offset: usize| -> (u32, u32) {
            let line = starts.partition_point(|&s| s <= offset) - 1;
            let column = text[starts[line]..offset].encode_utf16().count();
            (line as u32, column as u32)
        };
        xcpc_convert::markdown_spans(&text)
            .into_iter()
            .map(|s| {
                let (start_line, start_column) = at(s.range.start);
                let (end_line, end_column) = at(s.range.end);
                let kind = match s.kind {
                    K::Heading => "heading",
                    K::Strong => "strong",
                    K::Emphasis => "emphasis",
                    K::Strikethrough => "strikethrough",
                    K::InlineMath => "inlineMath",
                    K::DisplayMath => "displayMath",
                    K::MathCommand => "mathCommand",
                    K::MathOperator => "mathOperator",
                    K::Code => "code",
                    K::CodeBlock => "codeBlock",
                    K::CodeComment => "codeComment",
                    K::CodeString => "codeString",
                    K::CodeKeyword => "codeKeyword",
                    K::CodeNumber => "codeNumber",
                    K::Link => "link",
                    K::Url => "url",
                    K::Table => "table",
                    K::Markup => "markup",
                };
                MarkdownSpanJs {
                    kind: kind.into(),
                    level: s.level.into(),
                    start_line,
                    start_column,
                    end_line,
                    end_column,
                }
            })
            .collect()
    }

    /// Exports `request`'s PDF to `build/` in the workspace: its path, or why not.
    pub async fn export(this: Instance<Self>, request: PreviewRequest) -> WorkspaceFile {
        if this.is_disposed() {
            return WorkspaceFile {
                error: Some("preview disposed".into()),
                ..Default::default()
            };
        }
        let state = this.with(|p| p.state.clone());
        blocking(move || state.export(&request))
            .await
            .unwrap_or_else(|e| WorkspaceFile {
                error: Some(format!("export failed: {e}")),
                ..Default::default()
            })
    }

    /// Reports independent PDF freshness (`exists`, `fresh`, `relPath`, `absPath`) for both
    /// the statements book and the editorial book of `request`'s target.
    pub fn export_status(&self, request: PreviewRequest) -> ExportBookStatusJs {
        self.state.export_status(&request)
    }

    /// Reveals a file or directory (absolute or workspace-relative path) inside the open
    /// workspace in the system file manager. Rejects paths outside the workspace or missing
    /// targets.
    pub async fn reveal_in_folder(this: Instance<Self>, path: String) -> ActionResult {
        if this.is_disposed() {
            return ActionResult::err("disposed", "preview disposed");
        }
        let resolved = this.with(|p| p.state.resolve_reveal_path(&path));
        let abs = match resolved {
            Ok(abs) => abs,
            Err(err) => return err,
        };
        match reveal_path(abs).await {
            Ok(()) => ActionResult::ok(),
            Err(e) => ActionResult::err("io", e),
        }
    }

    /// Loads user-global theme overrides from `<dataDir>/theme.toml`.
    pub fn load_theme_overrides(&self) -> ThemeOverridesJs {
        theme_config::load_theme_overrides(self.state.data_dir.as_deref())
    }

    /// Detects the operating system UI locale and normalizes it to `"zh-CN"` or `"en"`.
    pub fn system_locale(&self) -> String {
        detect_system_locale().to_owned()
    }

    /// Ensures `<dataDir>/theme.toml` exists and reveals it in the system file manager.
    pub async fn open_theme_file(this: Instance<Self>) -> ActionResult {
        if this.is_disposed() {
            return ActionResult::err("disposed", "preview disposed");
        }
        let ensured = this.with(|p| theme_config::ensure_theme_file(p.state.data_dir.as_deref()));
        let path = match ensured {
            Ok(p) => p,
            Err(e) => return ActionResult::err("io", e),
        };
        match reveal_path(path).await {
            Ok(()) => ActionResult::ok(),
            Err(e) => ActionResult::err("io", e),
        }
    }

    // ---- Problem library & editing -----------------------------------------------------

    /// Searches indexed problems using structured filters and normalized text matching.
    pub fn search_problems(&self, query: ProblemSearchQuery) -> Vec<ProblemSummaryJs> {
        self.state.search_problems(query)
    }

    /// Returns sorted distinct tags and external platforms across all indexed problems.
    pub fn workspace_facets(&self) -> WorkspaceFacets {
        self.state.workspace_facets()
    }

    /// Loads full problem details: metadata, statements & editorial with hashes, samples,
    /// solution filenames, and referencing contests.
    pub fn problem_detail(&self, id: String) -> ProblemDetailJs {
        self.state.problem_detail(&id)
    }

    /// Reads one solution file under `problems/<id>/solutions/<name>` read-only.
    pub fn read_solution(&self, id: String, name: String) -> WorkspaceFile {
        self.state.read_solution(&id, &name)
    }

    /// Assembles a standalone Markdown statement with `samples/` embedded before `## 提示` / `## Note`.
    pub fn export_markdown_with_samples(
        &self,
        id: String,
        unsaved_statement: Option<String>,
    ) -> Result<String, JsError> {
        self.state
            .export_markdown_with_samples(&id, unsaved_statement.as_deref())
            .map_err(JsError::Error)
    }

    /// Creates a new problem directory with `problem.toml`, a `statement.md` skeleton, and
    /// an empty `samples/` directory, refreshing the in-memory index.
    pub fn create_problem(&self, id: String, title: String) -> ActionResult {
        self.state.create_problem(&id, &title)
    }

    /// Recursively duplicates `problems/<sourceId>/` into `problems/<newId>/`, optionally
    /// updating `title` in `problem.toml`, and refreshes the in-memory index.
    pub fn duplicate_problem(
        &self,
        source_id: String,
        new_id: String,
        title: Option<String>,
    ) -> ActionResult {
        self.state
            .duplicate_problem(&source_id, &new_id, title.as_deref())
    }

    /// Partially updates `problems/<id>/problem.toml` via `toml_edit`, preserving comments,
    /// key order, and unknown fields.
    pub fn update_problem_meta(&self, id: String, patch: ProblemMetaPatchJs) -> ActionResult {
        self.state.update_problem_meta(&id, patch)
    }

    /// Deletes a problem from the library. Refuses with `code: "problem_referenced"` and
    /// `referencingContests` when any unfrozen contest references it.
    pub fn delete_problem(&self, id: String) -> ActionResult {
        self.state.delete_problem(&id)
    }

    // ---- Persistent problem groups & batch operations ----------------------------------

    /// Creates a new persistent problem group in `workspace.toml`.
    pub fn create_group(&self, name: String) -> ActionResult {
        self.state.create_group(&name)
    }

    /// Renames a persistent problem group in `workspace.toml` and all member `problem.toml` files.
    pub fn rename_group(&self, old_name: String, new_name: String) -> ActionResult {
        self.state.rename_group(&old_name, &new_name)
    }

    /// Deletes a persistent problem group from `workspace.toml` and ungroups member problems
    /// without deleting any problems.
    pub fn delete_group(&self, name: String) -> ActionResult {
        self.state.delete_group(&name)
    }

    /// Reorders declared persistent problem groups in `workspace.toml`.
    pub fn reorder_groups(&self, names: Vec<String>) -> ActionResult {
        self.state.reorder_groups(&names)
    }

    /// Sets or clears the persistent `group` on multiple problems in one batch.
    pub fn batch_set_group(
        &self,
        ids: Vec<String>,
        group: Option<String>,
    ) -> BatchMutationResultJs {
        self.state.batch_set_group(&ids, group.as_deref())
    }

    /// Adds and/or removes tags across multiple problems in one batch.
    pub fn batch_edit_tags(
        &self,
        ids: Vec<String>,
        add: Vec<String>,
        remove: Vec<String>,
    ) -> BatchMutationResultJs {
        self.state.batch_edit_tags(&ids, &add, &remove)
    }

    /// Sets or clears `rating` across multiple problems in one batch.
    pub fn batch_set_rating(&self, ids: Vec<String>, rating: Option<u32>) -> BatchMutationResultJs {
        self.state.batch_set_rating(&ids, rating)
    }

    /// Atomically adds multiple problems to an existing contest, refusing if frozen/archived
    /// or if the resulting total exceeds 26 problems.
    pub fn batch_add_to_contest(
        &self,
        contest_id: String,
        problem_ids: Vec<String>,
    ) -> BatchMutationResultJs {
        self.state.batch_add_to_contest(&contest_id, &problem_ids)
    }

    /// Batch-deletes multiple problems, skipping any referenced by unfrozen contests with
    /// `code: "problem_referenced"` and `referencingContests`.
    pub fn batch_delete_problems(&self, ids: Vec<String>) -> BatchDeleteResultJs {
        self.state.batch_delete_problems(&ids)
    }

    /// Starts a strictly sequential background batch package build/verification task
    /// (`mode`: `"verify"` or `"debug"`). Stream progress via `nextBatchEvent(taskId)`.
    /// With `contest_id`, each problem is checked under that contest's overridden limits.
    pub fn start_batch_build(
        &self,
        ids: Vec<String>,
        mode: String,
        contest_id: Option<String>,
    ) -> BatchStartResultJs {
        self.state.start_batch_build(ids, mode, contest_id)
    }

    /// Starts a background batch export task (`format`: `"domjudge"`, `"hydro"`, or
    /// `"merged_pdf"`). Stream progress via `nextBatchEvent(taskId)`.
    pub fn start_batch_export(&self, req: BatchExportRequestJs) -> BatchStartResultJs {
        self.state.start_batch_export(req)
    }

    /// Cooperatively cancels the currently running background batch task, if any.
    pub fn cancel_batch_task(&self) -> ActionResult {
        self.state.cancel_batch_task()
    }

    /// Returns the latest snapshot of the active or most recently completed batch task.
    pub fn batch_task_status(&self) -> BatchTaskSnapshotJs {
        self.state.batch_task_status()
    }

    /// Polling-free streaming event receiver for `taskId` returned by `startBatchBuild` or
    /// `startBatchExport`. Blocks off the UI thread until the next event arrives or the task
    /// stream closes (`kind === "closed"`).
    pub async fn next_batch_event(this: Instance<Self>, task_id: u32) -> BatchTaskEventJs {
        if this.is_disposed() {
            return BatchTaskEventJs::closed(task_id, BatchTaskSnapshotJs::default());
        }
        let (rx_opt, fallback_snap) = this.with(|p| p.state.batch_event_receiver(task_id));
        let Some(rx) = rx_opt else {
            return BatchTaskEventJs::closed(task_id, fallback_snap);
        };
        blocking(move || {
            let guard = match rx.lock() {
                Ok(g) => g,
                Err(_) => return BatchTaskEventJs::closed(task_id, fallback_snap),
            };
            match guard.recv() {
                Ok(ev) => ev,
                Err(_) => BatchTaskEventJs::closed(task_id, fallback_snap),
            }
        })
        .await
        .unwrap_or_else(|_| BatchTaskEventJs::closed(task_id, BatchTaskSnapshotJs::default()))
    }

    /// Saves `samples` into `problems/<id>/samples/` as `01.in`/`01.ans`..`NN.in`/`NN.ans`,
    /// stripping UTF-8 BOM and `\r` atomically and refreshing cache and index.
    pub fn save_samples(&self, id: String, samples: Vec<SamplePairJs>) -> ActionResult {
        self.state.save_samples(&id, samples)
    }

    /// Opens the system single-file picker filtered to image formats (`png`, `jpg`, `jpeg`,
    /// `svg`, `webp`, `pdf`) and atomically copies the chosen file into the target problem's
    /// `assets/<clean-name>` (never overwriting existing files). Returns `"assets/<name>"`,
    /// or `null` when cancelled. Rejects read-only workspaces and frozen contests.
    pub async fn pick_and_import_image(
        this: Instance<Self>,
        target: String,
        id: String,
        label: Option<String>,
        labels: Option<DialogLabelsJs>,
    ) -> Result<Option<String>, JsError> {
        if this.is_disposed() {
            return Err(JsError::Error("preview disposed".into()));
        }
        // Fail fast before opening the dialog if the workspace is read-only or contest frozen.
        let check = this.with(|p| {
            p.state
                .validate_image_target(&target, &id, label.as_deref())
        });
        if let Err(err) = check {
            return Err(JsError::Error(err));
        }
        let title = label_or(labels.as_ref().and_then(|l| l.title.as_deref()), "插入图片");
        let button = label_or(
            labels.as_ref().and_then(|l| l.button_label.as_deref()),
            "插入",
        );
        let filter = label_or(
            labels.as_ref().and_then(|l| l.filter_label.as_deref()),
            "图片文件",
        );
        let dialog = OpenDialog::files()
            .title(title)
            .button_label(button)
            .filter(FileFilter::new(
                filter,
                ["png", "jpg", "jpeg", "svg", "webp", "pdf"],
            ));
        let picked = pick_paths(dialog).await.map_err(JsError::Error)?;
        let Some(src_path) = picked.and_then(|paths| paths.into_iter().next()) else {
            return Ok(None);
        };
        if this.is_disposed() {
            return Err(JsError::Error("preview disposed".into()));
        }
        let state = this.with(|p| p.state.clone());
        let rel =
            blocking(move || state.import_image_file(&target, &id, label.as_deref(), &src_path))
                .await
                .map_err(|e| JsError::Error(format!("image import task failed: {e}")))?
                .map_err(JsError::Error)?;
        Ok(Some(rel))
    }

    /// Atomically writes raw image `bytes` (e.g. from `Clipboard.readImage()`) into the
    /// target problem's `assets/paste-YYYYMMDD-HHMMSS.<ext>` (auto-incrementing suffix on
    /// collision) and returns `"assets/<name>"`. Rejects read-only workspaces and frozen
    /// contests.
    pub fn import_image_bytes(
        &self,
        target: String,
        id: String,
        label: Option<String>,
        ext: String,
        bytes: Vec<u8>,
    ) -> Result<String, JsError> {
        self.state
            .import_image_bytes(&target, &id, label.as_deref(), &ext, &bytes)
            .map_err(JsError::Error)
    }

    /// Lists all files inside the target problem's `assets/` directory off the UI thread,
    /// returning file size, image dimensions (when readable), and exact Markdown references.
    pub async fn list_problem_assets(
        this: Instance<Self>,
        target: String,
        id: String,
        label: Option<String>,
    ) -> Vec<ProblemAssetJs> {
        if this.is_disposed() {
            return Vec::new();
        }
        let state = this.with(|p| p.state.clone());
        blocking(move || state.list_problem_assets(&target, &id, label.as_deref()))
            .await
            .unwrap_or_default()
    }

    /// Atomically renames `assets/<oldName>` to `assets/<newName>` and updates matching
    /// `assets/<oldName>` references in `statement*.md` and `editorial.md` (rolling back on error).
    pub fn rename_problem_asset(
        &self,
        target: String,
        id: String,
        label: Option<String>,
        old_name: String,
        new_name: String,
    ) -> ActionResult {
        self.state
            .rename_problem_asset(&target, &id, label.as_deref(), &old_name, &new_name)
    }

    /// Deletes `assets/<name>` inside the target problem while enforcing workspace sandbox,
    /// read-only mode, and frozen-contest guards.
    pub fn delete_problem_asset(
        &self,
        target: String,
        id: String,
        label: Option<String>,
        name: String,
    ) -> ActionResult {
        self.state
            .delete_problem_asset(&target, &id, label.as_deref(), &name)
    }

    /// Batch-deletes all unreferenced assets inside the target problem's `assets/` directory.
    pub fn clean_unused_assets(
        &self,
        target: String,
        id: String,
        label: Option<String>,
    ) -> CleanUnusedAssetsResultJs {
        self.state
            .clean_unused_assets(&target, &id, label.as_deref())
    }

    /// Sets or clears `{width=NN%}` on Markdown references to `assetName` in `docPath`
    /// (or across all `statement*.md` and `editorial.md` files when `docPath` is omitted).
    pub fn set_asset_width(
        &self,
        target: String,
        id: String,
        label: Option<String>,
        asset_name: String,
        width_pct: Option<f64>,
        doc_path: Option<String>,
    ) -> ActionResult {
        self.state.set_asset_width(
            &target,
            &id,
            label.as_deref(),
            &asset_name,
            width_pct,
            doc_path.as_deref(),
        )
    }

    // ---- Contest composition, overrides, stats, freeze & unfreeze ----------------------

    /// Loads full contest metadata and per-item effective/original values with override flags.
    pub fn contest_detail(&self, id: String) -> ContestDetailJs {
        self.state.contest_detail(&id)
    }

    /// Computes 8-tier difficulty distribution and tag counts for `id`.
    pub fn contest_stats(&self, id: String) -> ContestStatsJs {
        self.state.contest_stats(&id)
    }

    /// Creates a new empty contest via `compose_contest(id, [], { name })`, refusing if `id`
    /// already exists.
    pub fn create_contest(&self, id: String, name: String) -> ActionResult {
        self.state.create_contest(&id, &name)
    }

    /// Sets the ordered problem list for contest `id`, re-labelling `A..Z`, preserving
    /// existing overrides on kept problems, and removing overrides for dropped problems.
    pub fn compose_contest(&self, id: String, problem_ids: Vec<String>) -> ActionResult {
        self.state.compose_contest(&id, &problem_ids)
    }

    /// Partially updates `contests/<id>/contest.toml` metadata via `toml_edit`.
    pub fn update_contest_meta(&self, id: String, patch: ContestMetaPatchJs) -> ActionResult {
        self.state.update_contest_meta(&id, patch)
    }

    /// Archives or unarchives contest `id` (`archived = true` or removes `archived` in `contest.toml`).
    pub fn set_contest_archived(&self, id: String, archived: bool) -> ActionResult {
        self.state.set_contest_archived(&id, archived)
    }

    /// Sets or clears metadata overrides for `label` inside `contestId`. Passing `null`
    /// deletes that override; passing the original library/contest value auto-deletes it.
    pub fn set_override(
        &self,
        contest_id: String,
        label: String,
        patch: OverridePatchJs,
    ) -> ActionResult {
        self.state.set_override(&contest_id, &label, patch)
    }

    /// Compiles statements (and optional editorial), checks preflight lint, and atomically
    /// freezes `id` into `contests/<id>/snapshot/` off the UI thread.
    pub async fn freeze_contest(this: Instance<Self>, id: String) -> FreezeResultJs {
        if this.is_disposed() {
            return FreezeResultJs {
                ok: false,
                frozen: false,
                error: Some("preview disposed".into()),
                ..Default::default()
            };
        }
        let state = this.with(|p| p.state.clone());
        blocking(move || state.freeze_contest(&id))
            .await
            .unwrap_or_else(|e| FreezeResultJs {
                ok: false,
                frozen: false,
                error: Some(format!("freeze task failed: {e}")),
                ..Default::default()
            })
    }

    /// Byte-for-byte diff between a frozen contest's snapshot and the live library, for the
    /// unfreeze confirmation dialog.
    pub fn unfreeze_diff(&self, id: String) -> UnfreezeDiffResultJs {
        self.state.unfreeze_diff(&id)
    }

    /// Unfreezes a frozen contest (GUI-only, after human confirmation), deleting `snapshot/`
    /// and restoring live references.
    pub fn unfreeze_contest(&self, id: String) -> ActionResult {
        self.state.unfreeze_contest(&id)
    }

    // ---- Templates & declarative schemas -----------------------------------------------

    /// Returns JSON `{ default, workspaceOptions, templates, presets, fonts }` describing
    /// all workspace templates (`templates/*.typ`) with their evaluated `#let schema`
    /// definitions, installable preset seeds, workspace-level default options, and loaded
    /// font families.
    pub fn list_templates_json(&self) -> String {
        self.state.list_templates_json()
    }

    /// Updates `workspace.toml`'s default `template` and/or `[template-options.<tpl>]`.
    pub fn update_workspace_template(
        &self,
        template: Option<String>,
        clear_template: bool,
        options_json: Option<String>,
    ) -> ActionResult {
        self.state
            .update_workspace_template(template, clear_template, options_json)
    }

    /// Forks a workspace template `source` into `templates/<newName>.typ`.
    pub fn fork_template(&self, source: String, new_name: String) -> ActionResult {
        self.state.fork_template(&source, &new_name)
    }

    /// Installs a bundled preset seed (`olymp-classic`, `cnoi-classic`, `minimal`) into
    /// `templates/<targetName>.typ` (defaulting to `<presetId>`).
    pub fn install_preset_template(
        &self,
        preset_id: String,
        target_name: Option<String>,
    ) -> ActionResult {
        self.state
            .install_preset_template(&preset_id, target_name.as_deref())
    }

    /// Copies the contest's currently effective template to `contests/<contestId>/template.typ`
    /// so it can be edited independently for this contest.
    pub fn detach_contest_template(&self, contest_id: String) -> ActionResult {
        self.state.detach_contest_template(&contest_id)
    }

    /// Removes `contests/<contestId>/template.typ`, restoring reference to workspace templates.
    pub fn remove_contest_template(&self, contest_id: String) -> ActionResult {
        self.state.remove_contest_template(&contest_id)
    }

    /// Opens `templates/<name>.typ` with the system's default external editor (falling
    /// back to revealing it in the file manager if spawning fails).
    pub async fn open_template_file(this: Instance<Self>, name: String) -> ActionResult {
        if this.is_disposed() {
            return ActionResult::err("disposed", "preview disposed");
        }
        let resolved = this.with(|p| p.state.resolve_template_file(&name));
        let path = match resolved {
            Ok(p) => p,
            Err(err) => return err,
        };
        match open_in_external_editor(path).await {
            Ok(()) => ActionResult::ok(),
            Err(e) => ActionResult::err("io", e),
        }
    }

    /// Opens `contests/<contestId>/template.typ` with the system's default external editor
    /// (falling back to revealing it in the file manager if spawning fails).
    pub async fn open_contest_template_file(
        this: Instance<Self>,
        contest_id: String,
    ) -> ActionResult {
        if this.is_disposed() {
            return ActionResult::err("disposed", "preview disposed");
        }
        let resolved = this.with(|p| p.state.resolve_contest_template_file(&contest_id));
        let path = match resolved {
            Ok(p) => p,
            Err(err) => return err,
        };
        match open_in_external_editor(path).await {
            Ok(()) => ActionResult::ok(),
            Err(e) => ActionResult::err("io", e),
        }
    }

    /// Opens the system image picker and atomically copies the chosen file into
    /// `contests/<contestId>/assets/` (when `contestId` is given) or workspace `assets/`
    /// (for workspace-level defaults), returning its workspace-relative path or `null`
    /// when cancelled.
    pub async fn pick_and_import_template_image(
        this: Instance<Self>,
        contest_id: Option<String>,
        labels: Option<DialogLabelsJs>,
    ) -> Result<Option<String>, JsError> {
        if this.is_disposed() {
            return Err(JsError::Error("preview disposed".into()));
        }
        let check = this.with(|p| {
            p.state
                .validate_template_image_target(contest_id.as_deref())
        });
        if let Err(err) = check {
            return Err(JsError::Error(err));
        }
        let title = label_or(
            labels.as_ref().and_then(|l| l.title.as_deref()),
            "选择模板图片",
        );
        let button = label_or(
            labels.as_ref().and_then(|l| l.button_label.as_deref()),
            "选择",
        );
        let filter = label_or(
            labels.as_ref().and_then(|l| l.filter_label.as_deref()),
            "图片文件",
        );
        let dialog = OpenDialog::files()
            .title(title)
            .button_label(button)
            .filter(FileFilter::new(
                filter,
                ["png", "jpg", "jpeg", "svg", "webp", "pdf"],
            ));
        let picked = pick_paths(dialog).await.map_err(JsError::Error)?;
        let Some(src_path) = picked.and_then(|paths| paths.into_iter().next()) else {
            return Ok(None);
        };
        if this.is_disposed() {
            return Err(JsError::Error("preview disposed".into()));
        }
        let state = this.with(|p| p.state.clone());
        let rel = blocking(move || state.import_template_image(contest_id.as_deref(), &src_path))
            .await
            .map_err(|e| JsError::Error(format!("template image import failed: {e}")))?
            .map_err(JsError::Error)?;
        Ok(Some(rel))
    }

    /// Ensures `templates/preamble.typ` exists and opens it in the external editor.
    pub async fn open_preamble_file(this: Instance<Self>) -> ActionResult {
        if this.is_disposed() {
            return ActionResult::err("disposed", "preview disposed");
        }
        let ensured = this.with(|p| p.state.ensure_preamble_file());
        let path = match ensured {
            Ok(p) => p,
            Err(err) => return err,
        };
        match open_in_external_editor(path).await {
            Ok(()) => ActionResult::ok(),
            Err(e) => ActionResult::err("io", e),
        }
    }

    // ---- Cmd+K fuzzy jump, Frecency & preferences --------------------------------------

    /// Fuzzy-searches problems, contests, and (when `activeContest` is given) the active
    /// contest's problem labels, ranked with persisted Frecency.
    pub fn fuzzy_jump(&self, query: String, active_contest: Option<String>) -> Vec<FuzzyHitJs> {
        self.state.fuzzy_jump(&query, active_contest.as_deref())
    }

    /// Records a visit (`"problem:<id>"`, `"contest:<id>"`, or
    /// `"contest_item:<contestId>:<label>"`) for Frecency ranking and persists `prefs.json`.
    pub fn record_visit(&self, key: String) {
        self.state.record_visit(&key);
    }

    /// Loads persisted desktop preferences from `<dataDir>/prefs.json`.
    pub fn load_prefs(&self) -> AppPrefsJs {
        self.state.load_prefs()
    }

    /// Persists desktop preferences atomically to `<dataDir>/prefs.json` (preserving
    /// persisted Frecency history).
    pub fn save_prefs(&self, prefs: AppPrefsJs) {
        self.state.save_prefs(prefs);
    }

    /// Saves a cold-start preview snapshot (metadata JSON + 1–2 visible pages' pixels)
    /// into `<dataDir>/snapshots/` off the UI thread. Uses `try_lock()` on the compiler
    /// so it skips cleanly if a compilation is in flight. `renderScale` (`scale * dpr`,
    /// clamped to `1.0..=2.0`) controls HiDPI snapshot sharpness.
    pub async fn save_cold_start_snapshot(
        this: Instance<Self>,
        meta_json: String,
        pages: Vec<u32>,
        render_scale: Option<f64>,
    ) -> bool {
        if this.is_disposed() {
            return false;
        }
        let state = this.with(|p| p.state.clone());
        let scale = render_scale.unwrap_or(1.0) as f32;
        blocking(move || state.save_cold_start_snapshot(&meta_json, &pages, scale))
            .await
            .unwrap_or(false)
    }

    /// Restores the cold-start preview snapshot from `<dataDir>/snapshots/` if it matches
    /// the active workspace and current on-disk content hash; returns `null` otherwise.
    pub fn load_cold_start_snapshot(&self) -> Option<ColdStartSnapshotJs> {
        self.state.load_cold_start_snapshot()
    }

    // ---- Phase 3A: Local Polygon toolchain & package build -----------------------------

    /// Detects host C++ compilers (`g++` / `clang++`) taking persisted `cxxOverride` and
    /// `XCPC_CXX` into account.
    pub fn toolchain_status(&self) -> ToolchainStatusJs {
        self.state.toolchain_status()
    }

    /// Persists an explicit C++ compiler path override (passing `null` or empty clears it)
    /// and returns the refreshed toolchain detection status.
    pub fn set_cxx_override(&self, path: Option<String>) -> ToolchainStatusJs {
        self.state.set_cxx_override(path)
    }

    /// Pure read-only inspection of a problem package (`hasPackage`, freshness, files,
    /// expanded tests, latest report, gates, and next steps).
    /// With `contest_id`, freshness follows that contest's overridden limits.
    pub fn inspect_package(
        &self,
        slug: String,
        contest_id: Option<String>,
    ) -> PackageInspectionResultJs {
        self.state.inspect_package_in(&slug, contest_id.as_deref())
    }

    /// Scaffolds a problem package with `testlib.h` templates without overwriting existing
    /// files, and returns the refreshed inspection JSON.
    pub fn init_package(&self, slug: String) -> PackageInitResultJs {
        self.state.init_package(&slug)
    }

    /// Scaffolds a problem package with optional Phase 3C features (`interactive`, `runs`, `subtasks`).
    pub fn init_package_with_options(
        &self,
        slug: String,
        interactive: bool,
        runs: Option<u32>,
        subtasks: bool,
    ) -> PackageInitResultJs {
        self.state
            .init_package_with_options(&slug, interactive, runs, subtasks)
    }

    /// Spawns a background thread to run `xcpc_judge::build_package` (`"debug"` or
    /// `"verify"`) with optional test/solution subset filters.
    pub fn start_package_build(
        &self,
        slug: String,
        mode: String,
        tests: Option<Vec<String>>,
        solutions: Option<Vec<String>>,
        contest_id: Option<String>,
    ) -> ActionResult {
        self.state
            .start_package_build(&slug, &mode, tests, solutions, contest_id)
    }

    /// Signals cancellation to the currently running background package build, if any.
    pub fn cancel_package_build(&self) -> ActionResult {
        self.state.cancel_package_build()
    }

    /// Returns the latest progress and completion snapshot of the background package build.
    pub fn package_build_status(&self) -> PackageBuildStatusJs {
        self.state.package_build_status()
    }

    /// Validates that `relPath` (file or directory) stays inside `problems/<slug>/` and opens
    /// it in the system default external editor (or file manager for directories).
    pub async fn open_package_file(
        this: Instance<Self>,
        slug: String,
        rel_path: String,
    ) -> ActionResult {
        if this.is_disposed() {
            return ActionResult::err("disposed", "preview disposed");
        }
        let resolved = this.with(|p| p.state.resolve_package_file(&slug, &rel_path));
        let path = match resolved {
            Ok(p) => p,
            Err(err) => return err,
        };
        let res = if path.is_dir() {
            reveal_path(path).await
        } else {
            open_in_external_editor(path).await
        };
        match res {
            Ok(()) => ActionResult::ok(),
            Err(e) => ActionResult::err("io", e),
        }
    }

    /// Safely previews the head and tail lines of a single test case without loading huge
    /// files into the UI.
    pub fn inspect_test_case(
        &self,
        slug: String,
        selector: String,
        head_lines: Option<u32>,
        tail_lines: Option<u32>,
    ) -> TestCasePreviewJs {
        self.state
            .inspect_test_case(&slug, &selector, head_lines, tail_lines)
    }

    /// Updates `[package].checker` in `problem.toml` (built-in checker name or `"checker.cpp"`).
    pub fn set_package_checker(&self, slug: String, checker: String) -> ActionResult {
        self.state.set_package_checker(&slug, &checker)
    }

    /// Updates `[solutions.<name>]` (`main`, `expect`, `tags`) in `problem.toml`.
    pub fn set_solution_config(
        &self,
        slug: String,
        name: String,
        is_main: Option<bool>,
        expect: Option<Vec<String>>,
        tags: Option<Vec<String>>,
    ) -> ActionResult {
        self.state
            .set_solution_config(&slug, &name, is_main, expect, tags)
    }

    /// Creates a new boilerplate package source file (`solution`, `generator`, `validator`,
    /// `custom_checker`, `validator_self_test_valid`, `validator_self_test_invalid`).
    pub fn create_package_source(
        &self,
        slug: String,
        kind: String,
        name: String,
    ) -> PackageCreateResultJs {
        self.state.create_package_source(&slug, &kind, &name)
    }

    /// Adds a hand-written test file under `tests/` and appends a `[[test]]` rule.
    pub fn add_hand_test(
        &self,
        slug: String,
        name: Option<String>,
        tag: Option<String>,
        content: String,
    ) -> AddedTestResultJs {
        self.state
            .add_hand_test(&slug, name.as_deref(), tag.as_deref(), &content)
    }

    /// Appends a generator `[[test]]` batch rule to `problem.toml`.
    pub fn add_generator_test(
        &self,
        slug: String,
        generator: String,
        args: String,
        repeat: u32,
        tag: Option<String>,
        name: Option<String>,
    ) -> AddedTestResultJs {
        self.state.add_generator_test(
            &slug,
            &generator,
            &args,
            repeat,
            tag.as_deref(),
            name.as_deref(),
        )
    }

    /// Updates the 0-based generator `[[test]]` rule in `problem.toml` in-place via `toml_edit`.
    #[allow(clippy::too_many_arguments)]
    pub fn update_generator_test(
        &self,
        slug: String,
        rule_index: u32,
        generator: String,
        args: String,
        repeat: u32,
        tag: Option<String>,
        name: Option<String>,
    ) -> ActionResult {
        self.state.update_generator_test(
            &slug,
            rule_index,
            &generator,
            &args,
            repeat,
            tag.as_deref(),
            name.as_deref(),
        )
    }

    /// Removes the 0-based `[[test]]` rule from `problem.toml`.
    pub fn remove_test_rule(&self, slug: String, rule_index: u32) -> ActionResult {
        self.state.remove_test_rule(&slug, rule_index)
    }

    /// Deletes a solution source file and removes its `[solutions.<name>]` table from `problem.toml`.
    pub fn delete_solution(&self, slug: String, name: String) -> ActionResult {
        self.state.delete_solution(&slug, &name)
    }

    /// Deletes a generator source file and optionally removes referencing `[[test]]` rules.
    pub fn delete_generator(
        &self,
        slug: String,
        filename: String,
        remove_referencing_tests: bool,
    ) -> ActionResult {
        self.state
            .delete_generator(&slug, &filename, remove_referencing_tests)
    }

    /// Deletes a validator or checker self-test file inside `problems/<slug>/tests/{validator,checker}/`.
    pub fn delete_self_test(&self, slug: String, rel_path: String) -> ActionResult {
        self.state.delete_self_test(&slug, &rel_path)
    }

    /// Deletes `interactor.cpp` (or versioned C++ variants) from `problems/<slug>/`.
    pub fn delete_interactor(&self, slug: String) -> ActionResult {
        self.state.delete_interactor(&slug)
    }

    /// Updates `[package].runs` in `problem.toml` (`null` or `1` returns to single-run default).
    pub fn set_package_runs(&self, slug: String, runs: Option<u32>) -> ActionResult {
        self.state.set_package_runs(&slug, runs)
    }

    /// Adds or updates a `[[subtask]]` entry in `problem.toml` (matched by `id`).
    pub fn upsert_subtask(
        &self,
        slug: String,
        id: String,
        score: u32,
        depends: Vec<String>,
        scoring: String,
    ) -> ActionResult {
        self.state
            .upsert_subtask(&slug, &id, score, depends, &scoring)
    }

    /// Deletes a `[[subtask]]` entry by `subtaskId` and optionally clears references.
    pub fn delete_subtask(
        &self,
        slug: String,
        subtask_id: String,
        clear_references: bool,
    ) -> ActionResult {
        self.state
            .delete_subtask(&slug, &subtask_id, clear_references)
    }

    /// Sets or clears `[[test]].subtask` on the 0-based `ruleIndex` test rule.
    pub fn set_test_subtask(
        &self,
        slug: String,
        rule_index: u32,
        subtask: Option<String>,
    ) -> ActionResult {
        self.state
            .set_test_subtask(&slug, rule_index, subtask.as_deref())
    }

    /// Updates `[solutions.<name>]` OI scoring assertions (`scoreJson`, `subtasksJson`).
    pub fn set_solution_scoring(
        &self,
        slug: String,
        name: String,
        score_json: Option<String>,
        subtasks_json: Option<String>,
    ) -> ActionResult {
        self.state.set_solution_scoring(
            &slug,
            &name,
            score_json.as_deref(),
            subtasks_json.as_deref(),
        )
    }

    /// Deletes the 1-based sample pair (`samples/NN.{in,ans}`) and renumbers remaining samples contiguously.
    pub fn delete_sample(&self, slug: String, sample_index: u32) -> ActionResult {
        self.state.delete_sample(&slug, sample_index)
    }

    /// Normalizes CRLF, UTF-8 BOM, and missing trailing newlines across hand-written inputs.
    pub fn normalize_dirty_inputs(&self, slug: String) -> NormalizeDirtyResultJs {
        self.state.normalize_dirty_inputs(&slug)
    }

    /// Restores the original bytes from the most recent `normalizeDirtyInputs` call for `slug`.
    pub fn undo_normalize_dirty_inputs(&self, slug: String) -> ActionResult {
        self.state.undo_normalize_dirty_inputs(&slug)
    }

    /// Bounded read of a package file inside `problems/<slug>/` for the lightweight quick-editor
    /// (enforces a strict 256 KiB cap).
    pub fn read_package_file(&self, slug: String, rel_path: String) -> PackageFileContentJs {
        self.state.read_package_file(&slug, &rel_path)
    }

    /// Atomically saves a package file inside `problems/<slug>/` (max 256 KiB, sandboxed,
    /// conflict-checked against `expectedHash`, watcher self-write suppressed).
    pub fn save_package_file(
        &self,
        slug: String,
        rel_path: String,
        content: String,
        expected_hash: Option<String>,
    ) -> SavePackageFileResultJs {
        self.state
            .save_package_file(&slug, &rel_path, &content, expected_hash.as_deref())
    }

    // ---- Phase 3B: Polygon import & OJ package export ----------------------------------

    /// Opens the system file picker filtered to `.zip` archives, imports the chosen Polygon
    /// single-problem or contest package off the UI thread, and refreshes the workspace index.
    pub async fn pick_and_import_polygon(
        this: Instance<Self>,
        target_id: Option<String>,
        labels: Option<DialogLabelsJs>,
    ) -> PolygonImportResultJs {
        if this.is_disposed() {
            return PolygonImportResultJs {
                ok: false,
                cancelled: true,
                ..Default::default()
            };
        }
        let precheck = this.with(|p| p.state.validate_polygon_import_writable());
        if let Err((code, error, detail)) = precheck {
            return PolygonImportResultJs {
                ok: false,
                cancelled: false,
                code: Some(code),
                error: Some(error),
                detail: Some(detail),
                ..Default::default()
            };
        }
        let title = label_or(
            labels.as_ref().and_then(|l| l.title.as_deref()),
            "导入 Polygon 压缩包",
        );
        let button = label_or(
            labels.as_ref().and_then(|l| l.button_label.as_deref()),
            "导入",
        );
        let filter = label_or(
            labels.as_ref().and_then(|l| l.filter_label.as_deref()),
            "Polygon 压缩包",
        );
        let dialog = OpenDialog::files()
            .title(title)
            .button_label(button)
            .filter(FileFilter::new(filter, ["zip"]));
        let picked = match pick_paths(dialog).await {
            Ok(Some(paths)) => paths.into_iter().next(),
            Ok(None) => None,
            Err(e) => {
                return PolygonImportResultJs {
                    ok: false,
                    cancelled: false,
                    code: Some("io".into()),
                    error: Some(e.clone()),
                    detail: Some(e),
                    ..Default::default()
                };
            }
        };
        let Some(zip_path) = picked else {
            return PolygonImportResultJs {
                ok: false,
                cancelled: true,
                ..Default::default()
            };
        };
        if this.is_disposed() {
            return PolygonImportResultJs {
                ok: false,
                cancelled: true,
                ..Default::default()
            };
        }
        let state = this.with(|p| p.state.clone());
        blocking(move || state.import_polygon(&zip_path, target_id))
            .await
            .unwrap_or_else(|e| PolygonImportResultJs {
                ok: false,
                cancelled: false,
                code: Some("io".into()),
                error: Some(format!("导入任务执行失败：{e}")),
                detail: Some(format!("polygon import task failed: {e}")),
                ..Default::default()
            })
    }

    /// Imports a Polygon single-problem or contest ZIP at `zipPath` off the UI thread and
    /// refreshes the workspace index.
    pub async fn import_polygon(
        this: Instance<Self>,
        zip_path: String,
        target_id: Option<String>,
    ) -> PolygonImportResultJs {
        if this.is_disposed() {
            return PolygonImportResultJs {
                ok: false,
                cancelled: true,
                ..Default::default()
            };
        }
        let state = this.with(|p| p.state.clone());
        let path = PathBuf::from(zip_path);
        blocking(move || state.import_polygon(&path, target_id))
            .await
            .unwrap_or_else(|e| PolygonImportResultJs {
                ok: false,
                cancelled: false,
                code: Some("io".into()),
                error: Some(format!("导入任务执行失败：{e}")),
                detail: Some(format!("polygon import task failed: {e}")),
                ..Default::default()
            })
    }

    /// Exports a DOMjudge (`"domjudge"`) or Hydro (`"hydro"`) minimal-set OJ package off the
    /// UI thread with strict verification gate enabled (`force: false`).
    pub async fn export_oj_package(
        this: Instance<Self>,
        format: String,
        target: String,
        id: String,
    ) -> ExportOjResultJs {
        if this.is_disposed() {
            return ExportOjResultJs {
                ok: false,
                format,
                code: Some("disposed".into()),
                error: Some("窗口已关闭".into()),
                detail: Some("preview disposed".into()),
                ..Default::default()
            };
        }
        let state = this.with(|p| p.state.clone());
        let fmt_fallback = format.clone();
        blocking(move || state.export_oj_package(&format, &target, &id))
            .await
            .unwrap_or_else(|e| ExportOjResultJs {
                ok: false,
                format: fmt_fallback,
                code: Some("io".into()),
                error: Some(format!("导出任务执行失败：{e}")),
                detail: Some(format!("oj package export task failed: {e}")),
                ..Default::default()
            })
    }

    // ---- Phase 3D: Sanitizer, external trial run, adopt & contest readiness audit ------

    /// Loads the latest isolated sanitizer report (`build/problems/<slug>/sanitizer-report.json`),
    /// returning `ok: true, reportJson: null` when no sanitizer run has been recorded yet.
    pub fn load_sanitizer_report(&self, slug: String) -> JsonReportResultJs {
        self.state.load_sanitizer_report(&slug)
    }

    /// Runs ASan/UBSan check on `slug` off the UI thread and returns the serialized `SanitizerReport`.
    pub async fn sanitize_problem(
        this: Instance<Self>,
        slug: String,
        tests: Option<Vec<String>>,
        solutions: Option<Vec<String>>,
    ) -> JsonReportResultJs {
        if this.is_disposed() {
            return JsonReportResultJs::err("disposed", "preview disposed");
        }
        let state = this.with(|p| p.state.clone());
        blocking(move || state.sanitize_problem(&slug, tests, solutions))
            .await
            .unwrap_or_else(|e| {
                JsonReportResultJs::err("io", format!("sanitizer task failed: {e}"))
            })
    }

    /// Opens the native multi-file picker filtered to C++ source files (`.cpp`, `.cc`, `.cxx`).
    pub async fn pick_cpp_files(
        this: Instance<Self>,
        labels: Option<DialogLabelsJs>,
    ) -> PickedPathsJs {
        if this.is_disposed() {
            return PickedPathsJs {
                cancelled: true,
                paths: Vec::new(),
                error: None,
            };
        }
        let title = label_or(
            labels.as_ref().and_then(|l| l.title.as_deref()),
            "选择外部解法文件",
        );
        let button = label_or(
            labels.as_ref().and_then(|l| l.button_label.as_deref()),
            "选择",
        );
        let filter = label_or(
            labels.as_ref().and_then(|l| l.filter_label.as_deref()),
            "C++ 源码",
        );
        let dialog = OpenDialog::files()
            .multiple(true)
            .title(title)
            .button_label(button)
            .filter(FileFilter::new(filter, ["cpp", "cc", "cxx"]));
        match pick_paths(dialog).await {
            Ok(Some(paths)) if !paths.is_empty() => PickedPathsJs {
                cancelled: false,
                paths: paths
                    .into_iter()
                    .map(|p| p.to_string_lossy().into_owned())
                    .collect(),
                error: None,
            },
            Ok(_) => PickedPathsJs {
                cancelled: true,
                paths: Vec::new(),
                error: None,
            },
            Err(e) => PickedPathsJs {
                cancelled: false,
                paths: Vec::new(),
                error: Some(e),
            },
        }
    }

    /// Opens the native folder picker for selecting an external tester directory.
    pub async fn pick_external_folder(
        this: Instance<Self>,
        labels: Option<DialogLabelsJs>,
    ) -> PickedFolder {
        if this.is_disposed() {
            return PickedFolder::default();
        }
        let title = label_or(
            labels.as_ref().and_then(|l| l.title.as_deref()),
            "选择验题人解法目录",
        );
        let button = label_or(
            labels.as_ref().and_then(|l| l.button_label.as_deref()),
            "选择",
        );
        let dialog = OpenDialog::folders().title(title).button_label(button);
        match pick_paths(dialog).await {
            Ok(Some(mut paths)) if !paths.is_empty() => PickedFolder {
                cancelled: false,
                path: paths.remove(0).to_string_lossy().into_owned(),
                error: None,
                ..Default::default()
            },
            Ok(_) => PickedFolder {
                cancelled: true,
                ..Default::default()
            },
            Err(e) => PickedFolder {
                cancelled: false,
                path: String::new(),
                error: Some(e),
                ..Default::default()
            },
        }
    }

    /// Zero-pollution trial run of external `.cpp` files against `slug` off the UI thread.
    /// Returns serialized `ExternalProblemReport` in `reportJson` and Markdown in `markdown`.
    pub async fn run_external_problem_solutions(
        this: Instance<Self>,
        slug: String,
        files: Vec<String>,
        auto_build: bool,
    ) -> JsonReportResultJs {
        if this.is_disposed() {
            return JsonReportResultJs::err("disposed", "preview disposed");
        }
        let state = this.with(|p| p.state.clone());
        let file_paths: Vec<PathBuf> = files.into_iter().map(PathBuf::from).collect();
        blocking(move || state.run_external_problem_solutions(&slug, &file_paths, auto_build))
            .await
            .unwrap_or_else(|e| {
                JsonReportResultJs::err("io", format!("external run task failed: {e}"))
            })
    }

    /// Zero-pollution trial run of a single-directory or multi-tester directory against `contestId`
    /// off the UI thread. Returns serialized `ExternalContestReport` + rendered Markdown.
    pub async fn run_external_contest_solutions(
        this: Instance<Self>,
        contest_id: String,
        dir: String,
        auto_build: bool,
    ) -> JsonReportResultJs {
        if this.is_disposed() {
            return JsonReportResultJs::err("disposed", "preview disposed");
        }
        let state = this.with(|p| p.state.clone());
        let dir_path = PathBuf::from(dir);
        blocking(move || state.run_external_contest_solutions(&contest_id, &dir_path, auto_build))
            .await
            .unwrap_or_else(|e| {
                JsonReportResultJs::err("io", format!("external contest run task failed: {e}"))
            })
    }

    /// Explicitly adopts an external `.cpp` file into `problems/<slug>/solutions/` and registers
    /// it in `problem.toml` atomically.
    pub fn adopt_external_solution(
        &self,
        slug: String,
        source_file: String,
        name: Option<String>,
        expect: Option<Vec<String>>,
        tags: Option<Vec<String>>,
        is_main: Option<bool>,
    ) -> AdoptSolutionResultJs {
        self.state.adopt_external_solution(
            &slug,
            Path::new(&source_file),
            name,
            expect,
            tags,
            is_main,
        )
    }

    /// Performs a read-only contest readiness audit across all problems in `contestId` off the
    /// UI thread (populating `xcpc-lint` counts per problem), returning serialized
    /// `ContestAuditReport` + rendered Markdown.
    pub async fn audit_contest(this: Instance<Self>, contest_id: String) -> JsonReportResultJs {
        if this.is_disposed() {
            return JsonReportResultJs::err("disposed", "preview disposed");
        }
        let state = this.with(|p| p.state.clone());
        blocking(move || state.audit_contest(&contest_id))
            .await
            .unwrap_or_else(|e| {
                JsonReportResultJs::err("io", format!("contest audit task failed: {e}"))
            })
    }

    /// Opens the native save dialog and writes a Markdown report atomically off the UI thread.
    pub async fn save_markdown_report(
        this: Instance<Self>,
        suggested_name: String,
        content: String,
        labels: Option<DialogLabelsJs>,
    ) -> SaveReportResultJs {
        if this.is_disposed() {
            return SaveReportResultJs {
                ok: false,
                cancelled: true,
                path: None,
                error: None,
            };
        }
        let title = label_or(
            labels.as_ref().and_then(|l| l.title.as_deref()),
            "导出 Markdown 报告",
        );
        let button = label_or(
            labels.as_ref().and_then(|l| l.button_label.as_deref()),
            "导出",
        );
        let filter = label_or(
            labels.as_ref().and_then(|l| l.filter_label.as_deref()),
            "Markdown 文件",
        );
        let dialog = SaveDialog::new()
            .title(title)
            .button_label(button)
            .file_name(suggested_name)
            .filter(FileFilter::new(filter, ["md"]));
        let picked = match pick_save_path(dialog).await {
            Ok(Some(path)) => path,
            Ok(None) => {
                return SaveReportResultJs {
                    ok: false,
                    cancelled: true,
                    path: None,
                    error: None,
                };
            }
            Err(e) => {
                return SaveReportResultJs {
                    ok: false,
                    cancelled: false,
                    path: None,
                    error: Some(e),
                };
            }
        };
        let path_str = picked.to_string_lossy().into_owned();
        let write_res = blocking(move || -> Result<(), String> {
            if let Some(parent) = picked.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            let tmp = picked.with_extension(format!("md.tmp.{}", std::process::id()));
            std::fs::write(&tmp, content.as_bytes()).map_err(|e| e.to_string())?;
            std::fs::rename(&tmp, &picked).map_err(|e| {
                let _ = std::fs::remove_file(&tmp);
                e.to_string()
            })
        })
        .await;
        match write_res {
            Ok(Ok(())) => SaveReportResultJs {
                ok: true,
                cancelled: false,
                path: Some(path_str),
                error: None,
            },
            Ok(Err(e)) => SaveReportResultJs {
                ok: false,
                cancelled: false,
                path: None,
                error: Some(e),
            },
            Err(e) => SaveReportResultJs {
                ok: false,
                cancelled: false,
                path: None,
                error: Some(format!("save markdown task failed: {e}")),
            },
        }
    }

    // ---- Phase 4A: Training status, URL resolution, Plugins & OJ Sync ------------------

    /// Appends a manual training status event (`todo | attempted | ac | upsolved | clear`)
    /// to `training/logs.jsonl`.
    pub fn set_training_status(&self, problem_id: String, status: String) -> ActionResult {
        self.state.set_training_status(&problem_id, &status)
    }

    /// Resolves an external OJ problem URL via enabled `oj_source` plugins and `oj_sync.db`.
    pub async fn resolve_problem_url(this: Instance<Self>, url: String) -> ResolvedProblemUrlJs {
        if this.is_disposed() {
            return ResolvedProblemUrlJs {
                ok: false,
                error: Some("preview disposed".into()),
                ..Default::default()
            };
        }
        let state = this.with(|p| p.state.clone());
        blocking(move || state.resolve_problem_url(&url))
            .await
            .unwrap_or_else(|e| ResolvedProblemUrlJs {
                ok: false,
                error: Some(format!("resolve problem URL failed: {e}")),
                ..Default::default()
            })
    }

    /// Creates a problem from a resolved OJ URL (or returns the existing problem ID if a
    /// matching `(platform, sourceId)` already exists in the workspace).
    #[allow(clippy::too_many_arguments)]
    pub fn create_problem_from_url(
        &self,
        id: String,
        title: String,
        platform: String,
        source_id: String,
        url: String,
        tags: Vec<String>,
        rating: Option<u32>,
    ) -> CreateProblemFromUrlResultJs {
        self.state
            .create_problem_from_url(&id, &title, &platform, &source_id, &url, tags, rating)
    }

    /// Returns all discovered plugins, subscribed repositories, and global sync settings.
    pub fn get_plugins_overview(&self) -> PluginsOverviewJs {
        self.state.get_plugins_overview()
    }

    /// Enables or disables a discovered plugin by ID.
    pub fn set_plugin_enabled(&self, id: String, enabled: bool) -> ActionResult {
        self.state.set_plugin_enabled(&id, enabled)
    }

    /// Re-scans `<dataDir>/plugins/` and returns the refreshed plugins overview.
    pub fn reload_plugins(&self) -> PluginsOverviewJs {
        self.state.get_plugins_overview()
    }

    /// Exports an embedded built-in plugin to `<dataDir>/plugins/<id>/` for local customization.
    pub fn export_builtin_plugin(&self, id: String) -> ExportPluginResultJs {
        self.state.export_builtin_plugin(&id)
    }

    /// Opens a native folder or `.zip` picker (`kind`: `"dir" | "zip"`) and installs the plugin.
    pub async fn install_plugin_from_dialog(
        this: Instance<Self>,
        kind: String,
        labels: Option<DialogLabelsJs>,
    ) -> InstallPluginResultJs {
        if this.is_disposed() {
            return InstallPluginResultJs {
                ok: false,
                cancelled: false,
                plugin_id: None,
                overview: None,
                error: Some("preview disposed".into()),
            };
        }
        let is_zip = kind.trim().eq_ignore_ascii_case("zip");
        let dialog = if is_zip {
            let title = label_or(
                labels.as_ref().and_then(|l| l.title.as_deref()),
                "安装插件（ZIP）",
            );
            let button = label_or(
                labels.as_ref().and_then(|l| l.button_label.as_deref()),
                "安装",
            );
            let filter = label_or(
                labels.as_ref().and_then(|l| l.filter_label.as_deref()),
                "插件包 (.zip)",
            );
            OpenDialog::files()
                .title(title)
                .button_label(button)
                .filter(FileFilter::new(filter, ["zip"]))
        } else {
            let title = label_or(
                labels.as_ref().and_then(|l| l.title.as_deref()),
                "安装本地插件目录",
            );
            let button = label_or(
                labels.as_ref().and_then(|l| l.button_label.as_deref()),
                "安装",
            );
            OpenDialog::folders().title(title).button_label(button)
        };

        let picked = match pick_paths(dialog).await {
            Ok(Some(mut paths)) if !paths.is_empty() => paths.remove(0),
            Ok(_) => {
                return InstallPluginResultJs {
                    ok: false,
                    cancelled: true,
                    plugin_id: None,
                    overview: None,
                    error: None,
                };
            }
            Err(err) => {
                return InstallPluginResultJs {
                    ok: false,
                    cancelled: false,
                    plugin_id: None,
                    overview: None,
                    error: Some(err),
                };
            }
        };

        if this.is_disposed() {
            return InstallPluginResultJs {
                ok: false,
                cancelled: false,
                plugin_id: None,
                overview: None,
                error: Some("preview disposed".into()),
            };
        }
        let state = this.with(|p| p.state.clone());
        blocking(move || state.install_plugin_from_path(&picked, is_zip))
            .await
            .unwrap_or_else(|e| InstallPluginResultJs {
                ok: false,
                cancelled: false,
                plugin_id: None,
                overview: None,
                error: Some(format!("install plugin task failed: {e}")),
            })
    }

    /// Subscribes to a decentralized plugin repository (`index.json` URL).
    pub fn add_plugin_repo(&self, url: String) -> ActionResult {
        self.state.add_plugin_repo(&url)
    }

    /// Unsubscribes from a decentralized plugin repository URL.
    pub fn remove_plugin_repo(&self, url: String) -> ActionResult {
        self.state.remove_plugin_repo(&url)
    }

    /// Fetches a decentralized plugin repository's `index.json` catalog off the UI thread.
    pub async fn fetch_plugin_repo_catalog(
        this: Instance<Self>,
        url: String,
    ) -> RepoCatalogResultJs {
        if this.is_disposed() {
            return RepoCatalogResultJs {
                ok: false,
                error: Some("preview disposed".into()),
                ..Default::default()
            };
        }
        let state = this.with(|p| p.state.clone());
        blocking(move || state.fetch_plugin_repo_catalog(&url))
            .await
            .unwrap_or_else(|e| RepoCatalogResultJs {
                ok: false,
                error: Some(format!("fetch repo catalog failed: {e}")),
                ..Default::default()
            })
    }

    /// Downloads, verifies SHA-256, and installs a plugin from a decentralized repository.
    pub async fn install_plugin_from_repo(
        this: Instance<Self>,
        repo_url: String,
        plugin_id: String,
    ) -> InstallPluginResultJs {
        if this.is_disposed() {
            return InstallPluginResultJs {
                ok: false,
                error: Some("preview disposed".into()),
                ..Default::default()
            };
        }
        let state = this.with(|p| p.state.clone());
        blocking(move || state.install_plugin_from_repo(&repo_url, &plugin_id))
            .await
            .unwrap_or_else(|e| InstallPluginResultJs {
                ok: false,
                error: Some(format!("install plugin from repo failed: {e}")),
                ..Default::default()
            })
    }

    /// Sets (or clears when empty) the single OJ handle for `platform`.
    pub fn set_sync_handle(&self, platform: String, handle: String) -> ActionResult {
        self.state.set_sync_handle(&platform, &handle)
    }

    /// Toggles silent OJ auto-sync on application startup.
    pub fn set_auto_sync_on_startup(&self, enabled: bool) -> ActionResult {
        self.state.set_auto_sync_on_startup(enabled)
    }

    /// Stores (or deletes when empty) a plugin credential in the OS keyring.
    pub fn set_plugin_credential(
        &self,
        plugin_id: String,
        key: String,
        value: String,
    ) -> ActionResult {
        self.state.set_plugin_credential(&plugin_id, &key, &value)
    }

    /// Runs a plugin's `verify(ctx)` action off the UI thread.
    /// When `handle` is provided, verifies that candidate handle without requiring it to be saved first.
    pub async fn verify_plugin(
        this: Instance<Self>,
        plugin_id: String,
        handle: Option<String>,
    ) -> VerifyPluginResultJs {
        if this.is_disposed() {
            return VerifyPluginResultJs {
                ok: false,
                message: None,
                error: Some("preview disposed".into()),
            };
        }
        let state = this.with(|p| p.state.clone());
        blocking(move || state.verify_plugin(&plugin_id, handle.as_deref()))
            .await
            .unwrap_or_else(|e| VerifyPluginResultJs {
                ok: false,
                message: None,
                error: Some(format!("verify plugin failed: {e}")),
            })
    }

    /// Runs OJ sync in a background thread so the UI never blocks.
    pub async fn sync_oj(
        this: Instance<Self>,
        platform: Option<String>,
        full: bool,
    ) -> SyncReportJs {
        if this.is_disposed() {
            return SyncReportJs {
                ok: false,
                error: Some("preview disposed".into()),
                ..Default::default()
            };
        }
        let state = this.with(|p| p.state.clone());
        blocking(move || state.sync_oj(platform, full))
            .await
            .unwrap_or_else(|e| SyncReportJs {
                ok: false,
                error: Some(format!("sync task failed: {e}")),
                ..Default::default()
            })
    }

    /// Triggers silent startup auto-sync once per session in a background thread when
    /// enabled and at least one enabled `oj_source` plugin has a configured handle.
    pub async fn startup_sync_if_enabled(this: Instance<Self>) -> bool {
        if this.is_disposed() {
            return false;
        }
        let state = this.with(|p| p.state.clone());
        blocking(move || state.startup_sync_if_enabled())
            .await
            .unwrap_or(false)
    }

    // ---- Phase 4B: Training Dashboard, Contests Calendar, Submissions & Ingest ---------

    /// Computes the training dashboard analytics payload (returned as JSON in `reportJson`).
    pub fn training_dashboard(
        &self,
        window: Option<String>,
        heatmap_year: Option<i32>,
        tag_min_rating: Option<i32>,
    ) -> JsonReportResultJs {
        self.state
            .training_dashboard(window, heatmap_year, tag_min_rating)
    }

    /// Queries aggregated upcoming / running (and optional finished) OJ contests as JSON.
    pub fn training_contests(
        &self,
        platform: Option<String>,
        include_finished: bool,
    ) -> JsonReportResultJs {
        self.state.training_contests(platform, include_finished)
    }

    /// Queries synced submissions (raw or deduplicated per problem) as JSON, enriched with
    /// `library_problem_id` from the open workspace.
    pub fn training_submissions(&self, query: TrainingSubmissionsQueryJs) -> JsonReportResultJs {
        self.state.training_submissions(query)
    }

    /// Ingests an external OJ problem (by URL or `(platform, problemKey)`) into the open
    /// workspace, deduplicating by `[[source]]` and refreshing the workspace index.
    pub fn training_ingest_problem(&self, req: IngestProblemRequestJs) -> IngestProblemResultJs {
        self.state.training_ingest_problem(req)
    }

    /// Records a manual training log entry (workspace check-in or standalone) into
    /// `training/logs.jsonl`, optionally creating a linked library problem.
    pub fn training_log_record(&self, req_json: String) -> ManualLogResultJs {
        self.state.training_log_record(&req_json)
    }

    /// Lists non-empty problem editorials in the open workspace, filtered by `queryJson`.
    pub fn training_list_editorials(&self, query_json: String) -> EditorialListResultJs {
        self.state.training_list_editorials(&query_json)
    }

    /// Opens an external `http://` or `https://` URL in the user's default browser.
    pub fn open_external_url(url: String) -> ActionResult {
        let trimmed = url.trim();
        if !(trimmed.starts_with("https://") || trimmed.starts_with("http://")) {
            return ActionResult::err(
                "invalid_url",
                "only http:// and https:// URLs are supported",
            );
        }
        let spawned = {
            #[cfg(target_os = "macos")]
            {
                std::process::Command::new("open")
                    .arg(trimmed)
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn()
            }
            #[cfg(target_os = "windows")]
            {
                std::process::Command::new("cmd")
                    .args(["/C", "start", "", trimmed])
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn()
            }
            #[cfg(all(unix, not(target_os = "macos")))]
            {
                std::process::Command::new("xdg-open")
                    .arg(trimmed)
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn()
            }
        };
        match spawned {
            Ok(_) => ActionResult::ok(),
            Err(err) => ActionResult::err("io", err.to_string()),
        }
    }

    // ---- Phase 4C: Toolbox, Plugin UI SDK, Commands & Importer/Exporter ----------------

    /// Lists all enabled `tool` plugins and their declared headless tools as JSON.
    pub fn plugin_list_tools(&self) -> JsonReportResultJs {
        self.state.plugin_list_tools()
    }

    /// Executes a headless plugin tool action `runTool(toolId, inputs, ctx)` off the UI thread.
    pub async fn plugin_run_tool(
        this: Instance<Self>,
        plugin_id: String,
        tool_id: String,
        inputs_json: String,
    ) -> JsonReportResultJs {
        if this.is_disposed() {
            return JsonReportResultJs::err("disposed", "preview disposed");
        }
        let state = this.with(|p| p.state.clone());
        blocking(move || state.plugin_run_tool(&plugin_id, &tool_id, &inputs_json))
            .await
            .unwrap_or_else(|e| JsonReportResultJs::err("task", format!("run tool failed: {e}")))
    }

    /// Reads the `ui.js` source for an enabled desktop UI plugin.
    pub fn plugin_ui_source(&self, plugin_id: String) -> PluginUiSourceResultJs {
        self.state.plugin_ui_source(&plugin_id)
    }

    /// Renders an SVG string to a PNG image (base64-encoded) via `resvg` + `tiny-skia`.
    pub fn plugin_render_svg_png(svg: String, scale: Option<f64>) -> PluginRenderPngResultJs {
        State::plugin_render_svg_png(&svg, scale)
    }

    /// Opens a native save dialog and atomically writes a base64-encoded PNG file.
    pub async fn plugin_save_png_dialog(
        png_base64: String,
        default_name: String,
        labels: Option<DialogLabelsJs>,
    ) -> SaveReportResultJs {
        use base64::Engine as _;
        let bytes = match base64::engine::general_purpose::STANDARD.decode(png_base64.trim()) {
            Ok(b) => b,
            Err(err) => {
                return SaveReportResultJs {
                    ok: false,
                    cancelled: false,
                    path: None,
                    error: Some(format!("invalid PNG base64: {err}")),
                };
            }
        };
        let title = label_or(labels.as_ref().and_then(|l| l.title.as_deref()), "保存图片");
        let button = label_or(
            labels.as_ref().and_then(|l| l.button_label.as_deref()),
            "保存",
        );
        let filter = label_or(
            labels.as_ref().and_then(|l| l.filter_label.as_deref()),
            "PNG 图片",
        );
        let file_name = if default_name.trim().is_empty() {
            "output.png"
        } else {
            default_name.trim()
        };
        let dialog = SaveDialog::new()
            .title(title)
            .button_label(button)
            .file_name(file_name)
            .filter(FileFilter::new(filter, ["png"]));
        let picked = match pick_save_path(dialog).await {
            Ok(Some(path)) => path,
            Ok(None) => {
                return SaveReportResultJs {
                    ok: false,
                    cancelled: true,
                    path: None,
                    error: None,
                };
            }
            Err(err) => {
                return SaveReportResultJs {
                    ok: false,
                    cancelled: false,
                    path: None,
                    error: Some(err),
                };
            }
        };
        let path_str = picked.display().to_string();
        match blocking(move || crate::plugins::atomic_write_file(&picked, &bytes)).await {
            Ok(Ok(())) => SaveReportResultJs {
                ok: true,
                cancelled: false,
                path: Some(path_str),
                error: None,
            },
            Ok(Err(err)) => SaveReportResultJs {
                ok: false,
                cancelled: false,
                path: None,
                error: Some(err.to_string()),
            },
            Err(err) => SaveReportResultJs {
                ok: false,
                cancelled: false,
                path: None,
                error: Some(format!("save png task failed: {err}")),
            },
        }
    }

    /// Opens a native save dialog and atomically writes a UTF-8 text file (e.g. SVG / Markdown).
    pub async fn plugin_save_text_dialog(
        text: String,
        default_name: String,
        labels: Option<DialogLabelsJs>,
    ) -> SaveReportResultJs {
        let title = label_or(labels.as_ref().and_then(|l| l.title.as_deref()), "保存文件");
        let button = label_or(
            labels.as_ref().and_then(|l| l.button_label.as_deref()),
            "保存",
        );
        let file_name = if default_name.trim().is_empty() {
            "output.txt"
        } else {
            default_name.trim()
        };
        let mut dialog = SaveDialog::new()
            .title(title)
            .button_label(button)
            .file_name(file_name);
        if let Some(ext) = Path::new(file_name).extension().and_then(|e| e.to_str())
            && !ext.is_empty()
        {
            let filter = label_or(labels.as_ref().and_then(|l| l.filter_label.as_deref()), ext);
            dialog = dialog.filter(FileFilter::new(filter, [ext]));
        }
        let picked = match pick_save_path(dialog).await {
            Ok(Some(path)) => path,
            Ok(None) => {
                return SaveReportResultJs {
                    ok: false,
                    cancelled: true,
                    path: None,
                    error: None,
                };
            }
            Err(err) => {
                return SaveReportResultJs {
                    ok: false,
                    cancelled: false,
                    path: None,
                    error: Some(err),
                };
            }
        };
        let path_str = picked.display().to_string();
        let bytes = text.into_bytes();
        match blocking(move || crate::plugins::atomic_write_file(&picked, &bytes)).await {
            Ok(Ok(())) => SaveReportResultJs {
                ok: true,
                cancelled: false,
                path: Some(path_str),
                error: None,
            },
            Ok(Err(err)) => SaveReportResultJs {
                ok: false,
                cancelled: false,
                path: None,
                error: Some(err.to_string()),
            },
            Err(err) => SaveReportResultJs {
                ok: false,
                cancelled: false,
                path: None,
                error: Some(format!("save text task failed: {err}")),
            },
        }
    }

    /// Lists commands registered by enabled plugins matching `context` (`"global" | "problem" | "contest"`).
    pub fn plugin_list_commands(&self, context: Option<String>) -> JsonReportResultJs {
        self.state.plugin_list_commands(context)
    }

    /// Runs a plugin command `runCommand(commandId, ctx)` off the UI thread.
    pub async fn plugin_run_command(
        this: Instance<Self>,
        plugin_id: String,
        command_id: String,
        context_payload_json: String,
    ) -> JsonReportResultJs {
        if this.is_disposed() {
            return JsonReportResultJs::err("disposed", "preview disposed");
        }
        let state = this.with(|p| p.state.clone());
        blocking(move || state.plugin_run_command(&plugin_id, &command_id, &context_payload_json))
            .await
            .unwrap_or_else(|e| JsonReportResultJs::err("task", format!("run command failed: {e}")))
    }

    /// Lists all importers declared by enabled plugins as JSON.
    pub fn plugin_list_importers(&self) -> JsonReportResultJs {
        self.state.plugin_list_importers()
    }

    /// Picks file(s) via the native dialog, runs a plugin importer, and atomically applies the draft.
    pub async fn plugin_run_importer(
        this: Instance<Self>,
        plugin_id: String,
        importer_id: String,
        labels: Option<DialogLabelsJs>,
    ) -> JsonReportResultJs {
        if this.is_disposed() {
            return JsonReportResultJs::err("disposed", "preview disposed");
        }
        let exts = this.with(|p| p.state.plugin_importer_extensions(&plugin_id, &importer_id));
        let title = label_or(
            labels.as_ref().and_then(|l| l.title.as_deref()),
            "选择导入文件",
        );
        let button = label_or(
            labels.as_ref().and_then(|l| l.button_label.as_deref()),
            "导入",
        );
        let mut dialog = OpenDialog::files()
            .title(title)
            .button_label(button)
            .multiple(true);
        if !exts.is_empty() {
            let filter = label_or(
                labels.as_ref().and_then(|l| l.filter_label.as_deref()),
                "导入文件",
            );
            let ext_refs: Vec<&str> = exts.iter().map(String::as_str).collect();
            dialog = dialog.filter(FileFilter::new(filter, ext_refs));
        }
        let picked = match pick_paths(dialog).await {
            Ok(Some(paths)) if !paths.is_empty() => paths,
            Ok(_) => return JsonReportResultJs::err("cancelled", "import cancelled"),
            Err(err) => return JsonReportResultJs::err("io", err),
        };
        if this.is_disposed() {
            return JsonReportResultJs::err("disposed", "preview disposed");
        }
        let state = this.with(|p| p.state.clone());
        blocking(move || state.plugin_run_importer_paths(&plugin_id, &importer_id, &picked))
            .await
            .unwrap_or_else(|e| {
                JsonReportResultJs::err("task", format!("run importer failed: {e}"))
            })
    }

    /// Lists all exporters declared by enabled plugins as JSON.
    pub fn plugin_list_exporters(&self) -> JsonReportResultJs {
        self.state.plugin_list_exporters()
    }

    /// Runs a plugin exporter for a problem or contest and atomically writes output under `build/exports/`.
    pub async fn plugin_run_exporter(
        this: Instance<Self>,
        plugin_id: String,
        exporter_id: String,
        kind: String,
        id: String,
    ) -> JsonReportResultJs {
        if this.is_disposed() {
            return JsonReportResultJs::err("disposed", "preview disposed");
        }
        let state = this.with(|p| p.state.clone());
        blocking(move || state.plugin_run_exporter(&plugin_id, &exporter_id, &kind, &id))
            .await
            .unwrap_or_else(|e| {
                JsonReportResultJs::err("task", format!("run exporter failed: {e}"))
            })
    }
}

/// If `dir` is non-empty, returns a fresh `xcpc-example` subdirectory inside it
/// (`xcpc-example`, `xcpc-example-2`, ...) so existing files are never mixed with
/// example content.
pub(crate) fn parse_example_language(raw: Option<&str>) -> xcpc_domain::Language {
    match raw.map(str::trim) {
        Some(s) if s.to_ascii_lowercase().starts_with("en") => xcpc_domain::Language::English,
        _ => xcpc_domain::Language::Chinese,
    }
}

pub(crate) fn resolve_example_target_dir(dir: &Path) -> PathBuf {
    let is_non_empty = |p: &Path| {
        std::fs::read_dir(p)
            .ok()
            .and_then(|mut it| it.next())
            .is_some()
    };
    if !is_non_empty(dir) {
        return dir.to_path_buf();
    }
    let first = dir.join("xcpc-example");
    if !first.exists() || !is_non_empty(&first) {
        return first;
    }
    for n in 2u32..=1000 {
        let candidate = dir.join(format!("xcpc-example-{n}"));
        if !candidate.exists() || !is_non_empty(&candidate) {
            return candidate;
        }
    }
    first
}

impl Preview {
    /// Makes `ws` the active workspace: cancels in-flight renders, swaps watcher outside
    /// the mutex, refreshes the index, and returns the new [`WorkspaceInfo`].
    fn switch(&self, ws: Workspace) -> WorkspaceInfo {
        self.generation.fetch_add(1, Ordering::AcqRel);
        let old_watcher = self.state.install_workspace(ws);
        drop(old_watcher);
        self.workspace()
    }
}

const UI_FONT_STEMS: &[&str] = &[
    "JetBrainsMono-Regular",
    "JetBrainsMono-Medium",
    "JetBrainsMono-Bold",
    "JetBrainsMono-Italic",
    "JetBrainsMono-BoldItalic",
];

pub(crate) fn load_ui_fonts_from(dirs: &[PathBuf]) -> Vec<Vec<u8>> {
    let mut found_ttf: Vec<Option<PathBuf>> = vec![None; UI_FONT_STEMS.len()];
    let mut found_otf: Vec<Option<PathBuf>> = vec![None; UI_FONT_STEMS.len()];
    let mut fallback: Vec<PathBuf> = Vec::new();

    for dir in dirs {
        scan_ui_fonts_dir(dir, 0, &mut found_ttf, &mut found_otf, &mut fallback);
    }

    let mut chosen: Vec<PathBuf> = Vec::new();
    for (ttf, otf) in found_ttf.iter_mut().zip(&mut found_otf) {
        if let Some(p) = ttf.take().or_else(|| otf.take()) {
            chosen.push(p);
        }
    }
    if chosen.is_empty() {
        fallback.sort();
        fallback.dedup();
        chosen = fallback;
    }

    chosen
        .into_iter()
        .filter_map(|p| std::fs::read(p).ok())
        .filter(|b| !b.is_empty())
        .collect()
}

fn scan_ui_fonts_dir(
    dir: &Path,
    depth: usize,
    found_ttf: &mut [Option<PathBuf>],
    found_otf: &mut [Option<PathBuf>],
    fallback: &mut Vec<PathBuf>,
) {
    if depth > 4 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut paths: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    paths.sort();
    for path in paths {
        if path.is_dir() {
            scan_ui_fonts_dir(&path, depth + 1, found_ttf, found_otf, fallback);
            continue;
        }
        let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
            continue;
        };
        let is_ttf = ext.eq_ignore_ascii_case("ttf");
        let is_otf = ext.eq_ignore_ascii_case("otf");
        if !is_ttf && !is_otf {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        if let Some(idx) = UI_FONT_STEMS
            .iter()
            .position(|target| stem.eq_ignore_ascii_case(target))
        {
            let slot = if is_ttf {
                &mut found_ttf[idx]
            } else {
                &mut found_otf[idx]
            };
            if slot.is_none() {
                *slot = Some(path);
            }
        } else if stem.starts_with("JetBrainsMono") && !stem.starts_with("JetBrainsMonoNL") {
            fallback.push(path);
        }
    }
}

async fn open_in_external_editor(path: PathBuf) -> Result<(), String> {
    let spawned = {
        #[cfg(target_os = "macos")]
        {
            std::process::Command::new("open")
                .arg(&path)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
        }
        #[cfg(target_os = "windows")]
        {
            std::process::Command::new("cmd")
                .args(["/C", "start", ""])
                .arg(&path)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            std::process::Command::new("xdg-open")
                .arg(&path)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
        }
    };
    if spawned.is_ok() {
        return Ok(());
    }
    reveal_path(path).await
}

fn label_or<'a>(opt: Option<&'a str>, fallback: &'a str) -> &'a str {
    opt.map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(fallback)
}

#[cfg(windows)]
fn windows_user_locale() -> Option<String> {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetUserDefaultLocaleName(lp_locale_name: *mut u16, cch_locale_name: i32) -> i32;
    }
    let mut buf = [0u16; 85];
    // SAFETY: `buf` is a valid writable buffer of `buf.len()` UTF-16 code units.
    let len = unsafe { GetUserDefaultLocaleName(buf.as_mut_ptr(), buf.len() as i32) };
    if len > 1 {
        let slice = &buf[..(len as usize - 1)];
        let s = String::from_utf16_lossy(slice);
        if !s.trim().is_empty() {
            return Some(s);
        }
    }
    None
}

fn detect_system_locale() -> &'static str {
    #[cfg(windows)]
    if let Some(raw) = windows_user_locale() {
        return normalize_system_locale(&raw);
    }

    for var in ["LC_ALL", "LC_MESSAGES", "LANG", "LANGUAGE"] {
        if let Ok(val) = std::env::var(var)
            && !val.trim().is_empty()
        {
            return normalize_system_locale(&val);
        }
    }
    "en"
}

pub(crate) fn normalize_system_locale(raw: &str) -> &'static str {
    let Some(first) = raw.split(':').map(str::trim).find(|s| !s.is_empty()) else {
        return "en";
    };
    let no_mod = first.split('@').next().unwrap_or(first);
    let no_enc = no_mod.split('.').next().unwrap_or(no_mod);
    let norm = no_enc.trim().replace('_', "-").to_ascii_lowercase();
    if norm.starts_with("zh") {
        "zh-CN"
    } else {
        "en"
    }
}

#[cfg(test)]
mod tests;

//! The desktop shell's Rust side: what `App.tsx` imports from `xcpc-workstation/native`.
//! Their TypeScript declarations are generated into `.nsh/types/xcpc-workstation/native.d.ts`.
//!
//! Only thin NSH glue lives here (`#[native_module]`, `#[class] Preview`, async `blocking`
//! dispatch, native file dialogs, JS DTO mappings, `PageImage`, and Linux chrome). All
//! pure-Rust business logic, state machines, and unit tests live in `crates/xcpc-app`.

mod linux_chrome;
mod page_image;
mod types;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;

use nsh_base as _;
use nsh_core::element::JsError;
use nsh_core::file_dialog::{FileFilter, OpenDialog, SaveDialog};
use nsh_core::prelude::*;
use xcpc_app::CancelToken;
use xcpc_storage::Workspace;

pub use crate::page_image::{PageImage, PageImageProps};
pub use crate::types::{
    AboutInfoJs, ActionResult, AddedTestResultJs, AdoptSolutionResultJs, AggregatedRepoPluginJs,
    AppPrefsJs, AssetReferenceJs, BatchDeleteItemJs, BatchDeleteResultJs, BatchExportRequestJs,
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
    PluginCredentialFieldJs, PluginKvResultJs, PluginPickFileResultJs, PluginPickFilesResultJs,
    PluginPickedFileJs, PluginRenderPngResultJs, PluginRepoCatalogJs, PluginRepoConfigJs,
    PluginSummaryJs, PluginUiHtmlJs, PluginWatchEventJs, PluginWorkspaceResultJs,
    PluginsOverviewJs, PolygonImportResultJs, PreviewDiagnostic, PreviewLayer, PreviewLocationJs,
    PreviewPage, PreviewRequest, PreviewResult, PreviewStart, PreviewView, ProblemAssetJs,
    ProblemDetailJs, ProblemMetaPatchJs, ProblemSearchQuery, ProblemSourceJs, ProblemSummaryJs,
    RatingBucketJs, RepoCatalogResultJs, RepoFetchErrorJs, RepoPluginEntryJs,
    RepoPluginPermissionJs, ResolvedProblemUrlJs, SamplePairJs, SavePackageFileResultJs,
    SaveReportResultJs, SaveResult, SourceLocationJs, SyncReportJs, TagCountJs, TestCasePreviewJs,
    ThemeColorEntryJs, ThemeOverridesJs, ToolchainStatusJs, TraceExportResultJs, TraceStatusJs,
    TrainingSubmissionsQueryJs, UnfreezeDiffResultJs, UnfreezeProblemDiffJs, VerifyPluginResultJs,
    WatchEventJs, WorkspaceContest, WorkspaceFacets, WorkspaceFile, WorkspaceInfo, WorkspaceItem,
    WorkspaceProblem,
};

/// The crate's UI set (`nsh.json` `"rust"`).
#[native_module("xcpc-workstation/native")]
fn native(registry: &mut Registry) {
    registry.class::<Preview>();
    registry.register::<PageImage>();
}

/// Thin NSH desktop wrapper around [`xcpc_app::AppController`].
#[class]
pub struct Preview {
    app: xcpc_app::AppController,
}

#[class]
impl Preview {
    #[allow(clippy::new_without_default)] // the JS constructor
    /// Opens the workspace named by `XCPC_WORKSPACE`, else the one containing the current
    /// directory, else the most recent workspace recorded in `<dataDir>/prefs.json` (or
    /// legacy `last-workspace`). Pass `process.dataDir` for persistence; `null` disables it.
    pub fn new(data_dir: Option<String>) -> Self {
        linux_chrome::strip_titlebar_icon_once();
        Preview {
            app: xcpc_app::AppController::new(data_dir),
        }
    }

    /// The workspace of the test fixture `contest-zh`.
    pub fn fixture() -> String {
        xcpc_app::AppController::fixture()
    }

    /// Returns raw font file bytes for JetBrains Mono (Regular, Medium, Bold, Italic,
    /// BoldItalic) discovered from `XCPC_FONT_DIRS` and the executable's sibling `fonts/`
    /// directory, ready to pass to `registerFont(bytes)`.
    pub fn ui_fonts() -> Vec<Vec<u8>> {
        xcpc_app::AppController::ui_fonts()
    }

    /// Returns cached `@font-face` CSS rules embedding the host's bundled Inter and
    /// JetBrains Mono WOFF2 fonts as `data:font/woff2;base64,...` URIs so sandboxed
    /// WebView2/WebKitGTK plugins share the exact same typography as the native shell.
    pub fn webview_font_css() -> String {
        use base64::Engine as _;
        use std::sync::OnceLock;
        static FONT_CSS: OnceLock<String> = OnceLock::new();
        FONT_CSS
            .get_or_init(|| {
                let b64 = |bytes: &[u8]| base64::engine::general_purpose::STANDARD.encode(bytes);
                let inter = b64(include_bytes!("ui/fonts/InterVariable.woff2"));
                let mono_reg = b64(include_bytes!("ui/fonts/JetBrainsMono-Regular.woff2"));
                let mono_sb = b64(include_bytes!("ui/fonts/JetBrainsMono-SemiBold.woff2"));
                format!(
                    "@font-face{{font-family:'Inter';font-style:normal;font-weight:100 900;font-display:swap;src:url('data:font/woff2;base64,{inter}') format('woff2');}}\n\
                     @font-face{{font-family:'JetBrains Mono';font-style:normal;font-weight:400;font-display:swap;src:url('data:font/woff2;base64,{mono_reg}') format('woff2');}}\n\
                     @font-face{{font-family:'JetBrains Mono';font-style:normal;font-weight:600;font-display:swap;src:url('data:font/woff2;base64,{mono_sb}') format('woff2');}}"
                )
            })
            .clone()
    }

    /// Returns build and runtime diagnostic info for the About view.
    pub fn about_info() -> AboutInfoJs {
        xcpc_app::AppController::about_info().into()
    }

    /// Reclaims idle pixel caches, Typst AST/comemo caches, and cold OS working-set pages.
    pub fn reclaim_memory(&self, clear_pixels: bool) {
        self.app.reclaim_memory(clear_pixels);
    }

    /// Returns current offline performance trace status and Rust monotonic clock reading.
    pub fn trace_status() -> TraceStatusJs {
        xcpc_app::AppController::trace_status().into()
    }

    /// Starts (or restarts) an offline performance trace recording session.
    pub fn trace_start() -> TraceStatusJs {
        xcpc_app::AppController::trace_start().into()
    }

    /// Stops offline performance trace recording while keeping buffered events for export.
    pub fn trace_stop() -> TraceStatusJs {
        xcpc_app::AppController::trace_stop().into()
    }

    /// Returns true when `XCPC_O11Y=1` requested opening the diagnostics window on launch.
    pub fn o11y_env_enabled() -> bool {
        xcpc_app::AppController::o11y_env_enabled()
    }

    /// Development-only motion slow-down multiplier (`XCPC_MOTION_SLOW=<factor>`).
    /// Release builds always return `1.0` without reading the environment.
    pub fn motion_slow_factor() -> f64 {
        #[cfg(debug_assertions)]
        {
            std::env::var("XCPC_MOTION_SLOW")
                .ok()
                .and_then(|v| v.trim().parse::<f64>().ok())
                .filter(|&v| v.is_finite() && v > 1.0)
                .unwrap_or(1.0)
        }
        #[cfg(not(debug_assertions))]
        {
            1.0
        }
    }

    /// Polls incremental Rust trace events, structured logs, and pixel cache stats as JSON.
    pub fn o11y_poll_delta(after_event_seq: f64, after_log_seq: f64) -> String {
        xcpc_app::AppController::o11y_poll_delta(after_event_seq, after_log_seq)
    }

    /// Clears buffered Rust trace events, always-on logs, and slow events.
    pub fn o11y_clear() {
        xcpc_app::AppController::o11y_clear();
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
        self.app.open_workspace(path, read_only).into()
    }

    /// Initializes a new XCPC workspace at `path` (creating `workspace.toml`, `problems/`,
    /// `contests/`, `.gitignore`, and a static `AGENTS.md` if absent) and switches to it.
    pub fn init_workspace(&self, path: String, template: Option<String>) -> WorkspaceInfo {
        self.app.init_workspace(path, template).into()
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
        self.app
            .init_example_workspace(dir, template, language)
            .into()
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
        let start = this.with(|p| p.app.state.workspace_root());
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
        let lang = xcpc_app::parse_example_language(language.as_deref());
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
            let target_dir = xcpc_app::resolve_example_target_dir(&dir);
            match xcpc_storage::init_example_workspace_with_options(
                &target_dir,
                template.as_deref(),
                lang,
            ) {
                Ok(ws) => p.app.switch(ws).into(),
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
        let start = this.with(|p| p.app.state.workspace_root());
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
        let start = this.with(|p| p.app.state.workspace_root());
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
                Ok(ws) => p.app.switch(ws).into(),
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
        self.app.workspace().into()
    }

    /// Recently opened workspace directories, most recent first.
    pub fn recent_workspaces(&self) -> Vec<String> {
        self.app.recent_workspaces()
    }

    /// Generates the ready-to-paste JSON snippet for configuring `xcpc mcp` against the
    /// currently open workspace.
    pub fn mcp_config_snippet(&self) -> String {
        self.app.mcp_config_snippet()
    }

    // ---- File read / save & live watcher -----------------------------------------------

    /// Reads a file of the workspace for the editor together with its SHA-256 content hash
    /// (empty string when the file does not exist yet).
    pub fn read(&self, path: String) -> WorkspaceFile {
        self.app.read(path).into()
    }

    /// Atomically saves a workspace file, checking `expectedHash` when provided.
    ///
    /// Returns `{ conflict: true }` without overwriting when the file changed on disk since
    /// `expectedHash`. Successful writes are recorded in `SelfWriteTracker` so they are not
    /// echoed back by `nextChange()`, incrementally refresh the index, and preserve the
    /// compiled preview cache when the saved Markdown matches the already-compiled state.
    pub fn save(&self, path: String, text: String, expected_hash: Option<String>) -> SaveResult {
        self.app.save(path, text, expected_hash).into()
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
            let session = p.app.state.watch_session();
            (Arc::downgrade(&p.app.state), session)
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
                    Ok(ev) => ev.into(),
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
            state.apply_watch_event(session_id, event).into()
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
        let (state, generation) = this.with(|p| (p.app.state.clone(), p.app.generation.clone()));
        let my_gen = generation.fetch_add(1, Ordering::AcqRel).wrapping_add(1);
        blocking(move || {
            let cancel = CancelToken {
                generation: my_gen,
                latest: &generation,
            };
            if cancel.is_cancelled() {
                return PreviewResult::default();
            }
            let req: xcpc_app::PreviewRequest = request.into();
            let view: Vec<xcpc_app::PreviewView> = view.into_iter().map(Into::into).collect();
            state.run(&req, scale as f32, &view, cancel).into()
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
        let (state, generation) = this.with(|p| (p.app.state.clone(), p.app.generation.clone()));
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
        let state = this.with(|p| p.app.state.clone());
        blocking(move || {
            state
                .render_math_preview(&latex, display, dark, scale as f32)
                .into()
        })
        .await
        .unwrap_or_default()
    }

    /// Maps a click at `(xPt, yPt)` (typographic points from the top-left of 0-based `page`)
    /// on the currently cached preview back to its 1-based Markdown source location.
    /// Returns `null` immediately without blocking if a compilation is in flight.
    pub fn locate_click(&self, page: u32, x_pt: f64, y_pt: f64) -> Option<SourceLocationJs> {
        self.app.locate_click(page, x_pt, y_pt).map(Into::into)
    }

    /// Maps a 1-based `line` in workspace-relative `file` (e.g. `"problems/a/statement.md"`)
    /// to its position `(page, yPt)` in the currently cached preview. Never blocks on an
    /// active compilation; falls back to the problem's first page when exact anchors are
    /// unavailable.
    pub fn locate_source(&self, file: String, line: u32) -> Option<PreviewLocationJs> {
        self.app.locate_source(file, line).map(Into::into)
    }

    /// Where `text`'s Markdown structure lies (headings, emphasis, math, code, markup), as
    /// the conversion reads it, for the editor to style its source.
    pub fn markdown_spans(text: String) -> Vec<MarkdownSpanJs> {
        xcpc_app::AppController::markdown_spans(text)
            .into_iter()
            .map(Into::into)
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
        let state = this.with(|p| p.app.state.clone());
        let req: xcpc_app::PreviewRequest = request.into();
        blocking(move || state.export(&req).into())
            .await
            .unwrap_or_else(|e| WorkspaceFile {
                error: Some(format!("export failed: {e}")),
                ..Default::default()
            })
    }

    /// Reports independent PDF freshness (`exists`, `fresh`, `relPath`, `absPath`) for both
    /// the statements book and the editorial book of `request`'s target.
    pub fn export_status(&self, request: PreviewRequest) -> ExportBookStatusJs {
        self.app.export_status(request.into()).into()
    }

    /// Off-UI-thread async variant of [`Self::export_status`] for Studio's status bar.
    pub async fn export_status_async(
        this: Instance<Self>,
        request: PreviewRequest,
    ) -> ExportBookStatusJs {
        if this.is_disposed() {
            return ExportBookStatusJs::default();
        }
        let state = this.with(|p| p.app.state.clone());
        let req: xcpc_app::PreviewRequest = request.into();
        blocking(move || state.export_status(&req).into())
            .await
            .unwrap_or_default()
    }

    /// Reveals a file or directory (absolute or workspace-relative path) inside the open
    /// workspace in the system file manager. Rejects paths outside the workspace or missing
    /// targets.
    pub async fn reveal_in_folder(this: Instance<Self>, path: String) -> ActionResult {
        if this.is_disposed() {
            return ActionResult::err("disposed", "preview disposed");
        }
        let resolved = this.with(|p| p.app.state.resolve_reveal_path(&path));
        let abs = match resolved {
            Ok(abs) => abs,
            Err(err) => return err.into(),
        };
        match reveal_path(abs).await {
            Ok(()) => ActionResult::ok(),
            Err(e) => ActionResult::err("io", e),
        }
    }

    /// Loads user-global theme overrides from `<dataDir>/theme.toml`.
    pub fn load_theme_overrides(&self) -> ThemeOverridesJs {
        self.app.load_theme_overrides().into()
    }

    /// Detects the operating system UI locale and normalizes it to `"zh-CN"` or `"en"`.
    pub fn system_locale(&self) -> String {
        self.app.system_locale()
    }

    /// Ensures `<dataDir>/theme.toml` exists and reveals it in the system file manager.
    pub async fn open_theme_file(this: Instance<Self>) -> ActionResult {
        if this.is_disposed() {
            return ActionResult::err("disposed", "preview disposed");
        }
        let ensured = this.with(|p| xcpc_app::ensure_theme_file(p.app.state.data_dir.as_deref()));
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
        self.app
            .search_problems(query.into())
            .into_iter()
            .map(Into::into)
            .collect()
    }

    /// Returns sorted distinct tags and external platforms across all indexed problems.
    pub fn workspace_facets(&self) -> WorkspaceFacets {
        self.app.workspace_facets().into()
    }

    /// Loads full problem details: metadata, statements & editorial with hashes, samples,
    /// solution filenames, and referencing contests.
    pub fn problem_detail(&self, id: String) -> ProblemDetailJs {
        self.app.problem_detail(id).into()
    }

    /// Reads one solution file under `problems/<id>/solutions/<name>` read-only.
    pub fn read_solution(&self, id: String, name: String) -> WorkspaceFile {
        self.app.read_solution(id, name).into()
    }

    /// Assembles a standalone Markdown statement with `samples/` embedded before `## 提示` / `## Note`.
    pub fn export_markdown_with_samples(
        &self,
        id: String,
        unsaved_statement: Option<String>,
    ) -> Result<String, JsError> {
        self.app
            .export_markdown_with_samples(id, unsaved_statement)
            .map_err(JsError::Error)
    }

    /// Creates a new problem directory with `problem.toml`, a `statement.md` skeleton, and
    /// an empty `samples/` directory, refreshing the in-memory index.
    pub fn create_problem(&self, id: String, title: String) -> ActionResult {
        self.app.create_problem(id, title).into()
    }

    /// Recursively duplicates `problems/<sourceId>/` into `problems/<newId>/`, optionally
    /// updating `title` in `problem.toml`, and refreshes the in-memory index.
    pub fn duplicate_problem(
        &self,
        source_id: String,
        new_id: String,
        title: Option<String>,
    ) -> ActionResult {
        self.app.duplicate_problem(source_id, new_id, title).into()
    }

    /// Partially updates `problems/<id>/problem.toml` via `toml_edit`, preserving comments,
    /// key order, and unknown fields.
    pub fn update_problem_meta(&self, id: String, patch: ProblemMetaPatchJs) -> ActionResult {
        self.app.update_problem_meta(id, patch.into()).into()
    }

    /// Deletes a problem from the library. Refuses with `code: "problem_referenced"` and
    /// `referencingContests` when any unfrozen contest references it.
    pub fn delete_problem(&self, id: String) -> ActionResult {
        self.app.delete_problem(id).into()
    }

    // ---- Persistent problem groups & batch operations ----------------------------------

    /// Creates a new persistent problem group in `workspace.toml`.
    pub fn create_group(&self, name: String) -> ActionResult {
        self.app.create_group(name).into()
    }

    /// Renames a persistent problem group in `workspace.toml` and all member `problem.toml` files.
    pub fn rename_group(&self, old_name: String, new_name: String) -> ActionResult {
        self.app.rename_group(old_name, new_name).into()
    }

    /// Deletes a persistent problem group from `workspace.toml` and ungroups member problems
    /// without deleting any problems.
    pub fn delete_group(&self, name: String) -> ActionResult {
        self.app.delete_group(name).into()
    }

    /// Reorders declared persistent problem groups in `workspace.toml`.
    pub fn reorder_groups(&self, names: Vec<String>) -> ActionResult {
        self.app.reorder_groups(names).into()
    }

    /// Sets or clears the persistent `group` on multiple problems in one batch.
    pub fn batch_set_group(
        &self,
        ids: Vec<String>,
        group: Option<String>,
    ) -> BatchMutationResultJs {
        self.app.batch_set_group(ids, group).into()
    }

    /// Adds and/or removes tags across multiple problems in one batch.
    pub fn batch_edit_tags(
        &self,
        ids: Vec<String>,
        add: Vec<String>,
        remove: Vec<String>,
    ) -> BatchMutationResultJs {
        self.app.batch_edit_tags(ids, add, remove).into()
    }

    /// Sets or clears `rating` across multiple problems in one batch.
    pub fn batch_set_rating(&self, ids: Vec<String>, rating: Option<u32>) -> BatchMutationResultJs {
        self.app.batch_set_rating(ids, rating).into()
    }

    /// Atomically adds multiple problems to an existing contest, refusing if frozen/archived
    /// or if the resulting total exceeds 26 problems.
    pub fn batch_add_to_contest(
        &self,
        contest_id: String,
        problem_ids: Vec<String>,
    ) -> BatchMutationResultJs {
        self.app
            .batch_add_to_contest(contest_id, problem_ids)
            .into()
    }

    /// Batch-deletes multiple problems, skipping any referenced by unfrozen contests with
    /// `code: "problem_referenced"` and `referencingContests`.
    pub fn batch_delete_problems(&self, ids: Vec<String>) -> BatchDeleteResultJs {
        self.app.batch_delete_problems(ids).into()
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
        self.app.start_batch_build(ids, mode, contest_id).into()
    }

    /// Starts a background batch export task (`format`: `"domjudge"`, `"hydro"`, or
    /// `"merged_pdf"`). Stream progress via `nextBatchEvent(taskId)`.
    pub fn start_batch_export(&self, req: BatchExportRequestJs) -> BatchStartResultJs {
        self.app.start_batch_export(req.into()).into()
    }

    /// Cooperatively cancels the currently running background batch task, if any.
    pub fn cancel_batch_task(&self) -> ActionResult {
        self.app.cancel_batch_task().into()
    }

    /// Returns the latest snapshot of the active or most recently completed batch task.
    pub fn batch_task_status(&self) -> BatchTaskSnapshotJs {
        self.app.batch_task_status().into()
    }

    /// Polling-free streaming event receiver for `taskId` returned by `startBatchBuild` or
    /// `startBatchExport`. Blocks off the UI thread until the next event arrives or the task
    /// stream closes (`kind === "closed"`).
    pub async fn next_batch_event(this: Instance<Self>, task_id: u32) -> BatchTaskEventJs {
        if this.is_disposed() {
            return BatchTaskEventJs::closed(task_id, BatchTaskSnapshotJs::default());
        }
        let (rx_opt, fallback_snap) = this.with(|p| p.app.state.batch_event_receiver(task_id));
        let fallback_snap: BatchTaskSnapshotJs = fallback_snap.into();
        let Some(rx) = rx_opt else {
            return BatchTaskEventJs::closed(task_id, fallback_snap);
        };
        blocking(move || {
            let guard = match rx.lock() {
                Ok(g) => g,
                Err(_) => return BatchTaskEventJs::closed(task_id, fallback_snap),
            };
            match guard.recv() {
                Ok(ev) => ev.into(),
                Err(_) => BatchTaskEventJs::closed(task_id, fallback_snap),
            }
        })
        .await
        .unwrap_or_else(|_| BatchTaskEventJs::closed(task_id, BatchTaskSnapshotJs::default()))
    }

    /// Saves `samples` into `problems/<id>/samples/` as `01.in`/`01.ans`..`NN.in`/`NN.ans`,
    /// stripping UTF-8 BOM and `\r` atomically and refreshing cache and index.
    pub fn save_samples(&self, id: String, samples: Vec<SamplePairJs>) -> ActionResult {
        self.app
            .save_samples(id, samples.into_iter().map(Into::into).collect())
            .into()
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
            p.app
                .state
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
        let state = this.with(|p| p.app.state.clone());
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
        self.app
            .import_image_bytes(target, id, label, ext, bytes)
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
        let state = this.with(|p| p.app.state.clone());
        blocking(move || {
            state
                .list_problem_assets(&target, &id, label.as_deref())
                .into_iter()
                .map(Into::into)
                .collect()
        })
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
        self.app
            .rename_problem_asset(target, id, label, old_name, new_name)
            .into()
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
        self.app
            .delete_problem_asset(target, id, label, name)
            .into()
    }

    /// Batch-deletes all unreferenced assets inside the target problem's `assets/` directory.
    pub fn clean_unused_assets(
        &self,
        target: String,
        id: String,
        label: Option<String>,
    ) -> CleanUnusedAssetsResultJs {
        self.app.clean_unused_assets(target, id, label).into()
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
        self.app
            .set_asset_width(target, id, label, asset_name, width_pct, doc_path)
            .into()
    }

    // ---- Contest composition, overrides, stats, freeze & unfreeze ----------------------

    /// Loads full contest metadata and per-item effective/original values with override flags.
    pub fn contest_detail(&self, id: String) -> ContestDetailJs {
        self.app.contest_detail(id).into()
    }

    /// Computes 8-tier difficulty distribution and tag counts for `id`.
    pub fn contest_stats(&self, id: String) -> ContestStatsJs {
        self.app.contest_stats(id).into()
    }

    /// Creates a new empty contest via `compose_contest(id, [], { name })`, refusing if `id`
    /// already exists.
    pub fn create_contest(&self, id: String, name: String) -> ActionResult {
        self.app.create_contest(id, name).into()
    }

    /// Sets the ordered problem list for contest `id`, re-labelling `A..Z`, preserving
    /// existing overrides on kept problems, and removing overrides for dropped problems.
    pub fn compose_contest(&self, id: String, problem_ids: Vec<String>) -> ActionResult {
        self.app.compose_contest(id, problem_ids).into()
    }

    /// Partially updates `contests/<id>/contest.toml` metadata via `toml_edit`.
    pub fn update_contest_meta(&self, id: String, patch: ContestMetaPatchJs) -> ActionResult {
        self.app.update_contest_meta(id, patch.into()).into()
    }

    /// Archives or unarchives contest `id` (`archived = true` or removes `archived` in `contest.toml`).
    pub fn set_contest_archived(&self, id: String, archived: bool) -> ActionResult {
        self.app.set_contest_archived(id, archived).into()
    }

    /// Sets or clears metadata overrides for `label` inside `contestId`. Passing `null`
    /// deletes that override; passing the original library/contest value auto-deletes it.
    pub fn set_override(
        &self,
        contest_id: String,
        label: String,
        patch: OverridePatchJs,
    ) -> ActionResult {
        self.app
            .set_override(contest_id, label, patch.into())
            .into()
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
        let state = this.with(|p| p.app.state.clone());
        blocking(move || state.freeze_contest(&id).into())
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
        self.app.unfreeze_diff(id).into()
    }

    /// Unfreezes a frozen contest (GUI-only, after human confirmation), deleting `snapshot/`
    /// and restoring live references.
    pub fn unfreeze_contest(&self, id: String) -> ActionResult {
        self.app.unfreeze_contest(id).into()
    }

    // ---- Templates & declarative schemas -----------------------------------------------

    /// Returns JSON `{ default, workspaceOptions, templates, presets, fonts }` describing
    /// all workspace templates (`templates/*.typ`) with their evaluated `#let schema`
    /// definitions, installable preset seeds, workspace-level default options, and loaded
    /// font families.
    pub fn list_templates_json(&self) -> String {
        self.app.list_templates_json()
    }

    /// Updates `workspace.toml`'s default `template` and/or `[template-options.<tpl>]`.
    pub fn update_workspace_template(
        &self,
        template: Option<String>,
        clear_template: bool,
        options_json: Option<String>,
    ) -> ActionResult {
        self.app
            .update_workspace_template(template, clear_template, options_json)
            .into()
    }

    /// Forks a workspace template `source` into `templates/<newName>.typ`.
    pub fn fork_template(&self, source: String, new_name: String) -> ActionResult {
        self.app.fork_template(source, new_name).into()
    }

    /// Copies a mounted plugin template (`plugin:<pluginId>/<templateId>`) into `templates/`.
    pub fn eject_plugin_template(
        &self,
        plugin_id: String,
        template_id: String,
        target_name: Option<String>,
    ) -> JsonReportResultJs {
        self.app
            .eject_plugin_template(plugin_id, template_id, target_name)
            .into()
    }

    /// Synchronizes mounted template plugins and invalidates template caches.
    pub fn sync_plugin_mounts(&self) {
        self.app.sync_plugin_mounts();
    }

    /// Installs a bundled preset seed (`olymp-classic`, `cnoi-classic`, `minimal`) into
    /// `templates/<targetName>.typ` (defaulting to `<presetId>`).
    pub fn install_preset_template(
        &self,
        preset_id: String,
        target_name: Option<String>,
    ) -> ActionResult {
        self.app
            .install_preset_template(preset_id, target_name)
            .into()
    }

    /// Copies the contest's currently effective template to `contests/<contestId>/template.typ`
    /// so it can be edited independently for this contest.
    pub fn detach_contest_template(&self, contest_id: String) -> ActionResult {
        self.app.detach_contest_template(contest_id).into()
    }

    /// Removes `contests/<contestId>/template.typ`, restoring reference to workspace templates.
    pub fn remove_contest_template(&self, contest_id: String) -> ActionResult {
        self.app.remove_contest_template(contest_id).into()
    }

    /// Opens `templates/<name>.typ` with the system's default external editor (falling
    /// back to revealing it in the file manager if spawning fails).
    pub async fn open_template_file(this: Instance<Self>, name: String) -> ActionResult {
        if this.is_disposed() {
            return ActionResult::err("disposed", "preview disposed");
        }
        let resolved = this.with(|p| p.app.state.resolve_template_file(&name));
        let path = match resolved {
            Ok(p) => p,
            Err(err) => return err.into(),
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
        let resolved = this.with(|p| p.app.state.resolve_contest_template_file(&contest_id));
        let path = match resolved {
            Ok(p) => p,
            Err(err) => return err.into(),
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
            p.app
                .state
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
        let state = this.with(|p| p.app.state.clone());
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
        let ensured = this.with(|p| p.app.state.ensure_preamble_file());
        let path = match ensured {
            Ok(p) => p,
            Err(err) => return err.into(),
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
        self.app
            .fuzzy_jump(query, active_contest)
            .into_iter()
            .map(Into::into)
            .collect()
    }

    /// Records a visit (`"problem:<id>"`, `"contest:<id>"`, or
    /// `"contest_item:<contestId>:<label>"`) for Frecency ranking and persists `prefs.json`.
    pub fn record_visit(&self, key: String) {
        self.app.record_visit(key)
    }

    /// Loads persisted desktop preferences from `<dataDir>/prefs.json`.
    pub fn load_prefs(&self) -> AppPrefsJs {
        self.app.load_prefs().into()
    }

    /// Persists desktop preferences atomically to `<dataDir>/prefs.json` (preserving
    /// persisted Frecency history).
    pub fn save_prefs(&self, prefs: AppPrefsJs) {
        self.app.save_prefs(prefs.into())
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
        let state = this.with(|p| p.app.state.clone());
        let scale = render_scale.unwrap_or(1.0) as f32;
        blocking(move || state.save_cold_start_snapshot(&meta_json, &pages, scale))
            .await
            .unwrap_or(false)
    }

    /// Restores the cold-start preview snapshot from `<dataDir>/snapshots/` if it matches
    /// the active workspace and current on-disk content hash; returns `null` otherwise.
    pub fn load_cold_start_snapshot(&self) -> Option<ColdStartSnapshotJs> {
        self.app.load_cold_start_snapshot().map(Into::into)
    }

    // ---- Phase 3A: Local Polygon toolchain & package build -----------------------------

    /// Detects host C++ compilers (`g++` / `clang++`) taking persisted `cxxOverride` and
    /// `XCPC_CXX` into account.
    pub fn toolchain_status(&self) -> ToolchainStatusJs {
        self.app.toolchain_status().into()
    }

    /// Detects host C++ compilers off the UI thread so cold launch and Build tab entry
    /// never block a frame on `g++ --version` / `clang++ --version` subprocess probing.
    pub async fn toolchain_status_async(this: Instance<Self>) -> ToolchainStatusJs {
        if this.is_disposed() {
            return ToolchainStatusJs::default();
        }
        let state = this.with(|p| p.app.state.clone());
        blocking(move || state.toolchain_status().into())
            .await
            .unwrap_or_default()
    }

    /// Persists an explicit C++ compiler path override (passing `null` or empty clears it)
    /// and returns the refreshed toolchain detection status.
    pub fn set_cxx_override(&self, path: Option<String>) -> ToolchainStatusJs {
        self.app.set_cxx_override(path).into()
    }

    /// Fast O(1) check for whether `slug` has a local Polygon package.
    pub fn has_package(&self, slug: String) -> bool {
        self.app.has_package(slug)
    }

    /// Pure read-only inspection of a problem package (`hasPackage`, freshness, files,
    /// expanded tests, latest report, gates, and next steps).
    /// With `contest_id`, freshness follows that contest's overridden limits.
    pub fn inspect_package(
        &self,
        slug: String,
        contest_id: Option<String>,
    ) -> PackageInspectionResultJs {
        self.app.inspect_package(slug, contest_id).into()
    }

    /// Off-UI-thread async variant of [`Self::inspect_package`] so Studio never blocks the
    /// UI thread while hashing package files.
    pub async fn inspect_package_async(
        this: Instance<Self>,
        slug: String,
        contest_id: Option<String>,
    ) -> PackageInspectionResultJs {
        if this.is_disposed() {
            return PackageInspectionResultJs {
                ok: false,
                error: Some("preview disposed".into()),
                ..Default::default()
            };
        }
        let state = this.with(|p| p.app.state.clone());
        blocking(move || {
            state
                .inspect_package_in(&slug, contest_id.as_deref())
                .into()
        })
        .await
        .unwrap_or_else(|e| PackageInspectionResultJs {
            ok: false,
            error: Some(format!("inspect_package task failed: {e}")),
            ..Default::default()
        })
    }

    /// Scaffolds a problem package with `testlib.h` templates without overwriting existing
    /// files, and returns the refreshed inspection JSON.
    pub fn init_package(&self, slug: String) -> PackageInitResultJs {
        self.app.init_package(slug).into()
    }

    /// Scaffolds a problem package with optional Phase 3C features (`interactive`, `runs`, `subtasks`).
    pub fn init_package_with_options(
        &self,
        slug: String,
        interactive: bool,
        runs: Option<u32>,
        subtasks: bool,
    ) -> PackageInitResultJs {
        self.app
            .init_package_with_options(slug, interactive, runs, subtasks)
            .into()
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
        self.app
            .start_package_build(slug, mode, tests, solutions, contest_id)
            .into()
    }

    /// Signals cancellation to the currently running background package build, if any.
    pub fn cancel_package_build(&self) -> ActionResult {
        self.app.cancel_package_build().into()
    }

    /// Returns the latest progress and completion snapshot of the background package build.
    pub fn package_build_status(&self) -> PackageBuildStatusJs {
        self.app.package_build_status().into()
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
        let resolved = this.with(|p| p.app.state.resolve_package_file(&slug, &rel_path));
        let path = match resolved {
            Ok(p) => p,
            Err(err) => return err.into(),
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
        self.app
            .inspect_test_case(slug, selector, head_lines, tail_lines)
            .into()
    }

    /// Updates `[package].checker` in `problem.toml` (built-in checker name or `"checker.cpp"`).
    pub fn set_package_checker(&self, slug: String, checker: String) -> ActionResult {
        self.app.set_package_checker(slug, checker).into()
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
        self.app
            .set_solution_config(slug, name, is_main, expect, tags)
            .into()
    }

    /// Creates a new boilerplate package source file (`solution`, `generator`, `validator`,
    /// `custom_checker`, `validator_self_test_valid`, `validator_self_test_invalid`).
    pub fn create_package_source(
        &self,
        slug: String,
        kind: String,
        name: String,
    ) -> PackageCreateResultJs {
        self.app.create_package_source(slug, kind, name).into()
    }

    /// Adds a hand-written test file under `tests/` and appends a `[[test]]` rule.
    pub fn add_hand_test(
        &self,
        slug: String,
        name: Option<String>,
        tag: Option<String>,
        content: String,
    ) -> AddedTestResultJs {
        self.app.add_hand_test(slug, name, tag, content).into()
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
        self.app
            .add_generator_test(slug, generator, args, repeat, tag, name)
            .into()
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
        self.app
            .update_generator_test(slug, rule_index, generator, args, repeat, tag, name)
            .into()
    }

    /// Removes the 0-based `[[test]]` rule from `problem.toml`.
    pub fn remove_test_rule(&self, slug: String, rule_index: u32) -> ActionResult {
        self.app.remove_test_rule(slug, rule_index).into()
    }

    /// Deletes a solution source file and removes its `[solutions.<name>]` table from `problem.toml`.
    pub fn delete_solution(&self, slug: String, name: String) -> ActionResult {
        self.app.delete_solution(slug, name).into()
    }

    /// Deletes a generator source file and optionally removes referencing `[[test]]` rules.
    pub fn delete_generator(
        &self,
        slug: String,
        filename: String,
        remove_referencing_tests: bool,
    ) -> ActionResult {
        self.app
            .delete_generator(slug, filename, remove_referencing_tests)
            .into()
    }

    /// Deletes a validator or checker self-test file inside `problems/<slug>/tests/{validator,checker}/`.
    pub fn delete_self_test(&self, slug: String, rel_path: String) -> ActionResult {
        self.app.delete_self_test(slug, rel_path).into()
    }

    /// Deletes `interactor.cpp` (or versioned C++ variants) from `problems/<slug>/`.
    pub fn delete_interactor(&self, slug: String) -> ActionResult {
        self.app.delete_interactor(slug).into()
    }

    /// Updates `[package].runs` in `problem.toml` (`null` or `1` returns to single-run default).
    pub fn set_package_runs(&self, slug: String, runs: Option<u32>) -> ActionResult {
        self.app.set_package_runs(slug, runs).into()
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
        self.app
            .upsert_subtask(slug, id, score, depends, scoring)
            .into()
    }

    /// Deletes a `[[subtask]]` entry by `subtaskId` and optionally clears references.
    pub fn delete_subtask(
        &self,
        slug: String,
        subtask_id: String,
        clear_references: bool,
    ) -> ActionResult {
        self.app
            .delete_subtask(slug, subtask_id, clear_references)
            .into()
    }

    /// Sets or clears `[[test]].subtask` on the 0-based `ruleIndex` test rule.
    pub fn set_test_subtask(
        &self,
        slug: String,
        rule_index: u32,
        subtask: Option<String>,
    ) -> ActionResult {
        self.app.set_test_subtask(slug, rule_index, subtask).into()
    }

    /// Updates `[solutions.<name>]` OI scoring assertions (`scoreJson`, `subtasksJson`).
    pub fn set_solution_scoring(
        &self,
        slug: String,
        name: String,
        score_json: Option<String>,
        subtasks_json: Option<String>,
    ) -> ActionResult {
        self.app
            .set_solution_scoring(slug, name, score_json, subtasks_json)
            .into()
    }

    /// Deletes the 1-based sample pair (`samples/NN.{in,ans}`) and renumbers remaining samples contiguously.
    pub fn delete_sample(&self, slug: String, sample_index: u32) -> ActionResult {
        self.app.delete_sample(slug, sample_index).into()
    }

    /// Normalizes CRLF, UTF-8 BOM, and missing trailing newlines across hand-written inputs.
    pub fn normalize_dirty_inputs(&self, slug: String) -> NormalizeDirtyResultJs {
        self.app.normalize_dirty_inputs(slug).into()
    }

    /// Restores the original bytes from the most recent `normalizeDirtyInputs` call for `slug`.
    pub fn undo_normalize_dirty_inputs(&self, slug: String) -> ActionResult {
        self.app.undo_normalize_dirty_inputs(slug).into()
    }

    /// Bounded read of a package file inside `problems/<slug>/` for the lightweight quick-editor
    /// (enforces a strict 256 KiB cap).
    pub fn read_package_file(&self, slug: String, rel_path: String) -> PackageFileContentJs {
        self.app.read_package_file(slug, rel_path).into()
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
        self.app
            .save_package_file(slug, rel_path, content, expected_hash)
            .into()
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
        let precheck = this.with(|p| p.app.state.validate_polygon_import_writable());
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
        let state = this.with(|p| p.app.state.clone());
        blocking(move || state.import_polygon(&zip_path, target_id).into())
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
        let state = this.with(|p| p.app.state.clone());
        let path = PathBuf::from(zip_path);
        blocking(move || state.import_polygon(&path, target_id).into())
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
        let state = this.with(|p| p.app.state.clone());
        let fmt_fallback = format.clone();
        blocking(move || state.export_oj_package(&format, &target, &id).into())
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
        self.app.load_sanitizer_report(slug).into()
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
        let state = this.with(|p| p.app.state.clone());
        blocking(move || state.sanitize_problem(&slug, tests, solutions).into())
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
        let state = this.with(|p| p.app.state.clone());
        let file_paths: Vec<PathBuf> = files.into_iter().map(PathBuf::from).collect();
        blocking(move || {
            state
                .run_external_problem_solutions(&slug, &file_paths, auto_build)
                .into()
        })
        .await
        .unwrap_or_else(|e| JsonReportResultJs::err("io", format!("external run task failed: {e}")))
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
        let state = this.with(|p| p.app.state.clone());
        let dir_path = PathBuf::from(dir);
        blocking(move || {
            state
                .run_external_contest_solutions(&contest_id, &dir_path, auto_build)
                .into()
        })
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
        self.app
            .adopt_external_solution(slug, source_file, name, expect, tags, is_main)
            .into()
    }

    /// Performs a read-only contest readiness audit across all problems in `contestId` off the
    /// UI thread (populating `xcpc-lint` counts per problem), returning serialized
    /// `ContestAuditReport` + rendered Markdown.
    pub async fn audit_contest(this: Instance<Self>, contest_id: String) -> JsonReportResultJs {
        if this.is_disposed() {
            return JsonReportResultJs::err("disposed", "preview disposed");
        }
        let state = this.with(|p| p.app.state.clone());
        blocking(move || state.audit_contest(&contest_id).into())
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
        self.app.set_training_status(problem_id, status).into()
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
        let state = this.with(|p| p.app.state.clone());
        blocking(move || state.resolve_problem_url(&url).into())
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
        self.app
            .create_problem_from_url(id, title, platform, source_id, url, tags, rating)
            .into()
    }

    /// Returns all discovered plugins, subscribed repositories, and global sync settings.
    pub fn get_plugins_overview(&self) -> PluginsOverviewJs {
        self.app.get_plugins_overview().into()
    }

    /// Enables or disables a discovered plugin by ID.
    pub fn set_plugin_enabled(&self, id: String, enabled: bool) -> ActionResult {
        self.app.set_plugin_enabled(id, enabled).into()
    }

    /// Explicitly trusts a plugin awaiting permission confirmation (`pendingTrust = false`).
    pub fn trust_plugin(&self, id: String) -> ActionResult {
        self.app.trust_plugin(id).into()
    }

    /// Re-scans `<dataDir>/plugins/` and returns the refreshed plugins overview.
    pub fn reload_plugins(&self) -> PluginsOverviewJs {
        self.app.reload_plugins().into()
    }

    /// Exports an embedded built-in plugin to `<dataDir>/plugins/<id>/` for local customization.
    pub fn export_builtin_plugin(&self, id: String) -> ExportPluginResultJs {
        self.app.export_builtin_plugin(id).into()
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
        let state = this.with(|p| p.app.state.clone());
        blocking(move || state.install_plugin_from_path(&picked, is_zip).into())
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
        self.app.add_plugin_repo(url).into()
    }

    /// Unsubscribes from a decentralized plugin repository URL.
    pub fn remove_plugin_repo(&self, url: String) -> ActionResult {
        self.app.remove_plugin_repo(url).into()
    }

    /// Restores the official plugin repository subscription if it was removed.
    pub fn restore_official_plugin_repo(&self) -> ActionResult {
        self.app.restore_official_plugin_repo().into()
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
        let state = this.with(|p| p.app.state.clone());
        blocking(move || state.fetch_plugin_repo_catalog(&url).into())
            .await
            .unwrap_or_else(|e| RepoCatalogResultJs {
                ok: false,
                error: Some(format!("fetch repo catalog failed: {e}")),
                ..Default::default()
            })
    }

    /// Fetches all subscribed plugin repositories' catalogs off the UI thread, returning
    /// aggregated plugin entries alongside per-repository fetch errors.
    pub async fn fetch_all_plugin_repo_catalogs(this: Instance<Self>) -> PluginRepoCatalogJs {
        if this.is_disposed() {
            return PluginRepoCatalogJs {
                ok: false,
                error: Some("preview disposed".into()),
                ..Default::default()
            };
        }
        let state = this.with(|p| p.app.state.clone());
        blocking(move || state.fetch_all_plugin_repo_catalogs().into())
            .await
            .unwrap_or_else(|e| PluginRepoCatalogJs {
                ok: false,
                error: Some(format!("fetch all repo catalogs failed: {e}")),
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
        let state = this.with(|p| p.app.state.clone());
        blocking(move || state.install_plugin_from_repo(&repo_url, &plugin_id).into())
            .await
            .unwrap_or_else(|e| InstallPluginResultJs {
                ok: false,
                error: Some(format!("install plugin from repo failed: {e}")),
                ..Default::default()
            })
    }

    /// Sets (or clears when empty) the single OJ handle for `platform`.
    pub fn set_sync_handle(&self, platform: String, handle: String) -> ActionResult {
        self.app.set_sync_handle(platform, handle).into()
    }

    /// Toggles silent OJ auto-sync on application startup.
    pub fn set_auto_sync_on_startup(&self, enabled: bool) -> ActionResult {
        self.app.set_auto_sync_on_startup(enabled).into()
    }

    /// Stores (or deletes when empty) a plugin credential in the OS keyring.
    pub fn set_plugin_credential(
        &self,
        plugin_id: String,
        key: String,
        value: String,
    ) -> ActionResult {
        self.app.set_plugin_credential(plugin_id, key, value).into()
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
        let state = this.with(|p| p.app.state.clone());
        blocking(move || state.verify_plugin(&plugin_id, handle.as_deref()).into())
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
        let state = this.with(|p| p.app.state.clone());
        blocking(move || state.sync_oj(platform, full).into())
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
        let state = this.with(|p| p.app.state.clone());
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
        self.app
            .training_dashboard(window, heatmap_year, tag_min_rating)
            .into()
    }

    /// Computes the training dashboard analytics payload off the UI thread so switching to
    /// the Training space never blocks a frame on SQLite aggregation.
    pub async fn training_dashboard_async(
        this: Instance<Self>,
        window: Option<String>,
        heatmap_year: Option<i32>,
        tag_min_rating: Option<i32>,
    ) -> JsonReportResultJs {
        if this.is_disposed() {
            return JsonReportResultJs {
                ok: false,
                error: Some("Preview disposed".to_owned()),
                ..Default::default()
            };
        }
        let state = this.with(|p| p.app.state.clone());
        blocking(move || {
            state
                .training_dashboard(window, heatmap_year, tag_min_rating)
                .into()
        })
        .await
        .unwrap_or_else(|e| JsonReportResultJs {
            ok: false,
            error: Some(format!("training_dashboard_async task failed: {e}")),
            ..Default::default()
        })
    }

    /// Queries aggregated upcoming / running (and optional finished) OJ contests as JSON.
    pub fn training_contests(
        &self,
        platform: Option<String>,
        include_finished: bool,
    ) -> JsonReportResultJs {
        self.app
            .training_contests(platform, include_finished)
            .into()
    }

    /// Queries synced submissions (raw or deduplicated per problem) as JSON, enriched with
    /// `library_problem_id` from the open workspace.
    pub fn training_submissions(&self, query: TrainingSubmissionsQueryJs) -> JsonReportResultJs {
        self.app.training_submissions(query.into()).into()
    }

    /// Ingests an external OJ problem (by URL or `(platform, problemKey)`) into the open
    /// workspace, deduplicating by `[[source]]` and refreshing the workspace index.
    pub fn training_ingest_problem(&self, req: IngestProblemRequestJs) -> IngestProblemResultJs {
        self.app.training_ingest_problem(req.into()).into()
    }

    /// Records a manual training log entry (workspace check-in or standalone) into
    /// `training/logs.jsonl`, optionally creating a linked library problem.
    pub fn training_log_record(&self, req_json: String) -> ManualLogResultJs {
        self.app.training_log_record(req_json).into()
    }

    /// Lists non-empty problem editorials in the open workspace, filtered by `queryJson`.
    pub fn training_list_editorials(&self, query_json: String) -> EditorialListResultJs {
        self.app.training_list_editorials(query_json).into()
    }

    /// Opens an external `http://` or `https://` URL in the user's default browser.
    pub fn open_external_url(url: String) -> ActionResult {
        xcpc_app::AppController::open_external_url(url).into()
    }

    // ---- Phase 4C: Toolbox, Plugin UI SDK, Commands & Importer/Exporter ----------------

    /// Lists all enabled `tool` plugins and their declared headless tools as JSON.
    pub fn plugin_list_tools(&self) -> JsonReportResultJs {
        self.app.plugin_list_tools().into()
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
        let state = this.with(|p| p.app.state.clone());
        blocking(move || {
            state
                .plugin_run_tool(&plugin_id, &tool_id, &inputs_json)
                .into()
        })
        .await
        .unwrap_or_else(|e| JsonReportResultJs::err("task", format!("run tool failed: {e}")))
    }

    /// Loads the pure-Web HTML entry (`ui.html`) for an enabled, trusted UI plugin.
    pub fn load_plugin_ui_html(&self, plugin_id: String) -> PluginUiHtmlJs {
        self.app.load_plugin_ui_html(plugin_id).into()
    }

    /// Executes a permission-checked read-only workspace query (`listProblems`, `getProblem`,
    /// `listContests`, `getContest`) on behalf of a plugin (`pending_trust == false` and
    /// `manifest.permissions.workspace_read == true` are enforced in Core).
    pub fn plugin_workspace_query(
        &self,
        plugin_id: String,
        method: String,
        args_json: Option<String>,
    ) -> PluginWorkspaceResultJs {
        self.app
            .plugin_workspace_query(plugin_id, method, args_json)
            .into()
    }

    /// Returns the user's bound OJ accounts (platform, handle, cached rating/rank) for an
    /// enabled, trusted plugin. Exposes no credentials or private submission data.
    pub fn plugin_list_oj_accounts(&self, plugin_id: String) -> PluginWorkspaceResultJs {
        self.app.plugin_list_oj_accounts(plugin_id).into()
    }

    /// Renders an SVG string to a PNG image (base64-encoded) via `resvg` + `tiny-skia`.
    pub fn plugin_render_svg_png(svg: String, scale: Option<f64>) -> PluginRenderPngResultJs {
        xcpc_app::AppController::plugin_render_svg_png(svg, scale).into()
    }

    /// Renders an untrusted Typst snippet off the UI thread for an enabled, trusted plugin.
    pub async fn plugin_render_typst(
        this: Instance<Self>,
        plugin_id: String,
        source: String,
        opts_json: Option<String>,
    ) -> PluginWorkspaceResultJs {
        if this.is_disposed() {
            return PluginWorkspaceResultJs {
                ok: false,
                data_json: None,
                error: Some("preview disposed".into()),
                error_code: Some("DISPOSED".into()),
            };
        }
        let state = this.with(|p| p.app.state.clone());
        blocking(move || {
            state
                .plugin_render_typst(&plugin_id, &source, opts_json.as_deref())
                .into()
        })
        .await
        .unwrap_or_else(|e| PluginWorkspaceResultJs {
            ok: false,
            data_json: None,
            error: Some(format!("render typst task failed: {e}")),
            error_code: Some("RENDER_ERROR".into()),
        })
    }

    /// Converts LaTeX math via MiTeX and renders it off the UI thread for an enabled, trusted plugin.
    pub async fn plugin_render_math(
        this: Instance<Self>,
        plugin_id: String,
        latex: String,
        opts_json: Option<String>,
    ) -> PluginWorkspaceResultJs {
        if this.is_disposed() {
            return PluginWorkspaceResultJs {
                ok: false,
                data_json: None,
                error: Some("preview disposed".into()),
                error_code: Some("DISPOSED".into()),
            };
        }
        let state = this.with(|p| p.app.state.clone());
        blocking(move || {
            state
                .plugin_render_math(&plugin_id, &latex, opts_json.as_deref())
                .into()
        })
        .await
        .unwrap_or_else(|e| PluginWorkspaceResultJs {
            ok: false,
            data_json: None,
            error: Some(format!("render math task failed: {e}")),
            error_code: Some("RENDER_ERROR".into()),
        })
    }

    /// Opens a native save dialog and atomically writes an arbitrary base64-encoded binary file
    /// (up to 50 MiB decoded) for `window.xcpc.files.saveBinary(...)`.
    pub async fn plugin_save_binary_dialog(
        base64_data: String,
        default_name: String,
        title: Option<String>,
        extensions: Option<Vec<String>>,
    ) -> SaveReportResultJs {
        use base64::Engine as _;
        const MAX_BINARY_BYTES: usize = 50 * 1024 * 1024;

        let trimmed = base64_data.trim();
        let bytes = match base64::engine::general_purpose::STANDARD.decode(trimmed) {
            Ok(b) => b,
            Err(err) => {
                return SaveReportResultJs {
                    ok: false,
                    cancelled: false,
                    path: None,
                    error: Some(format!("invalid base64 payload: {err}")),
                };
            }
        };
        if bytes.len() > MAX_BINARY_BYTES {
            return SaveReportResultJs {
                ok: false,
                cancelled: false,
                path: None,
                error: Some(format!(
                    "binary payload exceeds 50 MiB limit ({} bytes)",
                    bytes.len()
                )),
            };
        }
        let file_name = if default_name.trim().is_empty() {
            "output.bin"
        } else {
            default_name.trim()
        };
        let mut dialog = SaveDialog::new()
            .title(title.as_deref().unwrap_or("保存文件"))
            .button_label("保存")
            .file_name(file_name);
        if let Some(exts) = extensions {
            let clean: Vec<String> = exts
                .into_iter()
                .map(|e| e.trim().trim_start_matches('.').to_ascii_lowercase())
                .filter(|e| !e.is_empty())
                .collect();
            if !clean.is_empty() {
                let refs: Vec<&str> = clean.iter().map(String::as_str).collect();
                dialog = dialog.filter(FileFilter::new("Files", refs));
            }
        } else if let Some(ext) = Path::new(file_name).extension().and_then(|e| e.to_str())
            && !ext.is_empty()
        {
            dialog = dialog.filter(FileFilter::new(ext, [ext]));
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
        match blocking(move || xcpc_app::atomic_write_file(&picked, &bytes)).await {
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
                error: Some(format!("save binary task failed: {err}")),
            },
        }
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
        match blocking(move || xcpc_app::atomic_write_file(&picked, &bytes)).await {
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

    /// Writes a PNG image (raw base64 or `data:image/png;base64,...`) to the native
    /// clipboard as both `CF_DIB` and `"PNG"` on Windows; returns `unsupported_platform`
    /// elsewhere so JS falls back to NSH's built-in `Clipboard.writeImage`.
    pub async fn plugin_write_clipboard_image(png_src: String) -> ActionResult {
        match blocking(move || xcpc_app::clipboard::write_clipboard_image_png(&png_src)).await {
            Ok(Ok(true)) => ActionResult::ok(),
            Ok(Ok(false)) => {
                ActionResult::err("unsupported_platform", "native image clipboard not used")
            }
            Ok(Err(err)) => ActionResult::err("clipboard_failed", err),
            Err(err) => {
                ActionResult::err("clipboard_failed", format!("clipboard task failed: {err}"))
            }
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
        match blocking(move || xcpc_app::atomic_write_file(&picked, &bytes)).await {
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
        self.app.plugin_list_commands(context).into()
    }

    /// Synchronously reads a key from the plugin's persistent KV store (`<dataDir>/plugin_kv/<id>.json`).
    pub fn plugin_kv_get(&self, plugin_id: String, key: String) -> PluginKvResultJs {
        self.app.plugin_kv_get(plugin_id, key).into()
    }

    /// Synchronously writes a JSON-encoded value into the plugin's persistent KV store.
    pub fn plugin_kv_set(
        &self,
        plugin_id: String,
        key: String,
        value_json: String,
    ) -> ActionResult {
        self.app.plugin_kv_set(plugin_id, key, value_json).into()
    }

    /// Synchronously removes a key from the plugin's persistent KV store.
    pub fn plugin_kv_remove(&self, plugin_id: String, key: String) -> ActionResult {
        self.app.plugin_kv_remove(plugin_id, key).into()
    }

    /// Uninstalls a locally installed plugin, unlinks a dev-linked plugin (never deleting source dir),
    /// or reverts a local override back to the built-in version.
    pub fn uninstall_plugin(&self, id: String) -> InstallPluginResultJs {
        self.app.uninstall_plugin(id).into()
    }

    /// Prompts the user to pick a single local file (up to 16 MiB) and returns its UTF-8 text (when valid)
    /// and base64 bytes for `sdk.pickFile(...)`.
    pub async fn plugin_pick_file_dialog(
        extensions: Option<Vec<String>>,
        title: Option<String>,
    ) -> PluginPickFileResultJs {
        use base64::Engine as _;
        const MAX_BYTES: u64 = 16 * 1024 * 1024;

        let mut dialog = OpenDialog::files().title(title.as_deref().unwrap_or("选择文件"));
        if let Some(exts) = extensions {
            let clean: Vec<String> = exts
                .into_iter()
                .map(|e| e.trim().trim_start_matches('.').to_ascii_lowercase())
                .filter(|e| !e.is_empty())
                .collect();
            if !clean.is_empty() {
                let refs: Vec<&str> = clean.iter().map(String::as_str).collect();
                dialog = dialog.filter(FileFilter::new("Files", refs));
            }
        }
        let picked = match pick_paths(dialog).await {
            Ok(Some(paths)) => paths.into_iter().next(),
            Ok(None) => None,
            Err(err) => {
                return PluginPickFileResultJs {
                    ok: false,
                    cancelled: false,
                    error: Some(err),
                    ..Default::default()
                };
            }
        };
        let Some(path) = picked else {
            return PluginPickFileResultJs {
                ok: false,
                cancelled: true,
                ..Default::default()
            };
        };
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "file".to_owned());
        match blocking(
            move || -> Result<(String, Option<String>, String), String> {
                let meta = std::fs::metadata(&path).map_err(|e| e.to_string())?;
                if meta.len() > MAX_BYTES {
                    return Err(format!("file exceeds 16 MiB limit ({} bytes)", meta.len()));
                }
                let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
                let text = String::from_utf8(bytes.clone()).ok();
                let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
                Ok((name, text, b64))
            },
        )
        .await
        {
            Ok(Ok((name, text, content_base64))) => PluginPickFileResultJs {
                ok: true,
                cancelled: false,
                name: Some(name),
                text,
                content_base64: Some(content_base64),
                error: None,
            },
            Ok(Err(err)) => PluginPickFileResultJs {
                ok: false,
                cancelled: false,
                error: Some(err),
                ..Default::default()
            },
            Err(err) => PluginPickFileResultJs {
                ok: false,
                cancelled: false,
                error: Some(format!("read file task failed: {err}")),
                ..Default::default()
            },
        }
    }

    /// Prompts the user to pick one or more local files (default 10 MiB/file, 30 MiB total)
    /// and returns their filenames, optional UTF-8 text, and base64 bytes for `files.pickFiles`.
    pub async fn plugin_pick_files_dialog(
        extensions: Option<Vec<String>>,
        title: Option<String>,
        multiple: Option<bool>,
        max_bytes: Option<u32>,
    ) -> PluginPickFilesResultJs {
        use base64::Engine as _;
        const DEFAULT_SINGLE_LIMIT: u64 = 10 * 1024 * 1024;
        const MAX_TOTAL_BYTES: u64 = 30 * 1024 * 1024;

        let single_limit = max_bytes
            .map(u64::from)
            .filter(|&v| v > 0)
            .unwrap_or(DEFAULT_SINGLE_LIMIT)
            .min(MAX_TOTAL_BYTES);

        let mut dialog = OpenDialog::files()
            .title(title.as_deref().unwrap_or("选择文件"))
            .multiple(multiple.unwrap_or(true));
        if let Some(exts) = extensions {
            let clean: Vec<String> = exts
                .into_iter()
                .map(|e| e.trim().trim_start_matches('.').to_ascii_lowercase())
                .filter(|e| !e.is_empty())
                .collect();
            if !clean.is_empty() {
                let refs: Vec<&str> = clean.iter().map(String::as_str).collect();
                dialog = dialog.filter(FileFilter::new("Files", refs));
            }
        }
        let picked = match pick_paths(dialog).await {
            Ok(Some(paths)) if !paths.is_empty() => paths,
            Ok(_) => {
                return PluginPickFilesResultJs {
                    ok: false,
                    cancelled: true,
                    files: Vec::new(),
                    error: None,
                };
            }
            Err(err) => {
                return PluginPickFilesResultJs {
                    ok: false,
                    cancelled: false,
                    files: Vec::new(),
                    error: Some(err),
                };
            }
        };

        match blocking(move || -> Result<Vec<PluginPickedFileJs>, String> {
            let mut total: u64 = 0;
            let mut out = Vec::with_capacity(picked.len());
            for path in picked {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "file".to_owned());
                let meta = std::fs::metadata(&path).map_err(|e| format!("{name}: {e}"))?;
                if meta.len() > single_limit {
                    return Err(format!(
                        "file '{name}' exceeds single-file limit ({} > {single_limit} bytes)",
                        meta.len()
                    ));
                }
                total = total.saturating_add(meta.len());
                if total > MAX_TOTAL_BYTES {
                    return Err(format!(
                        "selected files exceed 30 MiB total limit ({total} bytes)"
                    ));
                }
                let bytes = std::fs::read(&path).map_err(|e| format!("{name}: {e}"))?;
                let text = String::from_utf8(bytes.clone()).ok();
                let content_base64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
                out.push(PluginPickedFileJs {
                    name,
                    text,
                    content_base64,
                });
            }
            Ok(out)
        })
        .await
        {
            Ok(Ok(files)) => PluginPickFilesResultJs {
                ok: true,
                cancelled: false,
                files,
                error: None,
            },
            Ok(Err(err)) => PluginPickFilesResultJs {
                ok: false,
                cancelled: false,
                files: Vec::new(),
                error: Some(err),
            },
            Err(err) => PluginPickFilesResultJs {
                ok: false,
                cancelled: false,
                files: Vec::new(),
                error: Some(format!("read files task failed: {err}")),
            },
        }
    }

    /// Long-polling async notification when plugin files (`manifest.toml`, `index.js`, `ui.html`,
    /// `ui.js`, or `plugin_links.json`) change in `<dataDir>/plugins/` or any registered linked target dir.
    pub async fn next_plugin_change(this: Instance<Self>) -> PluginWatchEventJs {
        let closed = || PluginWatchEventJs {
            kind: "closed".into(),
            paths: Vec::new(),
            changed_plugin_ids: Vec::new(),
        };
        if this.is_disposed() {
            return closed();
        }
        let rx = this.with(|p| p.app.state.ensure_plugin_watcher());
        let state = this.with(|p| p.app.state.clone());
        let Some(rx) = rx else {
            return closed();
        };
        blocking(move || {
            let guard = match rx.lock() {
                Ok(g) => g,
                Err(_) => return closed(),
            };
            match guard.recv() {
                Ok(ev) => {
                    state.sync_engine_plugin_mounts();
                    ev.into()
                }
                Err(_) => closed(),
            }
        })
        .await
        .unwrap_or_else(|_| closed())
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
        let state = this.with(|p| p.app.state.clone());
        blocking(move || {
            state
                .plugin_run_command(&plugin_id, &command_id, &context_payload_json)
                .into()
        })
        .await
        .unwrap_or_else(|e| JsonReportResultJs::err("task", format!("run command failed: {e}")))
    }

    /// Lists all importers declared by enabled plugins as JSON.
    pub fn plugin_list_importers(&self) -> JsonReportResultJs {
        self.app.plugin_list_importers().into()
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
        let exts = this.with(|p| {
            p.app
                .state
                .plugin_importer_extensions(&plugin_id, &importer_id)
        });
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
        let state = this.with(|p| p.app.state.clone());
        blocking(move || {
            state
                .plugin_run_importer_paths(&plugin_id, &importer_id, &picked)
                .into()
        })
        .await
        .unwrap_or_else(|e| JsonReportResultJs::err("task", format!("run importer failed: {e}")))
    }

    /// Lists all exporters declared by enabled plugins as JSON.
    pub fn plugin_list_exporters(&self) -> JsonReportResultJs {
        self.app.plugin_list_exporters().into()
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
        let state = this.with(|p| p.app.state.clone());
        blocking(move || {
            state
                .plugin_run_exporter(&plugin_id, &exporter_id, &kind, &id)
                .into()
        })
        .await
        .unwrap_or_else(|e| JsonReportResultJs::err("task", format!("run exporter failed: {e}")))
    }

    // ---- Phase 5: Algorithm Snippets, Bundler & Living TRDs ----------------------------

    /// Lists all workspace snippets with verification status, reverse dependencies, and tags.
    pub fn list_snippets_json(&self) -> JsonReportResultJs {
        self.app.list_snippets_json().into()
    }

    /// Loads one snippet's full detail, verification status, and last cached report.
    pub fn load_snippet_json(&self, slug: String) -> JsonReportResultJs {
        self.app.load_snippet_json(slug).into()
    }

    /// Creates a new snippet under `snippets/<slug>/`.
    pub fn create_snippet(
        &self,
        slug: String,
        meta_json: String,
        code: Option<String>,
        notes: Option<String>,
    ) -> ActionResult {
        self.app.create_snippet(slug, meta_json, code, notes).into()
    }

    /// Partially updates `snippets/<slug>/snippet.toml` via `toml_edit`.
    pub fn update_snippet_meta(&self, slug: String, patch_json: String) -> ActionResult {
        self.app.update_snippet_meta(slug, patch_json).into()
    }

    /// Atomically writes a single file inside `snippets/<slug>/` (`code.hpp`, `notes.md`,
    /// `test.cpp`, `verify_*.cpp`, etc.).
    pub fn write_snippet_file(
        &self,
        slug: String,
        file_name: String,
        content: String,
    ) -> ActionResult {
        self.app.write_snippet_file(slug, file_name, content).into()
    }

    /// Deletes `snippets/<slug>/`.
    pub fn delete_snippet(&self, slug: String) -> ActionResult {
        self.app.delete_snippet(slug).into()
    }

    /// Starts background verification (`mode`: `"single" | "cascade" | "all"`).
    pub fn start_snippet_verify(
        &self,
        mode: String,
        slug: Option<String>,
        force: bool,
    ) -> ActionResult {
        self.app.start_snippet_verify(mode, slug, force).into()
    }

    /// Returns the current background snippet verification status and reports as JSON.
    pub fn snippet_verify_status_json(&self) -> JsonReportResultJs {
        self.app.snippet_verify_status_json().into()
    }

    /// Persists Library Checker mirror URL and local repository path in `prefs.json`.
    pub fn set_library_checker_config(
        &self,
        mirror_url: Option<String>,
        local_repo_path: Option<String>,
    ) -> ActionResult {
        self.app
            .set_library_checker_config(mirror_url, local_repo_path)
            .into()
    }

    /// Expands a workspace snippet and all its transitive local `#include`s into one C++ file.
    pub fn bundle_snippet_json(&self, slug: String, compact: bool) -> JsonReportResultJs {
        self.app.bundle_snippet_json(slug, compact).into()
    }

    /// Expands a local C++ file on disk with optional `-I` include directories.
    pub fn bundle_local_file_json(
        &self,
        path: String,
        include_dirs: Vec<String>,
        compact: bool,
    ) -> JsonReportResultJs {
        self.app
            .bundle_local_file_json(path, include_dirs, compact)
            .into()
    }

    /// Expands pasted C++ source text with optional base directory and `-I` include directories.
    pub fn bundle_local_source_json(
        &self,
        source: String,
        base_dir: Option<String>,
        include_dirs: Vec<String>,
        compact: bool,
    ) -> JsonReportResultJs {
        self.app
            .bundle_local_source_json(source, base_dir, include_dirs, compact)
            .into()
    }

    /// Opens a native file dialog to pick a local `.cpp` / `.cc` / `.hpp` source file.
    pub async fn pick_cpp_file(
        this: Instance<Self>,
        labels: Option<DialogLabelsJs>,
    ) -> PickedPathsJs {
        if this.is_disposed() {
            return PickedPathsJs {
                cancelled: true,
                ..Default::default()
            };
        }
        let start = this.with(|p| p.app.state.workspace_root());
        let title = label_or(
            labels.as_ref().and_then(|l| l.title.as_deref()),
            "选择 C++ 源文件",
        );
        let button = label_or(
            labels.as_ref().and_then(|l| l.button_label.as_deref()),
            "选择",
        );
        let filter_label = label_or(
            labels.as_ref().and_then(|l| l.filter_label.as_deref()),
            "C++ 源码",
        );
        let mut dialog = OpenDialog::files()
            .title(title)
            .button_label(button)
            .filter(FileFilter::new(
                filter_label,
                ["cpp", "cc", "cxx", "c", "hpp", "h"],
            ));
        if let Some(dir) = start {
            dialog = dialog.directory(dir);
        }
        match pick_paths(dialog).await {
            Ok(Some(paths)) => PickedPathsJs {
                cancelled: paths.is_empty(),
                paths: paths.into_iter().map(|p| p.display().to_string()).collect(),
                error: None,
            },
            Ok(None) => PickedPathsJs {
                cancelled: true,
                ..Default::default()
            },
            Err(e) => PickedPathsJs {
                cancelled: false,
                paths: Vec::new(),
                error: Some(e),
            },
        }
    }

    /// Prompts for a save location and writes text content (e.g., bundled C++ output).
    pub async fn save_text_file_dialog(
        content: String,
        default_name: String,
        labels: Option<DialogLabelsJs>,
    ) -> ActionResult {
        let title = label_or(labels.as_ref().and_then(|l| l.title.as_deref()), "另存为");
        let button = label_or(
            labels.as_ref().and_then(|l| l.button_label.as_deref()),
            "保存",
        );
        let filter_label = label_or(
            labels.as_ref().and_then(|l| l.filter_label.as_deref()),
            "C++ 源码",
        );
        let dialog = SaveDialog::new()
            .title(title)
            .button_label(button)
            .file_name(default_name)
            .filter(FileFilter::new(filter_label, ["cpp", "hpp", "txt"]));
        match pick_save_path(dialog).await {
            Ok(Some(path)) => match xcpc_storage::write_atomic(&path, content.as_bytes()) {
                Ok(()) => ActionResult::ok(),
                Err(e) => ActionResult::err("io", e.to_string()),
            },
            Ok(None) => ActionResult::err("cancelled", "cancelled"),
            Err(e) => ActionResult::err("dialog", e),
        }
    }

    /// Lists all workspace TRD summaries as JSON.
    pub fn list_trds_json(&self) -> JsonReportResultJs {
        self.app.list_trds_json().into()
    }

    /// Loads one TRD's detail, preflight issues, and local template schema as JSON.
    pub fn load_trd_json(&self, id: String) -> JsonReportResultJs {
        self.app.load_trd_json(id).into()
    }

    /// Creates or replaces a TRD's chapters and optional metadata patch.
    pub fn compose_trd(
        &self,
        id: String,
        chapters_json: String,
        meta_patch_json: Option<String>,
    ) -> ActionResult {
        self.app
            .compose_trd(id, chapters_json, meta_patch_json)
            .into()
    }

    /// Partially updates `trds/<id>/trd.toml` metadata via `toml_edit`.
    pub fn update_trd_meta(&self, id: String, patch_json: String) -> ActionResult {
        self.app.update_trd_meta(id, patch_json).into()
    }

    /// Deletes an unfrozen, unarchived TRD directory.
    pub fn delete_trd(&self, id: String) -> ActionResult {
        self.app.delete_trd(id).into()
    }

    /// Archives or unarchives a TRD.
    pub fn set_trd_archived(&self, id: String, archived: bool) -> ActionResult {
        self.app.set_trd_archived(id, archived).into()
    }

    /// Runs TRD preflight checks (missing snippets, missing deps, cycles, verify warnings).
    pub fn preflight_trd_json(&self, id: String) -> JsonReportResultJs {
        self.app.preflight_trd_json(id).into()
    }

    /// Compiles and exports `trds/<id>/` to PDF off the UI thread.
    pub async fn export_trd_pdf(this: Instance<Self>, id: String) -> WorkspaceFile {
        if this.is_disposed() {
            return WorkspaceFile {
                error: Some("preview disposed".into()),
                ..Default::default()
            };
        }
        let state = this.with(|p| p.app.state.clone());
        blocking(move || state.export_trd_pdf(&id).into())
            .await
            .unwrap_or_else(|e| WorkspaceFile {
                error: Some(format!("export trd failed: {e}")),
                ..Default::default()
            })
    }

    /// Checks whether the default exported PDF for `trds/<id>/` exists on disk.
    pub fn trd_export_status(&self, id: String) -> PdfFreshnessJs {
        self.app.trd_export_status(id).into()
    }

    /// Freezes (finalizes) a TRD snapshot with its compiled PDF off the UI thread.
    pub async fn freeze_trd(this: Instance<Self>, id: String, strict: bool) -> JsonReportResultJs {
        if this.is_disposed() {
            return JsonReportResultJs::err("disposed", "preview disposed");
        }
        let state = this.with(|p| p.app.state.clone());
        blocking(move || state.freeze_trd(&id, strict).into())
            .await
            .unwrap_or_else(|e| JsonReportResultJs::err("task", format!("freeze trd failed: {e}")))
    }

    /// Computes the diff between a frozen TRD's snapshot and live workspace snippets.
    pub fn diff_trd_for_unfreeze_json(&self, id: String) -> JsonReportResultJs {
        self.app.diff_trd_for_unfreeze_json(id).into()
    }

    /// Unfreezes (reopens) a frozen TRD for live editing.
    pub fn unfreeze_trd(&self, id: String) -> ActionResult {
        self.app.unfreeze_trd(id).into()
    }

    /// Lists TRD templates (`TemplateKind::Trd`), TRD presets, and loaded font families.
    pub fn list_trd_templates_json(&self) -> String {
        self.app.list_trd_templates_json()
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
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            std::process::Command::new("cmd")
                .args(["/C", "start", ""])
                .arg(&path)
                .creation_flags(CREATE_NO_WINDOW)
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
#[cfg(test)]
mod tests;

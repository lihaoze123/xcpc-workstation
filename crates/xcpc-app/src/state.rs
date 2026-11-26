//! Mutable desktop bridge state: open workspace, in-memory index, file watcher session,
//! Typst engine + compiled cache, and persisted preferences.
//!
//! All domain and storage rules are delegated to `xcpc-domain`, `xcpc-storage`, and
//! `xcpc-convert`.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Instant, SystemTime};

use xcpc_convert::publish::{self, Kind, MergedBookOptions, Target};
use xcpc_domain::{
    BatchItemStatus, ContestMetaPatch, FuzzyMatchKind, GroupFilter, Language, Patch, Problem,
    RatingTier, SearchQuery, check_id,
};
use xcpc_engine::{Compiled, Engine, FontOptions};
use xcpc_storage::{
    WORKSPACE_FILE, WatchEvent, Workspace, WorkspaceIndex, WorkspaceWatcher, content_hash,
};

use crate::cache::{
    CachedCompile, CachedSnapshot, CancelToken, NEXT_PREVIEW_ID, PIXELS, SourceAnchor,
    rasterize_cached,
};
use crate::prefs::{StoredPrefs, mcp_config_snippet, now_epoch_secs};
use crate::snapshot;
use crate::types::{
    ActionResult, AddedTestResultJs, AdoptSolutionResultJs, AppPrefsJs, AssetReferenceJs,
    BatchDeleteResultJs, BatchExportRequestJs, BatchMutationResultJs, BatchStartResultJs,
    BatchTaskEventJs, BatchTaskItemJs, BatchTaskSnapshotJs, CleanUnusedAssetsResultJs,
    ColdStartSnapshotJs, ContestDetailJs, ContestItemDetailJs, ContestMetaPatchJs, ContestStatsJs,
    ExportBookStatusJs, ExportOjResultJs, FreezeResultJs, FuzzyHitJs, GroupSummaryJs,
    JsonReportResultJs, MathPreviewJs, NormalizeDirtyResultJs, OverridePatchJs,
    PackageBuildStatusJs, PackageCreateResultJs, PackageFileContentJs, PackageInitResultJs,
    PackageInspectionResultJs, PdfFreshnessJs, PolygonImportResultJs, PreviewDiagnostic,
    PreviewLocationJs, PreviewRequest, PreviewResult, PreviewStart, PreviewView, ProblemAssetJs,
    ProblemDetailJs, ProblemMetaPatchJs, ProblemSearchQuery, ProblemSummaryJs, RatingBucketJs,
    SamplePairJs, SavePackageFileResultJs, SaveResult, SourceLocationJs, TagCountJs,
    TestCasePreviewJs, ToolchainStatusJs, UnfreezeDiffResultJs, UnfreezeProblemDiffJs,
    WatchEventJs, WorkspaceContest, WorkspaceFacets, WorkspaceFile, WorkspaceInfo, WorkspaceItem,
    WorkspaceProblem, diff_kind_str, extract_report, first_error, limits_to_js,
    parse_namespaced_options_json, rating_tier_str, source_to_js,
};

/// Active watch receiver shared with `Preview::next_change` outside `State`'s mutex.
#[derive(Clone)]
pub struct WatchSession {
    pub id: u64,
    pub rx: Arc<Mutex<mpsc::Receiver<WatchEvent>>>,
}

/// In-session record of a PDF exported by `Preview::export`, used by `export_status` to
/// detect staleness immediately even within the same filesystem timestamp tick.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExportRecord {
    pub(crate) fingerprint: String,
    pub(crate) pdf_len: u64,
    pub(crate) pdf_mtime: Option<SystemTime>,
}

/// Fast UI-thread state: open workspace handle, in-memory index, file watcher,
/// persisted preferences, and in-session export records.
///
/// Never held during Typst compilation, PDF generation, or pixel rasterization.
pub(crate) struct CoreState {
    pub(crate) workspace: Option<Workspace>,
    pub(crate) last_workspace_missing: bool,
    pub(crate) index: WorkspaceIndex,
    pub(crate) watcher: Option<WorkspaceWatcher>,
    pub(crate) watch_session: Option<WatchSession>,
    watch_seq: u64,
    pub(crate) prefs: StoredPrefs,
    /// In-session PDF export fingerprints keyed by `(Target, Kind)`.
    pub(crate) exports: HashMap<(Target, Kind), ExportRecord>,
}

/// Heavy compilation and rasterization state, locked exclusively by background
/// `blocking(...)` tasks (`compile`, `export`, `freeze_contest`) and probed
/// non-blockingly (`try_lock()`) by `locate_click` and lazy `locate_source`.
pub(crate) struct CompilerState {
    pub(crate) engine: Engine,
    engine_root: Option<PathBuf>,
    last_files_rev: u64,
    pub(crate) cached: Option<CachedCompile>,
    /// Layer keys of the latest committed preview result, protected from LRU eviction.
    pub(crate) active: HashSet<String>,
}

impl CompilerState {
    pub fn sync_root(&mut self, ws: &Workspace) {
        let root = ws.root();
        if self.engine_root.as_deref() != Some(root) {
            self.engine.set_root(Some(root.to_path_buf()));
            self.engine_root = Some(root.to_path_buf());
            self.cached = None;
        }
    }

    fn compile(
        &mut self,
        ws: &Workspace,
        request: &PreviewRequest,
    ) -> Result<
        (
            Option<Compiled>,
            Option<xcpc_convert::Document>,
            publish::Report,
        ),
        String,
    > {
        self.sync_root(ws);
        match request.target.as_str() {
            "trd" => {
                let statuses = xcpc_judge::all_snippet_verify_statuses(ws).unwrap_or_default();
                publish::check_trd(ws, &mut self.engine, &request.id, &statuses)
                    .map_err(|e| e.to_string())
            }
            "snippet" => {
                let template = (!request.book.is_empty() && request.book != "statements")
                    .then_some(request.book.as_str());
                // Ensure workspace template edits (`templates/*.typ`) are reloaded on preview refresh (review T3).
                self.engine.reset_files();
                let (compiled, doc, report) = publish::compile_snippet_preview(
                    ws,
                    &mut self.engine,
                    &request.id,
                    template,
                    request.unsaved(),
                )
                .map_err(|e| e.to_string())?;
                Ok((compiled, Some(doc), report))
            }
            _ => publish::check_preview(
                ws,
                &mut self.engine,
                &request.target(),
                request.kind(),
                None,
                request.unsaved(),
            )
            .map_err(|e| e.to_string()),
        }
    }
}

/// Background package build runner state shared between UI polling and worker thread.
#[derive(Default)]
pub(crate) struct PackageBuildSlot {
    pub(crate) seq: u32,
    pub(crate) running: bool,
    pub(crate) slug: Option<String>,
    pub(crate) mode: Option<String>,
    pub(crate) cancel_flag: Option<Arc<AtomicBool>>,
    pub(crate) cancelled: bool,
    pub(crate) phase: Option<String>,
    pub(crate) detail: Option<String>,
    pub(crate) completed: u32,
    pub(crate) total: u32,
    pub(crate) live_cells: Vec<xcpc_judge::MatrixCell>,
    pub(crate) live_cells_index: HashMap<(u32, String), usize>,
    pub(crate) live_cells_json_cache: Option<String>,
    pub(crate) report_json: Option<String>,
    pub(crate) error: Option<String>,
}

/// Polling-free async batch task slot (`startBatchBuild` / `startBatchExport` + `nextBatchEvent`).
pub(crate) struct BatchTaskSlot {
    pub(crate) task_id: u32,
    pub(crate) running: bool,
    pub(crate) kind: String,
    pub(crate) cancel_flag: Option<Arc<AtomicBool>>,
    pub(crate) close_tx: Option<mpsc::Sender<BatchTaskEventJs>>,
    pub(crate) event_rx: Option<Arc<Mutex<mpsc::Receiver<BatchTaskEventJs>>>>,
    pub(crate) snapshot: BatchTaskSnapshotJs,
}

impl Default for BatchTaskSlot {
    fn default() -> Self {
        Self {
            task_id: 0,
            running: false,
            kind: "idle".to_owned(),
            cancel_flag: None,
            close_tx: None,
            event_rx: None,
            snapshot: BatchTaskSnapshotJs {
                kind: "idle".to_owned(),
                ..Default::default()
            },
        }
    }
}

/// Cached declarative schemas and font list for `list_templates_json` / `list_trd_templates_json`,
/// invalidated only when the workspace root or `templates/*.typ` `(name, len, mtime)` fingerprint changes.
pub(crate) struct TemplatesCache {
    pub(crate) root: PathBuf,
    pub(crate) fingerprint: Vec<(String, u64, Option<SystemTime>)>,
    pub(crate) templates: Vec<xcpc_domain::TemplateInfo>,
    pub(crate) fonts: Vec<String>,
}

/// Cached local template schema for `contests/<id>/template.typ` or `trds/<id>/template.typ`,
/// invalidated when the file's `(len, mtime)` changes.
pub(crate) struct LocalTemplateCacheEntry {
    len: u64,
    mtime: Option<SystemTime>,
    info: Option<xcpc_domain::TemplateInfo>,
}

pub struct State {
    pub(crate) id: u64,
    /// Where `prefs.json` (recent workspaces, layout & Frecency) is kept.
    pub data_dir: Option<PathBuf>,
    /// Bumped on workspace mutations, switches, and external watch events to invalidate `cached`.
    pub(crate) revision: Arc<AtomicU64>,
    pub(crate) core: Mutex<CoreState>,
    /// Lightweight UI-accessible snapshot of the latest compiled preview.
    pub(crate) snapshot: Mutex<Option<Arc<CachedSnapshot>>>,
    pub(crate) compiler: Mutex<CompilerState>,
    /// Dedicated lightweight Typst engine for inline floating math previews (never contends with `compiler`).
    pub(crate) math_engine: Mutex<Engine>,
    /// Cached evaluated statement template schemas + engine font families.
    templates_cache: Mutex<Option<TemplatesCache>>,
    /// Cached evaluated TRD template schemas + engine font families.
    pub(crate) trd_templates_cache: Mutex<Option<TemplatesCache>>,
    /// Cached evaluated contest/TRD local template schemas keyed by `(workspace_root, rel_path)`.
    local_template_cache: Mutex<HashMap<(PathBuf, String), LocalTemplateCacheEntry>>,
    /// Background local Polygon package build slot.
    pub(crate) package_build: Arc<Mutex<PackageBuildSlot>>,
    /// Background snippet verification runner slot.
    pub(crate) snippet_verify: Arc<Mutex<crate::snippets::SnippetVerifySlot>>,
    /// Polling-free async batch build / export task slot.
    pub(crate) batch_task: Arc<Mutex<BatchTaskSlot>>,
    /// In-memory byte backup of the last `normalize_dirty_inputs` call for single-click undo.
    #[allow(clippy::type_complexity)]
    pub(crate) dirty_inputs_backup: Mutex<Option<(String, Vec<(String, Vec<u8>)>)>>,
    /// Shared plugin credential store (OS keyring with in-memory fallback for headless tests).
    pub(crate) credentials: Arc<dyn xcpc_plugin::CredentialStore>,
    /// Ensures silent startup auto-sync fires at most once per session.
    pub(crate) startup_sync_triggered: AtomicBool,
    /// Watches `<dataDir>/plugins/` and registered linked dev plugin directories for hot reload.
    pub(crate) plugin_watcher: Mutex<Option<crate::plugins::PluginFileWatcher>>,
}

impl Drop for State {
    fn drop(&mut self) {
        if let Ok(mut build) = self.package_build.lock()
            && let Some(ref flag) = build.cancel_flag
        {
            flag.store(true, Ordering::Relaxed);
            build.cancelled = true;
        }
        self.stop_batch_task();
        self.stop_plugin_watcher();
        xcpc_trace::flush_if_env_configured();
        if let Ok(mut core) = self.core.lock() {
            core.watch_session = None;
            drop(core.watcher.take());
        }
        if let Ok(mut pixels) = PIXELS.lock() {
            pixels.remove_owner(self.id);
        }
    }
}

impl State {
    pub fn new(data_dir: Option<PathBuf>) -> Self {
        let env_ws = std::env::var_os("XCPC_WORKSPACE").map(PathBuf::from);
        Self::new_with_sources(data_dir, env_ws.as_deref(), Path::new("."))
    }

    pub fn new_with_sources(
        data_dir: Option<PathBuf>,
        env_workspace: Option<&Path>,
        cwd: &Path,
    ) -> Self {
        let prefs = StoredPrefs::load(data_dir.as_deref());
        let mut last_workspace_missing = false;
        let workspace = match env_workspace {
            Some(dir) => Workspace::open(dir, false).ok(),
            None => Workspace::discover(cwd, false).ok().or_else(|| {
                let last = prefs.latest_workspace()?;
                if last.is_dir() && last.join(WORKSPACE_FILE).is_file() {
                    match Workspace::open(&last, false) {
                        Ok(ws) => Some(ws),
                        Err(_) => {
                            last_workspace_missing = true;
                            None
                        }
                    }
                } else {
                    last_workspace_missing = true;
                    None
                }
            }),
        };

        let state = Self {
            id: NEXT_PREVIEW_ID.fetch_add(1, Ordering::Relaxed),
            data_dir,
            revision: Arc::new(AtomicU64::new(0)),
            core: Mutex::new(CoreState {
                workspace: None,
                last_workspace_missing,
                index: WorkspaceIndex::default(),
                watcher: None,
                watch_session: None,
                watch_seq: 0,
                prefs,
                exports: HashMap::new(),
            }),
            snapshot: Mutex::new(None),
            compiler: Mutex::new(CompilerState {
                engine: Engine::new(&FontOptions::from_env(), None),
                engine_root: None,
                last_files_rev: 0,
                cached: None,
                active: HashSet::new(),
            }),
            math_engine: Mutex::new(Engine::new(&FontOptions::from_env(), None)),
            templates_cache: Mutex::new(None),
            trd_templates_cache: Mutex::new(None),
            local_template_cache: Mutex::new(HashMap::new()),
            package_build: Arc::new(Mutex::new(PackageBuildSlot::default())),
            snippet_verify: Arc::new(Mutex::new(crate::snippets::SnippetVerifySlot::default())),
            batch_task: Arc::new(Mutex::new(BatchTaskSlot::default())),
            dirty_inputs_backup: Mutex::new(None),
            credentials: Arc::new(xcpc_plugin::SystemKeyringStore::new()),
            startup_sync_triggered: AtomicBool::new(false),
            plugin_watcher: Mutex::new(None),
        };

        if let Some(ws) = workspace {
            let old = state.install_workspace(ws);
            drop(old);
        }
        state
    }

    pub fn revision(&self) -> u64 {
        self.revision.load(Ordering::Acquire)
    }

    pub fn ensure_plugin_watcher(
        &self,
    ) -> Option<Arc<Mutex<std::sync::mpsc::Receiver<crate::types::PluginWatchEventJs>>>> {
        let mut slot = self.plugin_watcher.lock().ok()?;
        if slot.is_none() {
            let dir = self.effective_data_dir().ok()?;
            *slot = crate::plugins::PluginFileWatcher::start(dir);
        }
        slot.as_ref().map(|w| w.receiver())
    }

    pub fn refresh_plugin_watcher(&self) {
        if let Ok(slot) = self.plugin_watcher.lock()
            && let Some(ref w) = *slot
        {
            w.refresh();
        }
    }

    pub fn stop_plugin_watcher(&self) {
        if let Ok(mut slot) = self.plugin_watcher.lock() {
            drop(slot.take());
        }
    }

    pub fn workspace_root(&self) -> Option<PathBuf> {
        self.core
            .lock()
            .unwrap()
            .workspace
            .as_ref()
            .map(|w| w.root().to_path_buf())
    }

    pub fn recent_workspaces(&self) -> Vec<String> {
        self.core
            .lock()
            .unwrap()
            .prefs
            .app
            .recent_workspaces
            .clone()
    }

    pub fn watch_session(&self) -> Option<WatchSession> {
        self.core.lock().unwrap().watch_session.clone()
    }

    /// Switches to `ws`, starting a fresh [`WorkspaceWatcher`] wired with [`SelfWriteTracker`]
    /// and scanning a fresh [`WorkspaceIndex`]. Returns the previous watcher so the caller
    /// can drop it outside `core`'s mutex. Never blocks on `compiler`.
    pub fn install_workspace(&self, ws: Workspace) -> Option<WorkspaceWatcher> {
        let (tracked_ws, new_watcher, new_session_rx) = match ws.watch() {
            Ok((tracked, watcher, rx)) => (tracked, Some(watcher), Some(rx)),
            Err(_) => (ws, None, None),
        };

        let root = tracked_ws.root().to_path_buf();
        let new_index = WorkspaceIndex::scan(&tracked_ws).unwrap_or_default();

        let old_watcher = {
            if let Ok(mut build) = self.package_build.lock() {
                if let Some(ref flag) = build.cancel_flag {
                    flag.store(true, Ordering::Relaxed);
                }
                *build = PackageBuildSlot {
                    seq: build.seq.wrapping_add(1),
                    ..Default::default()
                };
            }
            self.stop_batch_task();
            xcpc_trace::flush_if_env_configured();
            let mut core = self.core.lock().unwrap();
            let new_session = new_session_rx.map(|rx| {
                core.watch_seq = core.watch_seq.wrapping_add(1);
                WatchSession {
                    id: core.watch_seq,
                    rx: Arc::new(Mutex::new(rx)),
                }
            });
            core.prefs.remember_workspace(&root);
            core.prefs.persist(self.data_dir.as_deref());
            core.index = new_index;
            core.workspace = Some(tracked_ws);
            core.last_workspace_missing = false;
            core.watch_session = new_session;
            core.exports.clear();
            std::mem::replace(&mut core.watcher, new_watcher)
        };
        self.invalidate_cache();
        old_watcher
    }

    /// Stops the active watcher (if any) and returns it to be dropped outside the lock.
    pub fn stop_watcher(&self) -> Option<WorkspaceWatcher> {
        let mut core = self.core.lock().ok()?;
        core.watch_session = None;
        core.watcher.take()
    }

    /// Cancels any active background batch task and immediately resolves waiting
    /// `next_batch_event` receivers with `"closed"`.
    pub fn stop_batch_task(&self) {
        let Ok(mut slot) = self.batch_task.lock() else {
            return;
        };
        if let Some(ref flag) = slot.cancel_flag {
            flag.store(true, Ordering::Relaxed);
        }
        let old_id = slot.task_id;
        slot.running = false;
        slot.snapshot.running = false;
        slot.snapshot.cancelled = true;
        slot.cancel_flag = None;
        if let Some(tx) = slot.close_tx.take() {
            let _ = tx.send(BatchTaskEventJs::closed(old_id, slot.snapshot.clone()));
        }
        slot.event_rx = None;
        let next_id = slot.task_id.wrapping_add(1).max(1);
        slot.task_id = next_id;
        slot.kind = "idle".to_owned();
        slot.snapshot = BatchTaskSnapshotJs {
            task_id: next_id,
            kind: "idle".to_owned(),
            ..Default::default()
        };
    }

    /// Invalidates the compiled preview cache without touching `compiler` or `core`.
    pub fn invalidate_cache(&self) {
        self.revision.fetch_add(1, Ordering::AcqRel);
        *self.snapshot.lock().unwrap() = None;
    }

    /// Applies an external [`WatchEvent`] if it still belongs to `session_id`: updates
    /// [`WorkspaceIndex`] incrementally (or rescans on overflow) and invalidates the
    /// compiled cache. Never blocks on `compiler`.
    pub fn apply_watch_event(&self, session_id: u64, event: WatchEvent) -> WatchEventJs {
        let out = match event {
            WatchEvent::Changed(paths) => {
                let mut core = self.core.lock().unwrap();
                if core
                    .watch_session
                    .as_ref()
                    .is_none_or(|s| s.id != session_id)
                {
                    return WatchEventJs {
                        kind: "closed".into(),
                        paths: Vec::new(),
                    };
                }
                let Some(ws) = core.workspace.clone() else {
                    return WatchEventJs {
                        kind: "closed".into(),
                        paths: Vec::new(),
                    };
                };
                for p in &paths {
                    let _ = core.index.refresh_path(&ws, p);
                }
                WatchEventJs {
                    kind: "changed".into(),
                    paths,
                }
            }
            WatchEvent::Rescan => {
                let ws = {
                    let core = self.core.lock().unwrap();
                    if core
                        .watch_session
                        .as_ref()
                        .is_none_or(|s| s.id != session_id)
                    {
                        return WatchEventJs {
                            kind: "closed".into(),
                            paths: Vec::new(),
                        };
                    }
                    let Some(ws) = core.workspace.clone() else {
                        return WatchEventJs {
                            kind: "closed".into(),
                            paths: Vec::new(),
                        };
                    };
                    ws
                };
                let scanned = WorkspaceIndex::scan(&ws).ok();
                {
                    let mut core = self.core.lock().unwrap();
                    if core
                        .watch_session
                        .as_ref()
                        .is_none_or(|s| s.id != session_id)
                    {
                        return WatchEventJs {
                            kind: "closed".into(),
                            paths: Vec::new(),
                        };
                    }
                    if let Some(idx) = scanned
                        && core
                            .workspace
                            .as_ref()
                            .is_some_and(|cur| cur.root() == ws.root())
                    {
                        core.index = idx;
                    }
                }
                WatchEventJs {
                    kind: "rescan".into(),
                    paths: Vec::new(),
                }
            }
        };
        self.invalidate_cache();
        out
    }

    // ---- Workspace info & file I/O -----------------------------------------------------

    pub fn info(&self) -> WorkspaceInfo {
        let _span = xcpc_trace::span("desktop", "state_info");
        let core = self.core.lock().unwrap();
        let Some(ws) = core.workspace.as_ref() else {
            return WorkspaceInfo {
                last_workspace_missing: core.last_workspace_missing,
                ..Default::default()
            };
        };
        let mut info = WorkspaceInfo {
            root: ws.root().display().to_string(),
            read_only: ws.is_read_only(),
            training_enabled: core.index.training_enabled(),
            snippets_enabled: core.index.snippets_enabled(),
            last_workspace_missing: false,
            ..Default::default()
        };
        for p in core.index.problems() {
            info.problems.push(WorkspaceProblem {
                statement: p
                    .statement_path
                    .clone()
                    .unwrap_or_else(|| format!("problems/{}/statement.md", p.id)),
                editorial: format!("problems/{}/editorial.md", p.id),
                id: p.id.clone(),
                title: p.title.clone(),
            });
        }
        for (id, meta) in core.index.contests() {
            let items = if meta.frozen {
                core.index
                    .contest_frozen_items(id)
                    .unwrap_or(&[])
                    .iter()
                    .map(|it| WorkspaceItem {
                        title: it.title.clone(),
                        statement: it.statement.clone(),
                        editorial: it.editorial.clone(),
                        label: it.label.clone(),
                        id: it.id.clone(),
                    })
                    .collect()
            } else {
                meta.problems
                    .iter()
                    .map(|item| {
                        let title = item.title.clone().unwrap_or_else(|| {
                            core.index
                                .problem(&item.id)
                                .map(|p| p.title.clone())
                                .unwrap_or_else(|| item.id.clone())
                        });
                        WorkspaceItem {
                            title,
                            statement: core.index.statement_path_for(&item.id, meta.language),
                            editorial: format!("problems/{}/editorial.md", item.id),
                            label: item.label.clone(),
                            id: item.id.clone(),
                        }
                    })
                    .collect()
            };
            info.contests.push(WorkspaceContest {
                id: id.to_owned(),
                name: meta.name.clone(),
                frozen: meta.frozen,
                archived: meta.archived,
                items,
            });
        }
        info
    }

    pub fn read(&self, path: String) -> WorkspaceFile {
        let ws = self.core.lock().unwrap().workspace.clone();
        let Some(ws) = ws else {
            return WorkspaceFile {
                path,
                read_only: true,
                ..Default::default()
            };
        };
        let read_only = ws.check_writable(&path).is_err();
        match ws.read_text_with_hash(&path) {
            Ok((text, hash)) => WorkspaceFile {
                path,
                text,
                hash,
                read_only,
                error: None,
            },
            Err(xcpc_storage::Error::NotFound(_)) => WorkspaceFile {
                path,
                text: String::new(),
                hash: String::new(),
                read_only,
                error: None,
            },
            Err(e) => WorkspaceFile {
                path,
                text: String::new(),
                hash: String::new(),
                read_only: true,
                error: Some(e.to_string()),
            },
        }
    }

    pub fn save(&self, path: String, text: String, expected_hash: Option<String>) -> SaveResult {
        let ws = self.core.lock().unwrap().workspace.clone();
        let Some(ws) = ws else {
            return SaveResult {
                ok: false,
                hash: String::new(),
                conflict: false,
                error: Some("no workspace open".into()),
            };
        };
        // `ws` is already wired with `SelfWriteTracker` via `Workspace::watch()`, so
        // `write_checked` records the expected hash before atomically renaming into place.
        match ws.write_checked(&path, text.as_bytes(), expected_hash.as_deref()) {
            Ok(hash) => {
                {
                    let mut core = self.core.lock().unwrap();
                    let _ = core.index.refresh_path(&ws, &path);
                }
                // Avoid redundant recompile when `save()` flushes the exact Markdown text
                // that is already compiled in `snapshot`/`cached`.
                let rev = self.revision();
                let norm_path = normalize_ws_rel(&path);
                let preserve_cache = is_overlayable_markdown_path(&norm_path) && {
                    let snap = self.snapshot.lock().unwrap();
                    if let Some(s) = snap.as_ref().filter(|s| {
                        s.revision == rev
                            && normalize_ws_rel(&s.request.path) == norm_path
                            && s.request.text == text
                    }) {
                        s.matches_disk.store(true, Ordering::Release);
                        true
                    } else {
                        false
                    }
                };
                if !preserve_cache {
                    self.invalidate_cache();
                }
                SaveResult {
                    ok: true,
                    hash,
                    conflict: false,
                    error: None,
                }
            }
            Err(err @ xcpc_storage::Error::Conflict { .. }) => SaveResult {
                ok: false,
                hash: String::new(),
                conflict: true,
                error: Some(err.to_string()),
            },
            Err(err) => SaveResult {
                ok: false,
                hash: String::new(),
                conflict: false,
                error: Some(err.to_string()),
            },
        }
    }

    pub fn mcp_config_snippet(&self) -> String {
        let root = self.workspace_root();
        mcp_config_snippet(root.as_deref())
    }

    // ---- Preview compilation, rasterization, export & bidirectional lookup -------------

    /// Speculatively pre-warms adjacent pages in the background without blocking any
    /// active or incoming `run()` compile (uses `try_lock` on `compiler`).
    pub fn prewarm_neighbors(
        &self,
        scale: f32,
        neighbor_pages: &[u32],
        cancel: CancelToken<'_>,
    ) -> u32 {
        if cancel.is_cancelled() || neighbor_pages.is_empty() {
            return 0;
        }
        let Ok(mut compiler) = self.compiler.try_lock() else {
            return 0;
        };
        if cancel.is_cancelled() {
            return 0;
        }
        let CompilerState { cached, active, .. } = &mut *compiler;
        let Some(cached) = cached.as_mut() else {
            return 0;
        };
        crate::cache::prewarm_cached(self.id, active, cached, scale, neighbor_pages, cancel)
    }

    pub fn run(
        &self,
        request: &PreviewRequest,
        scale: f32,
        view: &[PreviewView],
        cancel: CancelToken<'_>,
    ) -> PreviewResult {
        if cancel.is_cancelled() {
            return PreviewResult::default();
        }
        let ws = self.core.lock().unwrap().workspace.clone();
        let Some(ws) = ws else {
            return PreviewResult {
                error: Some("no workspace".into()),
                ..Default::default()
            };
        };

        let mut compiler = self.compiler.lock().unwrap();
        if cancel.is_cancelled() {
            return PreviewResult::default();
        }
        compiler.sync_root(&ws);

        let rev = self.revision();
        if compiler.last_files_rev != rev {
            compiler.engine.reset_files();
            compiler.last_files_rev = rev;
        }
        let cache_hit = compiler
            .cached
            .as_ref()
            .is_some_and(|c| c.matches_request(request, rev));
        let _preview_span = xcpc_trace::span_with("desktop", "preview_request", || {
            serde_json::json!({
                "kind": format!("{:?}", request.kind()),
                "id": request.id,
                "cacheHit": cache_hit,
                "scale": scale,
            })
        });
        let compile_ms = if cache_hit {
            0.0
        } else {
            // Clear stale cache & snapshot before mutating `compiler.engine` so `locate_click`
            // never queries spans against a mismatched `/main.typ`.
            compiler.cached = None;
            *self.snapshot.lock().unwrap() = None;

            let start = Instant::now();
            let compiled = compiler.compile(&ws, request);
            let elapsed = start.elapsed().as_secs_f64() * 1000.0;
            let (compiled, doc, report) = match compiled {
                Ok(r) => r,
                Err(error) => {
                    xcpc_trace::log_error(
                        "engine",
                        format!("Preview compile error for {}: {}", request.id, error),
                        Some(serde_json::json!({
                            "id": request.id,
                            "target": request.target,
                            "compileMs": (elapsed * 100.0).round() / 100.0,
                        })),
                    );
                    return PreviewResult {
                        error: Some(error),
                        compile_ms: elapsed,
                        ..Default::default()
                    };
                }
            };
            if elapsed > 50.0 {
                xcpc_trace::record_slow_event(
                    "engine",
                    "compile_preview",
                    elapsed,
                    Some(serde_json::json!({
                        "id": request.id,
                        "target": request.target,
                    })),
                );
            }
            let (Some(compiled), Some(doc)) = (compiled, doc) else {
                let (error, diagnostics, starts) = preview_report(&report);
                return PreviewResult {
                    error,
                    diagnostics,
                    starts,
                    compile_ms: elapsed,
                    ..Default::default()
                };
            };

            let page_count = compiled.page_count();
            let matches_disk = Arc::new(AtomicBool::new(request.path.is_empty()));

            // Forward-locate anchors (`Mod+Shift+L`) are indexed lazily on demand in
            // `locate_source` rather than walking every page 38-114x on each keystroke.
            let snap = Arc::new(CachedSnapshot {
                request: request.clone(),
                revision: rev,
                matches_disk: matches_disk.clone(),
                doc_source: doc.source.clone(),
                problems: report.problems.clone(),
                anchors: Mutex::new(HashMap::new()),
            });

            compiler.cached = Some(CachedCompile {
                request: request.clone(),
                revision: rev,
                matches_disk,
                compiled,
                doc,
                report,
                fingerprints: vec![None; page_count],
            });

            // Only publish snapshot if no mutation invalidated `rev` while compiling.
            if self.revision() == rev {
                *self.snapshot.lock().unwrap() = Some(snap);
            }
            elapsed
        };
        if cancel.is_cancelled() {
            return PreviewResult {
                compile_ms,
                ..Default::default()
            };
        }
        let CompilerState { cached, active, .. } = &mut *compiler;
        let cached = cached.as_mut().expect("cached above");
        let (mut result, cancelled) =
            rasterize_cached(self.id, active, cached, scale, view, cancel);
        result.compile_ms = compile_ms;
        if cancelled {
            return result;
        }
        let (error, diagnostics, starts) = preview_report(&cached.report);
        result.error = error;
        result.diagnostics = diagnostics;
        result.starts = starts;
        result
    }

    pub fn locate_click(&self, page: u32, x_pt: f64, y_pt: f64) -> Option<SourceLocationJs> {
        let Ok(compiler) = self.compiler.try_lock() else {
            return None;
        };
        let cached = compiler.cached.as_ref()?;
        if cached.revision != self.revision() {
            return None;
        }
        let loc = publish::locate_point(
            &compiler.engine,
            &cached.compiled,
            &cached.doc,
            page as usize,
            x_pt,
            y_pt,
        )?;
        Some(SourceLocationJs {
            file: loc.file,
            line: loc.line as u32,
            column: loc.column as u32,
        })
    }

    /// Maps a 1-based line inside workspace-relative `file` to its position `(page, yPt)`
    /// in the currently cached preview. Never blocks on an active compilation.
    pub fn locate_source(&self, file: &str, line: u32) -> Option<PreviewLocationJs> {
        let norm_file = normalize_ws_rel(file);
        if norm_file.is_empty() {
            return None;
        }
        let rev = self.revision();
        let snap = {
            let guard = self.snapshot.lock().unwrap();
            guard.as_ref().filter(|s| s.revision == rev).cloned()
        }?;

        if !file_matches_book_kind(&norm_file, snap.request.kind()) {
            return None;
        }

        let target_line = line.max(1);
        let fallback_page = problem_pages_for_file(&snap.problems, &norm_file)
            .map(|p| p.first.saturating_sub(1) as u32);

        // Fast path: anchors already indexed for this file (always true for active editor file).
        if let Some(anchors) = snap.anchors.lock().unwrap().get(&norm_file) {
            return interpolate_anchor(anchors, target_line).or_else(|| {
                fallback_page.map(|page| PreviewLocationJs {
                    page,
                    y_pt: 0.0,
                    exact: false,
                })
            });
        }

        // Lazy indexing for another file in the same compiled book (non-blocking `try_lock`).
        if let Ok(compiler) = self.compiler.try_lock()
            && let Some(cached) = compiler.cached.as_ref().filter(|c| c.revision == rev)
        {
            let anchors = sample_file_anchors(
                &compiler.engine,
                &cached.compiled,
                &cached.doc,
                &snap.problems,
                &norm_file,
            );
            let result = interpolate_anchor(&anchors, target_line);
            snap.anchors.lock().unwrap().insert(norm_file, anchors);
            return result.or_else(|| {
                fallback_page.map(|page| PreviewLocationJs {
                    page,
                    y_pt: 0.0,
                    exact: false,
                })
            });
        }

        // Compiler busy: fall back immediately to the problem's first page.
        fallback_page.map(|page| PreviewLocationJs {
            page,
            y_pt: 0.0,
            exact: false,
        })
    }

    pub fn export(&self, request: &PreviewRequest) -> WorkspaceFile {
        let target = request.target();
        let kind = request.kind();
        let ws = self.core.lock().unwrap().workspace.clone();
        let path = match ws.as_ref() {
            Some(ws) => publish::default_output(ws, &target, kind),
            None => publish::default_output_for_title(&target, kind, None),
        };
        let fail = |error: String| WorkspaceFile {
            path: path.clone(),
            error: Some(error),
            ..Default::default()
        };

        let Some(ws) = ws else {
            return fail("当前未打开工作区，无法导出 PDF。请先打开或新建工作区。".into());
        };

        // Compile / generate PDF under `compiler` lock (UI thread `core` stays free).
        let (pdf, doc_source) = {
            let mut compiler = self.compiler.lock().unwrap();
            compiler.sync_root(&ws);
            let rev = self.revision();

            if let Some(cached) = compiler
                .cached
                .as_ref()
                .filter(|c| c.matches_request(request, rev) && c.report.ok)
            {
                match cached.compiled.pdf() {
                    Ok(pdf) => (pdf, cached.doc.source.clone()),
                    Err(e) => {
                        return fail(e.first().map(|d| d.message.clone()).unwrap_or_default());
                    }
                }
            } else {
                compiler.cached = None;
                *self.snapshot.lock().unwrap() = None;
                let (compiled, doc, report) = match compiler.compile(&ws, request) {
                    Ok(r) => r,
                    Err(e) => return fail(e),
                };
                let (Some(compiled), Some(doc)) = (compiled.filter(|_| report.ok), doc) else {
                    return fail(first_error(&report).unwrap_or_default());
                };
                match compiled.pdf() {
                    Ok(pdf) => (pdf, doc.source),
                    Err(e) => {
                        return fail(e.first().map(|d| d.message.clone()).unwrap_or_default());
                    }
                }
            }
        };

        match ws.write(&path, &pdf) {
            Ok(()) => {
                let abs_path = ws.root().join(&path);
                let pdf_meta = std::fs::metadata(&abs_path).ok();
                let pdf_len = pdf_meta.as_ref().map_or(pdf.len() as u64, |m| m.len());
                let pdf_mtime = pdf_meta.and_then(|m| m.modified().ok());
                if let Some(fingerprint) =
                    compute_book_fingerprint(&ws, &target, kind, Some(doc_source.as_str()))
                {
                    self.core.lock().unwrap().exports.insert(
                        (target, kind),
                        ExportRecord {
                            fingerprint,
                            pdf_len,
                            pdf_mtime,
                        },
                    );
                }
                WorkspaceFile {
                    path: abs_path.display().to_string(),
                    ..Default::default()
                }
            }
            Err(e) => fail(e.to_string()),
        }
    }

    /// Resolves and validates `path` (absolute or workspace-relative) for `revealInFolder`,
    /// enforcing that the target exists and stays strictly inside the open workspace.
    pub fn resolve_reveal_path(&self, path: &str) -> Result<PathBuf, ActionResult> {
        let ws = self.core.lock().unwrap().workspace.clone();
        let Some(ws) = ws else {
            return Err(ActionResult::err("no_workspace", "no workspace open"));
        };
        if path.is_empty() {
            return Err(ActionResult::err("not_found", "path is empty"));
        }
        let candidate = Path::new(path);
        let resolved = if candidate.is_absolute() {
            if !candidate.exists() {
                return Err(ActionResult::err("not_found", format!("{path} not found")));
            }
            candidate.to_path_buf()
        } else {
            let p = ws.resolve(path).map_err(ActionResult::from_storage_err)?;
            if !p.exists() {
                return Err(ActionResult::err("not_found", format!("{path} not found")));
            }
            p
        };
        let canonical = resolved
            .canonicalize()
            .map_err(|e| ActionResult::err("io", format!("{path}: {e}")))?;
        if !canonical.starts_with(ws.root()) {
            return Err(ActionResult::err(
                "path_escape",
                format!("path {path:?} is outside the workspace"),
            ));
        }
        Ok(canonical)
    }

    /// Computes independent PDF freshness states for both `statements` and `editorial` books
    /// of `request`'s target (in-session content fingerprint with restart `mtime` fallback).
    /// Never blocks on `compiler`.
    pub fn export_status(&self, request: &PreviewRequest) -> ExportBookStatusJs {
        let _span = xcpc_trace::span_with("desktop", "export_status", || {
            serde_json::json!({
                "target": request.target,
                "id": request.id,
            })
        });
        let target = request.target();
        let (ws, stmt_rec, ed_rec) = {
            let core = self.core.lock().unwrap();
            (
                core.workspace.clone(),
                core.exports
                    .get(&(target.clone(), Kind::Statements))
                    .cloned(),
                core.exports
                    .get(&(target.clone(), Kind::Editorial))
                    .cloned(),
            )
        };
        let rev = self.revision();
        let snap = {
            let guard = self.snapshot.lock().unwrap();
            guard.as_ref().filter(|s| s.revision == rev).cloned()
        };
        ExportBookStatusJs {
            statements: book_freshness(
                ws.as_ref(),
                stmt_rec.as_ref(),
                snap.as_deref(),
                &target,
                Kind::Statements,
                request,
            ),
            editorial: book_freshness(
                ws.as_ref(),
                ed_rec.as_ref(),
                snap.as_deref(),
                &target,
                Kind::Editorial,
                request,
            ),
        }
    }

    // ---- Problem library & editing -----------------------------------------------------

    pub fn search_problems(&self, query: ProblemSearchQuery) -> Vec<ProblemSummaryJs> {
        let _span = xcpc_trace::span("desktop", "search_problems");
        let group = if query.ungrouped_only {
            Some(GroupFilter::Ungrouped)
        } else {
            query.group.and_then(|g| {
                let trimmed = g.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(GroupFilter::Named(trimmed.to_owned()))
                }
            })
        };
        let core_query = SearchQuery {
            text: query.text,
            tags: query.tags,
            rating_min: query.rating_min,
            rating_max: query.rating_max,
            platform: query.platform,
            group,
        };
        let (ws, contest_map, matched, has_pkg_flags) = {
            let core = self.core.lock().unwrap();
            let ws = core.workspace.clone();
            let contest_map = core.index.problem_contest_map();
            let matched = core.index.search_problems(&core_query);
            let has_pkg_flags: Vec<bool> = matched
                .iter()
                .map(|p| core.index.problem_has_package(&p.id).unwrap_or(false))
                .collect();
            (ws, contest_map, matched, has_pkg_flags)
        };
        let training_statuses = ws
            .as_ref()
            .map(|w| {
                let pairs: Vec<(&str, &[xcpc_domain::ProblemSource])> = matched
                    .iter()
                    .map(|p| (p.id.as_str(), p.sources.as_slice()))
                    .collect();
                crate::plugins::derive_library_training_statuses(
                    w,
                    self.data_dir.as_deref(),
                    &pairs,
                )
            })
            .unwrap_or_default();
        matched
            .into_iter()
            .zip(has_pkg_flags)
            .map(|(p, has_package)| {
                let package_status = ws
                    .as_ref()
                    .and_then(|w| package_status_fast(w, &p.id, has_package));
                let contest_ids = contest_map.get(&p.id).cloned().unwrap_or_default();
                let training_status = training_statuses
                    .get(&p.id)
                    .cloned()
                    .unwrap_or_else(crate::plugins::default_training_status_str);
                ProblemSummaryJs {
                    id: p.id,
                    title: p.title,
                    group: p.group,
                    rating: p.rating,
                    tags: p.tags,
                    sources: p.sources.iter().map(source_to_js).collect(),
                    has_editorial: p.has_editorial,
                    has_solutions: p.has_solutions,
                    statement_path: p.statement_path,
                    package_status,
                    contest_ids,
                    training_status,
                }
            })
            .collect()
    }

    pub fn workspace_facets(&self) -> WorkspaceFacets {
        let mut tags = BTreeSet::new();
        let mut platforms = BTreeSet::new();
        let core = self.core.lock().unwrap();
        let problems = core.index.problems();
        let total_count = problems.len() as u32;
        for p in problems {
            for t in &p.tags {
                if !t.is_empty() {
                    tags.insert(t.clone());
                }
            }
            for s in &p.sources {
                if !s.platform.is_empty() {
                    platforms.insert(s.platform.clone());
                }
            }
        }
        let (raw_groups, ungrouped_count) = core.index.group_counts();
        let groups = raw_groups
            .into_iter()
            .map(|g| GroupSummaryJs {
                name: g.name,
                count: g.count as u32,
                declared: g.declared,
            })
            .collect();
        WorkspaceFacets {
            tags: tags.into_iter().collect(),
            platforms: platforms.into_iter().collect(),
            groups,
            ungrouped_count: ungrouped_count as u32,
            total_count,
        }
    }

    pub fn problem_detail(&self, id: &str) -> ProblemDetailJs {
        let _span = xcpc_trace::span_with(
            "desktop",
            "problem_detail",
            || serde_json::json!({ "id": id }),
        );
        let (ws, referencing_contests) = {
            let core = self.core.lock().unwrap();
            let refs = core
                .index
                .contests()
                .into_iter()
                .filter(|(_, meta)| meta.problems.iter().any(|item| item.id == id))
                .map(|(cid, _)| cid.to_owned())
                .collect();
            (core.workspace.clone(), refs)
        };
        let Some(ws) = ws else {
            return ProblemDetailJs {
                id: id.to_owned(),
                error: Some("no workspace open".into()),
                ..Default::default()
            };
        };
        let problem = match ws.load_problem(id) {
            Ok(p) => p,
            Err(e) => {
                return ProblemDetailJs {
                    id: id.to_owned(),
                    error: Some(e.to_string()),
                    ..Default::default()
                };
            }
        };

        let read_only = ws.is_read_only();
        let statement = match problem.statement(problem.default_language()) {
            Some(s) => WorkspaceFile {
                path: s.file.path.clone(),
                hash: content_hash(s.file.text.as_bytes()),
                text: s.file.text.clone(),
                read_only,
                error: None,
            },
            None => WorkspaceFile {
                path: format!("{}/statement.md", problem.dir),
                text: String::new(),
                hash: String::new(),
                read_only,
                error: None,
            },
        };
        let statements = problem
            .statements
            .iter()
            .map(|s| WorkspaceFile {
                path: s.file.path.clone(),
                hash: content_hash(s.file.text.as_bytes()),
                text: s.file.text.clone(),
                read_only,
                error: None,
            })
            .collect();
        let has_editorial = problem.editorial.is_some();
        let editorial = match &problem.editorial {
            Some(ed) => WorkspaceFile {
                path: ed.path.clone(),
                hash: content_hash(ed.text.as_bytes()),
                text: ed.text.clone(),
                read_only,
                error: None,
            },
            None => WorkspaceFile {
                path: format!("{}/editorial.md", problem.dir),
                text: String::new(),
                hash: String::new(),
                read_only,
                error: None,
            },
        };
        let samples = problem
            .samples
            .iter()
            .map(|s| SamplePairJs {
                name: s.name.clone(),
                input: s.input.clone(),
                output: s.output.clone(),
            })
            .collect();
        let solutions = ws.list_solutions(id).unwrap_or_default();

        let template_options_json =
            serde_json::to_string(&problem.meta.template_options).unwrap_or_else(|_| "{}".into());

        ProblemDetailJs {
            id: problem.id,
            title: problem.meta.title,
            group: problem.meta.group,
            rating: problem.meta.rating,
            tags: problem.meta.tags,
            author: problem.meta.author,
            limits: limits_to_js(&problem.meta.limits),
            sources: problem.meta.sources.iter().map(source_to_js).collect(),
            statement,
            statements,
            editorial,
            has_editorial,
            samples,
            solutions,
            referencing_contests,
            template_options_json,
            error: None,
        }
    }

    pub fn read_solution(&self, id: &str, name: &str) -> WorkspaceFile {
        let path = format!("problems/{id}/solutions/{name}");
        let ws = self.core.lock().unwrap().workspace.clone();
        let Some(ws) = ws else {
            return WorkspaceFile {
                path,
                read_only: true,
                error: Some("no workspace open".into()),
                ..Default::default()
            };
        };
        match ws.read_solution(id, name) {
            Ok(text) => WorkspaceFile {
                path,
                hash: content_hash(text.as_bytes()),
                text,
                read_only: true,
                error: None,
            },
            Err(e) => WorkspaceFile {
                path,
                text: String::new(),
                hash: String::new(),
                read_only: true,
                error: Some(e.to_string()),
            },
        }
    }

    /// Assembles a standalone Markdown statement for `id` with its `samples/` embedded
    /// before `## 提示` / `## Note` (without modifying the source file on disk).
    pub fn export_markdown_with_samples(
        &self,
        id: &str,
        unsaved_statement: Option<&str>,
    ) -> Result<String, String> {
        let ws = self
            .core
            .lock()
            .unwrap()
            .workspace
            .clone()
            .ok_or_else(|| "当前没有打开工作区".to_owned())?;
        let problem = ws.load_problem(id).map_err(|e| e.human_message())?;
        let lang = problem.default_language();
        let stmt = match unsaved_statement {
            Some(text) => text.to_owned(),
            None => problem
                .statement(lang)
                .map(|s| s.file.text.clone())
                .unwrap_or_default(),
        };
        Ok(xcpc_convert::markdown_with_samples(
            &stmt,
            &problem.samples,
            lang,
        ))
    }

    /// Compiles and rasterizes a single LaTeX math expression into the shared `PIXELS`
    /// cache for the editor's floating mini math preview. Runs off the UI thread via
    /// `math_engine` so it never contends with full-document preview compilation.
    pub fn render_math_preview(
        &self,
        latex: &str,
        display: bool,
        dark: bool,
        scale: f32,
    ) -> MathPreviewJs {
        let trimmed = latex.trim();
        if trimmed.is_empty() {
            return MathPreviewJs::default();
        }
        let typst_math = match xcpc_convert::math_to_typst(trimmed) {
            Ok(code) if !code.is_empty() => code,
            Ok(_) => return MathPreviewJs::default(),
            Err(err) => {
                return MathPreviewJs {
                    error: Some(err),
                    ..Default::default()
                };
            }
        };
        let scale = scale.clamp(1.0, 3.5);
        let fg = if dark { "#FAFAFA" } else { "#09090B" };
        let fn_name = if display { "dm" } else { "m" };
        let escaped = typst_math.replace('\\', "\\\\").replace('"', "\\\"");
        let source = format!(
            "#import \"/xcpc/markdown.typ\": m, dm\n\
             #set page(width: auto, height: auto, margin: (x: 10pt, y: 7pt), fill: none)\n\
             #set text(font: (\"Latin Modern Roman 12\", \"FZShuSong-Z01\", \"Source Han Serif SC\", \"Noto Serif CJK SC\"), size: 12pt, fill: rgb(\"{fg}\"))\n\
             #show math.equation: set text(font: \"Latin Modern Math\")\n\
             #{fn_name}(\"{escaped}\")\n"
        );
        let hash = content_hash(format!("{source}@{scale:.2}").as_bytes());
        let key = format!("{}:math:{}", self.id, &hash[..16]);

        let compiled = {
            let mut engine = self.math_engine.lock().unwrap();
            engine.set_main(&source);
            match engine.compile() {
                Ok(doc) => doc,
                Err(err) => {
                    let msg = err
                        .errors
                        .into_iter()
                        .next()
                        .map(|d| d.message)
                        .unwrap_or_else(|| "公式渲染失败".into());
                    return MathPreviewJs {
                        error: Some(msg),
                        ..Default::default()
                    };
                }
            }
        };
        let Some((width, height)) = compiled.page_size(0) else {
            return MathPreviewJs::default();
        };
        if !PIXELS.lock().unwrap().contains_and_touch(&key) {
            let Some(pixmap) = compiled.render(0, scale) else {
                return MathPreviewJs::default();
            };
            let (w, h) = (pixmap.width(), pixmap.height());
            let bytes = (w as usize).saturating_mul(h as usize).saturating_mul(4);
            let src = crate::cache::RgbaPixels::new(w, h, pixmap.take());
            let mut pixels = PIXELS.lock().unwrap();
            pixels.insert(
                key.clone(),
                src,
                bytes,
                crate::cache::LayerKind::Tile,
                self.id,
            );
        }
        MathPreviewJs {
            key,
            width,
            height,
            error: None,
        }
    }

    pub fn create_problem(&self, id: &str, title: &str) -> ActionResult {
        let ws = self.core.lock().unwrap().workspace.clone();
        let Some(ws) = ws else {
            return ActionResult::err("no_workspace", "no workspace open");
        };
        match ws.create_problem(id, title) {
            Ok(_) => {
                let _ = self.core.lock().unwrap().index.refresh_problem(&ws, id);
                self.invalidate_cache();
                ActionResult::ok()
            }
            Err(e) => ActionResult::from_storage_err(e),
        }
    }

    pub fn duplicate_problem(
        &self,
        source_id: &str,
        new_id: &str,
        title: Option<&str>,
    ) -> ActionResult {
        let ws = self.core.lock().unwrap().workspace.clone();
        let Some(ws) = ws else {
            return ActionResult::err("no_workspace", "no workspace open");
        };
        match ws.duplicate_problem(source_id, new_id, title) {
            Ok(_) => {
                let _ = self.core.lock().unwrap().index.refresh_problem(&ws, new_id);
                self.invalidate_cache();
                ActionResult::ok()
            }
            Err(e) => ActionResult::from_storage_err(e),
        }
    }

    pub fn update_problem_meta(&self, id: &str, patch: ProblemMetaPatchJs) -> ActionResult {
        let _span = xcpc_trace::span_with(
            "desktop",
            "update_problem_meta",
            || serde_json::json!({ "id": id }),
        );
        let ws = self.core.lock().unwrap().workspace.clone();
        let Some(ws) = ws else {
            return ActionResult::err("no_workspace", "no workspace open");
        };
        let core_patch = match patch.into_core() {
            Ok(p) => p,
            Err(msg) => return ActionResult::err("invalid_argument", msg),
        };
        match ws.update_problem_meta(id, &core_patch) {
            Ok(_) => {
                let _ = self.core.lock().unwrap().index.refresh_problem(&ws, id);
                self.invalidate_cache();
                ActionResult::ok()
            }
            Err(e) => ActionResult::from_storage_err(e),
        }
    }

    pub fn delete_problem(&self, id: &str) -> ActionResult {
        let ws = self.core.lock().unwrap().workspace.clone();
        let Some(ws) = ws else {
            return ActionResult::err("no_workspace", "no workspace open");
        };
        match ws.delete_problem(id) {
            Ok(()) => {
                let _ = self.core.lock().unwrap().index.refresh_problem(&ws, id);
                self.invalidate_cache();
                ActionResult::ok()
            }
            Err(e) => ActionResult::from_storage_err(e),
        }
    }

    // ---- Persistent problem groups & synchronous batch mutations -----------------------

    pub fn create_group(&self, name: &str) -> ActionResult {
        let ws = self.core.lock().unwrap().workspace.clone();
        let Some(ws) = ws else {
            return ActionResult::err("no_workspace", "no workspace open");
        };
        match ws.create_group(name) {
            Ok(_) => {
                let _ = self
                    .core
                    .lock()
                    .unwrap()
                    .index
                    .refresh_path(&ws, WORKSPACE_FILE);
                self.invalidate_cache();
                ActionResult::ok()
            }
            Err(e) => ActionResult::from_storage_err(e),
        }
    }

    pub fn rename_group(&self, old_name: &str, new_name: &str) -> ActionResult {
        let ws = self.core.lock().unwrap().workspace.clone();
        let Some(ws) = ws else {
            return ActionResult::err("no_workspace", "no workspace open");
        };
        match ws.rename_group(old_name, new_name) {
            Ok(_) => {
                if let Ok(idx) = WorkspaceIndex::scan(&ws) {
                    self.core.lock().unwrap().index = idx;
                }
                self.invalidate_cache();
                ActionResult::ok()
            }
            Err(e) => ActionResult::from_storage_err(e),
        }
    }

    pub fn delete_group(&self, name: &str) -> ActionResult {
        let ws = self.core.lock().unwrap().workspace.clone();
        let Some(ws) = ws else {
            return ActionResult::err("no_workspace", "no workspace open");
        };
        match ws.delete_group(name) {
            Ok(_) => {
                if let Ok(idx) = WorkspaceIndex::scan(&ws) {
                    self.core.lock().unwrap().index = idx;
                }
                self.invalidate_cache();
                ActionResult::ok()
            }
            Err(e) => ActionResult::from_storage_err(e),
        }
    }

    pub fn reorder_groups(&self, names: &[String]) -> ActionResult {
        let ws = self.core.lock().unwrap().workspace.clone();
        let Some(ws) = ws else {
            return ActionResult::err("no_workspace", "no workspace open");
        };
        match ws.reorder_groups(names) {
            Ok(_) => {
                let _ = self
                    .core
                    .lock()
                    .unwrap()
                    .index
                    .refresh_path(&ws, WORKSPACE_FILE);
                self.invalidate_cache();
                ActionResult::ok()
            }
            Err(e) => ActionResult::from_storage_err(e),
        }
    }

    pub fn batch_set_group(&self, ids: &[String], group: Option<&str>) -> BatchMutationResultJs {
        let ws = self.core.lock().unwrap().workspace.clone();
        let Some(ws) = ws else {
            return BatchMutationResultJs::err("no_workspace", "no workspace open");
        };
        match ws.batch_set_group(ids, group) {
            Ok(report) => {
                {
                    let mut core = self.core.lock().unwrap();
                    let _ = core.index.refresh_path(&ws, WORKSPACE_FILE);
                    for item in &report.items {
                        if item.status == BatchItemStatus::Ok {
                            let _ = core.index.refresh_problem(&ws, &item.id);
                        }
                    }
                }
                if report.succeeded > 0 || group.is_some() {
                    self.invalidate_cache();
                }
                BatchMutationResultJs::from_report(report)
            }
            Err(e) => BatchMutationResultJs::from_storage_err(e),
        }
    }

    pub fn batch_edit_tags(
        &self,
        ids: &[String],
        add: &[String],
        remove: &[String],
    ) -> BatchMutationResultJs {
        let ws = self.core.lock().unwrap().workspace.clone();
        let Some(ws) = ws else {
            return BatchMutationResultJs::err("no_workspace", "no workspace open");
        };
        match ws.batch_edit_tags(ids, add, remove) {
            Ok(report) => {
                if report.succeeded > 0 {
                    {
                        let mut core = self.core.lock().unwrap();
                        for item in &report.items {
                            if item.status == BatchItemStatus::Ok {
                                let _ = core.index.refresh_problem(&ws, &item.id);
                            }
                        }
                    }
                    self.invalidate_cache();
                }
                BatchMutationResultJs::from_report(report)
            }
            Err(e) => BatchMutationResultJs::from_storage_err(e),
        }
    }

    pub fn batch_set_rating(&self, ids: &[String], rating: Option<u32>) -> BatchMutationResultJs {
        let ws = self.core.lock().unwrap().workspace.clone();
        let Some(ws) = ws else {
            return BatchMutationResultJs::err("no_workspace", "no workspace open");
        };
        match ws.batch_set_rating(ids, rating) {
            Ok(report) => {
                if report.succeeded > 0 {
                    {
                        let mut core = self.core.lock().unwrap();
                        for item in &report.items {
                            if item.status == BatchItemStatus::Ok {
                                let _ = core.index.refresh_problem(&ws, &item.id);
                            }
                        }
                    }
                    self.invalidate_cache();
                }
                BatchMutationResultJs::from_report(report)
            }
            Err(e) => BatchMutationResultJs::from_storage_err(e),
        }
    }

    pub fn batch_add_to_contest(
        &self,
        contest_id: &str,
        problem_ids: &[String],
    ) -> BatchMutationResultJs {
        let ws = self.core.lock().unwrap().workspace.clone();
        let Some(ws) = ws else {
            return BatchMutationResultJs::err("no_workspace", "no workspace open");
        };
        match ws.batch_add_to_contest(contest_id, problem_ids) {
            Ok(res) => {
                if !res.added_ids.is_empty() {
                    let rel = format!("contests/{contest_id}/contest.toml");
                    let _ = self.core.lock().unwrap().index.refresh_path(&ws, &rel);
                    self.invalidate_cache();
                }
                BatchMutationResultJs::from_report(res.report)
            }
            Err(e) => BatchMutationResultJs::from_storage_err(e),
        }
    }

    pub fn batch_delete_problems(&self, ids: &[String]) -> BatchDeleteResultJs {
        let ws = self.core.lock().unwrap().workspace.clone();
        let Some(ws) = ws else {
            return BatchDeleteResultJs::err("no_workspace", "no workspace open");
        };
        match ws.batch_delete_problems(ids) {
            Ok(report) => {
                if report.succeeded > 0 {
                    {
                        let mut core = self.core.lock().unwrap();
                        for item in &report.items {
                            if item.status == BatchItemStatus::Ok {
                                let _ = core.index.refresh_problem(&ws, &item.id);
                            }
                        }
                    }
                    self.invalidate_cache();
                }
                BatchDeleteResultJs::from_report(report)
            }
            Err(e) => BatchDeleteResultJs::from_storage_err(e),
        }
    }

    pub fn save_samples(&self, id: &str, samples: Vec<SamplePairJs>) -> ActionResult {
        let ws = self.core.lock().unwrap().workspace.clone();
        let Some(ws) = ws else {
            return ActionResult::err("no_workspace", "no workspace open");
        };
        let pairs: Vec<(String, String)> =
            samples.into_iter().map(|s| (s.input, s.output)).collect();
        match ws.save_samples(id, &pairs) {
            Ok(()) => {
                let _ = self.core.lock().unwrap().index.refresh_problem(&ws, id);
                self.invalidate_cache();
                ActionResult::ok()
            }
            Err(e) => ActionResult::from_storage_err(e),
        }
    }

    // ---- Contest composition, overrides, stats, freeze & unfreeze ----------------------

    pub fn contest_detail(&self, id: &str) -> ContestDetailJs {
        let _span = xcpc_trace::span_with(
            "desktop",
            "contest_detail",
            || serde_json::json!({ "id": id }),
        );
        let ws = self.core.lock().unwrap().workspace.clone();
        let Some(ws) = ws else {
            return ContestDetailJs {
                id: id.to_owned(),
                error: Some("no workspace open".into()),
                ..Default::default()
            };
        };
        let contest = match ws.load_contest(id) {
            Ok(c) => c,
            Err(e) => {
                return ContestDetailJs {
                    id: id.to_owned(),
                    error: Some(e.to_string()),
                    ..Default::default()
                };
            }
        };

        let meta = &contest.meta;
        let frozen = meta.frozen;
        let default_show = meta.show_metadata;
        let mut error = None;
        let effective_template = if frozen && let Some(snap) = ws.load_snapshot_template(id) {
            snap.template
        } else {
            meta.template
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .or_else(|| ws.default_template())
                .unwrap_or_else(|| "olymp-classic".into())
        };
        let has_contest_template = ws.has_contest_template(id);
        let contest_template_json = if has_contest_template {
            let rel = format!("contests/{id}/template.typ");
            self.inspect_local_template_cached(&ws, &rel, |engine| {
                publish::inspect_contest_template(&ws, engine, id)
            })
            .and_then(|info| serde_json::to_string(&info).ok())
        } else {
            None
        };
        let template_options_json =
            serde_json::to_string(&meta.template_options).unwrap_or_else(|_| "{}".into());

        let items = if frozen {
            match ws.contest_problems(&contest) {
                Ok(list) => list
                    .into_iter()
                    .map(|(item, p)| {
                        let show_metadata = item.show_metadata.unwrap_or(default_show);
                        let opts_json = serde_json::to_string(&p.meta.template_options)
                            .unwrap_or_else(|_| "{}".into());
                        ContestItemDetailJs {
                            label: item.label,
                            id: item.id,
                            missing: false,
                            title: p.meta.title.clone(),
                            original_title: p.meta.title.clone(),
                            limits: limits_to_js(&p.meta.limits),
                            original_limits: limits_to_js(&p.meta.limits),
                            show_metadata,
                            default_show_metadata: default_show,
                            overridden_title: false,
                            overridden_time: false,
                            overridden_memory: false,
                            overridden_input: false,
                            overridden_output: false,
                            overridden_show_metadata: false,
                            statement: statement_path(&p, meta.language),
                            editorial: format!("{}/editorial.md", p.dir),
                            rating: p.meta.rating,
                            tags: p.meta.tags,
                            options_json: opts_json.clone(),
                            original_options_json: opts_json,
                            override_options_json: "{}".into(),
                            package_status: None,
                        }
                    })
                    .collect(),
                Err(e) => {
                    error = Some(e.to_string());
                    Vec::new()
                }
            }
        } else {
            let indexed_items: Vec<(String, Option<bool>)> = {
                let core = self.core.lock().unwrap();
                meta.problems
                    .iter()
                    .map(|item| {
                        (
                            core.index.statement_path_for(&item.id, meta.language),
                            core.index.problem_has_package(&item.id),
                        )
                    })
                    .collect()
            };
            meta.problems
                .iter()
                .zip(indexed_items)
                .map(|(item, (stmt_path, indexed_has_pkg))| {
                    let overridden_title = item.title.is_some();
                    let overridden_time = item.limits.as_ref().is_some_and(|l| l.time.is_some());
                    let overridden_memory =
                        item.limits.as_ref().is_some_and(|l| l.memory.is_some());
                    let overridden_input = item.limits.as_ref().is_some_and(|l| l.input.is_some());
                    let overridden_output =
                        item.limits.as_ref().is_some_and(|l| l.output.is_some());
                    let overridden_show_metadata = item.show_metadata.is_some();
                    let show_metadata = item.show_metadata.unwrap_or(default_show);
                    let override_options_json =
                        serde_json::to_string(&item.options).unwrap_or_else(|_| "{}".into());

                    match ws.load_problem_meta(&item.id) {
                        Ok(p_meta) => {
                            let effective = item.apply(&p_meta);
                            let options_json = serde_json::to_string(&effective.template_options)
                                .unwrap_or_else(|_| "{}".into());
                            let original_options_json =
                                serde_json::to_string(&p_meta.template_options)
                                    .unwrap_or_else(|_| "{}".into());
                            let has_package = indexed_has_pkg.unwrap_or_else(|| {
                                ws.resolve(&format!("problems/{}", item.id))
                                    .ok()
                                    .is_some_and(|dir| p_meta.has_package_in_dir(&dir))
                            });
                            ContestItemDetailJs {
                                label: item.label.clone(),
                                id: item.id.clone(),
                                missing: false,
                                title: effective.title,
                                original_title: p_meta.title.clone(),
                                limits: limits_to_js(&effective.limits),
                                original_limits: limits_to_js(&p_meta.limits),
                                show_metadata,
                                default_show_metadata: default_show,
                                overridden_title,
                                overridden_time,
                                overridden_memory,
                                overridden_input,
                                overridden_output,
                                overridden_show_metadata,
                                statement: stmt_path,
                                editorial: format!("problems/{}/editorial.md", item.id),
                                rating: p_meta.rating,
                                tags: p_meta.tags,
                                options_json,
                                original_options_json,
                                override_options_json,
                                package_status: package_status_fast(&ws, &item.id, has_package),
                            }
                        }
                        Err(_) => {
                            let override_limits = item.limits.clone().unwrap_or_default();
                            ContestItemDetailJs {
                                label: item.label.clone(),
                                id: item.id.clone(),
                                missing: true,
                                title: item.title.clone().unwrap_or_else(|| item.id.clone()),
                                original_title: String::new(),
                                limits: limits_to_js(&override_limits),
                                original_limits: Default::default(),
                                show_metadata,
                                default_show_metadata: default_show,
                                overridden_title,
                                overridden_time,
                                overridden_memory,
                                overridden_input,
                                overridden_output,
                                overridden_show_metadata,
                                statement: format!("problems/{}/statement.md", item.id),
                                editorial: format!("problems/{}/editorial.md", item.id),
                                rating: None,
                                tags: Vec::new(),
                                options_json: override_options_json.clone(),
                                original_options_json: "{}".into(),
                                override_options_json,
                                package_status: None,
                            }
                        }
                    }
                })
                .collect()
        };

        ContestDetailJs {
            id: contest.id,
            name: meta.name.clone(),
            location: meta.location.clone(),
            date: meta.date.clone(),
            author: meta.author.clone(),
            language: meta.language.name().to_owned(),
            template: meta.template.clone(),
            effective_template,
            has_contest_template,
            contest_template_json,
            template_options_json,
            show_metadata: meta.show_metadata,
            cover: meta.cover,
            has_cover: meta.has_cover(),
            blank_pages: meta.blank_pages,
            frozen,
            archived: meta.archived,
            items,
            error,
        }
    }

    pub fn contest_stats(&self, id: &str) -> ContestStatsJs {
        let stats = {
            let core = self.core.lock().unwrap();
            if let Some(s) = core.index.contest_stats_by_id(id) {
                s
            } else {
                let Some(ws) = &core.workspace else {
                    return ContestStatsJs {
                        error: Some("no workspace open".into()),
                        ..Default::default()
                    };
                };
                match ws.load_contest(id) {
                    Ok(c) => core.index.contest_stats(&c.meta),
                    Err(e) => {
                        return ContestStatsJs {
                            error: Some(e.to_string()),
                            ..Default::default()
                        };
                    }
                }
            }
        };

        let rating_distribution = RatingTier::ALL
            .into_iter()
            .map(|tier| RatingBucketJs {
                tier: rating_tier_str(tier).to_owned(),
                count: *stats.rating_distribution.get(&tier).unwrap_or(&0) as u32,
            })
            .collect();
        let tag_counts = stats
            .tag_counts
            .into_iter()
            .map(|(tag, count)| TagCountJs {
                tag,
                count: count as u32,
            })
            .collect();

        ContestStatsJs {
            problem_count: stats.problem_count as u32,
            rating_distribution,
            tag_counts,
            error: None,
        }
    }

    pub fn create_contest(&self, id: &str, name: &str) -> ActionResult {
        let ws = self.core.lock().unwrap().workspace.clone();
        let Some(ws) = ws else {
            return ActionResult::err("no_workspace", "no workspace open");
        };
        if let Err(e) = check_id(id) {
            return ActionResult::from_storage_err(e.into());
        }
        let rel = format!("contests/{id}/contest.toml");
        if let Err(e) = ws.check_writable(&rel) {
            return ActionResult::from_storage_err(e);
        }
        match ws.resolve(&rel) {
            Ok(p) if p.exists() => {
                return ActionResult::err("conflict", format!("contest {id:?} already exists"));
            }
            Ok(_) => {}
            Err(e) => return ActionResult::from_storage_err(e),
        }
        let patch = ContestMetaPatch {
            name: Some(name.to_owned()),
            ..Default::default()
        };
        match ws.compose_contest(id, &[], Some(&patch)) {
            Ok(_) => {
                let _ = self.core.lock().unwrap().index.refresh_contest(&ws, id);
                self.invalidate_cache();
                ActionResult::ok()
            }
            Err(e) => ActionResult::from_storage_err(e),
        }
    }

    pub fn compose_contest(&self, id: &str, problem_ids: &[String]) -> ActionResult {
        let ws = self.core.lock().unwrap().workspace.clone();
        let Some(ws) = ws else {
            return ActionResult::err("no_workspace", "no workspace open");
        };
        match ws.compose_contest(id, problem_ids, None) {
            Ok(_) => {
                let _ = self.core.lock().unwrap().index.refresh_contest(&ws, id);
                self.invalidate_cache();
                ActionResult::ok()
            }
            Err(e) => ActionResult::from_storage_err(e),
        }
    }

    pub fn update_contest_meta(&self, id: &str, patch: ContestMetaPatchJs) -> ActionResult {
        let ws = self.core.lock().unwrap().workspace.clone();
        let Some(ws) = ws else {
            return ActionResult::err("no_workspace", "no workspace open");
        };
        let core_patch = match patch.into_core() {
            Ok(p) => p,
            Err(msg) => return ActionResult::err("invalid_argument", msg),
        };
        match ws.update_contest_meta(id, &core_patch) {
            Ok(_) => {
                let _ = self.core.lock().unwrap().index.refresh_contest(&ws, id);
                self.invalidate_cache();
                ActionResult::ok()
            }
            Err(e) => ActionResult::from_storage_err(e),
        }
    }

    pub fn set_contest_archived(&self, id: &str, archived: bool) -> ActionResult {
        let ws = self.core.lock().unwrap().workspace.clone();
        let Some(ws) = ws else {
            return ActionResult::err("no_workspace", "no workspace open");
        };
        match ws.set_contest_archived(id, archived) {
            Ok(_) => {
                let _ = self.core.lock().unwrap().index.refresh_contest(&ws, id);
                self.invalidate_cache();
                ActionResult::ok()
            }
            Err(e) => ActionResult::from_storage_err(e),
        }
    }

    pub fn set_override(
        &self,
        contest_id: &str,
        label: &str,
        patch: OverridePatchJs,
    ) -> ActionResult {
        let ws = self.core.lock().unwrap().workspace.clone();
        let Some(ws) = ws else {
            return ActionResult::err("no_workspace", "no workspace open");
        };
        let core_patch = match patch.into_core() {
            Ok(p) => p,
            Err(msg) => return ActionResult::err("invalid_argument", msg),
        };
        match ws.set_override(contest_id, label, &core_patch) {
            Ok(_) => {
                let _ = self
                    .core
                    .lock()
                    .unwrap()
                    .index
                    .refresh_contest(&ws, contest_id);
                self.invalidate_cache();
                ActionResult::ok()
            }
            Err(e) => ActionResult::from_storage_err(e),
        }
    }

    pub fn freeze_contest(&self, id: &str) -> FreezeResultJs {
        let ws = self.core.lock().unwrap().workspace.clone();
        let Some(ws) = ws else {
            return FreezeResultJs {
                ok: false,
                frozen: false,
                error: Some("no workspace open".into()),
                code: Some("no_workspace".into()),
                ..Default::default()
            };
        };
        // `publish::freeze_contest` compiles statements (and optionally editorial) using
        // `compiler.engine`, so clear `cached` & `snapshot` to preserve the engine/cached
        // span invariant while leaving `core` unlocked for the UI thread.
        let outcome = {
            let mut compiler = self.compiler.lock().unwrap();
            compiler.sync_root(&ws);
            compiler.cached = None;
            *self.snapshot.lock().unwrap() = None;
            publish::freeze_contest(&ws, &mut compiler.engine, id)
        };
        match outcome {
            Ok(outcome) => {
                let (first_err, diagnostics, _) = extract_report(&outcome.report);
                if outcome.frozen {
                    let _ = self.core.lock().unwrap().index.refresh_contest(&ws, id);
                    self.invalidate_cache();
                }
                FreezeResultJs {
                    ok: outcome.frozen,
                    frozen: outcome.frozen,
                    outputs: outcome.outputs,
                    diagnostics,
                    error: if outcome.frozen { None } else { first_err },
                    code: None,
                }
            }
            Err(e) => FreezeResultJs {
                ok: false,
                frozen: false,
                outputs: Vec::new(),
                diagnostics: Vec::new(),
                error: Some(e.human_message()),
                code: Some(e.code().to_owned()),
            },
        }
    }

    pub fn unfreeze_diff(&self, id: &str) -> UnfreezeDiffResultJs {
        let ws = self.core.lock().unwrap().workspace.clone();
        let Some(ws) = ws else {
            return UnfreezeDiffResultJs {
                ok: false,
                error: Some("no workspace open".into()),
                code: Some("no_workspace".into()),
                ..Default::default()
            };
        };
        match ws.unfreeze_diff(id) {
            Ok(diffs) => UnfreezeDiffResultJs {
                ok: true,
                diffs: diffs
                    .into_iter()
                    .map(|d| UnfreezeProblemDiffJs {
                        label: d.label,
                        problem_id: d.problem_id,
                        kinds: d
                            .kinds
                            .into_iter()
                            .map(|k| diff_kind_str(k).to_owned())
                            .collect(),
                        changed_files: d.changed_files,
                    })
                    .collect(),
                error: None,
                code: None,
            },
            Err(e) => UnfreezeDiffResultJs {
                ok: false,
                diffs: Vec::new(),
                error: Some(e.human_message()),
                code: Some(e.code().to_owned()),
            },
        }
    }

    pub fn unfreeze_contest(&self, id: &str) -> ActionResult {
        let ws = self.core.lock().unwrap().workspace.clone();
        let Some(ws) = ws else {
            return ActionResult::err("no_workspace", "no workspace open");
        };
        match ws.unfreeze_contest(id) {
            Ok(_) => {
                let _ = self.core.lock().unwrap().index.refresh_contest(&ws, id);
                self.invalidate_cache();
                ActionResult::ok()
            }
            Err(e) => ActionResult::from_storage_err(e),
        }
    }

    // ---- Templates & declarative schemas -----------------------------------------------

    pub fn inspect_local_template_cached(
        &self,
        ws: &Workspace,
        rel_path: &str,
        inspect: impl FnOnce(&Engine) -> Option<xcpc_domain::TemplateInfo>,
    ) -> Option<xcpc_domain::TemplateInfo> {
        let root = ws.root().to_path_buf();
        let abs = root.join(rel_path);
        let Ok(meta) = std::fs::metadata(&abs) else {
            self.local_template_cache
                .lock()
                .unwrap()
                .remove(&(root, rel_path.to_owned()));
            return None;
        };
        let len = meta.len();
        let mtime = meta.modified().ok();
        let key = (root.clone(), rel_path.to_owned());
        if let Some(entry) = self.local_template_cache.lock().unwrap().get(&key)
            && entry.len == len
            && entry.mtime == mtime
        {
            return entry.info.clone();
        }
        let info = {
            let mut engine = self.math_engine.lock().unwrap();
            engine.set_root(Some(root));
            engine.reset_files();
            inspect(&engine)
        };
        self.local_template_cache.lock().unwrap().insert(
            key,
            LocalTemplateCacheEntry {
                len,
                mtime,
                info: info.clone(),
            },
        );
        info
    }

    pub fn list_templates_json(&self) -> String {
        let presets = xcpc_engine::list_presets_by_kind(xcpc_domain::TemplateKind::Statement);
        let ws = self.core.lock().unwrap().workspace.clone();
        let Some(ws) = ws else {
            let _span = xcpc_trace::span_with(
                "desktop",
                "list_templates_json",
                || serde_json::json!({ "cacheHit": false }),
            );
            return serde_json::to_string(&serde_json::json!({
                "default": "olymp-classic",
                "workspaceOptions": {},
                "templates": [],
                "presets": presets,
                "fonts": [],
            }))
            .unwrap_or_else(|_| "{}".into());
        };
        let root = ws.root().to_path_buf();
        let fingerprint = template_files_fingerprint(&root);
        let cached = {
            let guard = self.templates_cache.lock().unwrap();
            guard
                .as_ref()
                .filter(|c| c.root == root && c.fingerprint == fingerprint)
                .map(|c| (c.templates.clone(), c.fonts.clone()))
        };
        let cache_hit = cached.is_some();
        let _span = xcpc_trace::span_with(
            "desktop",
            "list_templates_json",
            || serde_json::json!({ "cacheHit": cache_hit }),
        );
        let (templates, fonts) = match cached {
            Some(hit) => hit,
            None => {
                let (templates, fonts) = {
                    let mut engine = self.math_engine.lock().unwrap();
                    engine.set_root(Some(root.clone()));
                    engine.reset_files();
                    let templates = publish::list_templates_by_kind(
                        &ws,
                        &engine,
                        xcpc_domain::TemplateKind::Statement,
                    );
                    let mut fonts: Vec<String> = engine
                        .font_families()
                        .into_iter()
                        .map(|(name, _)| name)
                        .collect();
                    fonts.sort();
                    fonts.dedup();
                    (templates, fonts)
                };
                *self.templates_cache.lock().unwrap() = Some(TemplatesCache {
                    root,
                    fingerprint,
                    templates: templates.clone(),
                    fonts: fonts.clone(),
                });
                (templates, fonts)
            }
        };
        let default_tpl = ws
            .default_template()
            .unwrap_or_else(|| "olymp-classic".into());
        let workspace_options = ws.workspace_template_options();
        serde_json::to_string(&serde_json::json!({
            "default": default_tpl,
            "workspaceOptions": workspace_options,
            "templates": templates,
            "presets": presets,
            "fonts": fonts,
        }))
        .unwrap_or_else(|_| "{}".into())
    }

    pub fn update_workspace_template(
        &self,
        template: Option<String>,
        clear_template: bool,
        options_json: Option<String>,
    ) -> ActionResult {
        let ws = self.core.lock().unwrap().workspace.clone();
        let Some(ws) = ws else {
            return ActionResult::err("no_workspace", "no workspace open");
        };
        let tpl_patch = if clear_template {
            Patch::Clear
        } else if let Some(t) = template {
            Patch::Set(t)
        } else {
            Patch::Keep
        };
        let options = match parse_namespaced_options_json(options_json.as_deref()) {
            Ok(o) => o,
            Err(msg) => return ActionResult::err("invalid_argument", msg),
        };
        match ws.update_workspace_template(&tpl_patch, &options) {
            Ok(()) => {
                self.invalidate_cache();
                ActionResult::ok()
            }
            Err(e) => ActionResult::from_storage_err(e),
        }
    }

    pub fn fork_template(&self, source: &str, new_name: &str) -> ActionResult {
        let ws = self.core.lock().unwrap().workspace.clone();
        let Some(ws) = ws else {
            return ActionResult::err("no_workspace", "no workspace open");
        };
        match publish::fork_template(&ws, source, new_name) {
            Ok(_) => {
                self.invalidate_cache();
                ActionResult::ok()
            }
            Err(e) => ActionResult::from_storage_err(e),
        }
    }

    pub fn install_preset_template(
        &self,
        preset_id: &str,
        target_name: Option<&str>,
    ) -> ActionResult {
        let ws = self.core.lock().unwrap().workspace.clone();
        let Some(ws) = ws else {
            return ActionResult::err("no_workspace", "no workspace open");
        };
        match ws.install_preset_template(preset_id, target_name) {
            Ok(_) => {
                self.invalidate_cache();
                ActionResult::ok()
            }
            Err(e) => ActionResult::from_storage_err(e),
        }
    }

    pub fn detach_contest_template(&self, contest_id: &str) -> ActionResult {
        let ws = self.core.lock().unwrap().workspace.clone();
        let Some(ws) = ws else {
            return ActionResult::err("no_workspace", "no workspace open");
        };
        match ws.detach_contest_template(contest_id) {
            Ok(_) => {
                self.invalidate_cache();
                ActionResult::ok()
            }
            Err(e) => ActionResult::from_storage_err(e),
        }
    }

    pub fn remove_contest_template(&self, contest_id: &str) -> ActionResult {
        let ws = self.core.lock().unwrap().workspace.clone();
        let Some(ws) = ws else {
            return ActionResult::err("no_workspace", "no workspace open");
        };
        match ws.remove_contest_template(contest_id) {
            Ok(()) => {
                self.invalidate_cache();
                ActionResult::ok()
            }
            Err(e) => ActionResult::from_storage_err(e),
        }
    }

    pub fn validate_template_image_target(&self, contest_id: Option<&str>) -> Result<(), String> {
        let ws = self.core.lock().unwrap().workspace.clone();
        let Some(ws) = ws else {
            return Err("没有打开的工作区".into());
        };
        if ws.is_read_only() {
            return Err("工作区以只读方式打开".into());
        }
        if let Some(cid) = contest_id {
            let contest = ws.load_contest(cid).map_err(|e| e.human_message())?;
            if contest.meta.frozen {
                return Err("比赛已定稿，内容只读".into());
            }
        }
        Ok(())
    }

    pub fn import_template_image(
        &self,
        contest_id: Option<&str>,
        src_path: &Path,
    ) -> Result<String, String> {
        let ws = self.core.lock().unwrap().workspace.clone();
        let Some(ws) = ws else {
            return Err("没有打开的工作区".into());
        };
        let rel = ws
            .import_template_image(contest_id, src_path)
            .map_err(|e| e.human_message())?;
        self.invalidate_cache();
        Ok(rel)
    }

    pub fn resolve_template_file(&self, name: &str) -> Result<PathBuf, ActionResult> {
        let ws = self.core.lock().unwrap().workspace.clone();
        let Some(ws) = ws else {
            return Err(ActionResult::err("no_workspace", "no workspace open"));
        };
        match ws.template_file_path(name) {
            Ok(Some(path)) => Ok(path),
            Ok(None) => Err(ActionResult::err(
                "not_found",
                format!("模板文件 templates/{name}.typ 不存在"),
            )),
            Err(e) => Err(ActionResult::from_storage_err(e)),
        }
    }

    pub fn resolve_contest_template_file(&self, contest_id: &str) -> Result<PathBuf, ActionResult> {
        let ws = self.core.lock().unwrap().workspace.clone();
        let Some(ws) = ws else {
            return Err(ActionResult::err("no_workspace", "no workspace open"));
        };
        match ws.contest_template_path(contest_id) {
            Ok(Some(path)) => Ok(path),
            Ok(None) => Err(ActionResult::err(
                "not_found",
                format!("比赛「{contest_id}」没有专属模板"),
            )),
            Err(e) => Err(ActionResult::from_storage_err(e)),
        }
    }

    pub fn ensure_preamble_file(&self) -> Result<PathBuf, ActionResult> {
        let ws = self.core.lock().unwrap().workspace.clone();
        let Some(ws) = ws else {
            return Err(ActionResult::err("no_workspace", "no workspace open"));
        };
        ws.ensure_preamble().map_err(ActionResult::from_storage_err)
    }

    // ---- Cmd+K fuzzy jump, Frecency & preferences --------------------------------------

    pub fn fuzzy_jump(&self, query: &str, active_contest: Option<&str>) -> Vec<FuzzyHitJs> {
        self.fuzzy_jump_at(query, active_contest, now_epoch_secs())
    }

    pub fn fuzzy_jump_at(
        &self,
        query: &str,
        active_contest: Option<&str>,
        now_secs: u64,
    ) -> Vec<FuzzyHitJs> {
        const LIMIT: usize = 50;
        let core = self.core.lock().unwrap();
        let mut hits = Vec::new();

        // Core fuzzy search over all problems and contests (without double-counting
        // in-memory index frecency; persisted Frecency from `prefs.json` is applied here).
        for hit in core.index.fuzzy_search_at(query, usize::MAX, now_secs) {
            let kind = match hit.kind {
                FuzzyMatchKind::Problem => "problem",
                FuzzyMatchKind::Contest => "contest",
            };
            let archived = matches!(hit.kind, FuzzyMatchKind::Contest)
                && core.index.contest(&hit.id).is_some_and(|c| c.archived);
            let visit_key = format!("{kind}:{}", hit.id);
            let boost = core.prefs.frecency_boost(&visit_key, now_secs);
            let score = if archived {
                (hit.score + boost) * 0.5 - 100.0
            } else {
                hit.score + boost
            };
            hits.push(FuzzyHitJs {
                kind: kind.to_owned(),
                id: hit.id,
                title: hit.title,
                contest_id: None,
                label: None,
                visit_key,
                archived,
                score,
            });
        }

        // Contextual active-contest items (match by label `A`/`B`/…, ID, or title).
        if let Some(cid) = active_contest
            && let Some(contest) = core.index.contest(cid)
        {
            let norm_q = normalize_query(query);
            let terms: Vec<&str> = norm_q.split_whitespace().collect();
            for item in &contest.problems {
                let effective_title = item
                    .title
                    .clone()
                    .or_else(|| core.index.problem(&item.id).map(|p| p.title.clone()))
                    .unwrap_or_else(|| item.id.clone());
                let Some(base_score) =
                    score_contest_item(&terms, &norm_q, &item.label, &item.id, &effective_title)
                else {
                    continue;
                };
                let visit_key = format!("contest_item:{cid}:{}", item.label);
                let boost = core.prefs.frecency_boost(&visit_key, now_secs);
                hits.push(FuzzyHitJs {
                    kind: "contest_item".to_owned(),
                    id: item.id.clone(),
                    title: effective_title,
                    contest_id: Some(cid.to_owned()),
                    label: Some(item.label.clone()),
                    visit_key,
                    archived: false,
                    score: base_score + boost,
                });
            }
        }

        hits.sort_by(|a, b| {
            b.score
                .total_cmp(&a.score)
                .then_with(|| hit_kind_rank(&a.kind).cmp(&hit_kind_rank(&b.kind)))
                .then_with(|| a.label.cmp(&b.label))
                .then_with(|| a.id.cmp(&b.id))
        });
        hits.truncate(LIMIT);
        hits
    }

    pub fn record_visit(&self, key: &str) {
        let mut core = self.core.lock().unwrap();
        core.prefs.record_visit(key);
        core.prefs.persist(self.data_dir.as_deref());
    }

    pub fn load_prefs(&self) -> AppPrefsJs {
        self.core.lock().unwrap().prefs.app.clone()
    }

    pub fn save_prefs(&self, prefs: AppPrefsJs) {
        let mut core = self.core.lock().unwrap();
        core.prefs.apply_app_prefs(prefs);
        core.prefs.persist(self.data_dir.as_deref());
    }

    // ---- Native image asset import -----------------------------------------------------

    /// Pre-validates that `target` / `id` / `label` resolves to an existing writable problem
    /// (not read-only workspace, not a frozen contest, valid IDs).
    pub fn validate_image_target(
        &self,
        target: &str,
        id: &str,
        label: Option<&str>,
    ) -> Result<String, String> {
        self.resolve_image_target(target, id, label)
            .map(|(_, problem_id)| problem_id)
    }

    /// Reads `src_path` and atomically imports it into `problems/<problem_id>/assets/<name>`,
    /// avoiding overwrites with `-2`, `-3`, ... suffixes. Returns `"assets/<name>"`.
    pub fn import_image_file(
        &self,
        target: &str,
        id: &str,
        label: Option<&str>,
        src_path: &Path,
    ) -> Result<String, String> {
        let (ws, problem_id) = self.resolve_image_target(target, id, label)?;
        let ext = src_path
            .extension()
            .and_then(|e| e.to_str())
            .ok_or_else(|| "selected file has no extension".to_owned())?;
        let ext = normalize_image_ext(ext)?;
        let raw_stem = src_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("image");
        let stem = sanitize_asset_stem(raw_stem);
        let bytes = std::fs::read(src_path)
            .map_err(|e| format!("failed to read {}: {e}", src_path.display()))?;
        if bytes.is_empty() {
            return Err("selected image file is empty".to_owned());
        }
        self.write_unique_asset(&ws, &problem_id, &stem, &ext, &bytes)
    }

    /// Atomically writes raw image `bytes` into `problems/<problem_id>/assets/paste-YYYYMMDD-HHMMSS.<ext>`
    /// (with `-2`, `-3`, ... suffix on collision). Returns `"assets/<name>"`.
    pub fn import_image_bytes(
        &self,
        target: &str,
        id: &str,
        label: Option<&str>,
        ext: &str,
        bytes: &[u8],
    ) -> Result<String, String> {
        let (ws, problem_id) = self.resolve_image_target(target, id, label)?;
        if bytes.is_empty() {
            return Err("image bytes are empty".to_owned());
        }
        let ext = if ext.trim().is_empty() {
            "png".to_owned()
        } else {
            normalize_image_ext(ext)?
        };
        let stem = paste_timestamp_stem();
        self.write_unique_asset(&ws, &problem_id, &stem, &ext, bytes)
    }

    fn resolve_image_target(
        &self,
        target: &str,
        id: &str,
        label: Option<&str>,
    ) -> Result<(Workspace, String), String> {
        let ws = self
            .core
            .lock()
            .unwrap()
            .workspace
            .clone()
            .ok_or_else(|| "no workspace open".to_owned())?;
        if ws.is_read_only() {
            return Err("workspace is read-only".to_owned());
        }
        match target {
            "problem" => {
                check_id(id).map_err(|e| e.to_string())?;
                ws.load_problem(id).map_err(|e| e.to_string())?;
                Ok((ws, id.to_owned()))
            }
            "contest" => {
                check_id(id).map_err(|e| e.to_string())?;
                let contest = ws.load_contest(id).map_err(|e| e.to_string())?;
                if contest.meta.frozen {
                    return Err(format!("contest '{id}' is frozen"));
                }
                let label = label
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| "missing problem label for contest target".to_owned())?;
                let item = contest
                    .meta
                    .problems
                    .iter()
                    .find(|item| item.label.eq_ignore_ascii_case(label))
                    .ok_or_else(|| format!("contest '{id}' has no problem '{label}'"))?;
                check_id(&item.id).map_err(|e| e.to_string())?;
                ws.load_problem(&item.id).map_err(|e| e.to_string())?;
                Ok((ws, item.id.clone()))
            }
            other => Err(format!(
                "invalid target {other:?}: expected \"problem\" or \"contest\""
            )),
        }
    }

    fn write_unique_asset(
        &self,
        ws: &Workspace,
        problem_id: &str,
        stem: &str,
        ext: &str,
        bytes: &[u8],
    ) -> Result<String, String> {
        for n in 1u32..=10_000 {
            let filename = if n == 1 {
                format!("{stem}.{ext}")
            } else {
                format!("{stem}-{n}.{ext}")
            };
            let ws_rel = format!("problems/{problem_id}/assets/{filename}");
            let abs = ws.resolve(&ws_rel).map_err(|e| e.to_string())?;
            if abs.symlink_metadata().is_err() {
                ws.write(&ws_rel, bytes).map_err(|e| e.to_string())?;
                self.invalidate_cache();
                return Ok(format!("assets/{filename}"));
            }
        }
        Err("同名素材文件过多，无法自动分配新文件名。请先清理或重命名已有素材。".to_owned())
    }

    // ---- Problem asset library (list / rename / delete) --------------------------------

    /// Lists all files inside the target problem's `assets/` directory, sorted by filename,
    /// with byte size, image dimensions (when readable), and exact Markdown references.
    pub fn list_problem_assets(
        &self,
        target: &str,
        id: &str,
        label: Option<&str>,
    ) -> Vec<ProblemAssetJs> {
        let Ok((ws, problem_dir)) = self.resolve_problem_dir_for_assets(target, id, label, false)
        else {
            return Vec::new();
        };
        let Ok(entries) = xcpc_convert::list_problem_assets(&ws, &problem_dir) else {
            return Vec::new();
        };
        entries
            .into_iter()
            .map(|entry| {
                let referenced = !entry.references.is_empty();
                let references = entry
                    .references
                    .into_iter()
                    .map(|r| AssetReferenceJs {
                        file: r.file,
                        line: r.line,
                        width_pct: r.width_pct,
                    })
                    .collect();
                ProblemAssetJs {
                    name: entry.name,
                    rel_path: entry.rel_path,
                    abs_path: entry.abs_path.display().to_string(),
                    thumb_path: entry.thumb_path.map(|p| p.display().to_string()),
                    bytes: entry.bytes as f64,
                    width: entry.width,
                    height: entry.height,
                    referenced,
                    references,
                }
            })
            .collect()
    }

    /// Atomically renames `assets/<old_name>` to `assets/<new_name>` inside the target
    /// problem and updates matching references in `statement*.md` / `editorial.md` via Core.
    pub fn rename_problem_asset(
        &self,
        target: &str,
        id: &str,
        label: Option<&str>,
        old_name: &str,
        new_name: &str,
    ) -> ActionResult {
        let (ws, problem_dir) = match self.resolve_problem_dir_for_assets(target, id, label, true) {
            Ok(v) => v,
            Err(err) => return err,
        };
        match xcpc_convert::rename_problem_asset(&ws, &problem_dir, old_name, new_name) {
            Ok(_) => {
                self.invalidate_cache();
                ActionResult::ok()
            }
            Err(e) => ActionResult::from_storage_err(e),
        }
    }

    /// Deletes `assets/<name>` inside the target problem while enforcing workspace sandbox,
    /// read-only mode, and frozen-contest guards via Core.
    pub fn delete_problem_asset(
        &self,
        target: &str,
        id: &str,
        label: Option<&str>,
        name: &str,
    ) -> ActionResult {
        let (ws, problem_dir) = match self.resolve_problem_dir_for_assets(target, id, label, true) {
            Ok(v) => v,
            Err(err) => return err,
        };
        match xcpc_convert::delete_problem_asset(&ws, &problem_dir, name) {
            Ok(_) => {
                self.invalidate_cache();
                ActionResult::ok()
            }
            Err(e) => ActionResult::from_storage_err(e),
        }
    }

    /// Batch-removes all unreferenced assets inside the target problem's `assets/` directory.
    pub fn clean_unused_assets(
        &self,
        target: &str,
        id: &str,
        label: Option<&str>,
    ) -> CleanUnusedAssetsResultJs {
        let (ws, problem_dir) = match self.resolve_problem_dir_for_assets(target, id, label, true) {
            Ok(v) => v,
            Err(err) => {
                return CleanUnusedAssetsResultJs {
                    ok: false,
                    removed: Vec::new(),
                    freed_bytes: 0.0,
                    error: err.error,
                    code: err.code,
                };
            }
        };
        match xcpc_convert::clean_unused_problem_assets(&ws, &problem_dir) {
            Ok(res) => {
                if !res.removed.is_empty() {
                    self.invalidate_cache();
                }
                if res.failed.is_empty() {
                    CleanUnusedAssetsResultJs {
                        ok: true,
                        removed: res.removed,
                        freed_bytes: res.freed_bytes as f64,
                        error: None,
                        code: None,
                    }
                } else {
                    let details = res
                        .failed
                        .iter()
                        .map(|f| format!("{}（{}）", f.name, f.message))
                        .collect::<Vec<_>>()
                        .join("；");
                    CleanUnusedAssetsResultJs {
                        ok: false,
                        removed: res.removed,
                        freed_bytes: res.freed_bytes as f64,
                        error: Some(format!("部分素材未能删除：{details}")),
                        code: Some("partial_failure".to_owned()),
                    }
                }
            }
            Err(e) => {
                let err = ActionResult::from_storage_err(e);
                CleanUnusedAssetsResultJs {
                    ok: false,
                    removed: Vec::new(),
                    freed_bytes: 0.0,
                    error: err.error,
                    code: err.code,
                }
            }
        }
    }

    /// Sets or clears `{width=NN%}` on Markdown image references to `asset_name` inside the
    /// target problem (either in `doc_path` or across all `statement*.md` and `editorial.md`).
    pub fn set_asset_width(
        &self,
        target: &str,
        id: &str,
        label: Option<&str>,
        asset_name: &str,
        width_pct: Option<f64>,
        doc_path: Option<&str>,
    ) -> ActionResult {
        let (ws, problem_dir) = match self.resolve_problem_dir_for_assets(target, id, label, true) {
            Ok(v) => v,
            Err(err) => return err,
        };
        if let Some(pct) = width_pct
            && (!pct.is_finite() || !(10.0..=100.0).contains(&pct))
        {
            return ActionResult::err(
                "invalid_arguments",
                "图片宽度百分比必须在 10% 到 100% 之间。",
            );
        }
        match xcpc_convert::set_problem_asset_width(
            &ws,
            &problem_dir,
            asset_name,
            width_pct,
            doc_path,
        ) {
            Ok(updated) => {
                if !updated.is_empty() {
                    self.invalidate_cache();
                }
                ActionResult::ok()
            }
            Err(e) => ActionResult::from_storage_err(e),
        }
    }

    fn resolve_problem_dir_for_assets(
        &self,
        target: &str,
        id: &str,
        label: Option<&str>,
        writable: bool,
    ) -> Result<(Workspace, String), ActionResult> {
        let ws = self.core.lock().unwrap().workspace.clone().ok_or_else(|| {
            ActionResult::err(
                "no_workspace",
                "当前未打开工作区。请先打开或新建工作区后再操作。",
            )
        })?;
        if writable && ws.is_read_only() {
            return Err(ActionResult::from_storage_err(
                xcpc_storage::Error::ReadOnly,
            ));
        }
        match target {
            "problem" => {
                check_id(id).map_err(|e| ActionResult::from_storage_err(e.into()))?;
                ws.load_problem(id)
                    .map_err(ActionResult::from_storage_err)?;
                Ok((ws, format!("problems/{id}")))
            }
            "contest" => {
                check_id(id).map_err(|e| ActionResult::from_storage_err(e.into()))?;
                let contest = ws
                    .load_contest(id)
                    .map_err(ActionResult::from_storage_err)?;
                if writable && contest.meta.frozen {
                    return Err(ActionResult::from_storage_err(xcpc_storage::Error::Frozen(
                        id.to_owned(),
                    )));
                }
                let label = label
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| {
                        ActionResult::err(
                            "invalid_arguments",
                            "缺少比赛题目编号（label）。请先选中具体题目后再管理素材。",
                        )
                    })?;
                let item = contest
                    .meta
                    .problems
                    .iter()
                    .find(|item| item.label.eq_ignore_ascii_case(label))
                    .ok_or_else(|| {
                        ActionResult::err(
                            "not_found",
                            format!("比赛「{id}」中不存在题号「{label}」。请确认题号后重试。"),
                        )
                    })?;
                if contest.meta.frozen {
                    Ok((ws, format!("contests/{id}/snapshot/{}", item.label)))
                } else {
                    check_id(&item.id).map_err(|e| ActionResult::from_storage_err(e.into()))?;
                    ws.load_problem(&item.id)
                        .map_err(ActionResult::from_storage_err)?;
                    Ok((ws, format!("problems/{}", item.id)))
                }
            }
            other => Err(ActionResult::err(
                "invalid_arguments",
                format!("无效的目标类型「{other}」。应为 \"problem\" 或 \"contest\"。"),
            )),
        }
    }

    // ---- Cold-start preview snapshot ---------------------------------------------------

    /// Persists lightweight cold-start snapshot metadata and 1–2 visible pages from the
    /// currently cached compiled preview into `<dataDir>/snapshots/`.
    ///
    /// Uses `try_lock()` on `compiler` so it never blocks when a compilation is in flight.
    pub fn save_cold_start_snapshot(
        &self,
        meta_json: &str,
        pages: &[u32],
        render_scale: f32,
    ) -> bool {
        let ws = self.core.lock().unwrap().workspace.clone();
        let Ok(compiler) = self.compiler.try_lock() else {
            return false;
        };
        let compiled = compiler.cached.as_ref().map(|c| &c.compiled);
        snapshot::save_cold_start_snapshot(
            self.data_dir.as_deref(),
            ws.as_ref(),
            compiled,
            meta_json,
            pages,
            render_scale,
        )
    }

    /// Restores the cold-start preview snapshot from `<dataDir>/snapshots/` when it matches
    /// the open workspace and the target's current on-disk content hash.
    pub fn load_cold_start_snapshot(&self) -> Option<ColdStartSnapshotJs> {
        let ws = self.core.lock().unwrap().workspace.clone();
        snapshot::load_cold_start_snapshot(self.data_dir.as_deref(), ws.as_ref(), self.id)
    }

    // ---- Phase 3A: Local Polygon toolchain & package build -----------------------------

    pub fn toolchain_status(&self) -> ToolchainStatusJs {
        let override_path = {
            let core = self.core.lock().unwrap();
            core.prefs.app.cxx_override.clone()
        };
        let status = xcpc_judge::detect_toolchain(override_path.as_deref().map(Path::new));
        status.into()
    }

    pub fn set_cxx_override(&self, path: Option<String>) -> ToolchainStatusJs {
        let trimmed = path
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned);
        if let Some(ref candidate) = trimmed
            && let Err(e) = xcpc_judge::validate_compiler_path(Path::new(candidate))
        {
            let existing = {
                let core = self.core.lock().unwrap();
                core.prefs.app.cxx_override.clone()
            };
            let mut status: ToolchainStatusJs =
                xcpc_judge::detect_toolchain(existing.as_deref().map(Path::new)).into();
            status.error = Some(e.to_string());
            return status;
        }
        let override_path = {
            let mut core = self.core.lock().unwrap();
            core.prefs.set_cxx_override(trimmed.as_deref());
            core.prefs.persist(self.data_dir.as_deref());
            core.prefs.app.cxx_override.clone()
        };
        xcpc_judge::detect_toolchain(override_path.as_deref().map(Path::new)).into()
    }

    /// Fast O(1) check for whether `slug` has a local Polygon package, using the in-memory index.
    pub fn has_package(&self, slug: &str) -> bool {
        let ws = {
            let core = self.core.lock().unwrap();
            if let Some(has) = core.index.problem_has_package(slug) {
                return has;
            }
            core.workspace.clone()
        };
        let Some(ws) = ws else {
            return false;
        };
        let Ok(meta) = ws.load_problem_meta(slug) else {
            return false;
        };
        ws.resolve(&format!("problems/{slug}"))
            .ok()
            .is_some_and(|dir| meta.has_package_in_dir(&dir))
    }

    #[cfg(test)]
    pub fn inspect_package(&self, slug: &str) -> PackageInspectionResultJs {
        self.inspect_package_in(slug, None)
    }

    pub fn inspect_package_in(
        &self,
        slug: &str,
        contest: Option<&str>,
    ) -> PackageInspectionResultJs {
        let _span = xcpc_trace::span_with("desktop", "inspect_package_in", || {
            serde_json::json!({
                "slug": slug,
                "contest": contest,
            })
        });
        let ws = self.core.lock().unwrap().workspace.clone();
        let Some(ws) = ws else {
            return PackageInspectionResultJs {
                ok: false,
                error: Some("no workspace open".into()),
                ..Default::default()
            };
        };
        match xcpc_judge::inspect_package_in(&ws, slug, contest) {
            Ok(insp) => {
                let freshness_status = freshness_status_str(&insp.freshness).to_owned();
                let has_package = insp.has_package;
                match serde_json::to_value(&insp) {
                    Ok(mut val) => {
                        if let Some(obj) = val.as_object_mut()
                            && !obj.contains_key("solutions")
                            && let Ok(problem) = ws.load_problem(slug)
                            && let Ok(prob_dir) = ws.resolve(&problem.dir)
                            && let Ok(discovered) =
                                xcpc_judge::discover_solutions(&prob_dir, &problem.meta)
                        {
                            let sols: Vec<serde_json::Value> = discovered
                                .into_iter()
                                .map(|s| {
                                    serde_json::json!({
                                        "name": s.name,
                                        "is_main": s.is_main,
                                        "expect": s.expect,
                                        "tags": s.tags,
                                        "score": s.score,
                                        "subtasks": s.subtasks,
                                    })
                                })
                                .collect();
                            obj.insert("solutions".to_owned(), serde_json::Value::Array(sols));
                        }
                        let inspection_json = val.to_string();
                        PackageInspectionResultJs {
                            ok: true,
                            has_package,
                            freshness_status,
                            inspection_json,
                            error: None,
                        }
                    }
                    Err(e) => PackageInspectionResultJs {
                        ok: false,
                        has_package,
                        freshness_status,
                        inspection_json: String::new(),
                        error: Some(format!("serialize inspection failed: {e}")),
                    },
                }
            }
            Err(e) => PackageInspectionResultJs {
                ok: false,
                error: Some(e.to_string()),
                ..Default::default()
            },
        }
    }

    pub fn init_package(&self, slug: &str) -> PackageInitResultJs {
        self.init_package_with_options(slug, false, None, false)
    }

    pub fn init_package_with_options(
        &self,
        slug: &str,
        interactive: bool,
        runs: Option<u32>,
        subtasks: bool,
    ) -> PackageInitResultJs {
        let ws = self.core.lock().unwrap().workspace.clone();
        let Some(ws) = ws else {
            return PackageInitResultJs {
                ok: false,
                code: Some("no_workspace".into()),
                error: Some("no workspace open".into()),
                ..Default::default()
            };
        };
        if ws.is_read_only() {
            return PackageInitResultJs {
                ok: false,
                code: Some("read_only".into()),
                error: Some("workspace is read-only".into()),
                ..Default::default()
            };
        }
        let opts = xcpc_judge::InitPackageOptions {
            interactive,
            runs,
            subtasks,
        };
        match xcpc_judge::init_package_with_options(&ws, slug, &opts) {
            Ok(res) => {
                {
                    let mut core = self.core.lock().unwrap();
                    let _ = core
                        .index
                        .refresh_path(&ws, &format!("problems/{slug}/problem.toml"));
                }
                self.invalidate_cache();
                let inspection_json = xcpc_judge::inspect_package(&ws, slug)
                    .ok()
                    .and_then(|insp| serde_json::to_string(&insp).ok());
                PackageInitResultJs {
                    ok: true,
                    created_files: res.created_files,
                    skipped_files: res.skipped_files,
                    updated_problem_toml: res.updated_problem_toml,
                    inspection_json,
                    error: None,
                    code: None,
                }
            }
            Err(e) => PackageInitResultJs {
                ok: false,
                code: Some(e.code().to_owned()),
                error: Some(e.human_message()),
                ..Default::default()
            },
        }
    }

    pub fn start_package_build(
        &self,
        slug: &str,
        mode: &str,
        tests: Option<Vec<String>>,
        solutions: Option<Vec<String>>,
        contest_id: Option<String>,
    ) -> ActionResult {
        let (ws, cxx_override) = {
            let core = self.core.lock().unwrap();
            (core.workspace.clone(), core.prefs.app.cxx_override.clone())
        };
        let Some(ws) = ws else {
            return ActionResult::err("no_workspace", "no workspace open");
        };
        if ws.is_read_only() {
            return ActionResult::err("read_only", "workspace is read-only");
        }
        if let Err(e) = check_id(slug) {
            return ActionResult::err("invalid_id", e.to_string());
        }

        let is_sanitize = mode.eq_ignore_ascii_case("sanitize");
        let build_mode = if mode.eq_ignore_ascii_case("verify") {
            xcpc_judge::BuildMode::Verify
        } else {
            xcpc_judge::BuildMode::Debug
        };
        let mode_label = if is_sanitize {
            "sanitize"
        } else {
            match build_mode {
                xcpc_judge::BuildMode::Verify => "verify",
                xcpc_judge::BuildMode::Debug => "debug",
            }
        };

        let cancel_flag = Arc::new(AtomicBool::new(false));
        let my_seq = {
            let mut slot = self.package_build.lock().unwrap();
            if slot.running {
                return ActionResult::err("build_in_progress", "package build already running");
            }
            let next_seq = slot.seq.wrapping_add(1);
            *slot = PackageBuildSlot {
                seq: next_seq,
                running: true,
                slug: Some(slug.to_owned()),
                mode: Some(mode_label.to_owned()),
                cancel_flag: Some(cancel_flag.clone()),
                cancelled: false,
                phase: Some("compiling".to_owned()),
                detail: None,
                completed: 0,
                total: 0,
                live_cells: Vec::new(),
                live_cells_index: HashMap::new(),
                live_cells_json_cache: None,
                report_json: None,
                error: None,
            };
            next_seq
        };

        let tests_vec = tests.unwrap_or_default();
        let solutions_vec = solutions.unwrap_or_default();
        let cxx_path = cxx_override.map(PathBuf::from);
        let opts = xcpc_judge::BuildOptions {
            mode: build_mode,
            tests: tests_vec.clone(),
            solutions: solutions_vec.clone(),
            cxx_override: cxx_path.clone(),
            contest: contest_id,
            ..Default::default()
        };
        let sanitize_opts = xcpc_judge::SanitizeOptions {
            tests: tests_vec,
            solutions: solutions_vec,
            cxx_override: cxx_path,
            ..Default::default()
        };
        let slot_arc = self.package_build.clone();
        let revision_arc = self.revision.clone();
        let slug_owned = slug.to_owned();

        let spawned = std::thread::Builder::new()
            .name("xcpc-pkg-build".into())
            .spawn(move || {
                let slot_for_cb = slot_arc.clone();
                let progress_cb = move |ev: xcpc_judge::ProgressEvent| {
                    let Ok(mut slot) = slot_for_cb.lock() else {
                        return;
                    };
                    if slot.seq != my_seq {
                        return;
                    }
                    match ev {
                        xcpc_judge::ProgressEvent::Compiling {
                            target,
                            completed,
                            total,
                        } => {
                            slot.phase = Some("compiling".to_owned());
                            slot.detail = Some(target);
                            slot.completed = completed as u32;
                            slot.total = total as u32;
                        }
                        xcpc_judge::ProgressEvent::SelfTesting { completed, total } => {
                            slot.phase = Some("self_testing".to_owned());
                            slot.detail = None;
                            slot.completed = completed as u32;
                            slot.total = total as u32;
                        }
                        xcpc_judge::ProgressEvent::Generating {
                            test_name,
                            completed,
                            total,
                        } => {
                            slot.phase = Some("generating".to_owned());
                            slot.detail = Some(test_name);
                            slot.completed = completed as u32;
                            slot.total = total as u32;
                        }
                        xcpc_judge::ProgressEvent::Validating {
                            test_name,
                            completed,
                            total,
                        } => {
                            slot.phase = Some("validating".to_owned());
                            slot.detail = Some(test_name);
                            slot.completed = completed as u32;
                            slot.total = total as u32;
                        }
                        xcpc_judge::ProgressEvent::Running {
                            solution,
                            test_name,
                            verdict,
                            cpu_time_ms,
                            completed,
                            total,
                            cell,
                        } => {
                            slot.phase = Some("running".to_owned());
                            slot.detail = Some(format!(
                                "{solution} {test_name} ({verdict} {cpu_time_ms}ms)"
                            ));
                            slot.completed = completed as u32;
                            slot.total = total as u32;
                            if let Some(c) = cell {
                                let c = *c;
                                let key = (c.test_index as u32, c.solution.clone());
                                if let Some(&idx) = slot.live_cells_index.get(&key) {
                                    slot.live_cells[idx] = c;
                                } else {
                                    let idx = slot.live_cells.len();
                                    slot.live_cells.push(c);
                                    slot.live_cells_index.insert(key, idx);
                                }
                                slot.live_cells_json_cache = None;
                            }
                        }
                        xcpc_judge::ProgressEvent::Finished { .. } => {
                            slot.phase = Some("finished".to_owned());
                        }
                    }
                };

                let serialized_result: std::result::Result<String, xcpc_judge::JudgeError> =
                    if is_sanitize {
                        xcpc_judge::sanitize_package(
                            &ws,
                            &slug_owned,
                            &sanitize_opts,
                            Some(cancel_flag.clone()),
                            Some(&progress_cb),
                        )
                        .and_then(|report| {
                            serde_json::to_string(&report).map_err(|e| {
                                xcpc_judge::JudgeError::InvalidConfig(format!(
                                    "serialize sanitizer report failed: {e}"
                                ))
                            })
                        })
                    } else {
                        xcpc_judge::build_package(
                            &ws,
                            &slug_owned,
                            &opts,
                            Some(cancel_flag.clone()),
                            Some(&progress_cb),
                        )
                        .and_then(|report| {
                            serde_json::to_string(&report).map_err(|e| {
                                xcpc_judge::JudgeError::InvalidConfig(format!(
                                    "serialize build report failed: {e}"
                                ))
                            })
                        })
                    };

                if let Ok(mut slot) = slot_arc.lock()
                    && slot.seq == my_seq
                {
                    slot.running = false;
                    slot.cancel_flag = None;
                    match serialized_result {
                        Ok(json) => {
                            slot.phase = Some("finished".to_owned());
                            slot.report_json = Some(json);
                            slot.error = None;
                        }
                        Err(xcpc_judge::JudgeError::Cancelled) => {
                            slot.cancelled = true;
                            slot.phase = Some("finished".to_owned());
                            slot.error = None;
                        }
                        Err(e) => {
                            slot.phase = Some("finished".to_owned());
                            slot.error = Some(e.to_string());
                        }
                    }
                }
                revision_arc.fetch_add(1, Ordering::AcqRel);
            });

        if let Err(e) = spawned {
            let mut slot = self.package_build.lock().unwrap();
            if slot.seq == my_seq {
                slot.running = false;
                slot.cancel_flag = None;
                slot.error = Some(format!("spawn build thread failed: {e}"));
            }
            return ActionResult::err("io", format!("spawn build thread failed: {e}"));
        }

        ActionResult::ok()
    }

    pub fn cancel_package_build(&self) -> ActionResult {
        let mut slot = self.package_build.lock().unwrap();
        if let Some(ref flag) = slot.cancel_flag {
            flag.store(true, Ordering::Relaxed);
            slot.cancelled = true;
        }
        ActionResult::ok()
    }

    pub fn package_build_status(&self) -> PackageBuildStatusJs {
        let mut slot = self.package_build.lock().unwrap();
        let live_cells_json = if slot.live_cells.is_empty() {
            None
        } else {
            if slot.live_cells_json_cache.is_none() {
                slot.live_cells_json_cache = serde_json::to_string(&slot.live_cells).ok();
            }
            slot.live_cells_json_cache.clone()
        };
        PackageBuildStatusJs {
            seq: slot.seq,
            running: slot.running,
            slug: slot.slug.clone(),
            mode: slot.mode.clone(),
            cancelled: slot.cancelled,
            phase: slot.phase.clone(),
            detail: slot.detail.clone(),
            completed: slot.completed,
            total: slot.total,
            report_json: slot.report_json.clone(),
            live_cells_json,
            error: slot.error.clone(),
        }
    }

    pub fn resolve_package_file(
        &self,
        slug: &str,
        rel_path: &str,
    ) -> Result<PathBuf, ActionResult> {
        let ws = self.core.lock().unwrap().workspace.clone();
        let Some(ws) = ws else {
            return Err(ActionResult::err("no_workspace", "no workspace open"));
        };
        if let Err(e) = check_id(slug) {
            return Err(ActionResult::err("invalid_id", e.to_string()));
        }
        let cleaned = rel_path.trim().trim_start_matches('/');
        if cleaned.is_empty() || cleaned.contains("..") {
            return Err(ActionResult::err(
                "path_escape",
                "invalid package file path",
            ));
        }
        let prob_dir_rel = format!("problems/{slug}");
        let prob_dir = ws
            .resolve(&prob_dir_rel)
            .map_err(ActionResult::from_storage_err)?;
        let canonical_prob = prob_dir
            .canonicalize()
            .map_err(|e| ActionResult::err("not_found", format!("{prob_dir_rel}: {e}")))?;

        let ws_rel = format!("{prob_dir_rel}/{cleaned}");
        let resolved = ws
            .resolve(&ws_rel)
            .map_err(ActionResult::from_storage_err)?;
        if !resolved.exists() {
            return Err(ActionResult::err(
                "not_found",
                format!("{cleaned} not found"),
            ));
        }
        let canonical = resolved
            .canonicalize()
            .map_err(|e| ActionResult::err("io", format!("{cleaned}: {e}")))?;
        if !canonical.starts_with(ws.root()) || !canonical.starts_with(&canonical_prob) {
            return Err(ActionResult::err(
                "path_escape",
                format!("path {cleaned:?} is outside problem directory"),
            ));
        }
        Ok(canonical)
    }

    pub fn inspect_test_case(
        &self,
        slug: &str,
        selector: &str,
        head_lines: Option<u32>,
        tail_lines: Option<u32>,
    ) -> TestCasePreviewJs {
        let ws = self.core.lock().unwrap().workspace.clone();
        let Some(ws) = ws else {
            return TestCasePreviewJs {
                ok: false,
                error: Some("no workspace open".into()),
                ..Default::default()
            };
        };
        let head = head_lines.unwrap_or(40) as usize;
        let tail = tail_lines.unwrap_or(20) as usize;
        match xcpc_judge::inspect_test_case(&ws, slug, selector, head, tail) {
            Ok(p) => TestCasePreviewJs {
                ok: true,
                index: p.index as u32,
                name: p.name,
                tag: p.tag,
                is_sample: p.is_sample,
                gen_cmd: p.gen_cmd,
                available_on_disk: p.available_on_disk,
                size_bytes: p.size_bytes as f64,
                total_lines: p.total_lines as u32,
                total_lines_exact: p.total_lines_exact.map(|n| n as u32),
                omitted_bytes: p.omitted_bytes.map(|n| n as f64),
                head: p.head,
                tail: p.tail,
                truncated: p.truncated,
                ans_available: p.ans_available,
                ans_source: p.ans_source,
                ans_size_bytes: p.ans_size_bytes as f64,
                ans_head: p.ans_head,
                ans_tail: p.ans_tail,
                ans_truncated: p.ans_truncated,
                ans_total_lines: p.ans_total_lines.map(|n| n as u32),
                std_output_available: p.std_output_available,
                std_output_size_bytes: p.std_output_size_bytes as f64,
                std_output_head: p.std_output_head,
                std_output_tail: p.std_output_tail,
                std_output_truncated: p.std_output_truncated,
                rule_index: p.rule_index.map(|n| n as u32),
                file_rel: p.file_rel,
                error: None,
            },
            Err(e) => TestCasePreviewJs {
                ok: false,
                error: Some(e.to_string()),
                ..Default::default()
            },
        }
    }

    pub fn set_package_checker(&self, slug: &str, checker: &str) -> ActionResult {
        let Some(ws) = self.core.lock().unwrap().workspace.clone() else {
            return ActionResult::err("no_workspace", "no workspace open");
        };
        if ws.is_read_only() {
            return ActionResult::err("read_only", "workspace is read-only");
        }
        match xcpc_judge::set_package_checker(&ws, slug, checker) {
            Ok(()) => {
                {
                    let mut core = self.core.lock().unwrap();
                    let _ = core.index.refresh_problem(&ws, slug);
                }
                self.invalidate_cache();
                ActionResult::ok()
            }
            Err(e) => ActionResult::err(e.code(), e.human_message()),
        }
    }

    pub fn set_solution_config(
        &self,
        slug: &str,
        name: &str,
        is_main: Option<bool>,
        expect: Option<Vec<String>>,
        tags: Option<Vec<String>>,
    ) -> ActionResult {
        let Some(ws) = self.core.lock().unwrap().workspace.clone() else {
            return ActionResult::err("no_workspace", "no workspace open");
        };
        if ws.is_read_only() {
            return ActionResult::err("read_only", "workspace is read-only");
        }
        match xcpc_judge::set_solution_config(&ws, slug, name, is_main, expect, tags) {
            Ok(()) => {
                {
                    let mut core = self.core.lock().unwrap();
                    let _ = core.index.refresh_problem(&ws, slug);
                }
                self.invalidate_cache();
                ActionResult::ok()
            }
            Err(e) => ActionResult::err(e.code(), e.human_message()),
        }
    }

    pub fn create_package_source(
        &self,
        slug: &str,
        kind: &str,
        name: &str,
    ) -> PackageCreateResultJs {
        let Some(ws) = self.core.lock().unwrap().workspace.clone() else {
            return PackageCreateResultJs {
                ok: false,
                code: Some("no_workspace".into()),
                error: Some("no workspace open".into()),
                ..Default::default()
            };
        };
        if ws.is_read_only() {
            return PackageCreateResultJs {
                ok: false,
                code: Some("read_only".into()),
                error: Some("workspace is read-only".into()),
                ..Default::default()
            };
        }
        let parsed_kind = match kind.trim().to_ascii_lowercase().as_str() {
            "solution" => xcpc_judge::PackageSourceKind::Solution,
            "generator" => xcpc_judge::PackageSourceKind::Generator,
            "validator" => xcpc_judge::PackageSourceKind::Validator,
            "interactor" => xcpc_judge::PackageSourceKind::Interactor,
            "custom_checker" | "checker" => xcpc_judge::PackageSourceKind::CustomChecker,
            "validator_self_test_valid" => xcpc_judge::PackageSourceKind::ValidatorSelfTestValid,
            "validator_self_test_invalid" => {
                xcpc_judge::PackageSourceKind::ValidatorSelfTestInvalid
            }
            other => {
                return PackageCreateResultJs {
                    ok: false,
                    code: Some("invalid_config".into()),
                    error: Some(format!("unknown package source kind: {other}")),
                    ..Default::default()
                };
            }
        };
        match xcpc_judge::create_package_source(&ws, slug, parsed_kind, name) {
            Ok(rel_path) => {
                {
                    let mut core = self.core.lock().unwrap();
                    let _ = core.index.refresh_problem(&ws, slug);
                }
                self.invalidate_cache();
                PackageCreateResultJs {
                    ok: true,
                    rel_path,
                    error: None,
                    code: None,
                }
            }
            Err(e) => PackageCreateResultJs {
                ok: false,
                code: Some(e.code().to_owned()),
                error: Some(e.human_message()),
                ..Default::default()
            },
        }
    }

    pub fn add_hand_test(
        &self,
        slug: &str,
        name: Option<&str>,
        tag: Option<&str>,
        content: &str,
    ) -> AddedTestResultJs {
        let Some(ws) = self.core.lock().unwrap().workspace.clone() else {
            return AddedTestResultJs {
                ok: false,
                code: Some("no_workspace".into()),
                error: Some("no workspace open".into()),
                ..Default::default()
            };
        };
        if ws.is_read_only() {
            return AddedTestResultJs {
                ok: false,
                code: Some("read_only".into()),
                error: Some("workspace is read-only".into()),
                ..Default::default()
            };
        }
        match xcpc_judge::add_hand_test(&ws, slug, name, tag, content) {
            Ok(info) => {
                {
                    let mut core = self.core.lock().unwrap();
                    let _ = core.index.refresh_problem(&ws, slug);
                }
                self.invalidate_cache();
                AddedTestResultJs {
                    ok: true,
                    rule_index: info.rule_index as u32,
                    file_rel: info.file_rel,
                    name: info.name,
                    tag: info.tag,
                    error: None,
                    code: None,
                }
            }
            Err(e) => AddedTestResultJs {
                ok: false,
                code: Some(e.code().to_owned()),
                error: Some(e.human_message()),
                ..Default::default()
            },
        }
    }

    pub fn add_generator_test(
        &self,
        slug: &str,
        generator: &str,
        args: &str,
        repeat: u32,
        tag: Option<&str>,
        name: Option<&str>,
    ) -> AddedTestResultJs {
        let Some(ws) = self.core.lock().unwrap().workspace.clone() else {
            return AddedTestResultJs {
                ok: false,
                code: Some("no_workspace".into()),
                error: Some("no workspace open".into()),
                ..Default::default()
            };
        };
        if ws.is_read_only() {
            return AddedTestResultJs {
                ok: false,
                code: Some("read_only".into()),
                error: Some("workspace is read-only".into()),
                ..Default::default()
            };
        }
        match xcpc_judge::add_generator_test(&ws, slug, generator, args, repeat, tag, name) {
            Ok(info) => {
                {
                    let mut core = self.core.lock().unwrap();
                    let _ = core.index.refresh_problem(&ws, slug);
                }
                self.invalidate_cache();
                AddedTestResultJs {
                    ok: true,
                    rule_index: info.rule_index as u32,
                    file_rel: info.file_rel,
                    name: info.name,
                    tag: info.tag,
                    error: None,
                    code: None,
                }
            }
            Err(e) => AddedTestResultJs {
                ok: false,
                code: Some(e.code().to_owned()),
                error: Some(e.human_message()),
                ..Default::default()
            },
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn update_generator_test(
        &self,
        slug: &str,
        rule_index: u32,
        generator: &str,
        args: &str,
        repeat: u32,
        tag: Option<&str>,
        name: Option<&str>,
    ) -> ActionResult {
        let Some(ws) = self.core.lock().unwrap().workspace.clone() else {
            return ActionResult::err("no_workspace", "no workspace open");
        };
        if ws.is_read_only() {
            return ActionResult::err("read_only", "workspace is read-only");
        }
        match xcpc_judge::update_generator_test(
            &ws,
            slug,
            rule_index as usize,
            generator,
            args,
            repeat,
            tag,
            name,
        ) {
            Ok(()) => {
                {
                    let mut core = self.core.lock().unwrap();
                    let _ = core.index.refresh_problem(&ws, slug);
                }
                self.invalidate_cache();
                ActionResult::ok()
            }
            Err(e) => ActionResult::err(e.code(), e.human_message()),
        }
    }

    pub fn remove_test_rule(&self, slug: &str, rule_index: u32) -> ActionResult {
        let Some(ws) = self.core.lock().unwrap().workspace.clone() else {
            return ActionResult::err("no_workspace", "no workspace open");
        };
        if ws.is_read_only() {
            return ActionResult::err("read_only", "workspace is read-only");
        }
        match xcpc_judge::remove_test_rule(&ws, slug, rule_index as usize) {
            Ok(()) => {
                {
                    let mut core = self.core.lock().unwrap();
                    let _ = core.index.refresh_problem(&ws, slug);
                }
                self.invalidate_cache();
                ActionResult::ok()
            }
            Err(e) => ActionResult::err(e.code(), e.human_message()),
        }
    }

    pub fn delete_solution(&self, slug: &str, name: &str) -> ActionResult {
        let Some(ws) = self.core.lock().unwrap().workspace.clone() else {
            return ActionResult::err("no_workspace", "no workspace open");
        };
        if ws.is_read_only() {
            return ActionResult::err("read_only", "workspace is read-only");
        }
        match xcpc_judge::delete_solution(&ws, slug, name) {
            Ok(()) => {
                {
                    let mut core = self.core.lock().unwrap();
                    let _ = core
                        .index
                        .refresh_path(&ws, &format!("problems/{slug}/problem.toml"));
                }
                self.invalidate_cache();
                ActionResult::ok()
            }
            Err(e) => ActionResult::err(e.code(), e.human_message()),
        }
    }

    pub fn delete_generator(
        &self,
        slug: &str,
        filename: &str,
        remove_referencing_tests: bool,
    ) -> ActionResult {
        let Some(ws) = self.core.lock().unwrap().workspace.clone() else {
            return ActionResult::err("no_workspace", "no workspace open");
        };
        if ws.is_read_only() {
            return ActionResult::err("read_only", "workspace is read-only");
        }
        match xcpc_judge::delete_generator(&ws, slug, filename, remove_referencing_tests) {
            Ok(_) => {
                {
                    let mut core = self.core.lock().unwrap();
                    let _ = core
                        .index
                        .refresh_path(&ws, &format!("problems/{slug}/problem.toml"));
                }
                self.invalidate_cache();
                ActionResult::ok()
            }
            Err(e) => ActionResult::err(e.code(), e.human_message()),
        }
    }

    pub fn delete_self_test(&self, slug: &str, rel_path: &str) -> ActionResult {
        let Some(ws) = self.core.lock().unwrap().workspace.clone() else {
            return ActionResult::err("no_workspace", "no workspace open");
        };
        if ws.is_read_only() {
            return ActionResult::err("read_only", "workspace is read-only");
        }
        match xcpc_judge::delete_self_test(&ws, slug, rel_path) {
            Ok(()) => {
                self.invalidate_cache();
                ActionResult::ok()
            }
            Err(e) => ActionResult::err(e.code(), e.human_message()),
        }
    }

    pub fn delete_interactor(&self, slug: &str) -> ActionResult {
        let Some(ws) = self.core.lock().unwrap().workspace.clone() else {
            return ActionResult::err("no_workspace", "no workspace open");
        };
        if ws.is_read_only() {
            return ActionResult::err("read_only", "workspace is read-only");
        }
        match xcpc_judge::delete_interactor(&ws, slug) {
            Ok(_) => {
                {
                    let mut core = self.core.lock().unwrap();
                    let _ = core
                        .index
                        .refresh_path(&ws, &format!("problems/{slug}/problem.toml"));
                }
                self.invalidate_cache();
                ActionResult::ok()
            }
            Err(e) => ActionResult::err(e.code(), e.human_message()),
        }
    }

    pub fn set_package_runs(&self, slug: &str, runs: Option<u32>) -> ActionResult {
        let Some(ws) = self.core.lock().unwrap().workspace.clone() else {
            return ActionResult::err("no_workspace", "no workspace open");
        };
        if ws.is_read_only() {
            return ActionResult::err("read_only", "workspace is read-only");
        }
        match xcpc_judge::set_package_runs(&ws, slug, runs) {
            Ok(()) => {
                {
                    let mut core = self.core.lock().unwrap();
                    let _ = core
                        .index
                        .refresh_path(&ws, &format!("problems/{slug}/problem.toml"));
                }
                self.invalidate_cache();
                ActionResult::ok()
            }
            Err(e) => ActionResult::err(e.code(), e.human_message()),
        }
    }

    pub fn upsert_subtask(
        &self,
        slug: &str,
        id: &str,
        score: u32,
        depends: Vec<String>,
        scoring: &str,
    ) -> ActionResult {
        let Some(ws) = self.core.lock().unwrap().workspace.clone() else {
            return ActionResult::err("no_workspace", "no workspace open");
        };
        if ws.is_read_only() {
            return ActionResult::err("read_only", "workspace is read-only");
        }
        let parsed_scoring = match scoring.trim().to_ascii_lowercase().as_str() {
            "" | "min" => xcpc_domain::SubtaskScoring::Min,
            "sum" => xcpc_domain::SubtaskScoring::Sum,
            other => {
                return ActionResult::err(
                    "invalid_config",
                    format!("unknown subtask scoring mode: {other}"),
                );
            }
        };
        let spec = xcpc_domain::SubtaskSpec {
            id: id.to_owned(),
            score,
            depends,
            scoring: parsed_scoring,
        };
        match xcpc_judge::upsert_subtask(&ws, slug, &spec) {
            Ok(()) => {
                {
                    let mut core = self.core.lock().unwrap();
                    let _ = core
                        .index
                        .refresh_path(&ws, &format!("problems/{slug}/problem.toml"));
                }
                self.invalidate_cache();
                ActionResult::ok()
            }
            Err(e) => ActionResult::err(e.code(), e.human_message()),
        }
    }

    pub fn delete_subtask(
        &self,
        slug: &str,
        subtask_id: &str,
        clear_references: bool,
    ) -> ActionResult {
        let Some(ws) = self.core.lock().unwrap().workspace.clone() else {
            return ActionResult::err("no_workspace", "no workspace open");
        };
        if ws.is_read_only() {
            return ActionResult::err("read_only", "workspace is read-only");
        }
        match xcpc_judge::delete_subtask(&ws, slug, subtask_id, clear_references) {
            Ok(_) => {
                {
                    let mut core = self.core.lock().unwrap();
                    let _ = core
                        .index
                        .refresh_path(&ws, &format!("problems/{slug}/problem.toml"));
                }
                self.invalidate_cache();
                ActionResult::ok()
            }
            Err(e) => ActionResult::err(e.code(), e.human_message()),
        }
    }

    pub fn set_test_subtask(
        &self,
        slug: &str,
        rule_index: u32,
        subtask: Option<&str>,
    ) -> ActionResult {
        let Some(ws) = self.core.lock().unwrap().workspace.clone() else {
            return ActionResult::err("no_workspace", "no workspace open");
        };
        if ws.is_read_only() {
            return ActionResult::err("read_only", "workspace is read-only");
        }
        match xcpc_judge::set_test_subtask(&ws, slug, rule_index as usize, subtask) {
            Ok(()) => {
                self.invalidate_cache();
                ActionResult::ok()
            }
            Err(e) => ActionResult::err(e.code(), e.human_message()),
        }
    }

    pub fn set_solution_scoring(
        &self,
        slug: &str,
        name: &str,
        score_json: Option<&str>,
        subtasks_json: Option<&str>,
    ) -> ActionResult {
        let Some(ws) = self.core.lock().unwrap().workspace.clone() else {
            return ActionResult::err("no_workspace", "no workspace open");
        };
        if ws.is_read_only() {
            return ActionResult::err("read_only", "workspace is read-only");
        }
        let parsed_score: Option<Option<xcpc_domain::ScoreExpect>> = match score_json {
            None => None,
            Some(raw) => {
                let trimmed = raw.trim();
                if trimmed.is_empty() || trimmed == "null" {
                    Some(None)
                } else {
                    match serde_json::from_str::<xcpc_domain::ScoreExpect>(trimmed) {
                        Ok(exp) => Some(Some(exp)),
                        Err(e) => {
                            return ActionResult::err(
                                "invalid_config",
                                format!("invalid score expectation: {e}"),
                            );
                        }
                    }
                }
            }
        };
        let parsed_subtasks: Option<BTreeMap<String, xcpc_domain::ExpectVerdict>> =
            match subtasks_json {
                None => None,
                Some(raw) => {
                    let trimmed = raw.trim();
                    if trimmed.is_empty() || trimmed == "null" {
                        Some(BTreeMap::new())
                    } else {
                        match serde_json::from_str::<BTreeMap<String, xcpc_domain::ExpectVerdict>>(
                            trimmed,
                        ) {
                            Ok(map) => Some(map),
                            Err(e) => {
                                return ActionResult::err(
                                    "invalid_config",
                                    format!("invalid subtask expectations: {e}"),
                                );
                            }
                        }
                    }
                }
            };
        match xcpc_judge::set_solution_scoring(&ws, slug, name, parsed_score, parsed_subtasks) {
            Ok(()) => {
                self.invalidate_cache();
                ActionResult::ok()
            }
            Err(e) => ActionResult::err(e.code(), e.human_message()),
        }
    }

    pub fn delete_sample(&self, slug: &str, sample_index: u32) -> ActionResult {
        let Some(ws) = self.core.lock().unwrap().workspace.clone() else {
            return ActionResult::err("no_workspace", "no workspace open");
        };
        if ws.is_read_only() {
            return ActionResult::err("read_only", "workspace is read-only");
        }
        let problem = match ws.load_problem(slug) {
            Ok(p) => p,
            Err(e) => return ActionResult::from_storage_err(e),
        };
        let idx = sample_index.saturating_sub(1) as usize;
        if sample_index == 0 || idx >= problem.samples.len() {
            return ActionResult::err("not_found", format!("sample #{sample_index} not found"));
        }
        let remaining: Vec<SamplePairJs> = problem
            .samples
            .into_iter()
            .enumerate()
            .filter(|(i, _)| *i != idx)
            .map(|(_, s)| SamplePairJs {
                name: s.name,
                input: s.input,
                output: s.output,
            })
            .collect();
        self.save_samples(slug, remaining)
    }

    pub fn normalize_dirty_inputs(&self, slug: &str) -> NormalizeDirtyResultJs {
        let Some(ws) = self.core.lock().unwrap().workspace.clone() else {
            return NormalizeDirtyResultJs {
                ok: false,
                code: Some("no_workspace".into()),
                error: Some("no workspace open".into()),
                ..Default::default()
            };
        };
        if ws.is_read_only() {
            return NormalizeDirtyResultJs {
                ok: false,
                code: Some("read_only".into()),
                error: Some("workspace is read-only".into()),
                ..Default::default()
            };
        }
        // Snapshot original bytes of dirty inputs so user can undo the normalization in one click.
        let mut backups: Vec<(String, Vec<u8>)> = Vec::new();
        if let Ok(problem) = ws.load_problem(slug)
            && let Ok(prob_dir) = ws.resolve(&problem.dir)
        {
            for issue in xcpc_judge::check_dirty_inputs(&prob_dir) {
                let ws_rel = format!("problems/{slug}/{}", issue.path);
                if let Ok(resolved) = ws.resolve(&ws_rel)
                    && let Ok(raw) = std::fs::read(&resolved)
                {
                    backups.push((ws_rel, raw));
                }
            }
        }
        match xcpc_judge::normalize_dirty_inputs(&ws, slug) {
            Ok(fixed) => {
                if !fixed.is_empty() && !backups.is_empty() {
                    *self.dirty_inputs_backup.lock().unwrap() = Some((slug.to_owned(), backups));
                }
                self.invalidate_cache();
                NormalizeDirtyResultJs {
                    ok: true,
                    fixed,
                    error: None,
                    code: None,
                }
            }
            Err(e) => NormalizeDirtyResultJs {
                ok: false,
                code: Some(e.code().to_owned()),
                error: Some(e.human_message()),
                ..Default::default()
            },
        }
    }

    /// Restores the original bytes from the most recent `normalize_dirty_inputs` call for `slug`.
    pub fn undo_normalize_dirty_inputs(&self, slug: &str) -> ActionResult {
        let Some(ws) = self.core.lock().unwrap().workspace.clone() else {
            return ActionResult::err("no_workspace", "no workspace open");
        };
        if ws.is_read_only() {
            return ActionResult::err("read_only", "workspace is read-only");
        }
        let backup = {
            let mut guard = self.dirty_inputs_backup.lock().unwrap();
            match guard.as_ref() {
                Some((s, _)) if s == slug => guard.take().map(|(_, files)| files),
                _ => None,
            }
        };
        let Some(files) = backup else {
            return ActionResult::err("not_found", "nothing to undo");
        };
        for (ws_rel, raw) in &files {
            if let Err(e) = ws.write(ws_rel, raw) {
                return ActionResult::from_storage_err(e);
            }
        }
        self.invalidate_cache();
        ActionResult::ok()
    }

    /// Bounded read of a package file inside `problems/<slug>/` for the lightweight quick-editor.
    /// Never loads files larger than 256 KiB into memory.
    pub fn read_package_file(&self, slug: &str, rel_path: &str) -> PackageFileContentJs {
        const MAX_QUICK_EDIT_BYTES: u64 = 256 * 1024;
        let canonical = match self.resolve_package_file(slug, rel_path) {
            Ok(p) => p,
            Err(err) => {
                return PackageFileContentJs {
                    ok: false,
                    code: err.code,
                    error: err.error,
                    ..Default::default()
                };
            }
        };
        let meta = match std::fs::metadata(&canonical) {
            Ok(m) => m,
            Err(e) => {
                return PackageFileContentJs {
                    ok: false,
                    code: Some("io".into()),
                    error: Some(e.to_string()),
                    ..Default::default()
                };
            }
        };
        if !meta.is_file() {
            return PackageFileContentJs {
                ok: false,
                code: Some("invalid_path".into()),
                error: Some(format!("{rel_path} is not a file")),
                ..Default::default()
            };
        }
        let size = meta.len();
        if size > MAX_QUICK_EDIT_BYTES {
            return PackageFileContentJs {
                ok: true,
                content: String::new(),
                content_hash: String::new(),
                size_bytes: size as f64,
                too_large: true,
                error: None,
                code: None,
            };
        }
        match std::fs::read(&canonical) {
            Ok(bytes) => PackageFileContentJs {
                ok: true,
                content_hash: content_hash(&bytes),
                content: String::from_utf8_lossy(&bytes).into_owned(),
                size_bytes: bytes.len() as f64,
                too_large: false,
                error: None,
                code: None,
            },
            Err(e) => PackageFileContentJs {
                ok: false,
                code: Some("io".into()),
                error: Some(e.to_string()),
                ..Default::default()
            },
        }
    }

    /// Atomically saves a package file inside `problems/<slug>/` (max 256 KiB, sandboxed,
    /// conflict-checked against `expected_hash`, watcher self-write suppressed).
    pub fn save_package_file(
        &self,
        slug: &str,
        rel_path: &str,
        content: &str,
        expected_hash: Option<&str>,
    ) -> SavePackageFileResultJs {
        const MAX_QUICK_EDIT_BYTES: usize = 256 * 1024;
        let err_res = |code: &str, msg: String| SavePackageFileResultJs {
            ok: false,
            conflict: false,
            content_hash: None,
            error: Some(msg),
            code: Some(code.to_owned()),
        };
        if content.len() > MAX_QUICK_EDIT_BYTES {
            return err_res(
                "too_large",
                "file exceeds 256 KiB quick-edit limit; open in external editor".into(),
            );
        }
        let Some(ws) = self.core.lock().unwrap().workspace.clone() else {
            return err_res("no_workspace", "no workspace open".into());
        };
        if ws.is_read_only() {
            return err_res("read_only", "workspace is read-only".into());
        }
        if let Err(e) = check_id(slug) {
            return err_res("invalid_id", e.to_string());
        }
        let cleaned = rel_path.trim().trim_start_matches('/');
        if cleaned.is_empty() || cleaned.contains("..") {
            return err_res("path_escape", "invalid package file path".into());
        }
        let prob_dir_rel = format!("problems/{slug}");
        let prob_dir = match ws.resolve(&prob_dir_rel) {
            Ok(p) => p,
            Err(e) => {
                let a = ActionResult::from_storage_err(e);
                return SavePackageFileResultJs {
                    ok: false,
                    conflict: false,
                    content_hash: None,
                    error: a.error,
                    code: a.code,
                };
            }
        };
        let canonical_prob = match prob_dir.canonicalize() {
            Ok(p) => p,
            Err(e) => return err_res("not_found", format!("{prob_dir_rel}: {e}")),
        };
        let ws_rel = format!("{prob_dir_rel}/{cleaned}");
        let target = match ws.resolve(&ws_rel) {
            Ok(p) => p,
            Err(e) => {
                let a = ActionResult::from_storage_err(e);
                return SavePackageFileResultJs {
                    ok: false,
                    conflict: false,
                    content_hash: None,
                    error: a.error,
                    code: a.code,
                };
            }
        };
        // Reject symlink escape if target or its parent already exists.
        if target.exists() {
            let Ok(canonical) = target.canonicalize() else {
                return err_res("io", format!("cannot resolve {cleaned}"));
            };
            if !canonical.starts_with(ws.root()) || !canonical.starts_with(&canonical_prob) {
                return err_res(
                    "path_escape",
                    format!("path {cleaned:?} is outside problem directory"),
                );
            }
        } else if let Some(parent) = target.parent()
            && parent.exists()
            && let Ok(canon_parent) = parent.canonicalize()
            && (!canon_parent.starts_with(ws.root()) || !canon_parent.starts_with(&canonical_prob))
        {
            return err_res(
                "path_escape",
                format!("path {cleaned:?} is outside problem directory"),
            );
        }

        let expected = expected_hash.map(str::trim).filter(|s| !s.is_empty());
        match ws.write_checked(&ws_rel, content.as_bytes(), expected) {
            Ok(new_hash) => {
                {
                    let mut core = self.core.lock().unwrap();
                    let _ = core.index.refresh_path(&ws, &ws_rel);
                }
                self.invalidate_cache();
                SavePackageFileResultJs {
                    ok: true,
                    conflict: false,
                    content_hash: Some(new_hash),
                    error: None,
                    code: None,
                }
            }
            Err(xcpc_storage::Error::Conflict { .. }) => SavePackageFileResultJs {
                ok: false,
                conflict: true,
                content_hash: None,
                error: Some("file was modified externally since it was opened".into()),
                code: Some("conflict".into()),
            },
            Err(e) => {
                let a = ActionResult::from_storage_err(e);
                SavePackageFileResultJs {
                    ok: false,
                    conflict: false,
                    content_hash: None,
                    error: a.error,
                    code: a.code,
                }
            }
        }
    }

    // ---- Phase 3B: Polygon import & OJ package export ----------------------------------

    /// Fast pre-check before opening the native file picker for Polygon ZIP import.
    pub fn validate_polygon_import_writable(&self) -> Result<(), (String, String, String)> {
        let ws = self.core.lock().unwrap().workspace.clone();
        let Some(ws) = ws else {
            return Err((
                "no_workspace".into(),
                "没有打开的工作区".into(),
                "no workspace open".into(),
            ));
        };
        if ws.is_read_only() {
            return Err((
                "read_only".into(),
                "工作区以只读方式打开，无法导入。".into(),
                "workspace is read-only".into(),
            ));
        }
        Ok(())
    }

    /// Imports a Polygon single-problem or contest ZIP into the open workspace, refreshing
    /// the in-memory index and invalidating preview caches on success.
    pub fn import_polygon(
        &self,
        zip_path: &Path,
        target_id: Option<String>,
    ) -> PolygonImportResultJs {
        let ws = self.core.lock().unwrap().workspace.clone();
        let Some(ws) = ws else {
            return PolygonImportResultJs {
                ok: false,
                code: Some("no_workspace".into()),
                error: Some("没有打开的工作区".into()),
                detail: Some("no workspace open".into()),
                ..Default::default()
            };
        };
        if ws.is_read_only() {
            return PolygonImportResultJs {
                ok: false,
                code: Some("read_only".into()),
                error: Some("工作区以只读方式打开，无法导入。".into()),
                detail: Some("workspace is read-only".into()),
                ..Default::default()
            };
        }

        let opts = xcpc_packages::PolygonImportOptions {
            target_id: target_id.filter(|s| !s.trim().is_empty()),
            ..Default::default()
        };
        match xcpc_packages::import_polygon(&ws, zip_path, &opts) {
            Ok(res) => {
                if let Ok(new_index) = WorkspaceIndex::scan(&ws) {
                    let mut core = self.core.lock().unwrap();
                    if core
                        .workspace
                        .as_ref()
                        .is_some_and(|w| w.root() == ws.root())
                    {
                        core.index = new_index;
                    }
                }
                self.invalidate_cache();

                let (kind, id, title) = match res.kind {
                    xcpc_packages::PolygonImportKind::Contest => {
                        let cid = res.contest_id.unwrap_or_default();
                        let name = ws
                            .load_contest(&cid)
                            .map(|c| c.meta.name)
                            .unwrap_or_else(|_| cid.clone());
                        ("contest".to_owned(), cid, name)
                    }
                    xcpc_packages::PolygonImportKind::Problem => {
                        let first = res.problems.first();
                        let pid = first.map(|p| p.id.clone()).unwrap_or_default();
                        let ptitle = first
                            .map(|p| p.title.clone())
                            .unwrap_or_else(|| pid.clone());
                        ("problem".to_owned(), pid, ptitle)
                    }
                };
                let problem_ids = res.problems.into_iter().map(|p| p.id).collect();
                let warnings = res.warnings.into_iter().map(Into::into).collect();
                PolygonImportResultJs {
                    ok: true,
                    cancelled: false,
                    kind,
                    id,
                    title,
                    problem_ids,
                    warnings,
                    error: None,
                    code: None,
                    detail: None,
                }
            }
            Err(err) => PolygonImportResultJs {
                ok: false,
                cancelled: false,
                error: Some(err.human_message()),
                code: Some(err.code().to_owned()),
                detail: Some(err.to_string()),
                ..Default::default()
            },
        }
    }

    /// Exports a DOMjudge or Hydro minimal-set OJ package (strict verification gate enabled,
    /// `force: false`) into `build/packages/` inside the open workspace.
    pub fn export_oj_package(&self, format: &str, target: &str, id: &str) -> ExportOjResultJs {
        let Some(fmt) = xcpc_packages::OjFormat::parse(format) else {
            return ExportOjResultJs {
                ok: false,
                format: format.to_owned(),
                code: Some("invalid_format".into()),
                error: Some(format!("不支持的导出格式：{format}")),
                detail: Some(format!("unsupported OJ format: {format:?}")),
                ..Default::default()
            };
        };
        let export_target = match target {
            "problem" => xcpc_packages::ExportTarget::Problem(id.to_owned()),
            "contest" => xcpc_packages::ExportTarget::Contest(id.to_owned()),
            other => {
                return ExportOjResultJs {
                    ok: false,
                    format: fmt.as_str().to_owned(),
                    code: Some("invalid_format".into()),
                    error: Some(format!("不支持的导出目标：{other}")),
                    detail: Some(format!("unsupported export target: {other:?}")),
                    ..Default::default()
                };
            }
        };
        let ws = self.core.lock().unwrap().workspace.clone();
        let Some(ws) = ws else {
            return ExportOjResultJs {
                ok: false,
                format: fmt.as_str().to_owned(),
                code: Some("no_workspace".into()),
                error: Some("没有打开的工作区".into()),
                detail: Some("no workspace open".into()),
                ..Default::default()
            };
        };

        let packages_dir = ws.root().join("build").join("packages");
        let output_path = match &export_target {
            xcpc_packages::ExportTarget::Problem(slug) => {
                packages_dir.join(format!("{slug}-{}.zip", fmt.as_str()))
            }
            xcpc_packages::ExportTarget::Contest(cid) => {
                packages_dir.join(format!("{cid}-{}", fmt.as_str()))
            }
        };
        let opts = xcpc_packages::ExportOjOptions {
            format: fmt,
            output_path: output_path.clone(),
            force: false,
            language: None,
        };
        match xcpc_packages::export_oj_package(&ws, &export_target, &opts) {
            Ok(res) => {
                let paths: Vec<String> = res.artifacts.into_iter().map(|a| a.zip_path).collect();
                let reveal_path = match &export_target {
                    xcpc_packages::ExportTarget::Contest(_) => output_path.display().to_string(),
                    xcpc_packages::ExportTarget::Problem(_) => paths
                        .first()
                        .cloned()
                        .unwrap_or_else(|| output_path.display().to_string()),
                };
                let warnings = res.warnings.into_iter().map(Into::into).collect();
                ExportOjResultJs {
                    ok: true,
                    format: fmt.as_str().to_owned(),
                    paths,
                    reveal_path,
                    warnings,
                    error: None,
                    code: None,
                    detail: None,
                }
            }
            Err(err) => ExportOjResultJs {
                ok: false,
                format: fmt.as_str().to_owned(),
                error: Some(err.human_message()),
                code: Some(err.code().to_owned()),
                detail: Some(err.to_string()),
                ..Default::default()
            },
        }
    }

    // ---- Phase 3B+: Polling-free async batch task streaming ----------------------------

    pub fn start_batch_build(
        self: &Arc<Self>,
        ids: Vec<String>,
        mode: String,
        contest_id: Option<String>,
    ) -> BatchStartResultJs {
        let unique_ids = dedup_ids_preserve_order(&ids);
        if unique_ids.is_empty() {
            return BatchStartResultJs::err("invalid_argument", "请至少选择一道题目。");
        }
        let is_sanitize = mode.eq_ignore_ascii_case("sanitize");
        let (build_mode, kind) = if is_sanitize {
            (xcpc_judge::BuildMode::Verify, "build_sanitize")
        } else if mode.eq_ignore_ascii_case("verify") {
            (xcpc_judge::BuildMode::Verify, "build_verify")
        } else if mode.eq_ignore_ascii_case("debug") {
            (xcpc_judge::BuildMode::Debug, "build_debug")
        } else {
            return BatchStartResultJs::err(
                "invalid_mode",
                format!("不支持的构建模式：{mode}（期望 verify、debug 或 sanitize）"),
            );
        };

        let (ws, cxx_override) = {
            let core = self.core.lock().unwrap();
            (core.workspace.clone(), core.prefs.app.cxx_override.clone())
        };
        let Some(ws) = ws else {
            return BatchStartResultJs::err("no_workspace", "没有打开的工作区。");
        };
        if ws.is_read_only() {
            return BatchStartResultJs::err(
                "read_only",
                xcpc_storage::Error::ReadOnly.human_message(),
            );
        }

        let cancel_flag = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel::<BatchTaskEventJs>();
        let initial_items: Vec<BatchTaskItemJs> = unique_ids
            .iter()
            .map(|id| BatchTaskItemJs {
                problem_id: id.clone(),
                status: "pending".to_owned(),
                ..Default::default()
            })
            .collect();
        let total = initial_items.len() as u32;

        let my_task_id = {
            let mut slot = self.batch_task.lock().unwrap();
            if slot.running {
                return BatchStartResultJs::err(
                    "batch_task_in_progress",
                    "已有批量任务正在运行，请等待完成或先取消。",
                );
            }
            let next_id = slot.task_id.wrapping_add(1).max(1);
            let snapshot = BatchTaskSnapshotJs {
                task_id: next_id,
                running: true,
                kind: kind.to_owned(),
                total,
                items: initial_items,
                ..Default::default()
            };
            *slot = BatchTaskSlot {
                task_id: next_id,
                running: true,
                kind: kind.to_owned(),
                cancel_flag: Some(cancel_flag.clone()),
                close_tx: Some(tx.clone()),
                event_rx: Some(Arc::new(Mutex::new(rx))),
                snapshot,
            };
            next_id
        };

        let cxx_path = cxx_override.map(PathBuf::from);
        let opts = xcpc_judge::BuildOptions {
            mode: build_mode,
            cxx_override: cxx_path.clone(),
            contest: contest_id,
            ..Default::default()
        };
        let sanitize_opts = xcpc_judge::SanitizeOptions {
            cxx_override: cxx_path,
            ..Default::default()
        };
        let slot_arc = self.batch_task.clone();
        let weak_state = Arc::downgrade(self);
        let revision_arc = self.revision.clone();

        let spawned = std::thread::Builder::new()
            .name("xcpc-batch-build".into())
            .spawn(move || {
                let slot_cb = slot_arc.clone();
                let tx_cb = tx.clone();
                let progress_cb = move |ev: xcpc_judge::BatchBuildEvent| {
                    let Ok(mut slot) = slot_cb.lock() else {
                        return;
                    };
                    if slot.task_id != my_task_id {
                        return;
                    }
                    match ev {
                        xcpc_judge::BatchBuildEvent::ProblemStarted {
                            problem_id,
                            index,
                            total,
                        } => {
                            if let Some(item) = slot
                                .snapshot
                                .items
                                .iter_mut()
                                .find(|x| x.problem_id == problem_id)
                            {
                                item.status = "running".to_owned();
                                item.phase = None;
                                item.detail = None;
                            }
                            slot.snapshot.current_problem_id = Some(problem_id.clone());
                            slot.snapshot.current_phase = None;
                            slot.snapshot.current_detail = None;
                            let _ = tx_cb.send(BatchTaskEventJs {
                                kind: "problem_started".to_owned(),
                                task_id: my_task_id,
                                problem_id: Some(problem_id),
                                index: index as u32,
                                total: total as u32,
                                snapshot: slot.snapshot.clone(),
                                ..Default::default()
                            });
                        }
                        xcpc_judge::BatchBuildEvent::ProblemProgress {
                            problem_id,
                            index,
                            total,
                            event,
                        } => {
                            let (phase, detail) = progress_event_phase_detail(&event);
                            if let Some(item) = slot
                                .snapshot
                                .items
                                .iter_mut()
                                .find(|x| x.problem_id == problem_id)
                            {
                                item.phase = Some(phase.to_owned());
                                item.detail = detail.clone();
                            }
                            slot.snapshot.current_problem_id = Some(problem_id.clone());
                            slot.snapshot.current_phase = Some(phase.to_owned());
                            slot.snapshot.current_detail = detail.clone();
                            let _ = tx_cb.send(BatchTaskEventJs {
                                kind: "problem_progress".to_owned(),
                                task_id: my_task_id,
                                problem_id: Some(problem_id),
                                index: index as u32,
                                total: total as u32,
                                phase: Some(phase.to_owned()),
                                detail,
                                snapshot: slot.snapshot.clone(),
                                ..Default::default()
                            });
                        }
                        xcpc_judge::BatchBuildEvent::ProblemFinished { item, index, total } => {
                            let problem_id = item.problem_id.clone();
                            let item_js = batch_build_item_to_js(item);
                            match item_js.status.as_str() {
                                "passed" => slot.snapshot.succeeded += 1,
                                "skipped" => slot.snapshot.skipped += 1,
                                "cancelled" => slot.snapshot.cancelled_count += 1,
                                _ => slot.snapshot.failed += 1,
                            }
                            slot.snapshot.completed += 1;
                            if let Some(existing) = slot
                                .snapshot
                                .items
                                .iter_mut()
                                .find(|x| x.problem_id == problem_id)
                            {
                                *existing = item_js.clone();
                            } else {
                                slot.snapshot.items.push(item_js.clone());
                            }
                            let _ = tx_cb.send(BatchTaskEventJs {
                                kind: "problem_finished".to_owned(),
                                task_id: my_task_id,
                                problem_id: Some(problem_id),
                                index: index as u32,
                                total: total as u32,
                                item: Some(item_js),
                                snapshot: slot.snapshot.clone(),
                                ..Default::default()
                            });
                        }
                        xcpc_judge::BatchBuildEvent::Cancelled { completed, total } => {
                            slot.snapshot.cancelled = true;
                            let _ = tx_cb.send(BatchTaskEventJs {
                                kind: "cancelled".to_owned(),
                                task_id: my_task_id,
                                index: completed as u32,
                                total: total as u32,
                                snapshot: slot.snapshot.clone(),
                                ..Default::default()
                            });
                        }
                        xcpc_judge::BatchBuildEvent::AllFinished {
                            passed,
                            failed,
                            skipped,
                            cancelled,
                            total,
                            elapsed_ms,
                        } => {
                            slot.snapshot.succeeded = passed as u32;
                            slot.snapshot.failed = failed as u32;
                            slot.snapshot.skipped = skipped as u32;
                            slot.snapshot.cancelled_count = cancelled as u32;
                            slot.snapshot.total = total as u32;
                            slot.snapshot.completed = total as u32;
                            slot.snapshot.elapsed_ms = elapsed_ms as f64;
                            slot.snapshot.current_problem_id = None;
                            slot.snapshot.current_phase = None;
                            slot.snapshot.current_detail = None;
                        }
                    }
                };

                let result = if is_sanitize {
                    xcpc_judge::sanitize_packages_batch(
                        &ws,
                        &unique_ids,
                        &sanitize_opts,
                        Some(cancel_flag.clone()),
                        Some(&progress_cb),
                    )
                } else {
                    xcpc_judge::build_packages_batch(
                        &ws,
                        &unique_ids,
                        &opts,
                        Some(cancel_flag.clone()),
                        Some(&progress_cb),
                    )
                };

                // Refresh index outside `core.lock()` before emitting the terminal event so UI reads are fresh.
                if let Some(state) = weak_state.upgrade() {
                    let scanned = WorkspaceIndex::scan(&ws).ok();
                    if let Some(idx) = scanned
                        && let Ok(mut core) = state.core.lock()
                        && core
                            .workspace
                            .as_ref()
                            .is_some_and(|cur| cur.root() == ws.root())
                    {
                        core.index = idx;
                    }
                    state.invalidate_cache();
                } else {
                    revision_arc.fetch_add(1, Ordering::AcqRel);
                }

                if let Ok(mut slot) = slot_arc.lock()
                    && slot.task_id == my_task_id
                {
                    slot.running = false;
                    slot.snapshot.running = false;
                    slot.cancel_flag = None;
                    slot.close_tx = None;
                    slot.snapshot.current_problem_id = None;
                    slot.snapshot.current_phase = None;
                    slot.snapshot.current_detail = None;
                    match result {
                        Ok(report) => {
                            slot.snapshot.cancelled = report.cancelled;
                            slot.snapshot.total = report.total as u32;
                            slot.snapshot.completed = report.total as u32;
                            slot.snapshot.succeeded = report.passed as u32;
                            slot.snapshot.failed = report.failed as u32;
                            slot.snapshot.skipped = report.skipped as u32;
                            slot.snapshot.cancelled_count = report.cancelled_count as u32;
                            slot.snapshot.elapsed_ms = report.elapsed_ms as f64;
                            slot.snapshot.items = report
                                .items
                                .into_iter()
                                .map(batch_build_item_to_js)
                                .collect();
                            let _ = tx.send(BatchTaskEventJs {
                                kind: "all_finished".to_owned(),
                                task_id: my_task_id,
                                index: slot.snapshot.completed,
                                total: slot.snapshot.total,
                                snapshot: slot.snapshot.clone(),
                                ..Default::default()
                            });
                        }
                        Err(err) => {
                            let code = err.code().to_owned();
                            let message = err.human_message();
                            slot.snapshot.code = Some(code.clone());
                            slot.snapshot.error = Some(message.clone());
                            let _ = tx.send(BatchTaskEventJs {
                                kind: "error".to_owned(),
                                task_id: my_task_id,
                                total: slot.snapshot.total,
                                snapshot: slot.snapshot.clone(),
                                error: Some(message),
                                code: Some(code),
                                ..Default::default()
                            });
                        }
                    }
                }
            });

        if let Err(e) = spawned {
            let mut slot = self.batch_task.lock().unwrap();
            if slot.task_id == my_task_id {
                slot.running = false;
                slot.snapshot.running = false;
                slot.cancel_flag = None;
                slot.close_tx = None;
                slot.event_rx = None;
                slot.snapshot.error = Some(format!("spawn batch build thread failed: {e}"));
                slot.snapshot.code = Some("io".to_owned());
            }
            return BatchStartResultJs::err("io", format!("spawn batch build thread failed: {e}"));
        }

        BatchStartResultJs::ok(my_task_id)
    }

    pub fn start_batch_export(self: &Arc<Self>, req: BatchExportRequestJs) -> BatchStartResultJs {
        let unique_ids = dedup_ids_preserve_order(&req.problem_ids);
        if unique_ids.is_empty() {
            return BatchStartResultJs::err("invalid_argument", "请至少选择一道题目。");
        }

        enum ExportMode {
            Oj(xcpc_packages::OjFormat),
            MergedPdf,
        }

        let (export_mode, kind) = if req.format.eq_ignore_ascii_case("merged_pdf") {
            (ExportMode::MergedPdf, "export_merged_pdf")
        } else if let Some(fmt) = xcpc_packages::OjFormat::parse(&req.format) {
            let k = match fmt {
                xcpc_packages::OjFormat::Domjudge => "export_domjudge",
                xcpc_packages::OjFormat::Hydro => "export_hydro",
            };
            (ExportMode::Oj(fmt), k)
        } else {
            return BatchStartResultJs::err(
                "invalid_format",
                format!("不支持的批量导出格式：{}", req.format),
            );
        };

        let ws = self.core.lock().unwrap().workspace.clone();
        let Some(ws) = ws else {
            return BatchStartResultJs::err("no_workspace", "没有打开的工作区。");
        };
        if ws.is_read_only() {
            return BatchStartResultJs::err(
                "read_only",
                xcpc_storage::Error::ReadOnly.human_message(),
            );
        }

        let cancel_flag = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel::<BatchTaskEventJs>();
        let initial_items: Vec<BatchTaskItemJs> = unique_ids
            .iter()
            .map(|id| BatchTaskItemJs {
                problem_id: id.clone(),
                status: "pending".to_owned(),
                ..Default::default()
            })
            .collect();
        let total = initial_items.len() as u32;

        let my_task_id = {
            let mut slot = self.batch_task.lock().unwrap();
            if slot.running {
                return BatchStartResultJs::err(
                    "batch_task_in_progress",
                    "已有批量任务正在运行，请等待完成或先取消。",
                );
            }
            let next_id = slot.task_id.wrapping_add(1).max(1);
            let snapshot = BatchTaskSnapshotJs {
                task_id: next_id,
                running: true,
                kind: kind.to_owned(),
                total,
                items: initial_items,
                ..Default::default()
            };
            *slot = BatchTaskSlot {
                task_id: next_id,
                running: true,
                kind: kind.to_owned(),
                cancel_flag: Some(cancel_flag.clone()),
                close_tx: Some(tx.clone()),
                event_rx: Some(Arc::new(Mutex::new(rx))),
                snapshot,
            };
            next_id
        };

        let slot_arc = self.batch_task.clone();
        let language = req.language.as_deref().and_then(Language::parse);
        let book_kind = if req
            .book
            .as_deref()
            .is_some_and(|b| b.eq_ignore_ascii_case("editorial"))
        {
            Kind::Editorial
        } else {
            Kind::Statements
        };
        let title = req.title;
        let output_path_req = req.output_path.filter(|s| !s.trim().is_empty());

        let spawned = std::thread::Builder::new()
            .name("xcpc-batch-export".into())
            .spawn(move || {
                let started_at = Instant::now();
                match export_mode {
                    ExportMode::Oj(fmt) => {
                        let out_path = output_path_req.map(PathBuf::from).unwrap_or_default();
                        let opts = xcpc_packages::ExportOjOptions {
                            format: fmt,
                            output_path: out_path,
                            force: false,
                            language,
                        };
                        let slot_cb = slot_arc.clone();
                        let tx_cb = tx.clone();
                        let progress_cb = move |ev: xcpc_packages::BatchExportEvent| {
                            let Ok(mut slot) = slot_cb.lock() else {
                                return;
                            };
                            if slot.task_id != my_task_id {
                                return;
                            }
                            match ev {
                                xcpc_packages::BatchExportEvent::ProblemStarted {
                                    problem_id,
                                    index,
                                    total,
                                } => {
                                    if let Some(item) = slot
                                        .snapshot
                                        .items
                                        .iter_mut()
                                        .find(|x| x.problem_id == problem_id)
                                    {
                                        item.status = "running".to_owned();
                                    }
                                    slot.snapshot.current_problem_id = Some(problem_id.clone());
                                    slot.snapshot.current_phase = Some("exporting".to_owned());
                                    slot.snapshot.current_detail = None;
                                    let _ = tx_cb.send(BatchTaskEventJs {
                                        kind: "problem_started".to_owned(),
                                        task_id: my_task_id,
                                        problem_id: Some(problem_id),
                                        index: index as u32,
                                        total: total as u32,
                                        phase: Some("exporting".to_owned()),
                                        snapshot: slot.snapshot.clone(),
                                        ..Default::default()
                                    });
                                }
                                xcpc_packages::BatchExportEvent::ProblemFinished {
                                    item,
                                    index,
                                    total,
                                } => {
                                    let problem_id = item.problem_id.clone();
                                    let item_js = batch_export_item_to_js(item);
                                    match item_js.status.as_str() {
                                        "exported" => slot.snapshot.succeeded += 1,
                                        "skipped" => slot.snapshot.skipped += 1,
                                        "cancelled" => slot.snapshot.cancelled_count += 1,
                                        _ => slot.snapshot.failed += 1,
                                    }
                                    slot.snapshot.completed += 1;
                                    if let Some(existing) = slot
                                        .snapshot
                                        .items
                                        .iter_mut()
                                        .find(|x| x.problem_id == problem_id)
                                    {
                                        *existing = item_js.clone();
                                    } else {
                                        slot.snapshot.items.push(item_js.clone());
                                    }
                                    let _ = tx_cb.send(BatchTaskEventJs {
                                        kind: "problem_finished".to_owned(),
                                        task_id: my_task_id,
                                        problem_id: Some(problem_id),
                                        index: index as u32,
                                        total: total as u32,
                                        item: Some(item_js),
                                        snapshot: slot.snapshot.clone(),
                                        ..Default::default()
                                    });
                                }
                                xcpc_packages::BatchExportEvent::Cancelled { completed, total } => {
                                    slot.snapshot.cancelled = true;
                                    let _ = tx_cb.send(BatchTaskEventJs {
                                        kind: "cancelled".to_owned(),
                                        task_id: my_task_id,
                                        index: completed as u32,
                                        total: total as u32,
                                        snapshot: slot.snapshot.clone(),
                                        ..Default::default()
                                    });
                                }
                                xcpc_packages::BatchExportEvent::AllFinished {
                                    exported,
                                    skipped,
                                    failed,
                                    cancelled,
                                    total,
                                    output_dir,
                                } => {
                                    slot.snapshot.succeeded = exported as u32;
                                    slot.snapshot.skipped = skipped as u32;
                                    slot.snapshot.failed = failed as u32;
                                    slot.snapshot.cancelled_count = cancelled as u32;
                                    slot.snapshot.total = total as u32;
                                    slot.snapshot.completed = total as u32;
                                    slot.snapshot.output_path = Some(output_dir);
                                }
                            }
                        };

                        let result = xcpc_packages::export_oj_packages_batch(
                            &ws,
                            &unique_ids,
                            &opts,
                            Some(cancel_flag.clone()),
                            Some(&progress_cb),
                        );

                        if let Ok(mut slot) = slot_arc.lock()
                            && slot.task_id == my_task_id
                        {
                            slot.running = false;
                            slot.snapshot.running = false;
                            slot.cancel_flag = None;
                            slot.close_tx = None;
                            slot.snapshot.current_problem_id = None;
                            slot.snapshot.current_phase = None;
                            slot.snapshot.current_detail = None;
                            slot.snapshot.elapsed_ms = started_at.elapsed().as_secs_f64() * 1000.0;
                            match result {
                                Ok(res) => {
                                    slot.snapshot.cancelled = res.cancelled;
                                    slot.snapshot.total = res.total as u32;
                                    slot.snapshot.completed = res.total as u32;
                                    slot.snapshot.succeeded = res.exported as u32;
                                    slot.snapshot.skipped = res.skipped as u32;
                                    slot.snapshot.failed = res.failed as u32;
                                    slot.snapshot.cancelled_count = res.cancelled_count as u32;
                                    slot.snapshot.output_path = Some(res.output_dir);
                                    slot.snapshot.items = res
                                        .items
                                        .into_iter()
                                        .map(batch_export_item_to_js)
                                        .collect();
                                    let _ = tx.send(BatchTaskEventJs {
                                        kind: "all_finished".to_owned(),
                                        task_id: my_task_id,
                                        index: slot.snapshot.completed,
                                        total: slot.snapshot.total,
                                        snapshot: slot.snapshot.clone(),
                                        ..Default::default()
                                    });
                                }
                                Err(err) => {
                                    let code = err.code().to_owned();
                                    let message = err.human_message();
                                    slot.snapshot.code = Some(code.clone());
                                    slot.snapshot.error = Some(message.clone());
                                    let _ = tx.send(BatchTaskEventJs {
                                        kind: "error".to_owned(),
                                        task_id: my_task_id,
                                        total: slot.snapshot.total,
                                        snapshot: slot.snapshot.clone(),
                                        error: Some(message),
                                        code: Some(code),
                                        ..Default::default()
                                    });
                                }
                            }
                        }
                    }
                    ExportMode::MergedPdf => {
                        if cancel_flag.load(Ordering::Relaxed) {
                            if let Ok(mut slot) = slot_arc.lock()
                                && slot.task_id == my_task_id
                            {
                                slot.running = false;
                                slot.snapshot.running = false;
                                slot.snapshot.cancelled = true;
                                slot.snapshot.cancelled_count = total;
                                slot.snapshot.completed = total;
                                slot.cancel_flag = None;
                                slot.close_tx = None;
                                for item in &mut slot.snapshot.items {
                                    item.status = "cancelled".to_owned();
                                }
                                let _ = tx.send(BatchTaskEventJs {
                                    kind: "cancelled".to_owned(),
                                    task_id: my_task_id,
                                    index: 0,
                                    total,
                                    snapshot: slot.snapshot.clone(),
                                    ..Default::default()
                                });
                                let _ = tx.send(BatchTaskEventJs {
                                    kind: "all_finished".to_owned(),
                                    task_id: my_task_id,
                                    index: total,
                                    total,
                                    snapshot: slot.snapshot.clone(),
                                    ..Default::default()
                                });
                            }
                            return;
                        }

                        if let Ok(mut slot) = slot_arc.lock()
                            && slot.task_id == my_task_id
                        {
                            for item in &mut slot.snapshot.items {
                                item.status = "running".to_owned();
                            }
                            slot.snapshot.current_phase = Some("compiling".to_owned());
                            let first_id = unique_ids.first().cloned();
                            slot.snapshot.current_problem_id = first_id.clone();
                            let _ = tx.send(BatchTaskEventJs {
                                kind: "problem_started".to_owned(),
                                task_id: my_task_id,
                                problem_id: first_id,
                                index: 0,
                                total,
                                phase: Some("compiling".to_owned()),
                                snapshot: slot.snapshot.clone(),
                                ..Default::default()
                            });
                        }

                        let opts = MergedBookOptions {
                            title,
                            language,
                            kind: book_kind,
                            template: None,
                            cover: None,
                        };
                        let mut engine =
                            Engine::new(&FontOptions::from_env(), Some(ws.root().to_path_buf()));

                        let outcome: Result<(String, usize), (String, String)> =
                            match output_path_req {
                                Some(ref p) if Path::new(p).is_absolute() => {
                                    match publish::compile_merged_pdf_bytes(
                                        &ws,
                                        &mut engine,
                                        &unique_ids,
                                        &opts,
                                    ) {
                                        Ok((Some(bytes), report, _)) => {
                                            if cancel_flag.load(Ordering::Relaxed) {
                                                Err((
                                                    "cancelled".to_owned(),
                                                    "任务已取消".to_owned(),
                                                ))
                                            } else {
                                                let dest = Path::new(p);
                                                if let Some(parent) = dest.parent() {
                                                    let _ = std::fs::create_dir_all(parent);
                                                }
                                                match xcpc_storage::write_atomic(dest, &bytes) {
                                                    Ok(()) => Ok((
                                                        dest.display().to_string(),
                                                        report.pages.unwrap_or(0),
                                                    )),
                                                    Err(e) => Err((
                                                        "io".to_owned(),
                                                        format!("写入 PDF 失败：{e}"),
                                                    )),
                                                }
                                            }
                                        }
                                        Ok((None, report, _)) => Err((
                                            "pdf_compile_failed".to_owned(),
                                            first_error(&report)
                                                .unwrap_or_else(|| "合并 PDF 编译失败".to_owned()),
                                        )),
                                        Err(e) => Err((e.code().to_owned(), e.human_message())),
                                    }
                                }
                                other => {
                                    let rel_opt = other.as_deref().map(Path::new);
                                    match publish::export_merged_pdf(
                                        &ws,
                                        &mut engine,
                                        &unique_ids,
                                        &opts,
                                        rel_opt,
                                    ) {
                                        Ok(res) if !res.path.is_empty() => Ok((
                                            ws.root().join(&res.path).display().to_string(),
                                            res.pages,
                                        )),
                                        Ok(res) => Err((
                                            "pdf_compile_failed".to_owned(),
                                            first_error(&res.report)
                                                .unwrap_or_else(|| "合并 PDF 编译失败".to_owned()),
                                        )),
                                        Err(e) => Err((e.code().to_owned(), e.human_message())),
                                    }
                                }
                            };

                        let elapsed_ms = started_at.elapsed().as_secs_f64() * 1000.0;
                        if let Ok(mut slot) = slot_arc.lock()
                            && slot.task_id == my_task_id
                        {
                            slot.running = false;
                            slot.snapshot.running = false;
                            slot.cancel_flag = None;
                            slot.close_tx = None;
                            slot.snapshot.current_problem_id = None;
                            slot.snapshot.current_phase = None;
                            slot.snapshot.current_detail = None;
                            slot.snapshot.elapsed_ms = elapsed_ms;
                            match outcome {
                                Ok((abs_path, _pages)) => {
                                    slot.snapshot.output_path = Some(abs_path.clone());
                                    slot.snapshot.succeeded = total;
                                    slot.snapshot.completed = total;
                                    for item in &mut slot.snapshot.items {
                                        item.status = "exported".to_owned();
                                        item.output_path = Some(abs_path.clone());
                                    }
                                    let _ = tx.send(BatchTaskEventJs {
                                        kind: "all_finished".to_owned(),
                                        task_id: my_task_id,
                                        index: total,
                                        total,
                                        snapshot: slot.snapshot.clone(),
                                        ..Default::default()
                                    });
                                }
                                Err((code, _message)) if code == "cancelled" => {
                                    slot.snapshot.cancelled = true;
                                    slot.snapshot.cancelled_count = total;
                                    slot.snapshot.completed = total;
                                    for item in &mut slot.snapshot.items {
                                        item.status = "cancelled".to_owned();
                                    }
                                    let _ = tx.send(BatchTaskEventJs {
                                        kind: "cancelled".to_owned(),
                                        task_id: my_task_id,
                                        index: 0,
                                        total,
                                        snapshot: slot.snapshot.clone(),
                                        ..Default::default()
                                    });
                                    let _ = tx.send(BatchTaskEventJs {
                                        kind: "all_finished".to_owned(),
                                        task_id: my_task_id,
                                        index: total,
                                        total,
                                        snapshot: slot.snapshot.clone(),
                                        ..Default::default()
                                    });
                                }
                                Err((code, message)) => {
                                    slot.snapshot.failed = total;
                                    slot.snapshot.completed = total;
                                    slot.snapshot.code = Some(code.clone());
                                    slot.snapshot.error = Some(message.clone());
                                    for item in &mut slot.snapshot.items {
                                        item.status = "failed".to_owned();
                                        item.code = Some(code.clone());
                                        item.message = Some(message.clone());
                                    }
                                    let _ = tx.send(BatchTaskEventJs {
                                        kind: "error".to_owned(),
                                        task_id: my_task_id,
                                        total,
                                        snapshot: slot.snapshot.clone(),
                                        error: Some(message),
                                        code: Some(code),
                                        ..Default::default()
                                    });
                                }
                            }
                        }
                    }
                }
            });

        if let Err(e) = spawned {
            let mut slot = self.batch_task.lock().unwrap();
            if slot.task_id == my_task_id {
                slot.running = false;
                slot.snapshot.running = false;
                slot.cancel_flag = None;
                slot.close_tx = None;
                slot.event_rx = None;
                slot.snapshot.error = Some(format!("spawn batch export thread failed: {e}"));
                slot.snapshot.code = Some("io".to_owned());
            }
            return BatchStartResultJs::err("io", format!("spawn batch export thread failed: {e}"));
        }

        BatchStartResultJs::ok(my_task_id)
    }

    pub fn cancel_batch_task(&self) -> ActionResult {
        let mut slot = self.batch_task.lock().unwrap();
        if let Some(ref flag) = slot.cancel_flag {
            flag.store(true, Ordering::Relaxed);
            slot.snapshot.cancelled = true;
        }
        ActionResult::ok()
    }

    pub fn batch_task_status(&self) -> BatchTaskSnapshotJs {
        self.batch_task.lock().unwrap().snapshot.clone()
    }

    pub fn batch_event_receiver(
        &self,
        task_id: u32,
    ) -> (
        Option<Arc<Mutex<mpsc::Receiver<BatchTaskEventJs>>>>,
        BatchTaskSnapshotJs,
    ) {
        let slot = self.batch_task.lock().unwrap();
        if slot.task_id == task_id {
            (slot.event_rx.clone(), slot.snapshot.clone())
        } else {
            (None, slot.snapshot.clone())
        }
    }
}

fn dedup_ids_preserve_order(ids: &[String]) -> Vec<String> {
    let mut seen = HashSet::with_capacity(ids.len());
    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
        if seen.insert(id.as_str()) {
            out.push(id.clone());
        }
    }
    out
}

fn progress_event_phase_detail(ev: &xcpc_judge::ProgressEvent) -> (&'static str, Option<String>) {
    match ev {
        xcpc_judge::ProgressEvent::Compiling { target, .. } => ("compiling", Some(target.clone())),
        xcpc_judge::ProgressEvent::SelfTesting { .. } => ("self_testing", None),
        xcpc_judge::ProgressEvent::Generating { test_name, .. } => {
            ("generating", Some(test_name.clone()))
        }
        xcpc_judge::ProgressEvent::Validating { test_name, .. } => {
            ("validating", Some(test_name.clone()))
        }
        xcpc_judge::ProgressEvent::Running {
            solution,
            test_name,
            verdict,
            cpu_time_ms,
            ..
        } => (
            "running",
            Some(format!(
                "{solution} {test_name} ({verdict} {cpu_time_ms}ms)"
            )),
        ),
        xcpc_judge::ProgressEvent::Finished { .. } => ("finished", None),
    }
}

fn batch_build_item_to_js(item: xcpc_judge::BatchBuildItemResult) -> BatchTaskItemJs {
    let status = match item.status {
        xcpc_judge::BatchBuildItemStatus::Passed => "passed",
        xcpc_judge::BatchBuildItemStatus::Failed => "failed",
        xcpc_judge::BatchBuildItemStatus::Skipped => "skipped",
        xcpc_judge::BatchBuildItemStatus::Error => "error",
        xcpc_judge::BatchBuildItemStatus::Cancelled => "cancelled",
    };
    BatchTaskItemJs {
        problem_id: item.problem_id,
        status: status.to_owned(),
        verified: item.verified,
        elapsed_ms: item.elapsed_ms as f64,
        phase: None,
        detail: None,
        code: item.code,
        message: item.message,
        output_path: None,
    }
}

fn batch_export_item_to_js(item: xcpc_packages::BatchExportItemResult) -> BatchTaskItemJs {
    let status = match item.status {
        xcpc_packages::BatchExportItemStatus::Exported => "exported",
        xcpc_packages::BatchExportItemStatus::Skipped => "skipped",
        xcpc_packages::BatchExportItemStatus::Failed => "failed",
        xcpc_packages::BatchExportItemStatus::Cancelled => "cancelled",
    };
    BatchTaskItemJs {
        problem_id: item.problem_id,
        status: status.to_owned(),
        verified: item.verified,
        elapsed_ms: 0.0,
        phase: None,
        detail: None,
        code: item.code,
        message: item.message,
        output_path: item.zip_path,
    }
}

/// Fast package status lookup for list views (`search_problems`, `contest_detail`).
///
/// Avoids recursive `tests/` SHA-256 hashing on every search keystroke or contest list open:
/// relies on `has_package` from `core.index`, reads only `verified.json` / `latest.json`
/// report JSONs, and compares `verified.built_at_epoch_secs` against O(1) mtimes of
/// `problem.toml` and `tests/` to surface stale packages.
fn package_status_fast(ws: &Workspace, slug: &str, has_package: bool) -> Option<String> {
    if !has_package {
        return None;
    }
    let report_failed = |r: &xcpc_judge::BuildReport| {
        !r.ok
            || !r.compile_failures.is_empty()
            || r.gates
                .iter()
                .any(|g| g.status == xcpc_judge::GateStatus::Fail)
    };
    let latest = xcpc_judge::load_latest_report(ws, slug);
    if let Some(verified) = xcpc_judge::load_verified_report(ws, slug)
        && verified.verified
    {
        if let Some(ref lat) = latest
            && lat.built_at_epoch_secs >= verified.built_at_epoch_secs
            && report_failed(lat)
        {
            return Some("failed".to_owned());
        }
        if let Ok(prob_dir) = ws.resolve(&format!("problems/{slug}")) {
            let is_newer = |rel: &str| {
                std::fs::metadata(prob_dir.join(rel))
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .is_some_and(|d| d.as_secs() > verified.built_at_epoch_secs)
            };
            if is_newer("problem.toml") || is_newer("tests") {
                return Some("stale".to_owned());
            }
        }
        return Some("verified_fresh".to_owned());
    }
    if let Some(ref lat) = latest
        && report_failed(lat)
    {
        return Some("failed".to_owned());
    }
    Some("unverified".to_owned())
}

fn freshness_status_str(f: &xcpc_judge::PackageFreshness) -> &'static str {
    match f {
        xcpc_judge::PackageFreshness::NotAPackage => "not_a_package",
        xcpc_judge::PackageFreshness::VerifiedFresh { .. } => "verified_fresh",
        xcpc_judge::PackageFreshness::Stale { .. } => "stale",
        xcpc_judge::PackageFreshness::Unverified { .. } => "unverified",
        xcpc_judge::PackageFreshness::Failed { .. } => "failed",
    }
}

/// The preview HUD only reports the statement's own problems (authoring-ui-refactor §6.6):
/// whether the test data has been checked is the status slot's job, not a preflight error.
/// Freezing still checks it through `xcpc-lint`.
fn preview_report(
    report: &publish::Report,
) -> (Option<String>, Vec<PreviewDiagnostic>, Vec<PreviewStart>) {
    let (_, diagnostics, starts) = extract_report(report);
    let diagnostics: Vec<PreviewDiagnostic> = diagnostics
        .into_iter()
        .filter(|d| !d.code.as_deref().is_some_and(|c| c.starts_with("package_")))
        .collect();
    let error = report
        .diagnostics
        .iter()
        .filter(|d| !d.code.as_deref().is_some_and(|c| c.starts_with("package_")))
        .find(|d| d.is_error())
        .map(|d| match (&d.file, d.line) {
            (Some(f), Some(l)) => format!("{f}:{l}: {}", d.message),
            _ => d.message.clone(),
        });
    (error, diagnostics, starts)
}

fn book_freshness(
    ws: Option<&Workspace>,
    export_rec: Option<&ExportRecord>,
    snap: Option<&CachedSnapshot>,
    target: &Target,
    kind: Kind,
    request: &PreviewRequest,
) -> PdfFreshnessJs {
    let Some(ws) = ws else {
        return PdfFreshnessJs {
            rel_path: publish::default_output_for_title(target, kind, None),
            abs_path: String::new(),
            exists: false,
            fresh: false,
        };
    };
    let rel_path = publish::default_output(ws, target, kind);
    let abs_buf = ws.root().join(&rel_path);
    let abs_path = abs_buf.display().to_string();
    let Ok(pdf_meta) = std::fs::metadata(&abs_buf) else {
        return PdfFreshnessJs {
            rel_path,
            abs_path,
            exists: false,
            fresh: false,
        };
    };
    if !pdf_meta.is_file() {
        return PdfFreshnessJs {
            rel_path,
            abs_path,
            exists: false,
            fresh: false,
        };
    }

    // Unsaved editor edits belonging to this book immediately mark the on-disk PDF stale.
    if unsaved_affects_book(ws, target, kind, request.unsaved()) {
        return PdfFreshnessJs {
            rel_path,
            abs_path,
            exists: true,
            fresh: false,
        };
    }

    let disk_len = pdf_meta.len();
    let disk_mtime = pdf_meta.modified().ok();

    // In-session export cache: exact content fingerprint match when the on-disk PDF is
    // still the file we wrote in this session.
    if let Some(rec) = export_rec
        && rec.pdf_len == disk_len
        && rec.pdf_mtime == disk_mtime
    {
        let cached_source = snap
            .filter(|s| {
                s.request.target() == *target
                    && s.request.kind() == kind
                    && (s.matches_disk.load(Ordering::Acquire)
                        || !unsaved_affects_book(ws, target, kind, s.request.unsaved()))
            })
            .map(|s| s.doc_source.as_str());
        let fresh = compute_book_fingerprint(ws, target, kind, cached_source)
            .is_some_and(|fp| fp == rec.fingerprint);
        return PdfFreshnessJs {
            rel_path,
            abs_path,
            exists: true,
            fresh,
        };
    }

    // Restart recovery (or PDF produced externally): compare PDF mtime against the
    // newest source file mtime for this book.
    let fresh = match (disk_mtime, latest_book_source_mtime(ws, target, kind)) {
        (Some(pdf_t), Some(src_t)) => pdf_t >= src_t,
        _ => false,
    };
    PdfFreshnessJs {
        rel_path,
        abs_path,
        exists: true,
        fresh,
    }
}

// ---- Forward source → preview anchor sampling & interpolation --------------------------

fn is_overlayable_markdown_path(norm: &str) -> bool {
    let Some(name) = norm.rsplit('/').next() else {
        return false;
    };
    name == "statement.md"
        || name == "editorial.md"
        || (name.starts_with("statement.") && name.ends_with(".md"))
}

fn file_matches_book_kind(norm: &str, kind: Kind) -> bool {
    let Some(name) = norm.rsplit('/').next() else {
        return false;
    };
    match kind {
        Kind::Statements => {
            name == "statement.md" || (name.starts_with("statement.") && name.ends_with(".md"))
        }
        Kind::Editorial => name == "editorial.md",
    }
}

fn problem_pages_for_file<'a>(
    problems: &'a [publish::Pages],
    norm_file: &str,
) -> Option<&'a publish::Pages> {
    if let Some(rest) = norm_file.strip_prefix("problems/") {
        let id = rest.split('/').next()?;
        return problems.iter().find(|p| p.id == id);
    }
    if let Some((_, rest)) = norm_file.split_once("/snapshot/") {
        let label = rest.split('/').next()?;
        return problems.iter().find(|p| p.label == label);
    }
    None
}

/// Samples rendered points across the pages belonging to `norm_file` via
/// `publish::locate_point` and returns deduplicated `(line, page, y_pt)` anchors sorted
/// by ascending 1-based line number.
fn sample_file_anchors(
    engine: &Engine,
    compiled: &Compiled,
    doc: &xcpc_convert::Document,
    problems: &[publish::Pages],
    norm_file: &str,
) -> Vec<SourceAnchor> {
    const SAMPLE_STEP_PT: f64 = 20.0;
    const TOP_MARGIN_PT: f64 = 48.0;
    const BOTTOM_MARGIN_PT: f64 = 40.0;

    let page_count = compiled.page_count();
    if page_count == 0 {
        return Vec::new();
    }
    let page_range = match problem_pages_for_file(problems, norm_file) {
        Some(p) => {
            let start = p.first.saturating_sub(1).min(page_count);
            let end = start.saturating_add(p.count.max(1)).min(page_count);
            start..end
        }
        None => 0..page_count.min(32),
    };

    let mut by_line: BTreeMap<u32, (u32, f64)> = BTreeMap::new();
    for page_idx in page_range {
        let Some((width, height)) = compiled.page_size(page_idx) else {
            continue;
        };
        let xs = [width * 0.25, width * 0.5, width * 0.15];
        let y_end = (height - BOTTOM_MARGIN_PT).max(TOP_MARGIN_PT);
        let mut y = TOP_MARGIN_PT;
        while y <= y_end {
            for &x in &xs {
                if let Some(loc) = publish::locate_point(engine, compiled, doc, page_idx, x, y)
                    && normalize_ws_rel(&loc.file) == norm_file
                {
                    let line = (loc.line as u32).max(1);
                    by_line.entry(line).or_insert((page_idx as u32, y));
                    break;
                }
            }
            y += SAMPLE_STEP_PT;
        }
    }

    by_line
        .into_iter()
        .map(|(line, (page, y_pt))| SourceAnchor { line, page, y_pt })
        .collect()
}

fn interpolate_anchor(anchors: &[SourceAnchor], line: u32) -> Option<PreviewLocationJs> {
    if anchors.is_empty() {
        return None;
    }
    match anchors.binary_search_by_key(&line, |a| a.line) {
        Ok(i) => Some(PreviewLocationJs {
            page: anchors[i].page,
            y_pt: anchors[i].y_pt,
            exact: true,
        }),
        Err(0) => Some(PreviewLocationJs {
            page: anchors[0].page,
            y_pt: anchors[0].y_pt,
            exact: true,
        }),
        Err(i) if i >= anchors.len() => {
            let last = anchors[anchors.len() - 1];
            Some(PreviewLocationJs {
                page: last.page,
                y_pt: last.y_pt,
                exact: true,
            })
        }
        Err(i) => {
            let prev = anchors[i - 1];
            let next = anchors[i];
            if prev.page == next.page && next.line > prev.line {
                let t = f64::from(line - prev.line) / f64::from(next.line - prev.line);
                Some(PreviewLocationJs {
                    page: prev.page,
                    y_pt: prev.y_pt + (next.y_pt - prev.y_pt) * t,
                    exact: true,
                })
            } else {
                let chosen = if line - prev.line <= next.line - line {
                    prev
                } else {
                    next
                };
                Some(PreviewLocationJs {
                    page: chosen.page,
                    y_pt: chosen.y_pt,
                    exact: true,
                })
            }
        }
    }
}

impl State {
    // ---- Phase 3D: Sanitizer, external trial run, adopt & contest readiness audit ------

    pub fn load_sanitizer_report(&self, slug: &str) -> JsonReportResultJs {
        let ws = self.core.lock().unwrap().workspace.clone();
        let Some(ws) = ws else {
            return JsonReportResultJs::err("no_workspace", "no workspace open");
        };
        if let Err(e) = check_id(slug) {
            return JsonReportResultJs::err("invalid_id", e.to_string());
        }
        match xcpc_judge::load_sanitizer_report(&ws, slug) {
            Some(report) => match serde_json::to_string(&report) {
                Ok(json) => JsonReportResultJs::ok(Some(json), None),
                Err(e) => {
                    JsonReportResultJs::err("io", format!("serialize sanitizer report failed: {e}"))
                }
            },
            None => JsonReportResultJs::ok(None, None),
        }
    }

    pub fn sanitize_problem(
        &self,
        slug: &str,
        tests: Option<Vec<String>>,
        solutions: Option<Vec<String>>,
    ) -> JsonReportResultJs {
        let (ws, cxx_override) = {
            let core = self.core.lock().unwrap();
            (core.workspace.clone(), core.prefs.app.cxx_override.clone())
        };
        let Some(ws) = ws else {
            return JsonReportResultJs::err("no_workspace", "no workspace open");
        };
        if ws.is_read_only() {
            return JsonReportResultJs::err(
                "read_only",
                xcpc_storage::Error::ReadOnly.human_message(),
            );
        }
        if let Err(e) = check_id(slug) {
            return JsonReportResultJs::err("invalid_id", e.to_string());
        }
        let opts = xcpc_judge::SanitizeOptions {
            tests: tests.unwrap_or_default(),
            solutions: solutions.unwrap_or_default(),
            cxx_override: cxx_override.map(PathBuf::from),
            ..Default::default()
        };
        match xcpc_judge::sanitize_package(&ws, slug, &opts, None, None) {
            Ok(report) => {
                self.revision.fetch_add(1, Ordering::AcqRel);
                match serde_json::to_string(&report) {
                    Ok(json) => JsonReportResultJs::ok(Some(json), None),
                    Err(e) => JsonReportResultJs::err(
                        "io",
                        format!("serialize sanitizer report failed: {e}"),
                    ),
                }
            }
            Err(e) => JsonReportResultJs::err(e.code(), e.human_message()),
        }
    }

    pub fn run_external_problem_solutions(
        &self,
        slug: &str,
        files: &[PathBuf],
        auto_build: bool,
    ) -> JsonReportResultJs {
        let (ws, cxx_override) = {
            let core = self.core.lock().unwrap();
            (core.workspace.clone(), core.prefs.app.cxx_override.clone())
        };
        let Some(ws) = ws else {
            return JsonReportResultJs::err("no_workspace", "no workspace open");
        };
        if let Err(e) = check_id(slug) {
            return JsonReportResultJs::err("invalid_id", e.to_string());
        }
        let opts = xcpc_judge::ExternalRunOptions {
            auto_build: auto_build && !ws.is_read_only(),
            cxx_override: cxx_override.map(PathBuf::from),
            ..Default::default()
        };
        match xcpc_judge::run_external_solutions_for_problem(&ws, slug, files, &opts, None) {
            Ok(report) => {
                let md = xcpc_judge::render_external_problem_markdown(&report);
                match serde_json::to_string(&report) {
                    Ok(json) => JsonReportResultJs::ok(Some(json), Some(md)),
                    Err(e) => JsonReportResultJs::err(
                        "io",
                        format!("serialize external report failed: {e}"),
                    ),
                }
            }
            Err(e) => JsonReportResultJs::err(e.code(), e.human_message()),
        }
    }

    pub fn run_external_contest_solutions(
        &self,
        contest_id: &str,
        dir: &Path,
        auto_build: bool,
    ) -> JsonReportResultJs {
        let (ws, cxx_override) = {
            let core = self.core.lock().unwrap();
            (core.workspace.clone(), core.prefs.app.cxx_override.clone())
        };
        let Some(ws) = ws else {
            return JsonReportResultJs::err("no_workspace", "no workspace open");
        };
        if let Err(e) = check_id(contest_id) {
            return JsonReportResultJs::err("invalid_id", e.to_string());
        }
        let opts = xcpc_judge::ExternalRunOptions {
            auto_build: auto_build && !ws.is_read_only(),
            cxx_override: cxx_override.map(PathBuf::from),
            ..Default::default()
        };
        match xcpc_judge::run_external_solutions_for_contest(&ws, contest_id, dir, &opts, None) {
            Ok(report) => {
                let md = xcpc_judge::render_external_contest_markdown(&report);
                match serde_json::to_string(&report) {
                    Ok(json) => JsonReportResultJs::ok(Some(json), Some(md)),
                    Err(e) => JsonReportResultJs::err(
                        "io",
                        format!("serialize external contest report failed: {e}"),
                    ),
                }
            }
            Err(e) => JsonReportResultJs::err(e.code(), e.human_message()),
        }
    }

    pub fn adopt_external_solution(
        &self,
        slug: &str,
        source_file: &Path,
        name: Option<String>,
        expect: Option<Vec<String>>,
        tags: Option<Vec<String>>,
        is_main: Option<bool>,
    ) -> AdoptSolutionResultJs {
        let ws = self.core.lock().unwrap().workspace.clone();
        let Some(ws) = ws else {
            return AdoptSolutionResultJs::err("no_workspace", "no workspace open");
        };
        if ws.is_read_only() {
            return AdoptSolutionResultJs::err(
                "read_only",
                xcpc_storage::Error::ReadOnly.human_message(),
            );
        }
        if let Err(e) = check_id(slug) {
            return AdoptSolutionResultJs::err("invalid_id", e.to_string());
        }
        let opts = xcpc_judge::AdoptSolutionOptions {
            name,
            expect: expect.unwrap_or_default(),
            tags: tags.unwrap_or_default(),
            is_main: is_main.unwrap_or(false),
        };
        match xcpc_judge::adopt_external_solution(&ws, slug, source_file, &opts) {
            Ok(res) => {
                {
                    let mut core = self.core.lock().unwrap();
                    let _ = core.index.refresh_problem(&ws, slug);
                }
                self.invalidate_cache();
                AdoptSolutionResultJs {
                    ok: true,
                    problem_id: res.problem_id,
                    solution_name: res.solution_name,
                    target_rel_path: res.dest_rel,
                    updated_problem_toml: res.updated_problem_toml,
                    error: None,
                    code: None,
                }
            }
            Err(e) => AdoptSolutionResultJs::err(e.code(), e.human_message()),
        }
    }

    pub fn audit_contest(&self, contest_id: &str) -> JsonReportResultJs {
        let ws = self.core.lock().unwrap().workspace.clone();
        let Some(ws) = ws else {
            return JsonReportResultJs::err("no_workspace", "no workspace open");
        };
        if let Err(e) = check_id(contest_id) {
            return JsonReportResultJs::err("invalid_id", e.to_string());
        }
        let mut opts = xcpc_judge::AuditOptions::default();
        if let Ok(contest) = ws.load_contest(contest_id) {
            let lint_opts = xcpc_lint::LintOptions::default();
            for item in &contest.meta.problems {
                if let Ok(findings) = xcpc_lint::lint_problem(&ws, &item.id, &lint_opts) {
                    let mut summary = xcpc_judge::ProblemLintSummary::default();
                    for f in findings {
                        if f.code.starts_with("package_") {
                            continue;
                        }
                        if f.is_error() {
                            summary.error_count += 1;
                        } else {
                            summary.warning_count += 1;
                        }
                        summary.findings.push(xcpc_judge::AuditLintFinding {
                            severity: f.severity.as_str().to_owned(),
                            code: f.code,
                            message: f.message,
                            file: f.file,
                        });
                    }
                    opts.lint_by_problem.insert(item.id.clone(), summary);
                }
            }
        }
        match xcpc_judge::audit_contest(&ws, contest_id, &opts) {
            Ok(report) => {
                let md = xcpc_judge::render_contest_audit_markdown(&report);
                match serde_json::to_string(&report) {
                    Ok(json) => JsonReportResultJs::ok(Some(json), Some(md)),
                    Err(e) => JsonReportResultJs::err(
                        "io",
                        format!("serialize contest audit report failed: {e}"),
                    ),
                }
            }
            Err(e) => JsonReportResultJs::err(e.code(), e.human_message()),
        }
    }
}

/// Lightweight `(filename, len, mtime)` fingerprint of `templates/*.typ` for caching
/// evaluated template schemas without re-running Typst compilation on unrelated edits.
pub(crate) fn template_files_fingerprint(root: &Path) -> Vec<(String, u64, Option<SystemTime>)> {
    let dir = root.join("templates");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        if !name.ends_with(".typ") || name == "preamble.typ" {
            continue;
        }
        if let Ok(meta) = entry.metadata()
            && meta.is_file()
        {
            out.push((name, meta.len(), meta.modified().ok()));
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// The statement file set in `language` (`statement.md` for a problem that has none yet).
fn statement_path(p: &Problem, language: Language) -> String {
    p.statement(language)
        .map(|s| s.file.path.clone())
        .unwrap_or_else(|| format!("{}/statement.md", p.dir))
}

fn hit_kind_rank(kind: &str) -> u8 {
    match kind {
        "contest_item" => 0,
        "problem" => 1,
        "contest" => 2,
        _ => 3,
    }
}

fn normalize_query(input: &str) -> String {
    input
        .split_whitespace()
        .map(str::to_lowercase)
        .collect::<Vec<_>>()
        .join(" ")
}

fn score_contest_item(
    terms: &[&str],
    full_q: &str,
    label: &str,
    id: &str,
    title: &str,
) -> Option<f64> {
    if terms.is_empty() {
        // Surface active contest items near the top when opening Cmd+K inside a contest.
        return Some(10.0);
    }
    let label_l = label.to_lowercase();
    let label_dot = format!("{label_l}.");
    if full_q == label_l || full_q == label_dot {
        // Exact label match (`A`, `B`, `A.`) ranks ahead of exact problem IDs (`120.0`).
        return Some(150.0);
    }
    let id_l = id.to_lowercase();
    let title_l = title.to_lowercase();
    let mut total = 0.0;
    for &term in terms {
        let stripped = term.trim_end_matches('.');
        let s = if stripped == label_l && !stripped.is_empty() {
            Some(140.0)
        } else if label_l.starts_with(term) {
            Some(110.0)
        } else if id_l.contains(term) || title_l.contains(term) {
            Some(80.0)
        } else if is_subsequence(term, &id_l) || is_subsequence(term, &title_l) {
            Some(35.0)
        } else {
            None
        }?;
        total += s;
    }
    Some(total / terms.len() as f64)
}

fn is_subsequence(needle: &str, haystack: &str) -> bool {
    let mut hay = haystack.chars();
    for nc in needle.chars() {
        if !hay.any(|hc| hc == nc) {
            return false;
        }
    }
    true
}

// ---- PDF freshness helpers (fingerprint + unsaved check + restart mtime recovery) ------

/// Returns `(contest_dir_opt, problem_dirs)` for `target`:
/// - Single problem: `(None, ["problems/<id>"])`
/// - Unfrozen contest: `(Some("contests/<id>"), ["problems/<item.id>", ...])`
/// - Frozen contest: `(Some("contests/<id>"), ["contests/<id>/snapshot/<label>", ...])`
fn target_source_dirs(ws: &Workspace, target: &Target) -> Option<(Option<String>, Vec<String>)> {
    match target {
        Target::Problem(id) => {
            check_id(id).ok()?;
            Some((None, vec![format!("problems/{id}")]))
        }
        Target::Contest(id) => {
            let contest = ws.load_contest(id).ok()?;
            let dirs = contest
                .meta
                .problems
                .iter()
                .map(|item| {
                    if contest.meta.frozen {
                        format!("{}/snapshot/{}", contest.dir, item.label)
                    } else {
                        format!("problems/{}", item.id)
                    }
                })
                .collect();
            Some((Some(contest.dir), dirs))
        }
    }
}

fn normalize_ws_rel(rel: &str) -> String {
    let replaced = rel.replace('\\', "/");
    replaced
        .trim_start_matches("./")
        .trim_start_matches('/')
        .to_owned()
}

fn path_belongs_to_book(ws: &Workspace, target: &Target, kind: Kind, rel: &str) -> bool {
    let Some((contest_dir, problem_dirs)) = target_source_dirs(ws, target) else {
        return false;
    };
    let norm = normalize_ws_rel(rel);
    if let Some(cdir) = contest_dir
        && (norm == format!("{cdir}/contest.toml") || norm.starts_with(&format!("{cdir}/assets/")))
    {
        return true;
    }
    for pdir in problem_dirs {
        let Some(sub) = norm.strip_prefix(&format!("{pdir}/")) else {
            continue;
        };
        if sub == "problem.toml" || sub.starts_with("assets/") {
            return true;
        }
        match kind {
            Kind::Statements => {
                if sub == "statement.md"
                    || (sub.starts_with("statement.") && sub.ends_with(".md"))
                    || sub.starts_with("samples/")
                {
                    return true;
                }
            }
            Kind::Editorial => {
                if sub == "editorial.md" {
                    return true;
                }
            }
        }
    }
    false
}

fn unsaved_affects_book(
    ws: &Workspace,
    target: &Target,
    kind: Kind,
    unsaved: Option<(&str, &str)>,
) -> bool {
    let Some((path, text)) = unsaved else {
        return false;
    };
    if !path_belongs_to_book(ws, target, kind, path) {
        return false;
    }
    match ws.read_text(path) {
        Ok(disk) => disk != text,
        Err(xcpc_storage::Error::NotFound(_)) => !text.is_empty(),
        Err(_) => true,
    }
}

/// Computes a deterministic SHA-256 content fingerprint for `(target, kind)` combining
/// the generated Typst document source (from disk) and every file under `assets/**`.
fn compute_book_fingerprint(
    ws: &Workspace,
    target: &Target,
    kind: Kind,
    known_doc_source: Option<&str>,
) -> Option<String> {
    let owned_source;
    let doc_source = match known_doc_source {
        Some(s) => s,
        None => {
            let doc = publish::document_with(ws, target, kind, None, None).ok()?;
            owned_source = doc.source;
            &owned_source
        }
    };

    let mut summary = String::new();
    summary.push_str("doc:");
    summary.push_str(&content_hash(doc_source.as_bytes()));
    summary.push('\n');

    let mut asset_hashes = BTreeMap::<String, String>::new();
    if let Some((contest_dir, problem_dirs)) = target_source_dirs(ws, target) {
        if let Some(cdir) = contest_dir {
            collect_tree_hashes(ws, &format!("{cdir}/assets"), &mut asset_hashes);
        }
        for pdir in problem_dirs {
            collect_tree_hashes(ws, &format!("{pdir}/assets"), &mut asset_hashes);
        }
    }
    for (rel, hash) in asset_hashes {
        summary.push_str(&rel);
        summary.push(':');
        summary.push_str(&hash);
        summary.push('\n');
    }

    Some(content_hash(summary.as_bytes()))
}

fn collect_tree_hashes(ws: &Workspace, rel_dir: &str, out: &mut BTreeMap<String, String>) {
    let Ok(abs_dir) = ws.resolve(rel_dir) else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(&abs_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        if name.starts_with('.') {
            continue;
        }
        let child_rel = format!("{rel_dir}/{name}");
        let Ok(child_abs) = ws.resolve(&child_rel) else {
            continue;
        };
        if child_abs.is_dir() {
            collect_tree_hashes(ws, &child_rel, out);
        } else if child_abs.is_file()
            && let Ok(bytes) = std::fs::read(&child_abs)
        {
            out.insert(child_rel, content_hash(&bytes));
        }
    }
}

/// Returns the latest `mtime` across every source file contributing to `(target, kind)`
/// for restart freshness recovery.
fn latest_book_source_mtime(ws: &Workspace, target: &Target, kind: Kind) -> Option<SystemTime> {
    let (contest_dir, problem_dirs) = target_source_dirs(ws, target)?;
    let mut latest: Option<SystemTime> = None;
    let bump = |latest: &mut Option<SystemTime>, t: SystemTime| {
        *latest = Some(match *latest {
            Some(cur) => cur.max(t),
            None => t,
        });
    };

    if let Some(cdir) = contest_dir {
        record_file_mtime(ws, &format!("{cdir}/contest.toml"), &mut latest, &bump);
        record_tree_mtime(ws, &format!("{cdir}/assets"), &mut latest, &bump);
    }

    for pdir in problem_dirs {
        record_file_mtime(ws, &format!("{pdir}/problem.toml"), &mut latest, &bump);
        record_tree_mtime(ws, &format!("{pdir}/assets"), &mut latest, &bump);
        match kind {
            Kind::Statements => {
                for name in ["statement.md", "statement.zh.md", "statement.en.md"] {
                    record_file_mtime(ws, &format!("{pdir}/{name}"), &mut latest, &bump);
                }
                record_tree_mtime(ws, &format!("{pdir}/samples"), &mut latest, &bump);
            }
            Kind::Editorial => {
                record_file_mtime(ws, &format!("{pdir}/editorial.md"), &mut latest, &bump);
            }
        }
    }

    latest
}

fn record_file_mtime(
    ws: &Workspace,
    rel: &str,
    latest: &mut Option<SystemTime>,
    bump: &impl Fn(&mut Option<SystemTime>, SystemTime),
) {
    if let Ok(abs) = ws.resolve(rel)
        && let Ok(meta) = std::fs::metadata(&abs)
        && meta.is_file()
        && let Ok(mtime) = meta.modified()
    {
        bump(latest, mtime);
    }
}

fn record_tree_mtime(
    ws: &Workspace,
    rel_dir: &str,
    latest: &mut Option<SystemTime>,
    bump: &impl Fn(&mut Option<SystemTime>, SystemTime),
) {
    let Ok(abs_dir) = ws.resolve(rel_dir) else {
        return;
    };
    let Ok(dir_meta) = std::fs::metadata(&abs_dir) else {
        return;
    };
    if !dir_meta.is_dir() {
        return;
    }
    // Include the directory's own mtime so deleting a sample or asset also invalidates.
    if let Ok(mtime) = dir_meta.modified() {
        bump(latest, mtime);
    }
    let Ok(entries) = std::fs::read_dir(&abs_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        if name.starts_with('.') {
            continue;
        }
        let child_rel = format!("{rel_dir}/{name}");
        let Ok(child_abs) = ws.resolve(&child_rel) else {
            continue;
        };
        if child_abs.is_dir() {
            record_tree_mtime(ws, &child_rel, latest, bump);
        } else if let Ok(meta) = std::fs::metadata(&child_abs)
            && meta.is_file()
            && let Ok(mtime) = meta.modified()
        {
            bump(latest, mtime);
        }
    }
}

const ALLOWED_IMAGE_EXTS: &[&str] = &["png", "jpg", "jpeg", "svg", "webp", "pdf"];

fn normalize_image_ext(raw: &str) -> Result<String, String> {
    let ext = raw.trim().trim_start_matches('.').to_ascii_lowercase();
    if ALLOWED_IMAGE_EXTS.contains(&ext.as_str()) {
        Ok(ext)
    } else {
        Err(format!(
            "unsupported image extension {raw:?}: expected one of png, jpg, jpeg, svg, webp, pdf"
        ))
    }
}

fn sanitize_asset_stem(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len().min(80));
    let mut prev_dash = false;
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            out.push(ch);
            prev_dash = false;
        } else if (ch == '-' || ch.is_whitespace() || ch == '.') && !out.is_empty() && !prev_dash {
            out.push('-');
            prev_dash = true;
        }
        if out.len() >= 80 {
            break;
        }
    }
    let trimmed = out.trim_matches(['-', '_']);
    if trimmed.is_empty() {
        "image".to_owned()
    } else {
        trimmed.to_owned()
    }
}

fn paste_timestamp_stem() -> String {
    let secs = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let hour = rem / 3_600;
    let minute = (rem % 3_600) / 60;
    let second = rem % 60;

    // Howard Hinnant's civil_from_days (UTC).
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = y + i64::from(month <= 2);

    format!("paste-{year:04}{month:02}{day:02}-{hour:02}{minute:02}{second:02}")
}

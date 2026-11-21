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

use xcpc_convert::publish::{self, Kind, Target};
use xcpc_domain::{
    ContestMetaPatch, FuzzyMatchKind, Language, Problem, RatingTier, SearchQuery, check_id,
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
    ActionResult, AppPrefsJs, AssetReferenceJs, CleanUnusedAssetsResultJs, ColdStartSnapshotJs,
    ContestDetailJs, ContestItemDetailJs, ContestMetaPatchJs, ContestStatsJs, ExportBookStatusJs,
    FreezeResultJs, FuzzyHitJs, OverridePatchJs, PdfFreshnessJs, PreviewLocationJs, PreviewRequest,
    PreviewResult, PreviewView, ProblemAssetJs, ProblemDetailJs, ProblemMetaPatchJs,
    ProblemSearchQuery, ProblemSummaryJs, RatingBucketJs, SamplePairJs, SaveResult,
    SourceLocationJs, TagCountJs, UnfreezeDiffResultJs, UnfreezeProblemDiffJs, WatchEventJs,
    WorkspaceContest, WorkspaceFacets, WorkspaceFile, WorkspaceInfo, WorkspaceItem,
    WorkspaceProblem, diff_kind_str, extract_report, first_error, limits_to_js, rating_tier_str,
    source_to_js,
};

/// Active watch receiver shared with `Preview::next_change` outside `State`'s mutex.
#[derive(Clone)]
pub(crate) struct WatchSession {
    pub(crate) id: u64,
    pub(crate) rx: Arc<Mutex<mpsc::Receiver<WatchEvent>>>,
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
    pub(crate) cached: Option<CachedCompile>,
    /// Layer keys of the latest committed preview result, protected from LRU eviction.
    pub(crate) active: HashSet<String>,
}

impl CompilerState {
    fn sync_root(&mut self, ws: &Workspace) {
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
        publish::check(
            ws,
            &mut self.engine,
            &request.target(),
            request.kind(),
            None,
            request.unsaved(),
        )
        .map_err(|e| e.to_string())
    }
}

pub(crate) struct State {
    pub(crate) id: u64,
    /// Where `prefs.json` (recent workspaces, layout & Frecency) is kept.
    pub(crate) data_dir: Option<PathBuf>,
    /// Bumped on workspace mutations, switches, and external watch events to invalidate `cached`.
    pub(crate) revision: AtomicU64,
    pub(crate) core: Mutex<CoreState>,
    /// Lightweight UI-accessible snapshot of the latest compiled preview.
    pub(crate) snapshot: Mutex<Option<Arc<CachedSnapshot>>>,
    pub(crate) compiler: Mutex<CompilerState>,
}

impl Drop for State {
    fn drop(&mut self) {
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
    pub(crate) fn new(data_dir: Option<PathBuf>) -> Self {
        let env_ws = std::env::var_os("XCPC_WORKSPACE").map(PathBuf::from);
        Self::new_with_sources(data_dir, env_ws.as_deref(), Path::new("."))
    }

    pub(crate) fn new_with_sources(
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
            revision: AtomicU64::new(0),
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
                cached: None,
                active: HashSet::new(),
            }),
        };

        if let Some(ws) = workspace {
            let old = state.install_workspace(ws);
            drop(old);
        }
        state
    }

    pub(crate) fn revision(&self) -> u64 {
        self.revision.load(Ordering::Acquire)
    }

    pub(crate) fn workspace_root(&self) -> Option<PathBuf> {
        self.core
            .lock()
            .unwrap()
            .workspace
            .as_ref()
            .map(|w| w.root().to_path_buf())
    }

    pub(crate) fn recent_workspaces(&self) -> Vec<String> {
        self.core
            .lock()
            .unwrap()
            .prefs
            .app
            .recent_workspaces
            .clone()
    }

    pub(crate) fn watch_session(&self) -> Option<WatchSession> {
        self.core.lock().unwrap().watch_session.clone()
    }

    /// Switches to `ws`, starting a fresh [`WorkspaceWatcher`] wired with [`SelfWriteTracker`]
    /// and scanning a fresh [`WorkspaceIndex`]. Returns the previous watcher so the caller
    /// can drop it outside `core`'s mutex. Never blocks on `compiler`.
    pub(crate) fn install_workspace(&self, ws: Workspace) -> Option<WorkspaceWatcher> {
        let (tracked_ws, new_watcher, new_session_rx) = match ws.watch() {
            Ok((tracked, watcher, rx)) => (tracked, Some(watcher), Some(rx)),
            Err(_) => (ws, None, None),
        };

        let root = tracked_ws.root().to_path_buf();
        let new_index = WorkspaceIndex::scan(&tracked_ws).unwrap_or_default();

        let old_watcher = {
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
    pub(crate) fn stop_watcher(&self) -> Option<WorkspaceWatcher> {
        let mut core = self.core.lock().ok()?;
        core.watch_session = None;
        core.watcher.take()
    }

    /// Invalidates the compiled preview cache without touching `compiler` or `core`.
    pub(crate) fn invalidate_cache(&self) {
        self.revision.fetch_add(1, Ordering::AcqRel);
        *self.snapshot.lock().unwrap() = None;
    }

    /// Applies an external [`WatchEvent`] if it still belongs to `session_id`: updates
    /// [`WorkspaceIndex`] incrementally (or rescans on overflow) and invalidates the
    /// compiled cache. Never blocks on `compiler`.
    pub(crate) fn apply_watch_event(&self, session_id: u64, event: WatchEvent) -> WatchEventJs {
        let out = {
            let mut core = self.core.lock().unwrap();
            if !core
                .watch_session
                .as_ref()
                .is_some_and(|s| s.id == session_id)
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

            match event {
                WatchEvent::Changed(paths) => {
                    for p in &paths {
                        let _ = core.index.refresh_path(&ws, p);
                    }
                    WatchEventJs {
                        kind: "changed".into(),
                        paths,
                    }
                }
                WatchEvent::Rescan => {
                    if let Ok(idx) = WorkspaceIndex::scan(&ws) {
                        core.index = idx;
                    }
                    WatchEventJs {
                        kind: "rescan".into(),
                        paths: Vec::new(),
                    }
                }
            }
        };
        self.invalidate_cache();
        out
    }

    // ---- Workspace info & file I/O -----------------------------------------------------

    pub(crate) fn info(&self) -> WorkspaceInfo {
        let (ws, last_workspace_missing) = {
            let core = self.core.lock().unwrap();
            (core.workspace.clone(), core.last_workspace_missing)
        };
        let Some(ws) = ws else {
            return WorkspaceInfo {
                last_workspace_missing,
                ..Default::default()
            };
        };
        let mut info = WorkspaceInfo {
            root: ws.root().display().to_string(),
            read_only: ws.is_read_only(),
            last_workspace_missing: false,
            ..Default::default()
        };
        let mut errors = Vec::new();
        for id in ws.list_problems().unwrap_or_default() {
            match ws.load_problem(&id) {
                Ok(p) => info.problems.push(WorkspaceProblem {
                    statement: statement_path(&p, p.default_language()),
                    editorial: format!("{}/editorial.md", p.dir),
                    id,
                    title: p.meta.title,
                }),
                Err(e) => errors.push(e.to_string()),
            }
        }
        for id in ws.list_contests().unwrap_or_default() {
            let contest = match ws.load_contest(&id) {
                Ok(c) => c,
                Err(e) => {
                    errors.push(e.to_string());
                    continue;
                }
            };
            let items = if contest.meta.frozen {
                match ws.contest_problems(&contest) {
                    Ok(list) => list
                        .into_iter()
                        .map(|(item, p)| WorkspaceItem {
                            title: item.apply(&p.meta).title,
                            statement: statement_path(&p, contest.meta.language),
                            editorial: format!("{}/editorial.md", p.dir),
                            label: item.label,
                            id: item.id,
                        })
                        .collect(),
                    Err(e) => {
                        errors.push(e.to_string());
                        Vec::new()
                    }
                }
            } else {
                contest
                    .meta
                    .problems
                    .iter()
                    .map(|item| match ws.load_problem(&item.id) {
                        Ok(p) => WorkspaceItem {
                            title: item.apply(&p.meta).title,
                            statement: statement_path(&p, contest.meta.language),
                            editorial: format!("{}/editorial.md", p.dir),
                            label: item.label.clone(),
                            id: item.id.clone(),
                        },
                        Err(e) => {
                            errors.push(e.to_string());
                            WorkspaceItem {
                                title: item.title.clone().unwrap_or_else(|| item.id.clone()),
                                statement: format!("problems/{}/statement.md", item.id),
                                editorial: format!("problems/{}/editorial.md", item.id),
                                label: item.label.clone(),
                                id: item.id.clone(),
                            }
                        }
                    })
                    .collect()
            };
            info.contests.push(WorkspaceContest {
                id,
                name: contest.meta.name,
                frozen: contest.meta.frozen,
                items,
            });
        }
        info.error = errors.into_iter().next();
        info
    }

    pub(crate) fn read(&self, path: String) -> WorkspaceFile {
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

    pub(crate) fn save(
        &self,
        path: String,
        text: String,
        expected_hash: Option<String>,
    ) -> SaveResult {
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

    pub(crate) fn mcp_config_snippet(&self) -> String {
        let root = self.workspace_root();
        mcp_config_snippet(root.as_deref())
    }

    // ---- Preview compilation, rasterization, export & bidirectional lookup -------------

    pub(crate) fn run(
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
        let cache_hit = compiler
            .cached
            .as_ref()
            .is_some_and(|c| c.matches_request(request, rev));
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
                    return PreviewResult {
                        error: Some(error),
                        compile_ms: elapsed,
                        ..Default::default()
                    };
                }
            };
            let (Some(compiled), Some(doc)) = (compiled, doc) else {
                let (error, diagnostics, starts) = extract_report(&report);
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

            // Pre-index forward-locate anchors for the currently edited file on this
            // background thread so UI-thread `locateSource` never touches `compiler`.
            let mut initial_anchors = HashMap::new();
            if !request.path.is_empty() && !cancel.is_cancelled() {
                let norm = normalize_ws_rel(&request.path);
                if file_matches_book_kind(&norm, request.kind()) {
                    let anchors = sample_file_anchors(
                        &compiler.engine,
                        &compiled,
                        &doc,
                        &report.problems,
                        &norm,
                    );
                    initial_anchors.insert(norm, anchors);
                }
            }

            let snap = Arc::new(CachedSnapshot {
                request: request.clone(),
                revision: rev,
                matches_disk: matches_disk.clone(),
                doc_source: doc.source.clone(),
                problems: report.problems.clone(),
                anchors: Mutex::new(initial_anchors),
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
        let (error, diagnostics, starts) = extract_report(&cached.report);
        result.error = error;
        result.diagnostics = diagnostics;
        result.starts = starts;
        result
    }

    pub(crate) fn locate_click(&self, page: u32, x_pt: f64, y_pt: f64) -> Option<SourceLocationJs> {
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
    pub(crate) fn locate_source(&self, file: &str, line: u32) -> Option<PreviewLocationJs> {
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

    pub(crate) fn export(&self, request: &PreviewRequest) -> WorkspaceFile {
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
    pub(crate) fn resolve_reveal_path(&self, path: &str) -> Result<PathBuf, ActionResult> {
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
    pub(crate) fn export_status(&self, request: &PreviewRequest) -> ExportBookStatusJs {
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

    pub(crate) fn search_problems(&self, query: ProblemSearchQuery) -> Vec<ProblemSummaryJs> {
        let core_query = SearchQuery {
            text: query.text,
            tags: query.tags,
            rating_min: query.rating_min,
            rating_max: query.rating_max,
            platform: query.platform,
        };
        let core = self.core.lock().unwrap();
        core.index
            .search_problems(&core_query)
            .into_iter()
            .map(|p| ProblemSummaryJs {
                id: p.id,
                title: p.title,
                rating: p.rating,
                tags: p.tags,
                sources: p.sources.iter().map(source_to_js).collect(),
                has_editorial: p.has_editorial,
                has_solutions: p.has_solutions,
                statement_path: p.statement_path,
            })
            .collect()
    }

    pub(crate) fn workspace_facets(&self) -> WorkspaceFacets {
        let mut tags = BTreeSet::new();
        let mut platforms = BTreeSet::new();
        let core = self.core.lock().unwrap();
        for p in core.index.problems() {
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
        WorkspaceFacets {
            tags: tags.into_iter().collect(),
            platforms: platforms.into_iter().collect(),
        }
    }

    pub(crate) fn problem_detail(&self, id: &str) -> ProblemDetailJs {
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

        ProblemDetailJs {
            id: problem.id,
            title: problem.meta.title,
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
            error: None,
        }
    }

    pub(crate) fn read_solution(&self, id: &str, name: &str) -> WorkspaceFile {
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

    pub(crate) fn create_problem(&self, id: &str, title: &str) -> ActionResult {
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

    pub(crate) fn update_problem_meta(&self, id: &str, patch: ProblemMetaPatchJs) -> ActionResult {
        let ws = self.core.lock().unwrap().workspace.clone();
        let Some(ws) = ws else {
            return ActionResult::err("no_workspace", "no workspace open");
        };
        match ws.update_problem_meta(id, &patch.into_core()) {
            Ok(_) => {
                let _ = self.core.lock().unwrap().index.refresh_problem(&ws, id);
                self.invalidate_cache();
                ActionResult::ok()
            }
            Err(e) => ActionResult::from_storage_err(e),
        }
    }

    pub(crate) fn delete_problem(&self, id: &str) -> ActionResult {
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

    pub(crate) fn save_samples(&self, id: &str, samples: Vec<SamplePairJs>) -> ActionResult {
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

    pub(crate) fn contest_detail(&self, id: &str) -> ContestDetailJs {
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

        let items = if frozen {
            match ws.contest_problems(&contest) {
                Ok(list) => list
                    .into_iter()
                    .map(|(item, p)| {
                        let show_metadata = item.show_metadata.unwrap_or(default_show);
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
                        }
                    })
                    .collect(),
                Err(e) => {
                    error = Some(e.to_string());
                    Vec::new()
                }
            }
        } else {
            meta.problems
                .iter()
                .map(|item| {
                    let overridden_title = item.title.is_some();
                    let overridden_time = item.limits.as_ref().is_some_and(|l| l.time.is_some());
                    let overridden_memory =
                        item.limits.as_ref().is_some_and(|l| l.memory.is_some());
                    let overridden_input = item.limits.as_ref().is_some_and(|l| l.input.is_some());
                    let overridden_output =
                        item.limits.as_ref().is_some_and(|l| l.output.is_some());
                    let overridden_show_metadata = item.show_metadata.is_some();
                    let show_metadata = item.show_metadata.unwrap_or(default_show);

                    match ws.load_problem(&item.id) {
                        Ok(p) => {
                            let effective = item.apply(&p.meta);
                            ContestItemDetailJs {
                                label: item.label.clone(),
                                id: item.id.clone(),
                                missing: false,
                                title: effective.title,
                                original_title: p.meta.title.clone(),
                                limits: limits_to_js(&effective.limits),
                                original_limits: limits_to_js(&p.meta.limits),
                                show_metadata,
                                default_show_metadata: default_show,
                                overridden_title,
                                overridden_time,
                                overridden_memory,
                                overridden_input,
                                overridden_output,
                                overridden_show_metadata,
                                statement: statement_path(&p, meta.language),
                                editorial: format!("{}/editorial.md", p.dir),
                                rating: p.meta.rating,
                                tags: p.meta.tags,
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
            show_metadata: meta.show_metadata,
            cover: meta.cover,
            has_cover: meta.has_cover(),
            blank_pages: meta.blank_pages,
            frozen,
            items,
            error,
        }
    }

    pub(crate) fn contest_stats(&self, id: &str) -> ContestStatsJs {
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

    pub(crate) fn create_contest(&self, id: &str, name: &str) -> ActionResult {
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

    pub(crate) fn compose_contest(&self, id: &str, problem_ids: &[String]) -> ActionResult {
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

    pub(crate) fn update_contest_meta(&self, id: &str, patch: ContestMetaPatchJs) -> ActionResult {
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

    pub(crate) fn set_override(
        &self,
        contest_id: &str,
        label: &str,
        patch: OverridePatchJs,
    ) -> ActionResult {
        let ws = self.core.lock().unwrap().workspace.clone();
        let Some(ws) = ws else {
            return ActionResult::err("no_workspace", "no workspace open");
        };
        match ws.set_override(contest_id, label, &patch.into_core()) {
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

    pub(crate) fn freeze_contest(&self, id: &str) -> FreezeResultJs {
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
                error: Some(e.to_string()),
                code: Some(e.code().to_owned()),
            },
        }
    }

    pub(crate) fn unfreeze_diff(&self, id: &str) -> UnfreezeDiffResultJs {
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
                error: Some(e.to_string()),
                code: Some(e.code().to_owned()),
            },
        }
    }

    pub(crate) fn unfreeze_contest(&self, id: &str) -> ActionResult {
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

    // ---- Cmd+K fuzzy jump, Frecency & preferences --------------------------------------

    pub(crate) fn fuzzy_jump(&self, query: &str, active_contest: Option<&str>) -> Vec<FuzzyHitJs> {
        self.fuzzy_jump_at(query, active_contest, now_epoch_secs())
    }

    pub(crate) fn fuzzy_jump_at(
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
            let visit_key = format!("{kind}:{}", hit.id);
            let boost = core.prefs.frecency_boost(&visit_key, now_secs);
            hits.push(FuzzyHitJs {
                kind: kind.to_owned(),
                id: hit.id,
                title: hit.title,
                contest_id: None,
                label: None,
                visit_key,
                score: hit.score + boost,
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

    pub(crate) fn record_visit(&self, key: &str) {
        let mut core = self.core.lock().unwrap();
        core.prefs.record_visit(key);
        core.prefs.persist(self.data_dir.as_deref());
    }

    pub(crate) fn load_prefs(&self) -> AppPrefsJs {
        self.core.lock().unwrap().prefs.app.clone()
    }

    pub(crate) fn save_prefs(&self, prefs: AppPrefsJs) {
        let mut core = self.core.lock().unwrap();
        core.prefs.apply_app_prefs(prefs);
        core.prefs.persist(self.data_dir.as_deref());
    }

    // ---- Native image asset import -----------------------------------------------------

    /// Pre-validates that `target` / `id` / `label` resolves to an existing writable problem
    /// (not read-only workspace, not a frozen contest, valid IDs).
    pub(crate) fn validate_image_target(
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
    pub(crate) fn import_image_file(
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
    pub(crate) fn import_image_bytes(
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
    pub(crate) fn list_problem_assets(
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
    pub(crate) fn rename_problem_asset(
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
    pub(crate) fn delete_problem_asset(
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
    pub(crate) fn clean_unused_assets(
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
    pub(crate) fn set_asset_width(
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
    pub(crate) fn save_cold_start_snapshot(
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
    pub(crate) fn load_cold_start_snapshot(&self) -> Option<ColdStartSnapshotJs> {
        let ws = self.core.lock().unwrap().workspace.clone();
        snapshot::load_cold_start_snapshot(self.data_dir.as_deref(), ws.as_ref(), self.id)
    }
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

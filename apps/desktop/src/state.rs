//! Mutable desktop bridge state: open workspace, in-memory index, file watcher session,
//! Typst engine + compiled cache, and persisted preferences.
//!
//! All domain and storage rules are delegated to `xcpc-domain`, `xcpc-storage`, and
//! `xcpc-convert`.

use std::collections::{BTreeSet, HashSet};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, mpsc};
use std::time::Instant;

use xcpc_convert::publish;
use xcpc_domain::{
    ContestMetaPatch, FuzzyMatchKind, Language, Problem, RatingTier, SearchQuery, check_id,
};
use xcpc_engine::{Compiled, Engine, FontOptions};
use xcpc_storage::{WatchEvent, Workspace, WorkspaceIndex, WorkspaceWatcher, content_hash};

use crate::cache::{CachedCompile, CancelToken, NEXT_PREVIEW_ID, PIXELS, rasterize_cached};
use crate::prefs::{StoredPrefs, mcp_config_snippet, now_epoch_secs};
use crate::types::{
    ActionResult, AppPrefsJs, ContestDetailJs, ContestItemDetailJs, ContestMetaPatchJs,
    ContestStatsJs, FreezeResultJs, FuzzyHitJs, OverridePatchJs, PreviewRequest, PreviewResult,
    PreviewView, ProblemDetailJs, ProblemMetaPatchJs, ProblemSearchQuery, ProblemSummaryJs,
    RatingBucketJs, SamplePairJs, SaveResult, SourceLocationJs, TagCountJs, UnfreezeDiffResultJs,
    UnfreezeProblemDiffJs, WatchEventJs, WorkspaceContest, WorkspaceFacets, WorkspaceFile,
    WorkspaceInfo, WorkspaceItem, WorkspaceProblem, diff_kind_str, extract_report, first_error,
    limits_to_js, rating_tier_str, source_to_js,
};

/// Active watch receiver shared with `Preview::next_change` outside `State`'s mutex.
#[derive(Clone)]
pub(crate) struct WatchSession {
    pub(crate) id: u64,
    pub(crate) rx: Arc<Mutex<mpsc::Receiver<WatchEvent>>>,
}

pub(crate) struct State {
    pub(crate) id: u64,
    pub(crate) workspace: Option<Workspace>,
    pub(crate) index: WorkspaceIndex,
    pub(crate) watcher: Option<WorkspaceWatcher>,
    pub(crate) watch_session: Option<WatchSession>,
    watch_seq: u64,
    /// Where `prefs.json` (recent workspaces, layout & Frecency) is kept.
    pub(crate) data_dir: Option<PathBuf>,
    pub(crate) prefs: StoredPrefs,
    pub(crate) engine: Engine,
    /// Bumped on workspace mutations, switches, and external watch events to invalidate `cached`.
    pub(crate) revision: u64,
    pub(crate) cached: Option<CachedCompile>,
    /// Layer keys of the latest committed preview result, protected from LRU eviction.
    pub(crate) active: HashSet<String>,
}

impl Drop for State {
    fn drop(&mut self) {
        self.watch_session = None;
        drop(self.watcher.take());
        if let Ok(mut pixels) = PIXELS.lock() {
            pixels.remove_owner(self.id);
        }
    }
}

impl State {
    pub(crate) fn new(data_dir: Option<PathBuf>) -> Self {
        let prefs = StoredPrefs::load(data_dir.as_deref());
        let workspace = match std::env::var_os("XCPC_WORKSPACE") {
            Some(dir) => Workspace::open(dir, false).ok(),
            None => Workspace::discover(".", false).ok().or_else(|| {
                let last = prefs.latest_workspace()?;
                Workspace::open(last, false).ok()
            }),
        };

        let mut state = Self {
            id: NEXT_PREVIEW_ID.fetch_add(1, Ordering::Relaxed),
            workspace: None,
            index: WorkspaceIndex::default(),
            watcher: None,
            watch_session: None,
            watch_seq: 0,
            data_dir,
            prefs,
            engine: Engine::new(&FontOptions::from_env(), None),
            revision: 0,
            cached: None,
            active: HashSet::new(),
        };

        if let Some(ws) = workspace {
            let old = state.install_workspace(ws);
            drop(old);
        }
        state
    }

    /// Switches to `ws`, starting a fresh [`WorkspaceWatcher`] wired with [`SelfWriteTracker`]
    /// and scanning a fresh [`WorkspaceIndex`]. Returns the previous watcher so the caller
    /// can drop it outside `State`'s mutex.
    pub(crate) fn install_workspace(&mut self, ws: Workspace) -> Option<WorkspaceWatcher> {
        let (tracked_ws, new_watcher, new_session) = match ws.watch() {
            Ok((tracked, watcher, rx)) => {
                self.watch_seq = self.watch_seq.wrapping_add(1);
                let session = WatchSession {
                    id: self.watch_seq,
                    rx: Arc::new(Mutex::new(rx)),
                };
                (tracked, Some(watcher), Some(session))
            }
            Err(_) => (ws, None, None),
        };

        let root = tracked_ws.root().to_path_buf();
        self.prefs.remember_workspace(&root);
        self.prefs.persist(self.data_dir.as_deref());

        self.index = WorkspaceIndex::scan(&tracked_ws).unwrap_or_default();
        self.engine.set_root(Some(root));
        self.workspace = Some(tracked_ws);
        self.watch_session = new_session;
        self.invalidate_cache();

        std::mem::replace(&mut self.watcher, new_watcher)
    }

    /// Stops the active watcher (if any) and returns it to be dropped outside the lock.
    pub(crate) fn stop_watcher(&mut self) -> Option<WorkspaceWatcher> {
        self.watch_session = None;
        self.watcher.take()
    }

    pub(crate) fn invalidate_cache(&mut self) {
        self.revision = self.revision.wrapping_add(1);
        self.cached = None;
    }

    /// Applies an external [`WatchEvent`] if it still belongs to `session_id`: updates
    /// [`WorkspaceIndex`] incrementally (or rescans on overflow), resets the engine's disk
    /// file cache, and invalidates [`CachedCompile`].
    pub(crate) fn apply_watch_event(&mut self, session_id: u64, event: WatchEvent) -> WatchEventJs {
        if !self
            .watch_session
            .as_ref()
            .is_some_and(|s| s.id == session_id)
        {
            return WatchEventJs {
                kind: "closed".into(),
                paths: Vec::new(),
            };
        }
        let Some(ws) = self.workspace.clone() else {
            return WatchEventJs {
                kind: "closed".into(),
                paths: Vec::new(),
            };
        };

        match event {
            WatchEvent::Changed(paths) => {
                for p in &paths {
                    let _ = self.index.refresh_path(&ws, p);
                }
                self.engine.reset_files();
                self.invalidate_cache();
                WatchEventJs {
                    kind: "changed".into(),
                    paths,
                }
            }
            WatchEvent::Rescan => {
                if let Ok(idx) = WorkspaceIndex::scan(&ws) {
                    self.index = idx;
                }
                self.engine.reset_files();
                self.invalidate_cache();
                WatchEventJs {
                    kind: "rescan".into(),
                    paths: Vec::new(),
                }
            }
        }
    }

    // ---- Workspace info & file I/O -----------------------------------------------------

    pub(crate) fn info(&self) -> WorkspaceInfo {
        let Some(ws) = &self.workspace else {
            return WorkspaceInfo::default();
        };
        let mut info = WorkspaceInfo {
            root: ws.root().display().to_string(),
            read_only: ws.is_read_only(),
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
        let Some(ws) = &self.workspace else {
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
        &mut self,
        path: String,
        text: String,
        expected_hash: Option<String>,
    ) -> SaveResult {
        let Some(ws) = self.workspace.clone() else {
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
                let _ = self.index.refresh_path(&ws, &path);
                self.engine.reset_files();
                self.invalidate_cache();
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
        mcp_config_snippet(self.workspace.as_ref().map(|w| w.root()))
    }

    // ---- Preview compilation, rasterization, export & reverse click lookup -------------

    fn compile(
        &mut self,
        request: &PreviewRequest,
    ) -> Result<
        (
            Option<Compiled>,
            Option<xcpc_convert::Document>,
            publish::Report,
        ),
        String,
    > {
        let ws = self.workspace.as_ref().ok_or("no workspace")?;
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

    pub(crate) fn run(
        &mut self,
        request: &PreviewRequest,
        scale: f32,
        view: &[PreviewView],
        cancel: CancelToken<'_>,
    ) -> PreviewResult {
        if cancel.is_cancelled() {
            return PreviewResult::default();
        }
        let cache_hit = self
            .cached
            .as_ref()
            .is_some_and(|c| c.revision == self.revision && c.request == *request);
        let compile_ms = if cache_hit {
            0.0
        } else {
            // Clear stale cache before mutating `self.engine` so `locate_click` never
            // queries spans against a mismatched `/main.typ`.
            self.cached = None;
            let start = Instant::now();
            let compiled = self.compile(request);
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
            self.cached = Some(CachedCompile {
                request: request.clone(),
                revision: self.revision,
                compiled,
                doc,
                report,
                fingerprints: vec![None; page_count],
            });
            elapsed
        };
        if cancel.is_cancelled() {
            return PreviewResult {
                compile_ms,
                ..Default::default()
            };
        }
        let cached = self.cached.as_mut().expect("cached above");
        let (mut result, cancelled) =
            rasterize_cached(self.id, &mut self.active, cached, scale, view, cancel);
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
        let cached = self.cached.as_ref()?;
        let loc = publish::locate_point(
            &self.engine,
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

    pub(crate) fn export(&mut self, request: &PreviewRequest) -> WorkspaceFile {
        let path = publish::default_output(&request.target(), request.kind());
        let fail = |error: String| WorkspaceFile {
            path: path.clone(),
            error: Some(error),
            ..Default::default()
        };

        // Fast path: reuse cached compilation when it matches and passed all checks.
        let pdf = if let Some(cached) = self
            .cached
            .as_ref()
            .filter(|c| c.revision == self.revision && c.request == *request && c.report.ok)
        {
            match cached.compiled.pdf() {
                Ok(pdf) => pdf,
                Err(e) => return fail(e.first().map(|d| d.message.clone()).unwrap_or_default()),
            }
        } else {
            self.cached = None;
            let (compiled, _doc, report) = match self.compile(request) {
                Ok(r) => r,
                Err(e) => return fail(e),
            };
            let Some(compiled) = compiled.filter(|_| report.ok) else {
                return fail(first_error(&report).unwrap_or_default());
            };
            match compiled.pdf() {
                Ok(pdf) => pdf,
                Err(e) => return fail(e.first().map(|d| d.message.clone()).unwrap_or_default()),
            }
        };

        let ws = self.workspace.as_ref().expect("compiled in it");
        match ws.write(&path, &pdf) {
            Ok(()) => WorkspaceFile {
                path: ws.root().join(&path).display().to_string(),
                ..Default::default()
            },
            Err(e) => fail(e.to_string()),
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
        self.index
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
        for p in self.index.problems() {
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
        let Some(ws) = &self.workspace else {
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
        let referencing_contests = self
            .index
            .contests()
            .into_iter()
            .filter(|(_, meta)| meta.problems.iter().any(|item| item.id == id))
            .map(|(cid, _)| cid.to_owned())
            .collect();

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
        let Some(ws) = &self.workspace else {
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

    pub(crate) fn create_problem(&mut self, id: &str, title: &str) -> ActionResult {
        let Some(ws) = self.workspace.clone() else {
            return ActionResult::err("no_workspace", "no workspace open");
        };
        match ws.create_problem(id, title) {
            Ok(_) => {
                let _ = self.index.refresh_problem(&ws, id);
                self.invalidate_cache();
                ActionResult::ok()
            }
            Err(e) => ActionResult::from_storage_err(e),
        }
    }

    pub(crate) fn update_problem_meta(
        &mut self,
        id: &str,
        patch: ProblemMetaPatchJs,
    ) -> ActionResult {
        let Some(ws) = self.workspace.clone() else {
            return ActionResult::err("no_workspace", "no workspace open");
        };
        match ws.update_problem_meta(id, &patch.into_core()) {
            Ok(_) => {
                let _ = self.index.refresh_problem(&ws, id);
                self.invalidate_cache();
                ActionResult::ok()
            }
            Err(e) => ActionResult::from_storage_err(e),
        }
    }

    pub(crate) fn delete_problem(&mut self, id: &str) -> ActionResult {
        let Some(ws) = self.workspace.clone() else {
            return ActionResult::err("no_workspace", "no workspace open");
        };
        match ws.delete_problem(id) {
            Ok(()) => {
                let _ = self.index.refresh_problem(&ws, id);
                self.invalidate_cache();
                ActionResult::ok()
            }
            Err(e) => ActionResult::from_storage_err(e),
        }
    }

    pub(crate) fn save_samples(&mut self, id: &str, samples: Vec<SamplePairJs>) -> ActionResult {
        let Some(ws) = self.workspace.clone() else {
            return ActionResult::err("no_workspace", "no workspace open");
        };
        let pairs: Vec<(String, String)> =
            samples.into_iter().map(|s| (s.input, s.output)).collect();
        match ws.save_samples(id, &pairs) {
            Ok(()) => {
                let _ = self.index.refresh_problem(&ws, id);
                self.invalidate_cache();
                ActionResult::ok()
            }
            Err(e) => ActionResult::from_storage_err(e),
        }
    }

    // ---- Contest composition, overrides, stats, freeze & unfreeze ----------------------

    pub(crate) fn contest_detail(&self, id: &str) -> ContestDetailJs {
        let Some(ws) = &self.workspace else {
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
        let stats = if let Some(s) = self.index.contest_stats_by_id(id) {
            s
        } else {
            let Some(ws) = &self.workspace else {
                return ContestStatsJs {
                    error: Some("no workspace open".into()),
                    ..Default::default()
                };
            };
            match ws.load_contest(id) {
                Ok(c) => self.index.contest_stats(&c.meta),
                Err(e) => {
                    return ContestStatsJs {
                        error: Some(e.to_string()),
                        ..Default::default()
                    };
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

    pub(crate) fn create_contest(&mut self, id: &str, name: &str) -> ActionResult {
        let Some(ws) = self.workspace.clone() else {
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
                let _ = self.index.refresh_contest(&ws, id);
                self.invalidate_cache();
                ActionResult::ok()
            }
            Err(e) => ActionResult::from_storage_err(e),
        }
    }

    pub(crate) fn compose_contest(&mut self, id: &str, problem_ids: &[String]) -> ActionResult {
        let Some(ws) = self.workspace.clone() else {
            return ActionResult::err("no_workspace", "no workspace open");
        };
        match ws.compose_contest(id, problem_ids, None) {
            Ok(_) => {
                let _ = self.index.refresh_contest(&ws, id);
                self.invalidate_cache();
                ActionResult::ok()
            }
            Err(e) => ActionResult::from_storage_err(e),
        }
    }

    pub(crate) fn update_contest_meta(
        &mut self,
        id: &str,
        patch: ContestMetaPatchJs,
    ) -> ActionResult {
        let Some(ws) = self.workspace.clone() else {
            return ActionResult::err("no_workspace", "no workspace open");
        };
        let core_patch = match patch.into_core() {
            Ok(p) => p,
            Err(msg) => return ActionResult::err("invalid_argument", msg),
        };
        match ws.update_contest_meta(id, &core_patch) {
            Ok(_) => {
                let _ = self.index.refresh_contest(&ws, id);
                self.invalidate_cache();
                ActionResult::ok()
            }
            Err(e) => ActionResult::from_storage_err(e),
        }
    }

    pub(crate) fn set_override(
        &mut self,
        contest_id: &str,
        label: &str,
        patch: OverridePatchJs,
    ) -> ActionResult {
        let Some(ws) = self.workspace.clone() else {
            return ActionResult::err("no_workspace", "no workspace open");
        };
        match ws.set_override(contest_id, label, &patch.into_core()) {
            Ok(_) => {
                let _ = self.index.refresh_contest(&ws, contest_id);
                self.invalidate_cache();
                ActionResult::ok()
            }
            Err(e) => ActionResult::from_storage_err(e),
        }
    }

    pub(crate) fn freeze_contest(&mut self, id: &str) -> FreezeResultJs {
        let Some(ws) = self.workspace.clone() else {
            return FreezeResultJs {
                ok: false,
                frozen: false,
                error: Some("no workspace open".into()),
                code: Some("no_workspace".into()),
                ..Default::default()
            };
        };
        // `publish::freeze_contest` compiles statements (and optionally editorial) using
        // `self.engine`, so clear `cached` to preserve the engine/cached span invariant.
        self.cached = None;
        match publish::freeze_contest(&ws, &mut self.engine, id) {
            Ok(outcome) => {
                let (first_err, diagnostics, _) = extract_report(&outcome.report);
                if outcome.frozen {
                    let _ = self.index.refresh_contest(&ws, id);
                    self.engine.reset_files();
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
        let Some(ws) = &self.workspace else {
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

    pub(crate) fn unfreeze_contest(&mut self, id: &str) -> ActionResult {
        let Some(ws) = self.workspace.clone() else {
            return ActionResult::err("no_workspace", "no workspace open");
        };
        match ws.unfreeze_contest(id) {
            Ok(_) => {
                let _ = self.index.refresh_contest(&ws, id);
                self.engine.reset_files();
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
        let mut hits = Vec::new();

        // Core fuzzy search over all problems and contests (without double-counting
        // in-memory index frecency; persisted Frecency from `prefs.json` is applied here).
        for hit in self.index.fuzzy_search_at(query, usize::MAX, now_secs) {
            let kind = match hit.kind {
                FuzzyMatchKind::Problem => "problem",
                FuzzyMatchKind::Contest => "contest",
            };
            let visit_key = format!("{kind}:{}", hit.id);
            let boost = self.prefs.frecency_boost(&visit_key, now_secs);
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
            && let Some(contest) = self.index.contest(cid)
        {
            let norm_q = normalize_query(query);
            let terms: Vec<&str> = norm_q.split_whitespace().collect();
            for item in &contest.problems {
                let effective_title = item
                    .title
                    .clone()
                    .or_else(|| self.index.problem(&item.id).map(|p| p.title.clone()))
                    .unwrap_or_else(|| item.id.clone());
                let Some(base_score) =
                    score_contest_item(&terms, &norm_q, &item.label, &item.id, &effective_title)
                else {
                    continue;
                };
                let visit_key = format!("contest_item:{cid}:{}", item.label);
                let boost = self.prefs.frecency_boost(&visit_key, now_secs);
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

    pub(crate) fn record_visit(&mut self, key: &str) {
        self.prefs.record_visit(key);
        self.prefs.persist(self.data_dir.as_deref());
    }

    pub(crate) fn load_prefs(&self) -> AppPrefsJs {
        self.prefs.app.clone()
    }

    pub(crate) fn save_prefs(&mut self, prefs: AppPrefsJs) {
        self.prefs.apply_app_prefs(prefs);
        self.prefs.persist(self.data_dir.as_deref());
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

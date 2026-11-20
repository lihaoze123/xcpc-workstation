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
mod page_image;
mod prefs;
mod state;
mod types;

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use nsh_base as _;
use nsh_core::file_dialog::OpenDialog;
use nsh_core::prelude::*;
use xcpc_storage::Workspace;

use crate::cache::CancelToken;
use crate::state::State;

pub use crate::page_image::{PageImage, PageImageProps};
pub use crate::types::{
    ActionResult, AppPrefsJs, ContestDetailJs, ContestItemDetailJs, ContestMetaPatchJs,
    ContestStatsJs, ExportBookStatusJs, FreezeResultJs, FuzzyHitJs, LimitsJs, OverridePatchJs,
    PdfFreshnessJs, PickedFolder, PreviewDiagnostic, PreviewLayer, PreviewPage, PreviewRequest,
    PreviewResult, PreviewStart, PreviewView, ProblemDetailJs, ProblemMetaPatchJs,
    ProblemSearchQuery, ProblemSourceJs, ProblemSummaryJs, RatingBucketJs, SamplePairJs,
    SaveResult, SourceLocationJs, TagCountJs, UnfreezeDiffResultJs, UnfreezeProblemDiffJs,
    WatchEventJs, WorkspaceContest, WorkspaceFacets, WorkspaceFile, WorkspaceInfo, WorkspaceItem,
    WorkspaceProblem,
};

/// The crate's UI set (`nsh.json` `"rust"`).
#[native_module("xcpc-workstation/native")]
fn native(registry: &mut Registry) {
    registry.class::<Preview>();
    registry.register::<PageImage>();
}

/// The workspace bridge and preview engine: reads and saves files with SHA-256 conflict
/// detection, watches external changes, maintains the in-memory workspace index, exposes
/// Phase 2 semantic operations, compiles with `publish::check` off the UI thread, and
/// rasterizes visible pages for [`PageImage`].
#[class]
pub struct Preview {
    state: Arc<Mutex<State>>,
    /// Generation counter incremented on every `Preview::compile` (before taking `state`)
    /// so in-flight tile rasterization aborts as soon as a newer request arrives.
    generation: Arc<AtomicU64>,
}

impl Drop for Preview {
    fn drop(&mut self) {
        self.generation.fetch_add(1, Ordering::AcqRel);
        // Stop the watcher before dropping our `Arc<State>` so any pending `next_change`
        // blocking on `mpsc::Receiver::recv()` wakes up immediately and never leaks.
        let old_watcher = self
            .state
            .lock()
            .ok()
            .and_then(|mut state| state.stop_watcher());
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
        let data_dir = data_dir.map(PathBuf::from);
        Preview {
            state: Arc::new(Mutex::new(State::new(data_dir))),
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

    // ---- Workspace & settings ----------------------------------------------------------

    /// Opens the workspace at `path` instead.
    pub fn open_workspace(&self, path: String, read_only: bool) -> WorkspaceInfo {
        match Workspace::open(&path, read_only) {
            Ok(ws) => self.switch(ws),
            Err(e) => WorkspaceInfo {
                error: Some(e.to_string()),
                ..Default::default()
            },
        }
    }

    /// Initializes a new XCPC workspace at `path` (creating `workspace.toml`, `problems/`,
    /// `contests/`, `.gitignore`, and a static `AGENTS.md` if absent) and switches to it.
    pub fn init_workspace(&self, path: String) -> WorkspaceInfo {
        match xcpc_storage::init_workspace(&path) {
            Ok(ws) => self.switch(ws),
            Err(e) => WorkspaceInfo {
                error: Some(e.to_string()),
                ..self.workspace()
            },
        }
    }

    /// Opens the system folder picker and reports whether the selected directory belongs to
    /// an existing workspace (`isWorkspace`, `workspaceRoot`), allowing the GUI to offer
    /// initializing a new workspace when `isWorkspace` is false.
    pub async fn pick_folder(this: Instance<Self>) -> PickedFolder {
        if this.is_disposed() {
            return PickedFolder {
                cancelled: true,
                ..Default::default()
            };
        }
        let start = this.with(|p| {
            p.state
                .lock()
                .unwrap()
                .workspace
                .as_ref()
                .map(|w| w.root().to_path_buf())
        });
        let mut dialog = OpenDialog::folders()
            .title("选择工作区目录")
            .button_label("选择");
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
    pub async fn pick_workspace(this: Instance<Self>) -> WorkspaceInfo {
        if this.is_disposed() {
            return WorkspaceInfo::default();
        }
        let start = this.with(|p| {
            p.state
                .lock()
                .unwrap()
                .workspace
                .as_ref()
                .map(|w| w.root().to_path_buf())
        });
        let mut dialog = OpenDialog::folders()
            .title("打开工作区")
            .button_label("打开");
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
        self.state.lock().unwrap().info()
    }

    /// Recently opened workspace directories, most recent first.
    pub fn recent_workspaces(&self) -> Vec<String> {
        self.state
            .lock()
            .unwrap()
            .prefs
            .app
            .recent_workspaces
            .clone()
    }

    /// Generates the ready-to-paste JSON snippet for configuring `xcpc mcp` against the
    /// currently open workspace.
    pub fn mcp_config_snippet(&self) -> String {
        self.state.lock().unwrap().mcp_config_snippet()
    }

    // ---- File read / save & live watcher -----------------------------------------------

    /// Reads a file of the workspace for the editor together with its SHA-256 content hash
    /// (empty string when the file does not exist yet).
    pub fn read(&self, path: String) -> WorkspaceFile {
        self.state.lock().unwrap().read(path)
    }

    /// Atomically saves a workspace file, checking `expectedHash` when provided.
    ///
    /// Returns `{ conflict: true }` without overwriting when the file changed on disk since
    /// `expectedHash`. Successful writes are recorded in `SelfWriteTracker` so they are not
    /// echoed back by `nextChange()`, incrementally refresh the index, and invalidate the
    /// compiled preview cache.
    pub fn save(&self, path: String, text: String, expected_hash: Option<String>) -> SaveResult {
        self.state.lock().unwrap().save(path, text, expected_hash)
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
            let session = p.state.lock().unwrap().watch_session.clone();
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
            let mut state = state.lock().unwrap();
            state.apply_watch_event(session_id, event)
        })
        .await
        .unwrap_or_else(|_| WatchEventJs {
            kind: "closed".into(),
            paths: Vec::new(),
        })
    }

    // ---- Preview compile, locateClick & export -----------------------------------------

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
            state
                .lock()
                .unwrap()
                .run(&request, scale as f32, &view, cancel)
        })
        .await
        .unwrap_or_else(|e| PreviewResult {
            error: Some(format!("preview task failed: {e}")),
            ..Default::default()
        })
    }

    /// Maps a click at `(xPt, yPt)` (typographic points from the top-left of 0-based `page`)
    /// on the currently cached preview back to its 1-based Markdown source location.
    pub fn locate_click(&self, page: u32, x_pt: f64, y_pt: f64) -> Option<SourceLocationJs> {
        self.state.lock().unwrap().locate_click(page, x_pt, y_pt)
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
        blocking(move || state.lock().unwrap().export(&request))
            .await
            .unwrap_or_else(|e| WorkspaceFile {
                error: Some(format!("export failed: {e}")),
                ..Default::default()
            })
    }

    /// Reports independent PDF freshness (`exists`, `fresh`, `relPath`, `absPath`) for both
    /// the statements book and the editorial book of `request`'s target.
    pub fn export_status(&self, request: PreviewRequest) -> ExportBookStatusJs {
        self.state.lock().unwrap().export_status(&request)
    }

    /// Reveals a file or directory (absolute or workspace-relative path) inside the open
    /// workspace in the system file manager. Rejects paths outside the workspace or missing
    /// targets.
    pub async fn reveal_in_folder(this: Instance<Self>, path: String) -> ActionResult {
        if this.is_disposed() {
            return ActionResult::err("disposed", "preview disposed");
        }
        let resolved = this.with(|p| p.state.lock().unwrap().resolve_reveal_path(&path));
        let abs = match resolved {
            Ok(abs) => abs,
            Err(err) => return err,
        };
        match reveal_path(abs).await {
            Ok(()) => ActionResult::ok(),
            Err(e) => ActionResult::err("io", e),
        }
    }

    // ---- Problem library & editing -----------------------------------------------------

    /// Searches indexed problems using structured filters and normalized text matching.
    pub fn search_problems(&self, query: ProblemSearchQuery) -> Vec<ProblemSummaryJs> {
        self.state.lock().unwrap().search_problems(query)
    }

    /// Returns sorted distinct tags and external platforms across all indexed problems.
    pub fn workspace_facets(&self) -> WorkspaceFacets {
        self.state.lock().unwrap().workspace_facets()
    }

    /// Loads full problem details: metadata, statements & editorial with hashes, samples,
    /// solution filenames, and referencing contests.
    pub fn problem_detail(&self, id: String) -> ProblemDetailJs {
        self.state.lock().unwrap().problem_detail(&id)
    }

    /// Reads one solution file under `problems/<id>/solutions/<name>` read-only.
    pub fn read_solution(&self, id: String, name: String) -> WorkspaceFile {
        self.state.lock().unwrap().read_solution(&id, &name)
    }

    /// Creates a new problem directory with `problem.toml`, a `statement.md` skeleton, and
    /// an empty `samples/` directory, refreshing the in-memory index.
    pub fn create_problem(&self, id: String, title: String) -> ActionResult {
        self.state.lock().unwrap().create_problem(&id, &title)
    }

    /// Partially updates `problems/<id>/problem.toml` via `toml_edit`, preserving comments,
    /// key order, and unknown fields.
    pub fn update_problem_meta(&self, id: String, patch: ProblemMetaPatchJs) -> ActionResult {
        self.state.lock().unwrap().update_problem_meta(&id, patch)
    }

    /// Deletes a problem from the library. Refuses with `code: "problem_referenced"` and
    /// `referencingContests` when any unfrozen contest references it.
    pub fn delete_problem(&self, id: String) -> ActionResult {
        self.state.lock().unwrap().delete_problem(&id)
    }

    /// Saves `samples` into `problems/<id>/samples/` as `01.in`/`01.ans`..`NN.in`/`NN.ans`,
    /// stripping UTF-8 BOM and `\r` atomically and refreshing cache and index.
    pub fn save_samples(&self, id: String, samples: Vec<SamplePairJs>) -> ActionResult {
        self.state.lock().unwrap().save_samples(&id, samples)
    }

    // ---- Contest composition, overrides, stats, freeze & unfreeze ----------------------

    /// Loads full contest metadata and per-item effective/original values with override flags.
    pub fn contest_detail(&self, id: String) -> ContestDetailJs {
        self.state.lock().unwrap().contest_detail(&id)
    }

    /// Computes 8-tier difficulty distribution and tag counts for `id`.
    pub fn contest_stats(&self, id: String) -> ContestStatsJs {
        self.state.lock().unwrap().contest_stats(&id)
    }

    /// Creates a new empty contest via `compose_contest(id, [], { name })`, refusing if `id`
    /// already exists.
    pub fn create_contest(&self, id: String, name: String) -> ActionResult {
        self.state.lock().unwrap().create_contest(&id, &name)
    }

    /// Sets the ordered problem list for contest `id`, re-labelling `A..Z`, preserving
    /// existing overrides on kept problems, and removing overrides for dropped problems.
    pub fn compose_contest(&self, id: String, problem_ids: Vec<String>) -> ActionResult {
        self.state
            .lock()
            .unwrap()
            .compose_contest(&id, &problem_ids)
    }

    /// Partially updates `contests/<id>/contest.toml` metadata via `toml_edit`.
    pub fn update_contest_meta(&self, id: String, patch: ContestMetaPatchJs) -> ActionResult {
        self.state.lock().unwrap().update_contest_meta(&id, patch)
    }

    /// Sets or clears metadata overrides for `label` inside `contestId`. Passing `null`
    /// deletes that override; passing the original library/contest value auto-deletes it.
    pub fn set_override(
        &self,
        contest_id: String,
        label: String,
        patch: OverridePatchJs,
    ) -> ActionResult {
        self.state
            .lock()
            .unwrap()
            .set_override(&contest_id, &label, patch)
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
        blocking(move || state.lock().unwrap().freeze_contest(&id))
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
        self.state.lock().unwrap().unfreeze_diff(&id)
    }

    /// Unfreezes a frozen contest (GUI-only, after human confirmation), deleting `snapshot/`
    /// and restoring live references.
    pub fn unfreeze_contest(&self, id: String) -> ActionResult {
        self.state.lock().unwrap().unfreeze_contest(&id)
    }

    // ---- Cmd+K fuzzy jump, Frecency & preferences --------------------------------------

    /// Fuzzy-searches problems, contests, and (when `activeContest` is given) the active
    /// contest's problem labels, ranked with persisted Frecency.
    pub fn fuzzy_jump(&self, query: String, active_contest: Option<String>) -> Vec<FuzzyHitJs> {
        self.state
            .lock()
            .unwrap()
            .fuzzy_jump(&query, active_contest.as_deref())
    }

    /// Records a visit (`"problem:<id>"`, `"contest:<id>"`, or
    /// `"contest_item:<contestId>:<label>"`) for Frecency ranking and persists `prefs.json`.
    pub fn record_visit(&self, key: String) {
        self.state.lock().unwrap().record_visit(&key);
    }

    /// Loads persisted desktop preferences from `<dataDir>/prefs.json`.
    pub fn load_prefs(&self) -> AppPrefsJs {
        self.state.lock().unwrap().load_prefs()
    }

    /// Persists desktop preferences atomically to `<dataDir>/prefs.json` (preserving
    /// persisted Frecency history).
    pub fn save_prefs(&self, prefs: AppPrefsJs) {
        self.state.lock().unwrap().save_prefs(prefs);
    }
}

impl Preview {
    /// Makes `ws` the active workspace: cancels in-flight renders, swaps watcher outside
    /// the mutex, refreshes the index, and returns the new [`WorkspaceInfo`].
    fn switch(&self, ws: Workspace) -> WorkspaceInfo {
        self.generation.fetch_add(1, Ordering::AcqRel);
        let old_watcher = {
            let mut state = self.state.lock().unwrap();
            state.install_workspace(ws)
        };
        drop(old_watcher);
        self.workspace()
    }
}

#[cfg(test)]
mod tests;

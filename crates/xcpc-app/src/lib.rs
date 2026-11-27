//! Pure-Rust application layer for XCPC Workstation.
//!
//! Contains all NSH-independent business orchestration, workspace/preview/plugin/snippet/TRD
//! state machines, pixel-buffer LRU cache, cold-start snapshots, user preferences, theme
//! overrides, and pure-Rust DTOs. Zero dependency on `nsh-core`, `nsh-base`, GTK, or QuickJS.

pub mod cache;
pub mod clipboard;
mod plugins;
pub mod prefs;
pub mod snapshot;
mod snippets;
pub mod state;
pub mod theme_config;
pub mod types;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use xcpc_storage::Workspace;

pub use crate::cache::{CancelToken, LayerKind, PIXELS, PixelCache, RgbaPixels};
pub use crate::plugins::atomic_write_file;
pub use crate::state::{State, WatchSession};
pub use crate::theme_config::{
    STARTER_THEME_TOML, ThemeColorEntryJs, ThemeOverridesJs, ensure_theme_file,
    load_theme_overrides,
};
pub use crate::types::*;

/// Pure-Rust application controller owning the shared [`State`] and render generation counter.
///
/// Desktop shells (like `apps/desktop`'s NSH `Preview` class) wrap `AppController` and add
/// only UI-toolkit glue (async task scheduling, native file dialogs, JS DTO bindings, and
/// native widgets).
pub struct AppController {
    pub state: Arc<State>,
    /// Generation counter incremented on every compile/workspace switch so in-flight tile
    /// rasterization aborts as soon as a newer request arrives.
    pub generation: Arc<AtomicU64>,
}

impl Drop for AppController {
    fn drop(&mut self) {
        self.generation.fetch_add(1, Ordering::AcqRel);
        self.state.stop_batch_task();
        self.state.stop_plugin_watcher();
        let old_watcher = self.state.stop_watcher();
        drop(old_watcher);
    }
}

impl AppController {
    #[allow(clippy::new_without_default)] // the JS constructor
    /// Opens the workspace named by `XCPC_WORKSPACE`, else the one containing the current
    /// directory, else the most recent workspace recorded in `<dataDir>/prefs.json` (or
    /// legacy `last-workspace`). Pass `process.dataDir` for persistence; `null` disables it.
    pub fn new(data_dir: Option<String>) -> Self {
        let data_dir = data_dir.map(|passed| {
            std::env::var_os("XCPC_DATA_DIR")
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(passed))
        });
        AppController {
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
    /// BoldItalic) discovered from `XCPC_FONT_DIRS`, the executable's sibling `fonts/`
    /// directory, and (on macOS `.app` bundles) `Contents/Resources/fonts`, ready to pass
    /// to `registerFont(bytes)`.
    pub fn ui_fonts() -> Vec<Vec<u8>> {
        let mut dirs = Vec::new();
        if let Some(env_dirs) = std::env::var_os("XCPC_FONT_DIRS").filter(|v| !v.is_empty()) {
            dirs.extend(std::env::split_paths(&env_dirs));
        }
        if let Ok(exe) = std::env::current_exe()
            && let Some(parent) = exe.parent()
        {
            for candidate in app_font_dirs(parent) {
                if candidate.is_dir() {
                    dirs.push(candidate);
                }
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
                .and_then(|p| {
                    p.parent()
                        .map(|d| app_font_dirs(d).into_iter().any(|dir| dir.is_dir()))
                })
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

    /// Returns true when `XCPC_O11Y=1` requested opening the diagnostics window on launch.
    pub fn o11y_env_enabled() -> bool {
        xcpc_trace::o11y_env_enabled()
    }

    /// Polls incremental Rust trace events, structured logs, and pixel cache stats as JSON.
    pub fn o11y_poll_delta(after_event_seq: f64, after_log_seq: f64) -> String {
        let ev_seq = if after_event_seq > 0.0 {
            after_event_seq as u64
        } else {
            0
        };
        let lg_seq = if after_log_seq > 0.0 {
            after_log_seq as u64
        } else {
            0
        };
        let delta = xcpc_trace::poll_delta(ev_seq, lg_seq, 4_000, 1_000);
        let cache_stats = cache::pixel_cache_stats();
        serde_json::json!({
            "delta": delta,
            "cacheStats": cache_stats,
        })
        .to_string()
    }

    /// Clears buffered Rust trace events, always-on logs, and slow events.
    pub fn o11y_clear() {
        xcpc_trace::clear_all();
    }

    /// Reclaims idle pixel caches, Typst AST/comemo caches, and cold OS working-set pages.
    pub fn reclaim_memory(&self, clear_pixels: bool) {
        self.state.reclaim_memory(clear_pixels);
    }

    // ---- Workspace & settings ----------------------------------------------------------

    /// Opens the workspace at `path` instead.
    pub fn open_workspace(&self, path: String, read_only: bool) -> WorkspaceInfo {
        match Workspace::open(&path, read_only) {
            Ok(ws) => {
                let info = self.switch(ws);
                xcpc_trace::log_info(
                    "desktop",
                    format!("Opened workspace: {}", info.root),
                    Some(serde_json::json!({
                        "problems": info.problems.len(),
                        "contests": info.contests.len(),
                        "readOnly": info.read_only,
                    })),
                );
                info
            }
            Err(e) => {
                let msg = e.human_message();
                xcpc_trace::log_error(
                    "desktop",
                    format!("Failed to open workspace {path}: {msg}"),
                    None,
                );
                WorkspaceInfo {
                    error: Some(msg),
                    ..Default::default()
                }
            }
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

    /// Reports independent PDF freshness (`exists`, `fresh`, `relPath`, `absPath`) for both
    /// the statements book and the editorial book of `request`'s target.
    pub fn export_status(&self, request: PreviewRequest) -> ExportBookStatusJs {
        self.state.export_status(&request)
    }

    /// Loads user-global theme overrides from `<dataDir>/theme.toml`.
    pub fn load_theme_overrides(&self) -> ThemeOverridesJs {
        theme_config::load_theme_overrides(self.state.data_dir.as_deref())
    }

    /// Detects the operating system UI locale and normalizes it to `"zh-CN"` or `"en"`.
    pub fn system_locale(&self) -> String {
        detect_system_locale().to_owned()
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
    ) -> Result<String, String> {
        self.state
            .export_markdown_with_samples(&id, unsaved_statement.as_deref())
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

    /// Saves `samples` into `problems/<id>/samples/` as `01.in`/`01.ans`..`NN.in`/`NN.ans`,
    /// stripping UTF-8 BOM and `\r` atomically and refreshing cache and index.
    pub fn save_samples(&self, id: String, samples: Vec<SamplePairJs>) -> ActionResult {
        self.state.save_samples(&id, samples)
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
    ) -> Result<String, String> {
        self.state
            .import_image_bytes(&target, &id, label.as_deref(), &ext, &bytes)
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

    /// Fast O(1) check for whether `slug` has a local Polygon package.
    pub fn has_package(&self, slug: String) -> bool {
        self.state.has_package(&slug)
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

    // ---- Phase 3D: Sanitizer, external trial run, adopt & contest readiness audit ------

    /// Loads the latest isolated sanitizer report (`build/problems/<slug>/sanitizer-report.json`),
    /// returning `ok: true, reportJson: null` when no sanitizer run has been recorded yet.
    pub fn load_sanitizer_report(&self, slug: String) -> JsonReportResultJs {
        self.state.load_sanitizer_report(&slug)
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

    // ---- Phase 4A: Training status, URL resolution, Plugins & OJ Sync ------------------

    /// Appends a manual training status event (`todo | attempted | ac | upsolved | clear`)
    /// to `training/logs.jsonl`.
    pub fn set_training_status(&self, problem_id: String, status: String) -> ActionResult {
        self.state.set_training_status(&problem_id, &status)
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

    /// Explicitly trusts a plugin awaiting permission confirmation (`pendingTrust = false`).
    pub fn trust_plugin(&self, id: String) -> ActionResult {
        self.state.trust_plugin(&id)
    }

    /// Re-scans `<dataDir>/plugins/` and returns the refreshed plugins overview.
    pub fn reload_plugins(&self) -> PluginsOverviewJs {
        self.state.get_plugins_overview()
    }

    /// Exports an embedded built-in plugin to `<dataDir>/plugins/<id>/` for local customization.
    pub fn export_builtin_plugin(&self, id: String) -> ExportPluginResultJs {
        self.state.export_builtin_plugin(&id)
    }

    /// Subscribes to a decentralized plugin repository (`index.json` URL).
    pub fn add_plugin_repo(&self, url: String) -> ActionResult {
        self.state.add_plugin_repo(&url)
    }

    /// Unsubscribes from a decentralized plugin repository URL.
    pub fn remove_plugin_repo(&self, url: String) -> ActionResult {
        self.state.remove_plugin_repo(&url)
    }

    /// Restores the official plugin repository subscription if it was removed.
    pub fn restore_official_plugin_repo(&self) -> ActionResult {
        self.state.restore_official_plugin_repo()
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
                use std::os::windows::process::CommandExt;
                const CREATE_NO_WINDOW: u32 = 0x0800_0000;
                std::process::Command::new("cmd")
                    .args(["/C", "start", "", trimmed])
                    .creation_flags(CREATE_NO_WINDOW)
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

    /// Loads the pure-Web HTML entry (`ui.html`) for an enabled, trusted UI plugin.
    pub fn load_plugin_ui_html(&self, plugin_id: String) -> PluginUiHtmlJs {
        self.state.load_plugin_ui_html(&plugin_id)
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
        self.state
            .plugin_workspace_query(&plugin_id, &method, args_json.as_deref())
    }

    /// Returns the user's bound OJ accounts (platform, handle, cached rating/rank) for an
    /// enabled, trusted plugin. Exposes no credentials or private submission data.
    pub fn plugin_list_oj_accounts(&self, plugin_id: String) -> PluginWorkspaceResultJs {
        self.state.plugin_list_oj_accounts(&plugin_id)
    }

    /// Renders an SVG string to a PNG image (base64-encoded) via `resvg` + `tiny-skia`.
    pub fn plugin_render_svg_png(svg: String, scale: Option<f64>) -> PluginRenderPngResultJs {
        State::plugin_render_svg_png(&svg, scale)
    }

    /// Lists commands registered by enabled plugins matching `context` (`"global" | "problem" | "contest"`).
    pub fn plugin_list_commands(&self, context: Option<String>) -> JsonReportResultJs {
        self.state.plugin_list_commands(context)
    }

    /// Synchronously reads a key from the plugin's persistent KV store (`<dataDir>/plugin_kv/<id>.json`).
    pub fn plugin_kv_get(&self, plugin_id: String, key: String) -> PluginKvResultJs {
        self.state.plugin_kv_get(&plugin_id, &key)
    }

    /// Synchronously writes a JSON-encoded value into the plugin's persistent KV store.
    pub fn plugin_kv_set(
        &self,
        plugin_id: String,
        key: String,
        value_json: String,
    ) -> ActionResult {
        self.state.plugin_kv_set(&plugin_id, &key, &value_json)
    }

    /// Synchronously removes a key from the plugin's persistent KV store.
    pub fn plugin_kv_remove(&self, plugin_id: String, key: String) -> ActionResult {
        self.state.plugin_kv_remove(&plugin_id, &key)
    }

    /// Uninstalls a locally installed plugin, unlinks a dev-linked plugin (never deleting source dir),
    /// or reverts a local override back to the built-in version.
    pub fn uninstall_plugin(&self, id: String) -> InstallPluginResultJs {
        self.state.uninstall_plugin(&id)
    }

    /// Lists all importers declared by enabled plugins as JSON.
    pub fn plugin_list_importers(&self) -> JsonReportResultJs {
        self.state.plugin_list_importers()
    }

    /// Lists all exporters declared by enabled plugins as JSON.
    pub fn plugin_list_exporters(&self) -> JsonReportResultJs {
        self.state.plugin_list_exporters()
    }

    // ---- Phase 5: Algorithm Snippets, Bundler & Living TRDs ----------------------------

    /// Lists all workspace snippets with verification status, reverse dependencies, and tags.
    pub fn list_snippets_json(&self) -> JsonReportResultJs {
        self.state.list_snippets_json()
    }

    /// Loads one snippet's full detail, verification status, and last cached report.
    pub fn load_snippet_json(&self, slug: String) -> JsonReportResultJs {
        self.state.load_snippet_json(&slug)
    }

    /// Creates a new snippet under `snippets/<slug>/`.
    pub fn create_snippet(
        &self,
        slug: String,
        meta_json: String,
        code: Option<String>,
        notes: Option<String>,
    ) -> ActionResult {
        self.state
            .create_snippet(&slug, &meta_json, code.as_deref(), notes.as_deref())
    }

    /// Partially updates `snippets/<slug>/snippet.toml` via `toml_edit`.
    pub fn update_snippet_meta(&self, slug: String, patch_json: String) -> ActionResult {
        self.state.update_snippet_meta(&slug, &patch_json)
    }

    /// Atomically writes a single file inside `snippets/<slug>/` (`code.hpp`, `notes.md`,
    /// `test.cpp`, `verify_*.cpp`, etc.).
    pub fn write_snippet_file(
        &self,
        slug: String,
        file_name: String,
        content: String,
    ) -> ActionResult {
        self.state.write_snippet_file(&slug, &file_name, &content)
    }

    /// Deletes `snippets/<slug>/`.
    pub fn delete_snippet(&self, slug: String) -> ActionResult {
        self.state.delete_snippet(&slug)
    }

    /// Starts background verification (`mode`: `"single" | "cascade" | "all"`).
    pub fn start_snippet_verify(
        &self,
        mode: String,
        slug: Option<String>,
        force: bool,
    ) -> ActionResult {
        self.state.start_snippet_verify(&mode, slug, force)
    }

    /// Returns the current background snippet verification status and reports as JSON.
    pub fn snippet_verify_status_json(&self) -> JsonReportResultJs {
        self.state.snippet_verify_status_json()
    }

    /// Persists Library Checker mirror URL and local repository path in `prefs.json`.
    pub fn set_library_checker_config(
        &self,
        mirror_url: Option<String>,
        local_repo_path: Option<String>,
    ) -> ActionResult {
        self.state
            .set_library_checker_config(mirror_url, local_repo_path)
    }

    /// Expands a workspace snippet and all its transitive local `#include`s into one C++ file.
    pub fn bundle_snippet_json(&self, slug: String, compact: bool) -> JsonReportResultJs {
        self.state.bundle_snippet_json(&slug, compact)
    }

    /// Expands a local C++ file on disk with optional `-I` include directories.
    pub fn bundle_local_file_json(
        &self,
        path: String,
        include_dirs: Vec<String>,
        compact: bool,
    ) -> JsonReportResultJs {
        self.state
            .bundle_local_file_json(&path, include_dirs, compact)
    }

    /// Expands pasted C++ source text with optional base directory and `-I` include directories.
    pub fn bundle_local_source_json(
        &self,
        source: String,
        base_dir: Option<String>,
        include_dirs: Vec<String>,
        compact: bool,
    ) -> JsonReportResultJs {
        self.state
            .bundle_local_source_json(&source, base_dir, include_dirs, compact)
    }

    /// Lists all workspace TRD summaries as JSON.
    pub fn list_trds_json(&self) -> JsonReportResultJs {
        self.state.list_trds_json()
    }

    /// Loads one TRD's detail, preflight issues, and local template schema as JSON.
    pub fn load_trd_json(&self, id: String) -> JsonReportResultJs {
        self.state.load_trd_json(&id)
    }

    /// Creates or replaces a TRD's chapters and optional metadata patch.
    pub fn compose_trd(
        &self,
        id: String,
        chapters_json: String,
        meta_patch_json: Option<String>,
    ) -> ActionResult {
        self.state
            .compose_trd(&id, &chapters_json, meta_patch_json.as_deref())
    }

    /// Partially updates `trds/<id>/trd.toml` metadata via `toml_edit`.
    pub fn update_trd_meta(&self, id: String, patch_json: String) -> ActionResult {
        self.state.update_trd_meta(&id, &patch_json)
    }

    /// Deletes an unfrozen, unarchived TRD directory.
    pub fn delete_trd(&self, id: String) -> ActionResult {
        self.state.delete_trd(&id)
    }

    /// Archives or unarchives a TRD.
    pub fn set_trd_archived(&self, id: String, archived: bool) -> ActionResult {
        self.state.set_trd_archived(&id, archived)
    }

    /// Runs TRD preflight checks (missing snippets, missing deps, cycles, verify warnings).
    pub fn preflight_trd_json(&self, id: String) -> JsonReportResultJs {
        self.state.preflight_trd_json(&id)
    }

    /// Checks whether the default exported PDF for `trds/<id>/` exists on disk.
    pub fn trd_export_status(&self, id: String) -> PdfFreshnessJs {
        self.state.trd_export_status(&id)
    }

    /// Computes the diff between a frozen TRD's snapshot and live workspace snippets.
    pub fn diff_trd_for_unfreeze_json(&self, id: String) -> JsonReportResultJs {
        self.state.diff_trd_for_unfreeze_json(&id)
    }

    /// Unfreezes (reopens) a frozen TRD for live editing.
    pub fn unfreeze_trd(&self, id: String) -> ActionResult {
        self.state.unfreeze_trd(&id)
    }

    /// Lists TRD templates (`TemplateKind::Trd`), TRD presets, and loaded font families.
    pub fn list_trd_templates_json(&self) -> String {
        self.state.list_trd_templates_json()
    }

    /// Makes `ws` the active workspace: cancels in-flight renders, swaps watcher outside
    /// the mutex, refreshes the index, and returns the new [`WorkspaceInfo`].
    pub fn switch(&self, ws: Workspace) -> WorkspaceInfo {
        self.generation.fetch_add(1, Ordering::AcqRel);
        let old_watcher = self.state.install_workspace(ws);
        drop(old_watcher);
        self.workspace()
    }
}

/// If `dir` is non-empty, returns a fresh `xcpc-example` subdirectory inside it
/// (`xcpc-example`, `xcpc-example-2`, ...) so existing files are never mixed with
/// example content.
pub fn parse_example_language(raw: Option<&str>) -> xcpc_domain::Language {
    match raw.map(str::trim) {
        Some(s) if s.to_ascii_lowercase().starts_with("en") => xcpc_domain::Language::English,
        _ => xcpc_domain::Language::Chinese,
    }
}

pub fn resolve_example_target_dir(dir: &Path) -> PathBuf {
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

const UI_FONT_STEMS: &[&str] = &[
    "JetBrainsMono-Regular",
    "JetBrainsMono-Medium",
    "JetBrainsMono-Bold",
    "JetBrainsMono-Italic",
    "JetBrainsMono-BoldItalic",
];

pub fn load_ui_fonts_from(dirs: &[PathBuf]) -> Vec<Vec<u8>> {
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

/// Candidate app-bundled font directories relative to the executable's parent directory.
/// Includes `<exe_dir>/fonts` on all platforms, plus `../Resources/fonts` when running
/// from inside a macOS `.app/Contents/MacOS` bundle.
fn app_font_dirs(exe_dir: &Path) -> Vec<PathBuf> {
    let mut out = vec![exe_dir.join("fonts")];
    if exe_dir.file_name().and_then(|n| n.to_str()) == Some("MacOS")
        && let Some(contents) = exe_dir.parent()
        && contents.file_name().and_then(|n| n.to_str()) == Some("Contents")
    {
        out.push(contents.join("Resources").join("fonts"));
    }
    out
}

#[cfg(target_os = "macos")]
fn macos_user_locale() -> Option<String> {
    let output = std::process::Command::new("/usr/bin/defaults")
        .args(["read", "-g", "AppleLanguages"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_macos_apple_languages(&stdout)
}

/// Parses the first preferred language from `defaults read -g AppleLanguages` output,
/// e.g. `(\n    "zh-Hans-CN",\n    "en-US"\n)`.
pub fn parse_macos_apple_languages(raw: &str) -> Option<String> {
    for line in raw.lines() {
        let trimmed = line.trim().trim_end_matches(',');
        if trimmed.is_empty() || trimmed == "(" || trimmed == ")" {
            continue;
        }
        let cleaned = trimmed.trim_matches('"').trim();
        if !cleaned.is_empty() {
            return Some(cleaned.to_owned());
        }
    }
    None
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

pub fn detect_system_locale() -> &'static str {
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

    // macOS `.app` bundles launched from Finder/Dock do not inherit shell `LANG`/`LC_ALL`.
    #[cfg(target_os = "macos")]
    if let Some(raw) = macos_user_locale() {
        return normalize_system_locale(&raw);
    }

    "en"
}

pub fn normalize_system_locale(raw: &str) -> &'static str {
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

//! JavaScript-facing data transfer objects (`#[derive(JsObject)]` and tri-state patch
//! types) plus zero-copy `From` conversions to/from `xcpc_app` pure-Rust DTOs.

use nsh_core::element::{JsHost, JsIn, JsKind, JsOwned, JsValue, PropError, TsTypes};
use nsh_core::prelude::*;
use xcpc_domain::Patch;

// ---- Preview & diagnostics -------------------------------------------------------------

/// Pixels to draw over a page.
#[derive(Debug, Clone, Default, PartialEq, JsObject)]
pub struct PreviewLayer {
    /// Its pixels, named by what the page draws, the scale and the tile.
    pub key: String,
    /// Where it goes, as fractions of the page's width and height.
    pub left: f64,
    pub top: f64,
    pub width: f64,
    pub height: f64,
}

/// One rendered page.
#[derive(Debug, Clone, Default, PartialEq, JsObject)]
pub struct PreviewPage {
    /// Its size in typographic points.
    pub width: f64,
    pub height: f64,
    /// What `PageImage` draws, bottom first: the whole page, then tiles. None for a
    /// page out of view that has no pixels yet.
    pub layers: Vec<PreviewLayer>,
}

/// Rendered inline/display math snippet for the editor's floating mini math preview.
#[derive(Debug, Clone, Default, PartialEq, JsObject)]
pub struct MathPreviewJs {
    /// Pixel cache key for `PageImage` (empty when `error` is set).
    pub key: String,
    /// Rendered box width in typographic points.
    pub width: f64,
    /// Rendered box height in typographic points.
    pub height: f64,
    pub error: Option<String>,
}

/// The part of a page in view, in its points: tiled when the scale asks for more than a
/// whole-page layer has. A page listed with an empty part gets a whole-page layer only.
#[derive(Debug, Clone, Default, PartialEq, JsObject)]
pub struct PreviewView {
    /// The page (0-based).
    pub page: u32,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// A problem or contest of the workspace to typeset, with the file the editor has
/// open as it is there (saved or not).
#[derive(Debug, Clone, Default, PartialEq, JsObject)]
pub struct PreviewRequest {
    /// `"problem"` or `"contest"`.
    pub target: String,
    pub id: String,
    /// `"statements"` or `"editorial"`.
    pub book: String,
    /// The open file, relative to the workspace, and its text; an empty path for none.
    pub path: String,
    pub text: String,
}

/// A diagnostic from conversion, Typst compilation, overflow checks, or `xcpc-lint`.
#[derive(Debug, Clone, Default, PartialEq, JsObject)]
pub struct PreviewDiagnostic {
    /// `"error"` or `"warning"`.
    pub severity: String,
    /// Stable preflight/lint rule code when present (e.g. `"missing_image"`, `"missing_problem"`).
    #[prop(skip_none)]
    pub code: Option<String>,
    pub message: String,
    /// The file (relative to the workspace), and the line and column (1-based).
    #[prop(skip_none)]
    pub file: Option<String>,
    #[prop(skip_none)]
    pub line: Option<u32>,
    #[prop(skip_none)]
    pub column: Option<u32>,
    /// The page (0-based) a layout warning is about.
    #[prop(skip_none)]
    pub page: Option<u32>,
}

/// Where a problem sits in the compiled book, for outline navigation and page stats.
#[derive(Debug, Clone, Default, PartialEq, Eq, JsObject)]
pub struct PreviewStart {
    pub label: String,
    /// Problem ID (slug).
    pub id: String,
    /// The first page of this problem, 0-based (index into `PreviewResult.pages`).
    pub page: u32,
    /// The first physical page of this problem, 1-based (for outline/stats display).
    pub first: u32,
    /// Number of physical pages occupied by this problem.
    pub count: u32,
}

/// What a compilation gave.
#[derive(Debug, Clone, Default, PartialEq, JsObject)]
pub struct PreviewResult {
    /// The pages; empty when compilation failed.
    pub pages: Vec<PreviewPage>,
    /// The first error, as the user should read it, if any.
    #[prop(skip_none)]
    pub error: Option<String>,
    /// Errors and warnings (including `xcpc-lint` preflight findings).
    pub diagnostics: Vec<PreviewDiagnostic>,
    /// Where each problem starts and how many pages it spans.
    pub starts: Vec<PreviewStart>,
    /// Milliseconds spent typesetting and rasterizing, and how many layers were rasterized.
    pub compile_ms: f64,
    pub render_ms: f64,
    pub rendered: u32,
}

/// A 1-based location inside a workspace Markdown source file, returned by `locateClick`.
#[derive(Debug, Clone, Default, PartialEq, Eq, JsObject)]
pub struct SourceLocationJs {
    /// Workspace-relative file path (e.g. `"problems/a/statement.md"`).
    pub file: String,
    /// 1-based line number.
    pub line: u32,
    /// 1-based column number.
    pub column: u32,
}

/// A position on a rendered preview page, returned by `locateSource`.
#[derive(Debug, Clone, Default, PartialEq, JsObject)]
pub struct PreviewLocationJs {
    /// 0-based page index into `PreviewResult.pages`.
    pub page: u32,
    /// Vertical offset from the top of `page`, in typographic points.
    pub y_pt: f64,
    /// True when the position came from a mapped source anchor rather than a
    /// problem-start fallback.
    pub exact: bool,
}

// ---- Workspace, files & file watching --------------------------------------------------

/// A problem of the workspace.
#[derive(Debug, Clone, Default, PartialEq, JsObject)]
pub struct WorkspaceProblem {
    pub id: String,
    pub title: String,
    /// The statement and editorial files, relative to the workspace.
    pub statement: String,
    pub editorial: String,
}

/// A problem of a contest.
#[derive(Debug, Clone, Default, PartialEq, JsObject)]
pub struct WorkspaceItem {
    pub label: String,
    pub id: String,
    pub title: String,
    /// Its statement (in the contest's language) and editorial files, relative to the
    /// workspace: the library's, or the snapshot's once the contest is frozen.
    pub statement: String,
    pub editorial: String,
}

/// A contest of the workspace.
#[derive(Debug, Clone, Default, PartialEq, JsObject)]
pub struct WorkspaceContest {
    pub id: String,
    pub name: String,
    pub frozen: bool,
    pub archived: bool,
    pub items: Vec<WorkspaceItem>,
}

/// The open workspace.
#[derive(Debug, Clone, Default, PartialEq, JsObject)]
pub struct WorkspaceInfo {
    /// Its directory; empty when none is open.
    pub root: String,
    pub read_only: bool,
    /// True when `workspace.toml` enables training features (defaults to `true` when open).
    pub training_enabled: bool,
    /// True when `workspace.toml` enables snippets / TRD features (defaults to `true` when open).
    pub snippets_enabled: bool,
    /// True when startup tried to reopen the last remembered workspace (`prefs.json` or
    /// legacy `last-workspace`) and its directory no longer exists or lacks `workspace.toml`.
    /// False on a fresh launch with no history or once any workspace is opened/initialized.
    pub last_workspace_missing: bool,
    /// What could not be read, or why the folder picked is no workspace.
    #[prop(skip_none)]
    pub error: Option<String>,
    pub problems: Vec<WorkspaceProblem>,
    pub contests: Vec<WorkspaceContest>,
}

/// Freshness state of one exported book PDF (`build/<id>.pdf` or `build/<id>-editorial.pdf`).
#[derive(Debug, Clone, Default, PartialEq, Eq, JsObject)]
pub struct PdfFreshnessJs {
    /// Default output path relative to the workspace.
    pub rel_path: String,
    /// Absolute path on disk; empty when no workspace is open.
    pub abs_path: String,
    /// True when the PDF file currently exists on disk.
    pub exists: bool,
    /// True when the on-disk PDF matches the current sources and unsaved editor state.
    pub fresh: bool,
}

/// Independent PDF freshness states for statements and editorial books.
#[derive(Debug, Clone, Default, PartialEq, Eq, JsObject)]
pub struct ExportBookStatusJs {
    pub statements: PdfFreshnessJs,
    pub editorial: PdfFreshnessJs,
}

/// Result of picking a folder via the system dialog (may or may not be inside a workspace).
#[derive(Debug, Clone, Default, PartialEq, JsObject)]
pub struct PickedFolder {
    pub cancelled: bool,
    /// Selected folder path; empty when cancelled or errored.
    pub path: String,
    /// Discovered workspace root (containing `workspace.toml`) when `isWorkspace` is true.
    #[prop(skip_none)]
    pub workspace_root: Option<String>,
    /// True when `path` or an ancestor directory contains `workspace.toml`.
    pub is_workspace: bool,
    #[prop(skip_none)]
    pub error: Option<String>,
}

/// A file of the workspace as the editor opens it.
#[derive(Debug, Clone, Default, PartialEq, JsObject)]
pub struct WorkspaceFile {
    pub path: String,
    /// Its text; empty for a file that does not exist yet.
    pub text: String,
    /// Lowercase SHA-256 hex digest of the file's contents; empty if the file does not exist yet.
    pub hash: String,
    /// Edits cannot be saved (a frozen contest, a read-only workspace, or a read-only solution).
    pub read_only: bool,
    #[prop(skip_none)]
    pub error: Option<String>,
}

/// Result of saving a workspace file with optional content-hash conflict detection.
#[derive(Debug, Clone, Default, PartialEq, JsObject)]
pub struct SaveResult {
    pub ok: bool,
    /// New lowercase SHA-256 hex digest after writing; empty on failure.
    pub hash: String,
    /// True when the file on disk was modified externally since `expectedHash`.
    pub conflict: bool,
    #[prop(skip_none)]
    pub error: Option<String>,
}

/// External filesystem change notification yielded by `nextChange()`.
#[derive(Debug, Clone, Default, PartialEq, Eq, JsObject)]
pub struct WatchEventJs {
    /// `"changed"` (external files modified in `paths`), `"rescan"` (full workspace rescan
    /// performed), or `"closed"` (watcher stopped due to workspace switch, no workspace open,
    /// or `Preview` disposal).
    pub kind: String,
    /// Workspace-relative `/`-separated paths that changed externally (non-empty for `"changed"`).
    pub paths: Vec<String>,
}

/// Result of a workspace mutation.
#[derive(Debug, Clone, Default, PartialEq, Eq, JsObject)]
pub struct ActionResult {
    pub ok: bool,
    #[prop(skip_none)]
    pub error: Option<String>,
    /// Stable Core error code (e.g. `"contest_frozen"`, `"problem_referenced"`, `"invalid_id"`,
    /// `"too_many_problems"`, `"conflict"`, `"read_only"`).
    #[prop(skip_none)]
    pub code: Option<String>,
    /// Referencing unfrozen contest IDs when deleting a problem fails with `"problem_referenced"`.
    #[prop(default)]
    pub referencing_contests: Vec<String>,
}

impl ActionResult {
    pub(crate) fn ok() -> Self {
        Self {
            ok: true,
            ..Default::default()
        }
    }

    pub(crate) fn err(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            ok: false,
            code: Some(code.into()),
            error: Some(message.into()),
            ..Default::default()
        }
    }
}

// ---- Problem library & editing ---------------------------------------------------------

/// One external OJ identity (`[[source]]`).
#[derive(Debug, Clone, Default, PartialEq, Eq, JsObject)]
pub struct ProblemSourceJs {
    pub platform: String,
    pub id: String,
    #[prop(skip_none)]
    pub url: Option<String>,
}

/// Time/memory limits and optional file I/O names.
#[derive(Debug, Clone, Default, PartialEq, Eq, JsObject)]
pub struct LimitsJs {
    #[prop(skip_none)]
    pub time: Option<String>,
    #[prop(skip_none)]
    pub memory: Option<String>,
    #[prop(skip_none)]
    pub input: Option<String>,
    #[prop(skip_none)]
    pub output: Option<String>,
}

/// Structured filter + normalized text query for `searchProblems`.
#[derive(Debug, Clone, Default, PartialEq, Eq, JsObject)]
pub struct ProblemSearchQuery {
    #[prop(skip_none)]
    pub text: Option<String>,
    #[prop(default)]
    pub tags: Vec<String>,
    #[prop(skip_none)]
    pub rating_min: Option<u32>,
    #[prop(skip_none)]
    pub rating_max: Option<u32>,
    #[prop(skip_none)]
    pub platform: Option<String>,
    #[prop(skip_none)]
    pub group: Option<String>,
    #[prop(default)]
    pub ungrouped_only: bool,
}

/// Indexed problem summary returned by `searchProblems` (without statement body).
#[derive(Debug, Clone, Default, PartialEq, Eq, JsObject)]
pub struct ProblemSummaryJs {
    pub id: String,
    pub title: String,
    #[prop(skip_none)]
    pub group: Option<String>,
    #[prop(skip_none)]
    pub rating: Option<u32>,
    pub tags: Vec<String>,
    pub sources: Vec<ProblemSourceJs>,
    pub has_editorial: bool,
    pub has_solutions: bool,
    #[prop(skip_none)]
    pub statement_path: Option<String>,
    /// Static package freshness status (`"verified_fresh" | "stale" | "unverified" | "failed"`),
    /// or omitted for pure publishing problems without a package.
    #[prop(skip_none)]
    pub package_status: Option<String>,
    /// Sorted IDs of non-archived contests that include this problem.
    #[prop(default)]
    pub contest_ids: Vec<String>,
    /// Derived training status (`"none" | "todo" | "attempted" | "ac" | "upsolved"`).
    /// Always `"none"` when `workspace.toml` sets `training = false`.
    #[prop(default)]
    pub training_status: String,
}

/// Summary of one persistent problem group for the library sidebar.
#[derive(Debug, Clone, Default, PartialEq, Eq, JsObject)]
pub struct GroupSummaryJs {
    pub name: String,
    pub count: u32,
    /// True when declared in `workspace.toml`; false when only referenced by `problem.toml`.
    pub declared: bool,
}

/// Distinct tags, external platforms, and persistent group counts across the workspace problem library.
#[derive(Debug, Clone, Default, PartialEq, Eq, JsObject)]
pub struct WorkspaceFacets {
    pub tags: Vec<String>,
    pub platforms: Vec<String>,
    pub groups: Vec<GroupSummaryJs>,
    pub ungrouped_count: u32,
    pub total_count: u32,
}

/// One sample test case pair (`samples/01.in` & `samples/01.ans`).
#[derive(Debug, Clone, Default, PartialEq, Eq, JsObject)]
pub struct SamplePairJs {
    /// Sample stem (e.g. `"01"`). Ignored on `saveSamples` (re-numbered `"01"..="NN"`).
    #[prop(default)]
    pub name: String,
    pub input: String,
    pub output: String,
}

/// Full details of one problem for the library inspector and studio editor.
#[derive(Debug, Clone, Default, PartialEq, JsObject)]
pub struct ProblemDetailJs {
    pub id: String,
    pub title: String,
    #[prop(skip_none)]
    pub group: Option<String>,
    #[prop(skip_none)]
    pub rating: Option<u32>,
    pub tags: Vec<String>,
    #[prop(skip_none)]
    pub author: Option<String>,
    pub limits: LimitsJs,
    pub sources: Vec<ProblemSourceJs>,
    /// Primary statement file (path, text, SHA-256 hash, readOnly).
    pub statement: WorkspaceFile,
    /// All existing statement files (`statement.md`, `statement.zh.md`, `statement.en.md`).
    pub statements: Vec<WorkspaceFile>,
    /// Editorial file (`problems/<id>/editorial.md`; `text` and `hash` empty if absent).
    pub editorial: WorkspaceFile,
    pub has_editorial: bool,
    pub samples: Vec<SamplePairJs>,
    /// Filenames under `problems/<id>/solutions/`, sorted.
    pub solutions: Vec<String>,
    /// Sorted IDs of contests whose `contest.toml` references this problem.
    pub referencing_contests: Vec<String>,
    /// JSON-serialized `[template-options.<tpl>]` map from `problem.toml`.
    pub template_options_json: String,
    #[prop(skip_none)]
    pub error: Option<String>,
}

// ---- Contest composition, overrides, stats, freeze & unfreeze --------------------------

/// One problem entry in a contest, with effective values, original library values, and
/// per-field override markers for rendering override indicators and restore buttons.
#[derive(Debug, Clone, Default, PartialEq, JsObject)]
pub struct ContestItemDetailJs {
    pub label: String,
    pub id: String,
    /// True when the referenced problem directory is missing on disk.
    pub missing: bool,
    /// Effective title (with contest override applied).
    pub title: String,
    /// Original library title.
    pub original_title: String,
    /// Effective limits (with contest override merged).
    pub limits: LimitsJs,
    /// Original library limits.
    pub original_limits: LimitsJs,
    /// Effective `show-metadata` (item override or contest default).
    pub show_metadata: bool,
    /// Contest default `show-metadata`.
    pub default_show_metadata: bool,
    /// Per-field override flags (`false` when frozen because the snapshot is authoritative).
    pub overridden_title: bool,
    pub overridden_time: bool,
    pub overridden_memory: bool,
    pub overridden_input: bool,
    pub overridden_output: bool,
    pub overridden_show_metadata: bool,
    /// Statement path relative to workspace (library, or snapshot when frozen).
    pub statement: String,
    /// Editorial path relative to workspace (library, or snapshot when frozen).
    pub editorial: String,
    #[prop(skip_none)]
    pub rating: Option<u32>,
    pub tags: Vec<String>,
    /// JSON-serialized effective merged problem-scoped template options.
    pub options_json: String,
    /// JSON-serialized library problem `problem.toml` template options.
    pub original_options_json: String,
    /// JSON-serialized contest item `[problem.options.<tpl>]` overrides.
    pub override_options_json: String,
    /// Static package freshness status (`"verified_fresh" | "stale" | "unverified" | "failed"`),
    /// or omitted for pure publishing problems / frozen snapshots.
    #[prop(skip_none)]
    pub package_status: Option<String>,
}

/// Full contest details for the contest composer and studio inspector.
#[derive(Debug, Clone, Default, PartialEq, JsObject)]
pub struct ContestDetailJs {
    pub id: String,
    pub name: String,
    #[prop(skip_none)]
    pub location: Option<String>,
    #[prop(skip_none)]
    pub date: Option<String>,
    #[prop(skip_none)]
    pub author: Option<String>,
    /// `"chinese"` or `"english"`.
    pub language: String,
    /// Explicit template ID on this contest (`null` means inheriting workspace default).
    #[prop(skip_none)]
    pub template: Option<String>,
    /// Effective resolved template ID (contest override -> workspace default -> `"olymp-classic"`).
    pub effective_template: String,
    /// True when `contests/<id>/template.typ` exists and takes precedence over `templates/*.typ`.
    pub has_contest_template: bool,
    /// JSON-serialized `TemplateInfo` evaluated from `contests/<id>/template.typ` when present.
    #[prop(skip_none)]
    pub contest_template_json: Option<String>,
    /// JSON-serialized `[template-options.<tpl>]` map from `contest.toml`.
    pub template_options_json: String,
    pub show_metadata: bool,
    /// Explicit `cover` value (`null` means auto: enabled when `items.length >= 2`).
    #[prop(skip_none)]
    pub cover: Option<bool>,
    /// Effective cover flag (`cover ?? items.length >= 2`).
    pub has_cover: bool,
    pub blank_pages: bool,
    pub frozen: bool,
    pub archived: bool,
    pub items: Vec<ContestItemDetailJs>,
    #[prop(skip_none)]
    pub error: Option<String>,
}

/// Difficulty bucket count for `ContestStatsJs`.
#[derive(Debug, Clone, Default, PartialEq, Eq, JsObject)]
pub struct RatingBucketJs {
    /// One of `"newbie"`, `"pupil"`, `"specialist"`, `"expert"`, `"candidate_master"`,
    /// `"master"`, `"grandmaster"`, `"unrated"`.
    pub tier: String,
    pub count: u32,
}

/// Tag frequency entry for `ContestStatsJs`.
#[derive(Debug, Clone, Default, PartialEq, Eq, JsObject)]
pub struct TagCountJs {
    pub tag: String,
    pub count: u32,
}

/// Composition statistics for a contest: 8-tier difficulty distribution and tag counts.
#[derive(Debug, Clone, Default, PartialEq, Eq, JsObject)]
pub struct ContestStatsJs {
    pub problem_count: u32,
    /// All 8 rating tiers in display order (`newbie`..`grandmaster`, then `unrated`).
    pub rating_distribution: Vec<RatingBucketJs>,
    /// Tags sorted by count descending, then tag name ascending.
    pub tag_counts: Vec<TagCountJs>,
    #[prop(skip_none)]
    pub error: Option<String>,
}

/// Result of freezing a contest snapshot (compiling statements + optional editorial PDFs).
#[derive(Debug, Clone, Default, PartialEq, JsObject)]
pub struct FreezeResultJs {
    pub ok: bool,
    pub frozen: bool,
    /// Snapshot PDF paths written relative to workspace.
    pub outputs: Vec<String>,
    pub diagnostics: Vec<PreviewDiagnostic>,
    #[prop(skip_none)]
    pub error: Option<String>,
    #[prop(skip_none)]
    pub code: Option<String>,
}

/// One problem's differences between a frozen contest snapshot and the live library.
#[derive(Debug, Clone, Default, PartialEq, Eq, JsObject)]
pub struct UnfreezeProblemDiffJs {
    pub label: String,
    pub problem_id: String,
    /// Difference kinds: `"statement_changed"`, `"samples_changed"`, `"editorial_changed"`,
    /// `"assets_changed"`, `"meta_changed"`, `"problem_deleted"`.
    pub kinds: Vec<String>,
    /// Relative paths inside the problem directory whose bytes differ, sorted.
    pub changed_files: Vec<String>,
}

/// Diff preview before unfreezing a frozen contest.
#[derive(Debug, Clone, Default, PartialEq, Eq, JsObject)]
pub struct UnfreezeDiffResultJs {
    pub ok: bool,
    pub diffs: Vec<UnfreezeProblemDiffJs>,
    #[prop(skip_none)]
    pub error: Option<String>,
    #[prop(skip_none)]
    pub code: Option<String>,
}

// ---- Cmd+K fuzzy jump & persisted preferences ------------------------------------------

/// One ranked jump target returned by `fuzzyJump`.
#[derive(Debug, Clone, Default, PartialEq, JsObject)]
pub struct FuzzyHitJs {
    /// `"problem"`, `"contest"`, or `"contest_item"` (a problem inside `activeContest`).
    pub kind: String,
    /// Problem ID or contest ID (for `"contest_item"`, the problem ID).
    pub id: String,
    /// Display title (with contest overrides applied for `"contest_item"`).
    pub title: String,
    /// Contest ID when `kind === "contest_item"`.
    #[prop(skip_none)]
    pub contest_id: Option<String>,
    /// Contest label (`"A"`, `"B"`, …) when `kind === "contest_item"`.
    #[prop(skip_none)]
    pub label: Option<String>,
    /// Stable visit key to pass to `recordVisit(key)` (`"problem:<id>"`, `"contest:<id>"`,
    /// or `"contest_item:<contestId>:<label>"`).
    pub visit_key: String,
    /// True when `kind === "contest"` and the contest is archived.
    pub archived: bool,
    pub score: f64,
}

/// Persisted desktop application preferences stored in `<dataDir>/prefs.json`.
#[derive(Debug, Clone, PartialEq, JsObject)]
pub struct AppPrefsJs {
    /// Recently opened workspace roots, most recent first.
    #[prop(default)]
    pub recent_workspaces: Vec<String>,
    /// Editor/preview split ratio (`0.1..=0.9`, default `0.5`).
    #[prop(default = 0.5)]
    pub split_ratio: f64,
    /// Last active navigation space (`"problems"`, `"contests"`, `"settings"`).
    #[prop(default = "problems".to_owned())]
    pub last_space: String,
    /// Last opened studio target kind (`"problem"` or `"contest"`).
    #[prop(skip_none)]
    pub last_target: Option<String>,
    /// Last opened studio target ID.
    #[prop(skip_none)]
    pub last_id: Option<String>,
    /// Last selected problem label inside a contest (`"A"`, `"B"`, …).
    #[prop(skip_none)]
    pub last_label: Option<String>,
    /// Last active book (`"statements"` or `"editorial"`).
    #[prop(default = "statements".to_owned())]
    pub last_book: String,
    /// Whether the right-hand inspector/properties panel is open.
    #[prop(default = true)]
    pub props_open: bool,
    /// UI language preference (`"system"`, `"zh-CN"`, or `"en"`).
    #[prop(skip_none)]
    pub language: Option<String>,
    /// Optional explicit C++ compiler path override for local Polygon builds.
    #[prop(skip_none)]
    pub cxx_override: Option<String>,
    /// Whether the library left group sidebar is open (default `true`).
    #[prop(default = true)]
    pub library_sidebar_open: bool,
    /// Library dynamic grouping mode (`"none" | "group" | "rating" | "platform" | "contest" | "package_status"`).
    #[prop(skip_none)]
    pub library_group_by: Option<String>,
    /// Optional GitHub raw mirror URL prefix for Library Checker snippet verification.
    #[prop(skip_none)]
    pub library_checker_mirror_url: Option<String>,
    /// Optional local filesystem path to a `library-checker-problems` clone.
    #[prop(skip_none)]
    pub library_checker_local_repo_path: Option<String>,
}

impl Default for AppPrefsJs {
    fn default() -> Self {
        Self {
            recent_workspaces: Vec::new(),
            split_ratio: 0.5,
            last_space: "problems".to_owned(),
            last_target: None,
            last_id: None,
            last_label: None,
            last_book: "statements".to_owned(),
            props_open: true,
            language: None,
            cxx_override: None,
            library_sidebar_open: true,
            library_group_by: None,
            library_checker_mirror_url: None,
            library_checker_local_repo_path: None,
        }
    }
}

/// Optional localized labels passed from JS to native file/folder dialogs.
#[derive(Debug, Clone, Default, PartialEq, Eq, JsObject)]
pub struct DialogLabelsJs {
    #[prop(default)]
    pub title: Option<String>,
    #[prop(default)]
    pub button_label: Option<String>,
    #[prop(default)]
    pub filter_label: Option<String>,
}

// ---- Tri-state patch types (omitted/undefined = keep, null = clear, value = set) -------

/// Partial update for `problem.toml` via `toml_edit`.
///
/// For clearable fields (`rating`, `author`, `time`, `memory`, `input`, `output`), omitting
/// the key (`undefined`) leaves it untouched, passing `null` removes the key, and passing
/// a value replaces it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ProblemMetaPatchJs {
    pub title: Option<String>,
    pub rating: Patch<u32>,
    pub tags: Option<Vec<String>>,
    pub author: Patch<String>,
    pub time: Patch<String>,
    pub memory: Patch<String>,
    pub input: Patch<String>,
    pub output: Patch<String>,
    pub sources: Option<Vec<ProblemSourceJs>>,
    pub template_options_json: Option<String>,
}

impl JsValue for ProblemMetaPatchJs {
    fn ts_type(types: &mut TsTypes) -> String {
        types.declare("ProblemMetaPatchJs", |types| {
            let source_ty = ProblemSourceJs::ts_type(types);
            format!(
                "/** Partial update for `problem.toml` via `toml_edit`. Omitting a nullable field leaves it untouched; passing `null` clears it. */\n\
export interface ProblemMetaPatchJs {{\n  \
  title?: string;\n  \
  /** Rating (`null` clears). */\n  \
  rating?: number | null;\n  \
  tags?: (string)[];\n  \
  /** Author (`null` clears). */\n  \
  author?: string | null;\n  \
  /** Time limit (`null` clears). */\n  \
  time?: string | null;\n  \
  /** Memory limit (`null` clears). */\n  \
  memory?: string | null;\n  \
  /** Input filename (`null` clears). */\n  \
  input?: string | null;\n  \
  /** Output filename (`null` clears). */\n  \
  output?: string | null;\n  \
  sources?: ({source_ty})[];\n  \
  /** JSON-encoded namespaced template options patch: `{{ [tpl]: {{ [key]: value | null }} }}`. */\n  \
  templateOptionsJson?: string;\n\
}}"
            )
        })
    }

    fn from_js(value: JsIn<'_>) -> Result<Self, PropError> {
        if value.kind() != JsKind::Object {
            return Err(value.expected("an object"));
        }
        Ok(Self {
            title: read_opt_field(value, "title")?,
            rating: read_patch_field(value, "rating")?,
            tags: read_opt_field(value, "tags")?,
            author: read_patch_field(value, "author")?,
            time: read_patch_field(value, "time")?,
            memory: read_patch_field(value, "memory")?,
            input: read_patch_field(value, "input")?,
            output: read_patch_field(value, "output")?,
            sources: read_opt_field(value, "sources")?,
            template_options_json: read_opt_field(value, "templateOptionsJson")?,
        })
    }

    fn to_js<'a>(&self, host: &'a dyn JsHost) -> Result<JsOwned<'a>, PropError> {
        let owned = JsOwned::new(host, host.new_object()?);
        let obj = owned.get().raw();
        write_opt_field(host, obj, "title", &self.title)?;
        write_patch_field(host, obj, "rating", &self.rating)?;
        write_opt_field(host, obj, "tags", &self.tags)?;
        write_patch_field(host, obj, "author", &self.author)?;
        write_patch_field(host, obj, "time", &self.time)?;
        write_patch_field(host, obj, "memory", &self.memory)?;
        write_patch_field(host, obj, "input", &self.input)?;
        write_patch_field(host, obj, "output", &self.output)?;
        write_opt_field(host, obj, "sources", &self.sources)?;
        write_opt_field(
            host,
            obj,
            "templateOptionsJson",
            &self.template_options_json,
        )?;
        Ok(owned)
    }
}

/// Partial update for `contest.toml` metadata via `toml_edit`.
///
/// Omitting a nullable field (`location`, `date`, `author`, `cover`, `template`) leaves it untouched;
/// passing `null` removes the key (e.g. resetting `cover` to auto or `template` to workspace default).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ContestMetaPatchJs {
    pub name: Option<String>,
    pub location: Patch<String>,
    pub date: Patch<String>,
    pub author: Patch<String>,
    pub language: Option<String>,
    pub show_metadata: Option<bool>,
    pub cover: Patch<bool>,
    pub blank_pages: Option<bool>,
    pub template: Patch<String>,
    pub template_options_json: Option<String>,
}

impl JsValue for ContestMetaPatchJs {
    fn ts_type(types: &mut TsTypes) -> String {
        types.declare("ContestMetaPatchJs", |_| {
            "/** Partial update for `contest.toml` metadata via `toml_edit`. Omitting a nullable field leaves it untouched; passing `null` clears it. */\n\
export interface ContestMetaPatchJs {\n  \
  name?: string;\n  \
  /** Location (`null` clears). */\n  \
  location?: string | null;\n  \
  /** Date (`null` clears). */\n  \
  date?: string | null;\n  \
  /** Cover author line (`null` clears). */\n  \
  author?: string | null;\n  \
  /** `\"chinese\"` or `\"english\"`. */\n  \
  language?: string;\n  \
  showMetadata?: boolean;\n  \
  /** Explicit cover toggle (`null` resets to auto: enabled when 2+ problems). */\n  \
  cover?: boolean | null;\n  \
  blankPages?: boolean;\n  \
  /** Template ID (`null` restores workspace default). */\n  \
  template?: string | null;\n  \
  /** JSON-encoded namespaced template options patch: `{ [tpl]: { [key]: value | null } }`. */\n  \
  templateOptionsJson?: string;\n\
}"
            .to_owned()
        })
    }

    fn from_js(value: JsIn<'_>) -> Result<Self, PropError> {
        if value.kind() != JsKind::Object {
            return Err(value.expected("an object"));
        }
        Ok(Self {
            name: read_opt_field(value, "name")?,
            location: read_patch_field(value, "location")?,
            date: read_patch_field(value, "date")?,
            author: read_patch_field(value, "author")?,
            language: read_opt_field(value, "language")?,
            show_metadata: read_opt_field(value, "showMetadata")?,
            cover: read_patch_field(value, "cover")?,
            blank_pages: read_opt_field(value, "blankPages")?,
            template: read_patch_field(value, "template")?,
            template_options_json: read_opt_field(value, "templateOptionsJson")?,
        })
    }

    fn to_js<'a>(&self, host: &'a dyn JsHost) -> Result<JsOwned<'a>, PropError> {
        let owned = JsOwned::new(host, host.new_object()?);
        let obj = owned.get().raw();
        write_opt_field(host, obj, "name", &self.name)?;
        write_patch_field(host, obj, "location", &self.location)?;
        write_patch_field(host, obj, "date", &self.date)?;
        write_patch_field(host, obj, "author", &self.author)?;
        write_opt_field(host, obj, "language", &self.language)?;
        write_opt_field(host, obj, "showMetadata", &self.show_metadata)?;
        write_patch_field(host, obj, "cover", &self.cover)?;
        write_opt_field(host, obj, "blankPages", &self.blank_pages)?;
        write_patch_field(host, obj, "template", &self.template)?;
        write_opt_field(
            host,
            obj,
            "templateOptionsJson",
            &self.template_options_json,
        )?;
        Ok(owned)
    }
}

/// Partial update for one contest problem's metadata overrides (`setOverride`).
///
/// Omitting a field (`undefined`) leaves it untouched; passing `null` removes that override
/// (restoring the library/contest default); passing a value equal to the library/contest
/// default automatically deletes the override.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OverridePatchJs {
    pub title: Patch<String>,
    pub time: Patch<String>,
    pub memory: Patch<String>,
    pub input: Patch<String>,
    pub output: Patch<String>,
    pub show_metadata: Patch<bool>,
    pub options_json: Option<String>,
}

impl JsValue for OverridePatchJs {
    fn ts_type(types: &mut TsTypes) -> String {
        types.declare("OverridePatchJs", |_| {
            "/** Partial update for one contest item's metadata overrides (`setOverride`). Omitting a field leaves it untouched; passing `null` clears that override; passing the original library/contest value auto-deletes the override. */\n\
export interface OverridePatchJs {\n  \
  title?: string | null;\n  \
  time?: string | null;\n  \
  memory?: string | null;\n  \
  input?: string | null;\n  \
  output?: string | null;\n  \
  showMetadata?: boolean | null;\n  \
  /** JSON-encoded namespaced problem-scope template options patch: `{ [tpl]: { [key]: value | null } }`. */\n  \
  optionsJson?: string;\n\
}"
            .to_owned()
        })
    }

    fn from_js(value: JsIn<'_>) -> Result<Self, PropError> {
        if value.kind() != JsKind::Object {
            return Err(value.expected("an object"));
        }
        Ok(Self {
            title: read_patch_field(value, "title")?,
            time: read_patch_field(value, "time")?,
            memory: read_patch_field(value, "memory")?,
            input: read_patch_field(value, "input")?,
            output: read_patch_field(value, "output")?,
            show_metadata: read_patch_field(value, "showMetadata")?,
            options_json: read_opt_field(value, "optionsJson")?,
        })
    }

    fn to_js<'a>(&self, host: &'a dyn JsHost) -> Result<JsOwned<'a>, PropError> {
        let owned = JsOwned::new(host, host.new_object()?);
        let obj = owned.get().raw();
        write_patch_field(host, obj, "title", &self.title)?;
        write_patch_field(host, obj, "time", &self.time)?;
        write_patch_field(host, obj, "memory", &self.memory)?;
        write_patch_field(host, obj, "input", &self.input)?;
        write_patch_field(host, obj, "output", &self.output)?;
        write_patch_field(host, obj, "showMetadata", &self.show_metadata)?;
        write_opt_field(host, obj, "optionsJson", &self.options_json)?;
        Ok(owned)
    }
}

fn read_opt_field<T: JsValue>(obj: JsIn<'_>, key: &str) -> Result<Option<T>, PropError> {
    let field = obj.field(key)?;
    let v = field.get();
    if v.is_nullish() {
        Ok(None)
    } else {
        T::from_js(v)
            .map(Some)
            .map_err(|e| PropError(format!("`{key}`: {e}")))
    }
}

fn read_patch_field<T: JsValue>(obj: JsIn<'_>, key: &str) -> Result<Patch<T>, PropError> {
    let field = obj.field(key)?;
    let v = field.get();
    match v.kind() {
        JsKind::Undefined => Ok(Patch::Keep),
        JsKind::Null => Ok(Patch::Clear),
        _ => T::from_js(v)
            .map(Patch::Set)
            .map_err(|e| PropError(format!("`{key}`: {e}"))),
    }
}

fn write_opt_field<T: JsValue>(
    host: &dyn JsHost,
    obj: nsh_core::element::JsRaw,
    key: &str,
    val: &Option<T>,
) -> Result<(), PropError> {
    if let Some(v) = val {
        host.set_field(obj, key, v.to_js(host)?.into_raw())?;
    }
    Ok(())
}

fn write_patch_field<T: JsValue>(
    host: &dyn JsHost,
    obj: nsh_core::element::JsRaw,
    key: &str,
    val: &Patch<T>,
) -> Result<(), PropError> {
    match val {
        Patch::Keep => {}
        Patch::Clear => host.set_field(obj, key, host.null())?,
        Patch::Set(v) => host.set_field(obj, key, v.to_js(host)?.into_raw())?,
    }
    Ok(())
}

// ---- Editor styling -----------------------------------------------------------------

/// One styled span of a Markdown source (`markdownSpans`), as editor positions: 0-based
/// lines and UTF-16 columns, `end` not included.
#[derive(Debug, Clone, Default, PartialEq, JsObject)]
pub struct MarkdownSpanJs {
    /// `heading`, `strong`, `emphasis`, `strikethrough`, `inlineMath`, `displayMath`,
    /// `mathCommand`, `mathOperator`, `code`, `codeBlock`, `codeComment`, `codeString`,
    /// `codeKeyword`, `codeNumber`, `link`, `url`, `table` or `markup`.
    pub kind: String,
    /// A heading's level (1–6); 0 otherwise.
    pub level: u32,
    pub start_line: u32,
    pub start_column: u32,
    pub end_line: u32,
    pub end_column: u32,
}

// ---- Asset library, cold-start snapshot & diagnostics -------------------------------

/// One exact Markdown reference to a problem asset.
#[derive(Debug, Clone, Default, PartialEq, JsObject)]
pub struct AssetReferenceJs {
    /// Workspace-relative file path (e.g. `"problems/a/statement.md"`).
    pub file: String,
    /// 1-based line number.
    pub line: u32,
    /// Optional `{width=NN%}` percentage.
    #[prop(skip_none)]
    pub width_pct: Option<f64>,
}

/// One image/asset file inside a problem's `assets/` directory.
#[derive(Debug, Clone, Default, PartialEq, JsObject)]
pub struct ProblemAssetJs {
    pub name: String,
    /// Path relative to the problem directory, e.g. `"assets/fig.png"`.
    pub rel_path: String,
    /// Absolute path on disk.
    pub abs_path: String,
    /// Absolute path to a cached 336px PNG thumbnail under `.xcpc/thumbs/`, if available.
    #[prop(skip_none)]
    pub thumb_path: Option<String>,
    /// File size in bytes.
    pub bytes: f64,
    /// Image pixel width when readable (PNG/JPEG/WEBP/SVG).
    #[prop(skip_none)]
    pub width: Option<u32>,
    /// Image pixel height when readable (PNG/JPEG/WEBP/SVG).
    #[prop(skip_none)]
    pub height: Option<u32>,
    /// Whether `statement*.md` or `editorial.md` currently references this asset.
    pub referenced: bool,
    /// Exact Markdown reference locations for this asset.
    pub references: Vec<AssetReferenceJs>,
}

/// Result of batch-cleaning unreferenced assets in a problem.
#[derive(Debug, Clone, Default, PartialEq, JsObject)]
pub struct CleanUnusedAssetsResultJs {
    pub ok: bool,
    /// Filenames removed.
    pub removed: Vec<String>,
    /// Total bytes freed.
    pub freed_bytes: f64,
    #[prop(skip_none)]
    pub error: Option<String>,
    #[prop(skip_none)]
    pub code: Option<String>,
}

/// Restored cold-start preview snapshot from `<dataDir>/snapshots/`.
#[derive(Debug, Clone, Default, PartialEq, JsObject)]
pub struct ColdStartSnapshotJs {
    /// Raw JSON metadata string recorded at save time.
    pub meta_json: String,
    pub workspace: String,
    pub target: String,
    pub id: String,
    pub book: String,
    pub zoom: f64,
    pub scroll_y: f64,
    /// Pre-registered `PreviewPage[]` ready for `<PageImage layers={...} />`.
    pub pages: Vec<PreviewPage>,
}

/// Build and environment diagnostics for the About view (`Preview.aboutInfo()`).
#[derive(Debug, Clone, Default, PartialEq, Eq, JsObject)]
pub struct AboutInfoJs {
    pub version: String,
    pub commit: String,
    pub build_date: String,
    pub is_dev: bool,
    pub os: String,
    pub arch: String,
    pub font_source: String,
}

/// Offline performance trace recorder status (`Preview.traceStatus()`).
#[derive(Debug, Clone, Default, JsObject)]
pub struct TraceStatusJs {
    pub enabled: bool,
    pub event_count: u32,
    pub dropped_count: u32,
    pub rust_now_us: f64,
}

/// Result of exporting a merged Rust + JS Chrome Trace JSON file.
#[derive(Debug, Clone, Default, JsObject)]
pub struct TraceExportResultJs {
    pub ok: bool,
    pub cancelled: bool,
    pub path: Option<String>,
    pub bytes: u32,
    pub error: Option<String>,
}

// ---- Phase 3A: Local Polygon toolchain & package build ---------------------------------

/// One detected C++ compiler binary on the host.
#[derive(Debug, Clone, Default, PartialEq, Eq, JsObject)]
pub struct DetectedCompilerJs {
    pub name: String,
    pub path: String,
    pub version: String,
    /// `"gpp"`, `"clangpp"`, or `"other"`.
    pub kind: String,
}

/// Structured host C++ toolchain detection result for Settings -> Toolchain.
#[derive(Debug, Clone, Default, PartialEq, Eq, JsObject)]
pub struct ToolchainStatusJs {
    pub compilers: Vec<DetectedCompilerJs>,
    #[prop(skip_none)]
    pub active: Option<DetectedCompilerJs>,
    #[prop(skip_none)]
    pub override_path: Option<String>,
    #[prop(skip_none)]
    pub env_override: Option<String>,
    pub install_guide: String,
    /// Copyable install commands for this system only (Settings → Toolchain).
    pub install_commands: Vec<InstallCommandJs>,
    #[prop(skip_none)]
    pub error: Option<String>,
}

/// One copyable way to install a C++ compiler on this system.
#[derive(Debug, Clone, Default, PartialEq, Eq, JsObject)]
pub struct InstallCommandJs {
    pub label: String,
    pub command: String,
}

/// Read-only inspection result for a problem package (`inspectPackage`).
#[derive(Debug, Clone, Default, PartialEq, Eq, JsObject)]
pub struct PackageInspectionResultJs {
    pub ok: bool,
    pub has_package: bool,
    /// `"not_a_package"`, `"verified_fresh"`, `"stale"`, `"unverified"`, or `"failed"`.
    pub freshness_status: String,
    /// JSON-serialized `xcpc_judge::PackageInspection` payload (empty on error).
    pub inspection_json: String,
    #[prop(skip_none)]
    pub error: Option<String>,
}

/// Result of scaffolding a problem package (`initPackage`).
#[derive(Debug, Clone, Default, PartialEq, Eq, JsObject)]
pub struct PackageInitResultJs {
    pub ok: bool,
    pub created_files: Vec<String>,
    pub skipped_files: Vec<String>,
    pub updated_problem_toml: bool,
    /// Refreshed JSON-serialized `xcpc_judge::PackageInspection` when `ok` is true.
    #[prop(skip_none)]
    pub inspection_json: Option<String>,
    #[prop(skip_none)]
    pub error: Option<String>,
    #[prop(skip_none)]
    pub code: Option<String>,
}

/// Snapshot of the background package build runner (`packageBuildStatus`).
#[derive(Debug, Clone, Default, PartialEq, Eq, JsObject)]
pub struct PackageBuildStatusJs {
    pub seq: u32,
    pub running: bool,
    #[prop(skip_none)]
    pub slug: Option<String>,
    /// `"debug"` or `"verify"`.
    #[prop(skip_none)]
    pub mode: Option<String>,
    pub cancelled: bool,
    /// `"compiling"`, `"self_testing"`, `"generating"`, `"validating"`, `"running"`, or `"finished"`.
    #[prop(skip_none)]
    pub phase: Option<String>,
    #[prop(skip_none)]
    pub detail: Option<String>,
    pub completed: u32,
    pub total: u32,
    /// JSON-serialized `xcpc_judge::BuildReport` once finished.
    #[prop(skip_none)]
    pub report_json: Option<String>,
    /// JSON-serialized array of `xcpc_judge::MatrixCell` completed so far during an active run.
    #[prop(skip_none)]
    pub live_cells_json: Option<String>,
    #[prop(skip_none)]
    pub error: Option<String>,
}

/// Safe bounded head/tail preview of a single test case (`inspectTestCase`).
#[derive(Debug, Clone, Default, PartialEq, JsObject)]
pub struct TestCasePreviewJs {
    pub ok: bool,
    pub index: u32,
    pub name: String,
    #[prop(skip_none)]
    pub tag: Option<String>,
    pub is_sample: bool,
    #[prop(skip_none)]
    pub gen_cmd: Option<String>,
    pub available_on_disk: bool,
    pub size_bytes: f64,
    pub total_lines: u32,
    #[prop(skip_none)]
    pub total_lines_exact: Option<u32>,
    #[prop(skip_none)]
    pub omitted_bytes: Option<f64>,
    pub head: String,
    pub tail: String,
    pub truncated: bool,
    pub ans_available: bool,
    #[prop(skip_none)]
    pub ans_source: Option<String>,
    pub ans_size_bytes: f64,
    pub ans_head: String,
    pub ans_tail: String,
    pub ans_truncated: bool,
    #[prop(skip_none)]
    pub ans_total_lines: Option<u32>,
    pub std_output_available: bool,
    pub std_output_size_bytes: f64,
    pub std_output_head: String,
    pub std_output_tail: String,
    pub std_output_truncated: bool,
    #[prop(skip_none)]
    pub rule_index: Option<u32>,
    #[prop(skip_none)]
    pub file_rel: Option<String>,
    #[prop(skip_none)]
    pub error: Option<String>,
}

/// Bounded read result for the lightweight in-app quick-editor (`readPackageFile`, max 256 KiB).
#[derive(Debug, Clone, Default, PartialEq, JsObject)]
pub struct PackageFileContentJs {
    pub ok: bool,
    pub content: String,
    pub content_hash: String,
    pub size_bytes: f64,
    pub too_large: bool,
    #[prop(skip_none)]
    pub error: Option<String>,
    #[prop(skip_none)]
    pub code: Option<String>,
}

/// Result of conflict-checked atomic save for a package file (`savePackageFile`).
#[derive(Debug, Clone, Default, PartialEq, Eq, JsObject)]
pub struct SavePackageFileResultJs {
    pub ok: bool,
    pub conflict: bool,
    #[prop(skip_none)]
    pub content_hash: Option<String>,
    #[prop(skip_none)]
    pub error: Option<String>,
    #[prop(skip_none)]
    pub code: Option<String>,
}

/// Result of creating a new package source file (`createPackageSource`).
#[derive(Debug, Clone, Default, PartialEq, Eq, JsObject)]
pub struct PackageCreateResultJs {
    pub ok: bool,
    pub rel_path: String,
    #[prop(skip_none)]
    pub error: Option<String>,
    #[prop(skip_none)]
    pub code: Option<String>,
}

/// Result of adding a hand-written or generator `[[test]]` rule.
#[derive(Debug, Clone, Default, PartialEq, Eq, JsObject)]
pub struct AddedTestResultJs {
    pub ok: bool,
    pub rule_index: u32,
    #[prop(skip_none)]
    pub file_rel: Option<String>,
    #[prop(skip_none)]
    pub name: Option<String>,
    #[prop(skip_none)]
    pub tag: Option<String>,
    #[prop(skip_none)]
    pub error: Option<String>,
    #[prop(skip_none)]
    pub code: Option<String>,
}

/// Result of normalizing CRLF / BOM / missing trailing newline across hand-written inputs.
#[derive(Debug, Clone, Default, PartialEq, Eq, JsObject)]
pub struct NormalizeDirtyResultJs {
    pub ok: bool,
    pub fixed: Vec<String>,
    #[prop(skip_none)]
    pub error: Option<String>,
    #[prop(skip_none)]
    pub code: Option<String>,
}

// ---- Phase 3B: Polygon import & OJ package export --------------------------------------

/// Non-fatal warning emitted during Polygon import or OJ package export.
#[derive(Debug, Clone, Default, PartialEq, Eq, JsObject)]
pub struct PackageWarningJs {
    pub code: String,
    pub message: String,
    #[prop(skip_none)]
    pub path: Option<String>,
}

/// Result of importing a Polygon single-problem or contest ZIP (`pickAndImportPolygon` / `importPolygon`).
#[derive(Debug, Clone, Default, PartialEq, Eq, JsObject)]
pub struct PolygonImportResultJs {
    pub ok: bool,
    pub cancelled: bool,
    /// `"problem"` or `"contest"` when `ok` is true; empty otherwise.
    pub kind: String,
    /// Imported problem slug (for single problem) or contest ID (for contest).
    pub id: String,
    /// Imported problem title or contest name.
    pub title: String,
    /// All imported problem slugs.
    pub problem_ids: Vec<String>,
    /// Non-fatal structured warnings (e.g. interactive, non-C++, subtasks).
    pub warnings: Vec<PackageWarningJs>,
    /// Human-readable error message when `ok` is false.
    #[prop(skip_none)]
    pub error: Option<String>,
    /// Stable machine-readable error code when `ok` is false.
    #[prop(skip_none)]
    pub code: Option<String>,
    /// Raw technical detail for "Copy Details" when `ok` is false.
    #[prop(skip_none)]
    pub detail: Option<String>,
}

/// Result of exporting a DOMjudge or Hydro OJ package (`exportOjPackage`).
#[derive(Debug, Clone, Default, PartialEq, Eq, JsObject)]
pub struct ExportOjResultJs {
    pub ok: bool,
    /// `"domjudge"` or `"hydro"`.
    pub format: String,
    /// Absolute paths of exported ZIP files.
    pub paths: Vec<String>,
    /// Primary file or directory path to pass to `revealInFolder`.
    pub reveal_path: String,
    /// Non-fatal structured warnings.
    pub warnings: Vec<PackageWarningJs>,
    /// Human-readable error message when `ok` is false.
    #[prop(skip_none)]
    pub error: Option<String>,
    /// Stable machine-readable error code when `ok` is false.
    #[prop(skip_none)]
    pub code: Option<String>,
    /// Raw technical detail for "Copy Details" when `ok` is false.
    #[prop(skip_none)]
    pub detail: Option<String>,
}

// ---- Phase 3B+: Batch mutations, delete & async batch task streaming -------------------

/// Per-problem outcome in a synchronous batch mutation (`batchSetGroup`, `batchEditTags`,
/// `batchSetRating`, `batchAddToContest`).
#[derive(Debug, Clone, Default, PartialEq, Eq, JsObject)]
pub struct BatchItemResultJs {
    pub id: String,
    /// `"ok"`, `"skipped"`, or `"failed"`.
    pub status: String,
    #[prop(skip_none)]
    pub code: Option<String>,
    #[prop(skip_none)]
    pub message: Option<String>,
}

/// Aggregated result of a synchronous batch mutation.
#[derive(Debug, Clone, Default, PartialEq, Eq, JsObject)]
pub struct BatchMutationResultJs {
    pub ok: bool,
    pub total: u32,
    pub succeeded: u32,
    pub skipped: u32,
    pub failed: u32,
    pub items: Vec<BatchItemResultJs>,
    #[prop(skip_none)]
    pub error: Option<String>,
    #[prop(skip_none)]
    pub code: Option<String>,
}

/// Per-problem outcome in `batchDeleteProblems`.
#[derive(Debug, Clone, Default, PartialEq, Eq, JsObject)]
pub struct BatchDeleteItemJs {
    pub id: String,
    /// `"ok"`, `"skipped"`, or `"failed"`.
    pub status: String,
    #[prop(skip_none)]
    pub code: Option<String>,
    #[prop(skip_none)]
    pub message: Option<String>,
    #[prop(default)]
    pub referencing_contests: Vec<String>,
}

/// Aggregated result of `batchDeleteProblems`.
#[derive(Debug, Clone, Default, PartialEq, Eq, JsObject)]
pub struct BatchDeleteResultJs {
    pub ok: bool,
    pub total: u32,
    pub deleted: u32,
    pub skipped: u32,
    pub failed: u32,
    pub items: Vec<BatchDeleteItemJs>,
    #[prop(skip_none)]
    pub error: Option<String>,
    #[prop(skip_none)]
    pub code: Option<String>,
}

/// Request payload for `startBatchExport`.
#[derive(Debug, Clone, Default, PartialEq, Eq, JsObject)]
pub struct BatchExportRequestJs {
    /// `"domjudge"`, `"hydro"`, or `"merged_pdf"`.
    pub format: String,
    pub problem_ids: Vec<String>,
    /// Optional booklet title (used by `"merged_pdf"`).
    #[prop(skip_none)]
    pub title: Option<String>,
    /// Optional language (`"chinese"` / `"zh"` or `"english"` / `"en"`).
    #[prop(skip_none)]
    pub language: Option<String>,
    /// Optional book kind (`"statements"` or `"editorial"`, used by `"merged_pdf"`).
    #[prop(skip_none)]
    pub book: Option<String>,
    /// Optional output path (workspace-relative for in-workspace export, or absolute path
    /// for `"merged_pdf"` external save).
    #[prop(skip_none)]
    pub output_path: Option<String>,
}

/// Result of starting an async background batch task (`startBatchBuild` / `startBatchExport`).
#[derive(Debug, Clone, Default, PartialEq, Eq, JsObject)]
pub struct BatchStartResultJs {
    pub ok: bool,
    /// Monotonic task ID to pass to `nextBatchEvent(taskId)`; `0` on failure.
    pub task_id: u32,
    #[prop(skip_none)]
    pub error: Option<String>,
    #[prop(skip_none)]
    pub code: Option<String>,
}

/// Per-problem progress/result entry in an async batch build or export task.
#[derive(Debug, Clone, Default, PartialEq, JsObject)]
pub struct BatchTaskItemJs {
    pub problem_id: String,
    /// `"pending"`, `"running"`, `"passed"`, `"exported"`, `"failed"`, `"skipped"`,
    /// `"error"`, or `"cancelled"`.
    pub status: String,
    pub verified: bool,
    pub elapsed_ms: f64,
    #[prop(skip_none)]
    pub phase: Option<String>,
    #[prop(skip_none)]
    pub detail: Option<String>,
    #[prop(skip_none)]
    pub code: Option<String>,
    #[prop(skip_none)]
    pub message: Option<String>,
    #[prop(skip_none)]
    pub output_path: Option<String>,
}

/// Snapshot of the active or most recently completed background batch task (`batchTaskStatus`).
#[derive(Debug, Clone, Default, PartialEq, JsObject)]
pub struct BatchTaskSnapshotJs {
    pub task_id: u32,
    pub running: bool,
    /// `"idle"`, `"build_verify"`, `"build_debug"`, `"export_domjudge"`, `"export_hydro"`,
    /// or `"export_merged_pdf"`.
    pub kind: String,
    pub cancelled: bool,
    pub total: u32,
    pub completed: u32,
    pub succeeded: u32,
    pub failed: u32,
    pub skipped: u32,
    pub cancelled_count: u32,
    pub elapsed_ms: f64,
    #[prop(skip_none)]
    pub current_problem_id: Option<String>,
    #[prop(skip_none)]
    pub current_phase: Option<String>,
    #[prop(skip_none)]
    pub current_detail: Option<String>,
    /// Output directory or PDF file path for `revealInFolder`.
    #[prop(skip_none)]
    pub output_path: Option<String>,
    pub items: Vec<BatchTaskItemJs>,
    #[prop(skip_none)]
    pub error: Option<String>,
    #[prop(skip_none)]
    pub code: Option<String>,
}

/// Streaming event yielded by `nextBatchEvent(taskId)`.
#[derive(Debug, Clone, Default, PartialEq, JsObject)]
pub struct BatchTaskEventJs {
    /// `"problem_started"`, `"problem_progress"`, `"problem_finished"`, `"cancelled"`,
    /// `"all_finished"`, `"error"`, or `"closed"`.
    pub kind: String,
    pub task_id: u32,
    #[prop(skip_none)]
    pub problem_id: Option<String>,
    pub index: u32,
    pub total: u32,
    #[prop(skip_none)]
    pub phase: Option<String>,
    #[prop(skip_none)]
    pub detail: Option<String>,
    #[prop(skip_none)]
    pub item: Option<BatchTaskItemJs>,
    pub snapshot: BatchTaskSnapshotJs,
    #[prop(skip_none)]
    pub error: Option<String>,
    #[prop(skip_none)]
    pub code: Option<String>,
}

// ---- Phase 3D: Sanitizer, external trial run, adopt & contest readiness audit ----------

/// Result of picking one or more files via the native file dialog.
#[derive(Debug, Clone, Default, PartialEq, Eq, JsObject)]
pub struct PickedPathsJs {
    pub cancelled: bool,
    pub paths: Vec<String>,
    #[prop(skip_none)]
    pub error: Option<String>,
}

/// Generic JSON + optional Markdown report payload returned by Phase 3D Core operations
/// (`loadSanitizerReport`, `sanitizeProblem`, `runExternalProblemSolutions`,
/// `runExternalContestSolutions`, `auditContest`).
#[derive(Debug, Clone, Default, PartialEq, Eq, JsObject)]
pub struct JsonReportResultJs {
    pub ok: bool,
    #[prop(skip_none)]
    pub report_json: Option<String>,
    #[prop(skip_none)]
    pub markdown: Option<String>,
    #[prop(skip_none)]
    pub error: Option<String>,
    #[prop(skip_none)]
    pub code: Option<String>,
}

impl BatchTaskEventJs {
    pub(crate) fn closed(task_id: u32, snapshot: BatchTaskSnapshotJs) -> Self {
        xcpc_app::BatchTaskEventJs::closed(task_id, snapshot.into()).into()
    }
}

impl JsonReportResultJs {
    pub(crate) fn err(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            ok: false,
            code: Some(code.into()),
            error: Some(message.into()),
            report_json: None,
            markdown: None,
        }
    }
}

/// Result of adopting an external `.cpp` solution into `problems/<slug>/solutions/`.
#[derive(Debug, Clone, Default, PartialEq, Eq, JsObject)]
pub struct AdoptSolutionResultJs {
    pub ok: bool,
    pub problem_id: String,
    pub solution_name: String,
    pub target_rel_path: String,
    pub updated_problem_toml: bool,
    #[prop(skip_none)]
    pub error: Option<String>,
    #[prop(skip_none)]
    pub code: Option<String>,
}

/// Result of saving a Markdown report via the native save dialog.
#[derive(Debug, Clone, Default, PartialEq, Eq, JsObject)]
pub struct SaveReportResultJs {
    pub ok: bool,
    pub cancelled: bool,
    #[prop(skip_none)]
    pub path: Option<String>,
    #[prop(skip_none)]
    pub error: Option<String>,
}

// ---- Phase 4A: Plugins, OJ Sync & Training Status --------------------------------------

/// Result of resolving an external OJ problem URL via enabled `oj_source` plugins.
#[derive(Debug, Clone, Default, PartialEq, Eq, JsObject)]
pub struct ResolvedProblemUrlJs {
    pub ok: bool,
    #[prop(default)]
    pub platform: String,
    #[prop(default)]
    pub problem_key: String,
    #[prop(default)]
    pub suggested_slug: String,
    #[prop(skip_none)]
    pub title: Option<String>,
    #[prop(default)]
    pub tags: Vec<String>,
    #[prop(skip_none)]
    pub rating: Option<u32>,
    #[prop(default)]
    pub url: String,
    #[prop(skip_none)]
    pub existing_problem_id: Option<String>,
    #[prop(skip_none)]
    pub error: Option<String>,
}

/// Result of creating (or deduplicating) a problem from a resolved OJ URL.
#[derive(Debug, Clone, Default, PartialEq, Eq, JsObject)]
pub struct CreateProblemFromUrlResultJs {
    pub ok: bool,
    #[prop(default)]
    pub problem_id: String,
    pub existed: bool,
    #[prop(skip_none)]
    pub code: Option<String>,
    #[prop(skip_none)]
    pub error: Option<String>,
}

/// Credential input specification for a plugin in Settings -> Plugins.
#[derive(Debug, Clone, Default, PartialEq, Eq, JsObject)]
pub struct PluginCredentialFieldJs {
    pub name: String,
    pub label: String,
    pub secret: bool,
    pub has_value: bool,
    #[prop(skip_none)]
    pub description: Option<String>,
    #[prop(skip_none)]
    pub placeholder: Option<String>,
}

/// One discovered plugin (built-in, local override, or local install) in Settings -> Plugins.
#[derive(Debug, Clone, Default, PartialEq, Eq, JsObject)]
pub struct PluginSummaryJs {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    /// `"builtin" | "local_override" | "local"`.
    pub origin: String,
    pub enabled: bool,
    #[prop(default)]
    pub pending_trust: bool,
    #[prop(default)]
    pub overrides_builtin: bool,
    #[prop(default)]
    pub has_network: bool,
    pub capabilities: Vec<String>,
    pub domains: Vec<String>,
    pub workspace_read: bool,
    #[prop(default)]
    pub has_ui: bool,
    #[prop(default)]
    pub is_linked: bool,
    #[prop(skip_none)]
    pub link_target: Option<String>,
    #[prop(skip_none)]
    pub source_repo: Option<String>,
    pub credentials: Vec<PluginCredentialFieldJs>,
    #[prop(skip_none)]
    pub handle: Option<String>,
    #[prop(skip_none)]
    pub last_error: Option<String>,
}

/// Subscribed decentralized plugin repository entry.
#[derive(Debug, Clone, Default, PartialEq, Eq, JsObject)]
pub struct PluginRepoConfigJs {
    pub url: String,
    #[prop(skip_none)]
    pub name: Option<String>,
}

/// Full snapshot for the Settings -> Plugins panel.
#[derive(Debug, Clone, Default, PartialEq, Eq, JsObject)]
pub struct PluginsOverviewJs {
    pub plugins: Vec<PluginSummaryJs>,
    pub repos: Vec<PluginRepoConfigJs>,
    pub auto_sync_on_startup: bool,
    #[prop(skip_none)]
    pub last_synced_at: Option<String>,
    #[prop(skip_none)]
    pub error: Option<String>,
}

/// Result of exporting a built-in plugin to `<dataDir>/plugins/<id>/`.
#[derive(Debug, Clone, Default, PartialEq, Eq, JsObject)]
pub struct ExportPluginResultJs {
    pub ok: bool,
    #[prop(skip_none)]
    pub path: Option<String>,
    #[prop(skip_none)]
    pub error: Option<String>,
}

/// Result of installing a plugin from a local directory, ZIP archive, or remote repository.
#[derive(Debug, Clone, Default, PartialEq, Eq, JsObject)]
pub struct InstallPluginResultJs {
    pub ok: bool,
    pub cancelled: bool,
    #[prop(skip_none)]
    pub plugin_id: Option<String>,
    #[prop(skip_none)]
    pub overview: Option<PluginsOverviewJs>,
    #[prop(skip_none)]
    pub error: Option<String>,
}

/// Permission preview for one entry in a decentralized plugin repository catalog.
#[derive(Debug, Clone, Default, PartialEq, Eq, JsObject)]
pub struct RepoPluginPermissionJs {
    pub domains: Vec<String>,
    pub workspace_read: bool,
}

/// One installable plugin entry from a decentralized `index.json` catalog.
#[derive(Debug, Clone, Default, PartialEq, Eq, JsObject)]
pub struct RepoPluginEntryJs {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub download_url: String,
    pub sha256: String,
    #[prop(default)]
    pub capabilities: Vec<String>,
    #[prop(default)]
    pub has_ui: bool,
    #[prop(default)]
    pub official: bool,
    #[prop(skip_none)]
    pub permissions: Option<RepoPluginPermissionJs>,
}

/// Result of fetching a decentralized repository's `index.json` catalog.
#[derive(Debug, Clone, Default, PartialEq, Eq, JsObject)]
pub struct RepoCatalogResultJs {
    pub ok: bool,
    #[prop(default)]
    pub name: String,
    #[prop(default)]
    pub plugins: Vec<RepoPluginEntryJs>,
    #[prop(skip_none)]
    pub error: Option<String>,
}

/// One failed repository fetch reported alongside aggregated repo catalog results.
#[derive(Debug, Clone, Default, PartialEq, Eq, JsObject)]
pub struct RepoFetchErrorJs {
    pub repo_url: String,
    #[prop(skip_none)]
    pub repo_name: Option<String>,
    pub error: String,
}

/// One plugin entry inside an aggregated multi-repository catalog.
#[derive(Debug, Clone, Default, PartialEq, Eq, JsObject)]
pub struct AggregatedRepoPluginJs {
    pub repo_url: String,
    pub repo_name: String,
    pub entry: RepoPluginEntryJs,
}

/// Aggregated catalog across all subscribed plugin repositories, including per-repo errors.
#[derive(Debug, Clone, Default, PartialEq, Eq, JsObject)]
pub struct PluginRepoCatalogJs {
    pub ok: bool,
    #[prop(default)]
    pub plugins: Vec<AggregatedRepoPluginJs>,
    #[prop(default)]
    pub errors: Vec<RepoFetchErrorJs>,
    #[prop(skip_none)]
    pub error: Option<String>,
}

/// Result of running a plugin's `verify(ctx)` action.
#[derive(Debug, Clone, Default, PartialEq, Eq, JsObject)]
pub struct VerifyPluginResultJs {
    pub ok: bool,
    #[prop(skip_none)]
    pub message: Option<String>,
    #[prop(skip_none)]
    pub error: Option<String>,
}

/// Per-platform result inside [`SyncReportJs`].
#[derive(Debug, Clone, Default, PartialEq, Eq, JsObject)]
pub struct PlatformSyncReportJs {
    pub platform: String,
    pub handle: String,
    pub ok: bool,
    pub account_synced: bool,
    pub summary_synced: bool,
    pub submissions_synced: u32,
    pub rating_history_synced: u32,
    pub contests_synced: u32,
    #[prop(skip_none)]
    pub error: Option<String>,
}

/// Aggregated result of `syncOj(platform, full)`.
#[derive(Debug, Clone, Default, PartialEq, Eq, JsObject)]
pub struct SyncReportJs {
    pub ok: bool,
    #[prop(default)]
    pub synced_at: String,
    #[prop(default)]
    pub platforms: Vec<PlatformSyncReportJs>,
    #[prop(skip_none)]
    pub error: Option<String>,
}

// ---- Phase 4B: Training Dashboard, Contests Calendar, Submissions & Ingest -------------

/// Query parameters for `trainingSubmissions(query)`.
#[derive(Debug, Clone, Default, PartialEq, Eq, JsObject)]
pub struct TrainingSubmissionsQueryJs {
    #[prop(default)]
    pub by_problem: bool,
    #[prop(skip_none)]
    pub platform: Option<String>,
    #[prop(skip_none)]
    pub verdict: Option<String>,
    #[prop(default)]
    pub unsolved_only: bool,
    #[prop(skip_none)]
    pub min_rating: Option<i32>,
    #[prop(skip_none)]
    pub max_rating: Option<i32>,
    #[prop(skip_none)]
    pub tag: Option<String>,
    #[prop(skip_none)]
    pub search: Option<String>,
    #[prop(skip_none)]
    pub limit: Option<u32>,
    #[prop(skip_none)]
    pub offset: Option<u32>,
}

/// Request payload for `trainingIngestProblem(req)`.
#[derive(Debug, Clone, Default, PartialEq, Eq, JsObject)]
pub struct IngestProblemRequestJs {
    #[prop(skip_none)]
    pub url: Option<String>,
    #[prop(skip_none)]
    pub platform: Option<String>,
    #[prop(skip_none)]
    pub problem_key: Option<String>,
    #[prop(skip_none)]
    pub id: Option<String>,
    #[prop(skip_none)]
    pub title: Option<String>,
    #[prop(skip_none)]
    pub tags: Option<Vec<String>>,
    #[prop(skip_none)]
    pub rating: Option<i32>,
    #[prop(skip_none)]
    pub initial_status: Option<String>,
}

/// Result of `trainingIngestProblem(req)`.
#[derive(Debug, Clone, Default, PartialEq, Eq, JsObject)]
pub struct IngestProblemResultJs {
    pub ok: bool,
    #[prop(default)]
    pub problem_id: String,
    pub created: bool,
    #[prop(default)]
    pub title: String,
    #[prop(skip_none)]
    pub code: Option<String>,
    #[prop(skip_none)]
    pub error: Option<String>,
}

/// Result of `trainingLogRecord(reqJson)`.
#[derive(Debug, Clone, Default, PartialEq, Eq, JsObject)]
pub struct ManualLogResultJs {
    pub ok: bool,
    #[prop(skip_none)]
    pub library_problem_id: Option<String>,
    pub created_in_library: bool,
    #[prop(skip_none)]
    pub error: Option<String>,
}

/// One non-empty editorial summary returned by `trainingListEditorials`.
#[derive(Debug, Clone, Default, PartialEq, Eq, JsObject)]
pub struct EditorialSummaryJs {
    pub problem_id: String,
    pub title: String,
    #[prop(skip_none)]
    pub rating: Option<i32>,
    pub tags: Vec<String>,
    pub sources: Vec<ProblemSourceJs>,
    pub excerpt: String,
    pub updated_at: String,
}

/// Result of `trainingListEditorials(queryJson)`.
#[derive(Debug, Clone, Default, PartialEq, Eq, JsObject)]
pub struct EditorialListResultJs {
    pub items: Vec<EditorialSummaryJs>,
    pub total: u32,
    pub available_tags: Vec<String>,
    #[prop(skip_none)]
    pub error: Option<String>,
}

// ---- Phase 4C: Toolbox, Plugin UI SDK, Commands & Importer/Exporter --------------------

/// Loaded pure-Web HTML UI entry for `<PluginWebView>` (`loadPluginUiHtml`).
#[derive(Debug, Clone, Default, PartialEq, Eq, JsObject)]
pub struct PluginUiHtmlJs {
    pub ok: bool,
    #[prop(skip_none)]
    pub html: Option<String>,
    #[prop(skip_none)]
    pub plugin_id: Option<String>,
    #[prop(skip_none)]
    pub workspace_read: Option<bool>,
    #[prop(skip_none)]
    pub error: Option<String>,
    #[prop(skip_none)]
    pub error_code: Option<String>,
}

/// Result of a permission-checked plugin workspace read query (`pluginWorkspaceQuery`).
#[derive(Debug, Clone, Default, PartialEq, Eq, JsObject)]
pub struct PluginWorkspaceResultJs {
    pub ok: bool,
    #[prop(skip_none)]
    pub data_json: Option<String>,
    #[prop(skip_none)]
    pub error: Option<String>,
    #[prop(skip_none)]
    pub error_code: Option<String>,
}

/// Result of rendering an SVG string to a PNG image via `resvg` (`pluginRenderSvgPng`).
#[derive(Debug, Clone, Default, PartialEq, Eq, JsObject)]
pub struct PluginRenderPngResultJs {
    pub ok: bool,
    #[prop(skip_none)]
    pub png_base64: Option<String>,
    #[prop(skip_none)]
    pub error: Option<String>,
}

/// Result of synchronous plugin KV read (`pluginKvGet`).
#[derive(Debug, Clone, Default, PartialEq, Eq, JsObject)]
pub struct PluginKvResultJs {
    pub ok: bool,
    #[prop(default)]
    pub found: bool,
    #[prop(skip_none)]
    pub value_json: Option<String>,
    #[prop(skip_none)]
    pub error: Option<String>,
}

/// Long-polling notification event emitted when plugin files change on disk (`nextPluginChange`).
#[derive(Debug, Clone, Default, PartialEq, Eq, JsObject)]
pub struct PluginWatchEventJs {
    /// `"changed"` or `"closed"`.
    pub kind: String,
    #[prop(default)]
    pub paths: Vec<String>,
    #[prop(default)]
    pub changed_plugin_ids: Vec<String>,
}

/// Result of picking and reading a local file via `pluginPickFileDialog`.
#[derive(Debug, Clone, Default, PartialEq, Eq, JsObject)]
pub struct PluginPickFileResultJs {
    pub ok: bool,
    #[prop(default)]
    pub cancelled: bool,
    #[prop(skip_none)]
    pub name: Option<String>,
    #[prop(skip_none)]
    pub text: Option<String>,
    #[prop(skip_none)]
    pub content_base64: Option<String>,
    #[prop(skip_none)]
    pub error: Option<String>,
}

// ---- Theme override DTOs --------------------------------------------------------------

#[derive(Debug, Clone, Default, PartialEq, JsObject)]
pub struct ThemeColorEntryJs {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, Default, PartialEq, JsObject)]
pub struct ThemeOverridesJs {
    pub light_colors: Vec<ThemeColorEntryJs>,
    pub dark_colors: Vec<ThemeColorEntryJs>,
    pub radius: Option<f64>,
    pub editor_font_family: Option<String>,
    pub editor_font_size: Option<f64>,
    pub editor_line_height: Option<f64>,
    /// Absolute path of the active global `theme.toml`, if present.
    pub active_path: Option<String>,
}

// ---- Conversions between xcpc_app DTOs and JS DTOs -------------------------------------

impl From<xcpc_app::PreviewLayer> for PreviewLayer {
    fn from(v: xcpc_app::PreviewLayer) -> Self {
        Self {
            key: v.key,
            left: v.left,
            top: v.top,
            width: v.width,
            height: v.height,
        }
    }
}

impl From<PreviewLayer> for xcpc_app::PreviewLayer {
    fn from(v: PreviewLayer) -> Self {
        Self {
            key: v.key,
            left: v.left,
            top: v.top,
            width: v.width,
            height: v.height,
        }
    }
}

impl From<xcpc_app::PreviewPage> for PreviewPage {
    fn from(v: xcpc_app::PreviewPage) -> Self {
        Self {
            width: v.width,
            height: v.height,
            layers: v.layers.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<PreviewPage> for xcpc_app::PreviewPage {
    fn from(v: PreviewPage) -> Self {
        Self {
            width: v.width,
            height: v.height,
            layers: v.layers.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<xcpc_app::MathPreviewJs> for MathPreviewJs {
    fn from(v: xcpc_app::MathPreviewJs) -> Self {
        Self {
            key: v.key,
            width: v.width,
            height: v.height,
            error: v.error,
        }
    }
}

impl From<MathPreviewJs> for xcpc_app::MathPreviewJs {
    fn from(v: MathPreviewJs) -> Self {
        Self {
            key: v.key,
            width: v.width,
            height: v.height,
            error: v.error,
        }
    }
}

impl From<xcpc_app::PreviewView> for PreviewView {
    fn from(v: xcpc_app::PreviewView) -> Self {
        Self {
            page: v.page,
            x: v.x,
            y: v.y,
            width: v.width,
            height: v.height,
        }
    }
}

impl From<PreviewView> for xcpc_app::PreviewView {
    fn from(v: PreviewView) -> Self {
        Self {
            page: v.page,
            x: v.x,
            y: v.y,
            width: v.width,
            height: v.height,
        }
    }
}

impl From<xcpc_app::PreviewRequest> for PreviewRequest {
    fn from(v: xcpc_app::PreviewRequest) -> Self {
        Self {
            target: v.target,
            id: v.id,
            book: v.book,
            path: v.path,
            text: v.text,
        }
    }
}

impl From<PreviewRequest> for xcpc_app::PreviewRequest {
    fn from(v: PreviewRequest) -> Self {
        Self {
            target: v.target,
            id: v.id,
            book: v.book,
            path: v.path,
            text: v.text,
        }
    }
}

impl From<xcpc_app::PreviewDiagnostic> for PreviewDiagnostic {
    fn from(v: xcpc_app::PreviewDiagnostic) -> Self {
        Self {
            severity: v.severity,
            code: v.code,
            message: v.message,
            file: v.file,
            line: v.line,
            column: v.column,
            page: v.page,
        }
    }
}

impl From<PreviewDiagnostic> for xcpc_app::PreviewDiagnostic {
    fn from(v: PreviewDiagnostic) -> Self {
        Self {
            severity: v.severity,
            code: v.code,
            message: v.message,
            file: v.file,
            line: v.line,
            column: v.column,
            page: v.page,
        }
    }
}

impl From<xcpc_app::PreviewStart> for PreviewStart {
    fn from(v: xcpc_app::PreviewStart) -> Self {
        Self {
            label: v.label,
            id: v.id,
            page: v.page,
            first: v.first,
            count: v.count,
        }
    }
}

impl From<PreviewStart> for xcpc_app::PreviewStart {
    fn from(v: PreviewStart) -> Self {
        Self {
            label: v.label,
            id: v.id,
            page: v.page,
            first: v.first,
            count: v.count,
        }
    }
}

impl From<xcpc_app::PreviewResult> for PreviewResult {
    fn from(v: xcpc_app::PreviewResult) -> Self {
        Self {
            pages: v.pages.into_iter().map(Into::into).collect(),
            error: v.error,
            diagnostics: v.diagnostics.into_iter().map(Into::into).collect(),
            starts: v.starts.into_iter().map(Into::into).collect(),
            compile_ms: v.compile_ms,
            render_ms: v.render_ms,
            rendered: v.rendered,
        }
    }
}

impl From<PreviewResult> for xcpc_app::PreviewResult {
    fn from(v: PreviewResult) -> Self {
        Self {
            pages: v.pages.into_iter().map(Into::into).collect(),
            error: v.error,
            diagnostics: v.diagnostics.into_iter().map(Into::into).collect(),
            starts: v.starts.into_iter().map(Into::into).collect(),
            compile_ms: v.compile_ms,
            render_ms: v.render_ms,
            rendered: v.rendered,
        }
    }
}

impl From<xcpc_app::SourceLocationJs> for SourceLocationJs {
    fn from(v: xcpc_app::SourceLocationJs) -> Self {
        Self {
            file: v.file,
            line: v.line,
            column: v.column,
        }
    }
}

impl From<SourceLocationJs> for xcpc_app::SourceLocationJs {
    fn from(v: SourceLocationJs) -> Self {
        Self {
            file: v.file,
            line: v.line,
            column: v.column,
        }
    }
}

impl From<xcpc_app::PreviewLocationJs> for PreviewLocationJs {
    fn from(v: xcpc_app::PreviewLocationJs) -> Self {
        Self {
            page: v.page,
            y_pt: v.y_pt,
            exact: v.exact,
        }
    }
}

impl From<PreviewLocationJs> for xcpc_app::PreviewLocationJs {
    fn from(v: PreviewLocationJs) -> Self {
        Self {
            page: v.page,
            y_pt: v.y_pt,
            exact: v.exact,
        }
    }
}

impl From<xcpc_app::WorkspaceProblem> for WorkspaceProblem {
    fn from(v: xcpc_app::WorkspaceProblem) -> Self {
        Self {
            id: v.id,
            title: v.title,
            statement: v.statement,
            editorial: v.editorial,
        }
    }
}

impl From<WorkspaceProblem> for xcpc_app::WorkspaceProblem {
    fn from(v: WorkspaceProblem) -> Self {
        Self {
            id: v.id,
            title: v.title,
            statement: v.statement,
            editorial: v.editorial,
        }
    }
}

impl From<xcpc_app::WorkspaceItem> for WorkspaceItem {
    fn from(v: xcpc_app::WorkspaceItem) -> Self {
        Self {
            label: v.label,
            id: v.id,
            title: v.title,
            statement: v.statement,
            editorial: v.editorial,
        }
    }
}

impl From<WorkspaceItem> for xcpc_app::WorkspaceItem {
    fn from(v: WorkspaceItem) -> Self {
        Self {
            label: v.label,
            id: v.id,
            title: v.title,
            statement: v.statement,
            editorial: v.editorial,
        }
    }
}

impl From<xcpc_app::WorkspaceContest> for WorkspaceContest {
    fn from(v: xcpc_app::WorkspaceContest) -> Self {
        Self {
            id: v.id,
            name: v.name,
            frozen: v.frozen,
            archived: v.archived,
            items: v.items.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<WorkspaceContest> for xcpc_app::WorkspaceContest {
    fn from(v: WorkspaceContest) -> Self {
        Self {
            id: v.id,
            name: v.name,
            frozen: v.frozen,
            archived: v.archived,
            items: v.items.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<xcpc_app::WorkspaceInfo> for WorkspaceInfo {
    fn from(v: xcpc_app::WorkspaceInfo) -> Self {
        Self {
            root: v.root,
            read_only: v.read_only,
            training_enabled: v.training_enabled,
            snippets_enabled: v.snippets_enabled,
            last_workspace_missing: v.last_workspace_missing,
            error: v.error,
            problems: v.problems.into_iter().map(Into::into).collect(),
            contests: v.contests.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<WorkspaceInfo> for xcpc_app::WorkspaceInfo {
    fn from(v: WorkspaceInfo) -> Self {
        Self {
            root: v.root,
            read_only: v.read_only,
            training_enabled: v.training_enabled,
            snippets_enabled: v.snippets_enabled,
            last_workspace_missing: v.last_workspace_missing,
            error: v.error,
            problems: v.problems.into_iter().map(Into::into).collect(),
            contests: v.contests.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<xcpc_app::PdfFreshnessJs> for PdfFreshnessJs {
    fn from(v: xcpc_app::PdfFreshnessJs) -> Self {
        Self {
            rel_path: v.rel_path,
            abs_path: v.abs_path,
            exists: v.exists,
            fresh: v.fresh,
        }
    }
}

impl From<PdfFreshnessJs> for xcpc_app::PdfFreshnessJs {
    fn from(v: PdfFreshnessJs) -> Self {
        Self {
            rel_path: v.rel_path,
            abs_path: v.abs_path,
            exists: v.exists,
            fresh: v.fresh,
        }
    }
}

impl From<xcpc_app::ExportBookStatusJs> for ExportBookStatusJs {
    fn from(v: xcpc_app::ExportBookStatusJs) -> Self {
        Self {
            statements: v.statements.into(),
            editorial: v.editorial.into(),
        }
    }
}

impl From<ExportBookStatusJs> for xcpc_app::ExportBookStatusJs {
    fn from(v: ExportBookStatusJs) -> Self {
        Self {
            statements: v.statements.into(),
            editorial: v.editorial.into(),
        }
    }
}

impl From<xcpc_app::PickedFolder> for PickedFolder {
    fn from(v: xcpc_app::PickedFolder) -> Self {
        Self {
            cancelled: v.cancelled,
            path: v.path,
            workspace_root: v.workspace_root,
            is_workspace: v.is_workspace,
            error: v.error,
        }
    }
}

impl From<PickedFolder> for xcpc_app::PickedFolder {
    fn from(v: PickedFolder) -> Self {
        Self {
            cancelled: v.cancelled,
            path: v.path,
            workspace_root: v.workspace_root,
            is_workspace: v.is_workspace,
            error: v.error,
        }
    }
}

impl From<xcpc_app::WorkspaceFile> for WorkspaceFile {
    fn from(v: xcpc_app::WorkspaceFile) -> Self {
        Self {
            path: v.path,
            text: v.text,
            hash: v.hash,
            read_only: v.read_only,
            error: v.error,
        }
    }
}

impl From<WorkspaceFile> for xcpc_app::WorkspaceFile {
    fn from(v: WorkspaceFile) -> Self {
        Self {
            path: v.path,
            text: v.text,
            hash: v.hash,
            read_only: v.read_only,
            error: v.error,
        }
    }
}

impl From<xcpc_app::SaveResult> for SaveResult {
    fn from(v: xcpc_app::SaveResult) -> Self {
        Self {
            ok: v.ok,
            hash: v.hash,
            conflict: v.conflict,
            error: v.error,
        }
    }
}

impl From<SaveResult> for xcpc_app::SaveResult {
    fn from(v: SaveResult) -> Self {
        Self {
            ok: v.ok,
            hash: v.hash,
            conflict: v.conflict,
            error: v.error,
        }
    }
}

impl From<xcpc_app::WatchEventJs> for WatchEventJs {
    fn from(v: xcpc_app::WatchEventJs) -> Self {
        Self {
            kind: v.kind,
            paths: v.paths,
        }
    }
}

impl From<WatchEventJs> for xcpc_app::WatchEventJs {
    fn from(v: WatchEventJs) -> Self {
        Self {
            kind: v.kind,
            paths: v.paths,
        }
    }
}

impl From<xcpc_app::ActionResult> for ActionResult {
    fn from(v: xcpc_app::ActionResult) -> Self {
        Self {
            ok: v.ok,
            error: v.error,
            code: v.code,
            referencing_contests: v.referencing_contests,
        }
    }
}

impl From<ActionResult> for xcpc_app::ActionResult {
    fn from(v: ActionResult) -> Self {
        Self {
            ok: v.ok,
            error: v.error,
            code: v.code,
            referencing_contests: v.referencing_contests,
        }
    }
}

impl From<xcpc_app::ProblemSourceJs> for ProblemSourceJs {
    fn from(v: xcpc_app::ProblemSourceJs) -> Self {
        Self {
            platform: v.platform,
            id: v.id,
            url: v.url,
        }
    }
}

impl From<ProblemSourceJs> for xcpc_app::ProblemSourceJs {
    fn from(v: ProblemSourceJs) -> Self {
        Self {
            platform: v.platform,
            id: v.id,
            url: v.url,
        }
    }
}

impl From<xcpc_app::LimitsJs> for LimitsJs {
    fn from(v: xcpc_app::LimitsJs) -> Self {
        Self {
            time: v.time,
            memory: v.memory,
            input: v.input,
            output: v.output,
        }
    }
}

impl From<LimitsJs> for xcpc_app::LimitsJs {
    fn from(v: LimitsJs) -> Self {
        Self {
            time: v.time,
            memory: v.memory,
            input: v.input,
            output: v.output,
        }
    }
}

impl From<xcpc_app::ProblemSearchQuery> for ProblemSearchQuery {
    fn from(v: xcpc_app::ProblemSearchQuery) -> Self {
        Self {
            text: v.text,
            tags: v.tags,
            rating_min: v.rating_min,
            rating_max: v.rating_max,
            platform: v.platform,
            group: v.group,
            ungrouped_only: v.ungrouped_only,
        }
    }
}

impl From<ProblemSearchQuery> for xcpc_app::ProblemSearchQuery {
    fn from(v: ProblemSearchQuery) -> Self {
        Self {
            text: v.text,
            tags: v.tags,
            rating_min: v.rating_min,
            rating_max: v.rating_max,
            platform: v.platform,
            group: v.group,
            ungrouped_only: v.ungrouped_only,
        }
    }
}

impl From<xcpc_app::ProblemSummaryJs> for ProblemSummaryJs {
    fn from(v: xcpc_app::ProblemSummaryJs) -> Self {
        Self {
            id: v.id,
            title: v.title,
            group: v.group,
            rating: v.rating,
            tags: v.tags,
            sources: v.sources.into_iter().map(Into::into).collect(),
            has_editorial: v.has_editorial,
            has_solutions: v.has_solutions,
            statement_path: v.statement_path,
            package_status: v.package_status,
            contest_ids: v.contest_ids,
            training_status: v.training_status,
        }
    }
}

impl From<ProblemSummaryJs> for xcpc_app::ProblemSummaryJs {
    fn from(v: ProblemSummaryJs) -> Self {
        Self {
            id: v.id,
            title: v.title,
            group: v.group,
            rating: v.rating,
            tags: v.tags,
            sources: v.sources.into_iter().map(Into::into).collect(),
            has_editorial: v.has_editorial,
            has_solutions: v.has_solutions,
            statement_path: v.statement_path,
            package_status: v.package_status,
            contest_ids: v.contest_ids,
            training_status: v.training_status,
        }
    }
}

impl From<xcpc_app::GroupSummaryJs> for GroupSummaryJs {
    fn from(v: xcpc_app::GroupSummaryJs) -> Self {
        Self {
            name: v.name,
            count: v.count,
            declared: v.declared,
        }
    }
}

impl From<GroupSummaryJs> for xcpc_app::GroupSummaryJs {
    fn from(v: GroupSummaryJs) -> Self {
        Self {
            name: v.name,
            count: v.count,
            declared: v.declared,
        }
    }
}

impl From<xcpc_app::WorkspaceFacets> for WorkspaceFacets {
    fn from(v: xcpc_app::WorkspaceFacets) -> Self {
        Self {
            tags: v.tags,
            platforms: v.platforms,
            groups: v.groups.into_iter().map(Into::into).collect(),
            ungrouped_count: v.ungrouped_count,
            total_count: v.total_count,
        }
    }
}

impl From<WorkspaceFacets> for xcpc_app::WorkspaceFacets {
    fn from(v: WorkspaceFacets) -> Self {
        Self {
            tags: v.tags,
            platforms: v.platforms,
            groups: v.groups.into_iter().map(Into::into).collect(),
            ungrouped_count: v.ungrouped_count,
            total_count: v.total_count,
        }
    }
}

impl From<xcpc_app::SamplePairJs> for SamplePairJs {
    fn from(v: xcpc_app::SamplePairJs) -> Self {
        Self {
            name: v.name,
            input: v.input,
            output: v.output,
        }
    }
}

impl From<SamplePairJs> for xcpc_app::SamplePairJs {
    fn from(v: SamplePairJs) -> Self {
        Self {
            name: v.name,
            input: v.input,
            output: v.output,
        }
    }
}

impl From<xcpc_app::ProblemDetailJs> for ProblemDetailJs {
    fn from(v: xcpc_app::ProblemDetailJs) -> Self {
        Self {
            id: v.id,
            title: v.title,
            group: v.group,
            rating: v.rating,
            tags: v.tags,
            author: v.author,
            limits: v.limits.into(),
            sources: v.sources.into_iter().map(Into::into).collect(),
            statement: v.statement.into(),
            statements: v.statements.into_iter().map(Into::into).collect(),
            editorial: v.editorial.into(),
            has_editorial: v.has_editorial,
            samples: v.samples.into_iter().map(Into::into).collect(),
            solutions: v.solutions,
            referencing_contests: v.referencing_contests,
            template_options_json: v.template_options_json,
            error: v.error,
        }
    }
}

impl From<ProblemDetailJs> for xcpc_app::ProblemDetailJs {
    fn from(v: ProblemDetailJs) -> Self {
        Self {
            id: v.id,
            title: v.title,
            group: v.group,
            rating: v.rating,
            tags: v.tags,
            author: v.author,
            limits: v.limits.into(),
            sources: v.sources.into_iter().map(Into::into).collect(),
            statement: v.statement.into(),
            statements: v.statements.into_iter().map(Into::into).collect(),
            editorial: v.editorial.into(),
            has_editorial: v.has_editorial,
            samples: v.samples.into_iter().map(Into::into).collect(),
            solutions: v.solutions,
            referencing_contests: v.referencing_contests,
            template_options_json: v.template_options_json,
            error: v.error,
        }
    }
}

impl From<xcpc_app::ContestItemDetailJs> for ContestItemDetailJs {
    fn from(v: xcpc_app::ContestItemDetailJs) -> Self {
        Self {
            label: v.label,
            id: v.id,
            missing: v.missing,
            title: v.title,
            original_title: v.original_title,
            limits: v.limits.into(),
            original_limits: v.original_limits.into(),
            show_metadata: v.show_metadata,
            default_show_metadata: v.default_show_metadata,
            overridden_title: v.overridden_title,
            overridden_time: v.overridden_time,
            overridden_memory: v.overridden_memory,
            overridden_input: v.overridden_input,
            overridden_output: v.overridden_output,
            overridden_show_metadata: v.overridden_show_metadata,
            statement: v.statement,
            editorial: v.editorial,
            rating: v.rating,
            tags: v.tags,
            options_json: v.options_json,
            original_options_json: v.original_options_json,
            override_options_json: v.override_options_json,
            package_status: v.package_status,
        }
    }
}

impl From<ContestItemDetailJs> for xcpc_app::ContestItemDetailJs {
    fn from(v: ContestItemDetailJs) -> Self {
        Self {
            label: v.label,
            id: v.id,
            missing: v.missing,
            title: v.title,
            original_title: v.original_title,
            limits: v.limits.into(),
            original_limits: v.original_limits.into(),
            show_metadata: v.show_metadata,
            default_show_metadata: v.default_show_metadata,
            overridden_title: v.overridden_title,
            overridden_time: v.overridden_time,
            overridden_memory: v.overridden_memory,
            overridden_input: v.overridden_input,
            overridden_output: v.overridden_output,
            overridden_show_metadata: v.overridden_show_metadata,
            statement: v.statement,
            editorial: v.editorial,
            rating: v.rating,
            tags: v.tags,
            options_json: v.options_json,
            original_options_json: v.original_options_json,
            override_options_json: v.override_options_json,
            package_status: v.package_status,
        }
    }
}

impl From<xcpc_app::ContestDetailJs> for ContestDetailJs {
    fn from(v: xcpc_app::ContestDetailJs) -> Self {
        Self {
            id: v.id,
            name: v.name,
            location: v.location,
            date: v.date,
            author: v.author,
            language: v.language,
            template: v.template,
            effective_template: v.effective_template,
            has_contest_template: v.has_contest_template,
            contest_template_json: v.contest_template_json,
            template_options_json: v.template_options_json,
            show_metadata: v.show_metadata,
            cover: v.cover,
            has_cover: v.has_cover,
            blank_pages: v.blank_pages,
            frozen: v.frozen,
            archived: v.archived,
            items: v.items.into_iter().map(Into::into).collect(),
            error: v.error,
        }
    }
}

impl From<ContestDetailJs> for xcpc_app::ContestDetailJs {
    fn from(v: ContestDetailJs) -> Self {
        Self {
            id: v.id,
            name: v.name,
            location: v.location,
            date: v.date,
            author: v.author,
            language: v.language,
            template: v.template,
            effective_template: v.effective_template,
            has_contest_template: v.has_contest_template,
            contest_template_json: v.contest_template_json,
            template_options_json: v.template_options_json,
            show_metadata: v.show_metadata,
            cover: v.cover,
            has_cover: v.has_cover,
            blank_pages: v.blank_pages,
            frozen: v.frozen,
            archived: v.archived,
            items: v.items.into_iter().map(Into::into).collect(),
            error: v.error,
        }
    }
}

impl From<xcpc_app::RatingBucketJs> for RatingBucketJs {
    fn from(v: xcpc_app::RatingBucketJs) -> Self {
        Self {
            tier: v.tier,
            count: v.count,
        }
    }
}

impl From<RatingBucketJs> for xcpc_app::RatingBucketJs {
    fn from(v: RatingBucketJs) -> Self {
        Self {
            tier: v.tier,
            count: v.count,
        }
    }
}

impl From<xcpc_app::TagCountJs> for TagCountJs {
    fn from(v: xcpc_app::TagCountJs) -> Self {
        Self {
            tag: v.tag,
            count: v.count,
        }
    }
}

impl From<TagCountJs> for xcpc_app::TagCountJs {
    fn from(v: TagCountJs) -> Self {
        Self {
            tag: v.tag,
            count: v.count,
        }
    }
}

impl From<xcpc_app::ContestStatsJs> for ContestStatsJs {
    fn from(v: xcpc_app::ContestStatsJs) -> Self {
        Self {
            problem_count: v.problem_count,
            rating_distribution: v.rating_distribution.into_iter().map(Into::into).collect(),
            tag_counts: v.tag_counts.into_iter().map(Into::into).collect(),
            error: v.error,
        }
    }
}

impl From<ContestStatsJs> for xcpc_app::ContestStatsJs {
    fn from(v: ContestStatsJs) -> Self {
        Self {
            problem_count: v.problem_count,
            rating_distribution: v.rating_distribution.into_iter().map(Into::into).collect(),
            tag_counts: v.tag_counts.into_iter().map(Into::into).collect(),
            error: v.error,
        }
    }
}

impl From<xcpc_app::FreezeResultJs> for FreezeResultJs {
    fn from(v: xcpc_app::FreezeResultJs) -> Self {
        Self {
            ok: v.ok,
            frozen: v.frozen,
            outputs: v.outputs,
            diagnostics: v.diagnostics.into_iter().map(Into::into).collect(),
            error: v.error,
            code: v.code,
        }
    }
}

impl From<FreezeResultJs> for xcpc_app::FreezeResultJs {
    fn from(v: FreezeResultJs) -> Self {
        Self {
            ok: v.ok,
            frozen: v.frozen,
            outputs: v.outputs,
            diagnostics: v.diagnostics.into_iter().map(Into::into).collect(),
            error: v.error,
            code: v.code,
        }
    }
}

impl From<xcpc_app::UnfreezeProblemDiffJs> for UnfreezeProblemDiffJs {
    fn from(v: xcpc_app::UnfreezeProblemDiffJs) -> Self {
        Self {
            label: v.label,
            problem_id: v.problem_id,
            kinds: v.kinds,
            changed_files: v.changed_files,
        }
    }
}

impl From<UnfreezeProblemDiffJs> for xcpc_app::UnfreezeProblemDiffJs {
    fn from(v: UnfreezeProblemDiffJs) -> Self {
        Self {
            label: v.label,
            problem_id: v.problem_id,
            kinds: v.kinds,
            changed_files: v.changed_files,
        }
    }
}

impl From<xcpc_app::UnfreezeDiffResultJs> for UnfreezeDiffResultJs {
    fn from(v: xcpc_app::UnfreezeDiffResultJs) -> Self {
        Self {
            ok: v.ok,
            diffs: v.diffs.into_iter().map(Into::into).collect(),
            error: v.error,
            code: v.code,
        }
    }
}

impl From<UnfreezeDiffResultJs> for xcpc_app::UnfreezeDiffResultJs {
    fn from(v: UnfreezeDiffResultJs) -> Self {
        Self {
            ok: v.ok,
            diffs: v.diffs.into_iter().map(Into::into).collect(),
            error: v.error,
            code: v.code,
        }
    }
}

impl From<xcpc_app::FuzzyHitJs> for FuzzyHitJs {
    fn from(v: xcpc_app::FuzzyHitJs) -> Self {
        Self {
            kind: v.kind,
            id: v.id,
            title: v.title,
            contest_id: v.contest_id,
            label: v.label,
            visit_key: v.visit_key,
            archived: v.archived,
            score: v.score,
        }
    }
}

impl From<FuzzyHitJs> for xcpc_app::FuzzyHitJs {
    fn from(v: FuzzyHitJs) -> Self {
        Self {
            kind: v.kind,
            id: v.id,
            title: v.title,
            contest_id: v.contest_id,
            label: v.label,
            visit_key: v.visit_key,
            archived: v.archived,
            score: v.score,
        }
    }
}

impl From<xcpc_app::AppPrefsJs> for AppPrefsJs {
    fn from(v: xcpc_app::AppPrefsJs) -> Self {
        Self {
            recent_workspaces: v.recent_workspaces,
            split_ratio: v.split_ratio,
            last_space: v.last_space,
            last_target: v.last_target,
            last_id: v.last_id,
            last_label: v.last_label,
            last_book: v.last_book,
            props_open: v.props_open,
            language: v.language,
            cxx_override: v.cxx_override,
            library_sidebar_open: v.library_sidebar_open,
            library_group_by: v.library_group_by,
            library_checker_mirror_url: v.library_checker_mirror_url,
            library_checker_local_repo_path: v.library_checker_local_repo_path,
        }
    }
}

impl From<AppPrefsJs> for xcpc_app::AppPrefsJs {
    fn from(v: AppPrefsJs) -> Self {
        Self {
            recent_workspaces: v.recent_workspaces,
            split_ratio: v.split_ratio,
            last_space: v.last_space,
            last_target: v.last_target,
            last_id: v.last_id,
            last_label: v.last_label,
            last_book: v.last_book,
            props_open: v.props_open,
            language: v.language,
            cxx_override: v.cxx_override,
            library_sidebar_open: v.library_sidebar_open,
            library_group_by: v.library_group_by,
            library_checker_mirror_url: v.library_checker_mirror_url,
            library_checker_local_repo_path: v.library_checker_local_repo_path,
        }
    }
}

impl From<xcpc_app::DialogLabelsJs> for DialogLabelsJs {
    fn from(v: xcpc_app::DialogLabelsJs) -> Self {
        Self {
            title: v.title,
            button_label: v.button_label,
            filter_label: v.filter_label,
        }
    }
}

impl From<DialogLabelsJs> for xcpc_app::DialogLabelsJs {
    fn from(v: DialogLabelsJs) -> Self {
        Self {
            title: v.title,
            button_label: v.button_label,
            filter_label: v.filter_label,
        }
    }
}

impl From<xcpc_app::ProblemMetaPatchJs> for ProblemMetaPatchJs {
    fn from(v: xcpc_app::ProblemMetaPatchJs) -> Self {
        Self {
            title: v.title,
            rating: v.rating,
            tags: v.tags,
            author: v.author,
            time: v.time,
            memory: v.memory,
            input: v.input,
            output: v.output,
            sources: v.sources.map(|xs| xs.into_iter().map(Into::into).collect()),
            template_options_json: v.template_options_json,
        }
    }
}

impl From<ProblemMetaPatchJs> for xcpc_app::ProblemMetaPatchJs {
    fn from(v: ProblemMetaPatchJs) -> Self {
        Self {
            title: v.title,
            rating: v.rating,
            tags: v.tags,
            author: v.author,
            time: v.time,
            memory: v.memory,
            input: v.input,
            output: v.output,
            sources: v.sources.map(|xs| xs.into_iter().map(Into::into).collect()),
            template_options_json: v.template_options_json,
        }
    }
}

impl From<xcpc_app::ContestMetaPatchJs> for ContestMetaPatchJs {
    fn from(v: xcpc_app::ContestMetaPatchJs) -> Self {
        Self {
            name: v.name,
            location: v.location,
            date: v.date,
            author: v.author,
            language: v.language,
            show_metadata: v.show_metadata,
            cover: v.cover,
            blank_pages: v.blank_pages,
            template: v.template,
            template_options_json: v.template_options_json,
        }
    }
}

impl From<ContestMetaPatchJs> for xcpc_app::ContestMetaPatchJs {
    fn from(v: ContestMetaPatchJs) -> Self {
        Self {
            name: v.name,
            location: v.location,
            date: v.date,
            author: v.author,
            language: v.language,
            show_metadata: v.show_metadata,
            cover: v.cover,
            blank_pages: v.blank_pages,
            template: v.template,
            template_options_json: v.template_options_json,
        }
    }
}

impl From<xcpc_app::OverridePatchJs> for OverridePatchJs {
    fn from(v: xcpc_app::OverridePatchJs) -> Self {
        Self {
            title: v.title,
            time: v.time,
            memory: v.memory,
            input: v.input,
            output: v.output,
            show_metadata: v.show_metadata,
            options_json: v.options_json,
        }
    }
}

impl From<OverridePatchJs> for xcpc_app::OverridePatchJs {
    fn from(v: OverridePatchJs) -> Self {
        Self {
            title: v.title,
            time: v.time,
            memory: v.memory,
            input: v.input,
            output: v.output,
            show_metadata: v.show_metadata,
            options_json: v.options_json,
        }
    }
}

impl From<xcpc_app::MarkdownSpanJs> for MarkdownSpanJs {
    fn from(v: xcpc_app::MarkdownSpanJs) -> Self {
        Self {
            kind: v.kind,
            level: v.level,
            start_line: v.start_line,
            start_column: v.start_column,
            end_line: v.end_line,
            end_column: v.end_column,
        }
    }
}

impl From<MarkdownSpanJs> for xcpc_app::MarkdownSpanJs {
    fn from(v: MarkdownSpanJs) -> Self {
        Self {
            kind: v.kind,
            level: v.level,
            start_line: v.start_line,
            start_column: v.start_column,
            end_line: v.end_line,
            end_column: v.end_column,
        }
    }
}

impl From<xcpc_app::AssetReferenceJs> for AssetReferenceJs {
    fn from(v: xcpc_app::AssetReferenceJs) -> Self {
        Self {
            file: v.file,
            line: v.line,
            width_pct: v.width_pct,
        }
    }
}

impl From<AssetReferenceJs> for xcpc_app::AssetReferenceJs {
    fn from(v: AssetReferenceJs) -> Self {
        Self {
            file: v.file,
            line: v.line,
            width_pct: v.width_pct,
        }
    }
}

impl From<xcpc_app::ProblemAssetJs> for ProblemAssetJs {
    fn from(v: xcpc_app::ProblemAssetJs) -> Self {
        Self {
            name: v.name,
            rel_path: v.rel_path,
            abs_path: v.abs_path,
            thumb_path: v.thumb_path,
            bytes: v.bytes,
            width: v.width,
            height: v.height,
            referenced: v.referenced,
            references: v.references.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<ProblemAssetJs> for xcpc_app::ProblemAssetJs {
    fn from(v: ProblemAssetJs) -> Self {
        Self {
            name: v.name,
            rel_path: v.rel_path,
            abs_path: v.abs_path,
            thumb_path: v.thumb_path,
            bytes: v.bytes,
            width: v.width,
            height: v.height,
            referenced: v.referenced,
            references: v.references.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<xcpc_app::CleanUnusedAssetsResultJs> for CleanUnusedAssetsResultJs {
    fn from(v: xcpc_app::CleanUnusedAssetsResultJs) -> Self {
        Self {
            ok: v.ok,
            removed: v.removed,
            freed_bytes: v.freed_bytes,
            error: v.error,
            code: v.code,
        }
    }
}

impl From<CleanUnusedAssetsResultJs> for xcpc_app::CleanUnusedAssetsResultJs {
    fn from(v: CleanUnusedAssetsResultJs) -> Self {
        Self {
            ok: v.ok,
            removed: v.removed,
            freed_bytes: v.freed_bytes,
            error: v.error,
            code: v.code,
        }
    }
}

impl From<xcpc_app::ColdStartSnapshotJs> for ColdStartSnapshotJs {
    fn from(v: xcpc_app::ColdStartSnapshotJs) -> Self {
        Self {
            meta_json: v.meta_json,
            workspace: v.workspace,
            target: v.target,
            id: v.id,
            book: v.book,
            zoom: v.zoom,
            scroll_y: v.scroll_y,
            pages: v.pages.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<ColdStartSnapshotJs> for xcpc_app::ColdStartSnapshotJs {
    fn from(v: ColdStartSnapshotJs) -> Self {
        Self {
            meta_json: v.meta_json,
            workspace: v.workspace,
            target: v.target,
            id: v.id,
            book: v.book,
            zoom: v.zoom,
            scroll_y: v.scroll_y,
            pages: v.pages.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<xcpc_app::AboutInfoJs> for AboutInfoJs {
    fn from(v: xcpc_app::AboutInfoJs) -> Self {
        Self {
            version: v.version,
            commit: v.commit,
            build_date: v.build_date,
            is_dev: v.is_dev,
            os: v.os,
            arch: v.arch,
            font_source: v.font_source,
        }
    }
}

impl From<AboutInfoJs> for xcpc_app::AboutInfoJs {
    fn from(v: AboutInfoJs) -> Self {
        Self {
            version: v.version,
            commit: v.commit,
            build_date: v.build_date,
            is_dev: v.is_dev,
            os: v.os,
            arch: v.arch,
            font_source: v.font_source,
        }
    }
}

impl From<xcpc_app::TraceStatusJs> for TraceStatusJs {
    fn from(v: xcpc_app::TraceStatusJs) -> Self {
        Self {
            enabled: v.enabled,
            event_count: v.event_count,
            dropped_count: v.dropped_count,
            rust_now_us: v.rust_now_us,
        }
    }
}

impl From<TraceStatusJs> for xcpc_app::TraceStatusJs {
    fn from(v: TraceStatusJs) -> Self {
        Self {
            enabled: v.enabled,
            event_count: v.event_count,
            dropped_count: v.dropped_count,
            rust_now_us: v.rust_now_us,
        }
    }
}

impl From<xcpc_app::TraceExportResultJs> for TraceExportResultJs {
    fn from(v: xcpc_app::TraceExportResultJs) -> Self {
        Self {
            ok: v.ok,
            cancelled: v.cancelled,
            path: v.path,
            bytes: v.bytes,
            error: v.error,
        }
    }
}

impl From<TraceExportResultJs> for xcpc_app::TraceExportResultJs {
    fn from(v: TraceExportResultJs) -> Self {
        Self {
            ok: v.ok,
            cancelled: v.cancelled,
            path: v.path,
            bytes: v.bytes,
            error: v.error,
        }
    }
}

impl From<xcpc_app::DetectedCompilerJs> for DetectedCompilerJs {
    fn from(v: xcpc_app::DetectedCompilerJs) -> Self {
        Self {
            name: v.name,
            path: v.path,
            version: v.version,
            kind: v.kind,
        }
    }
}

impl From<DetectedCompilerJs> for xcpc_app::DetectedCompilerJs {
    fn from(v: DetectedCompilerJs) -> Self {
        Self {
            name: v.name,
            path: v.path,
            version: v.version,
            kind: v.kind,
        }
    }
}

impl From<xcpc_app::ToolchainStatusJs> for ToolchainStatusJs {
    fn from(v: xcpc_app::ToolchainStatusJs) -> Self {
        Self {
            compilers: v.compilers.into_iter().map(Into::into).collect(),
            active: v.active.map(Into::into),
            override_path: v.override_path,
            env_override: v.env_override,
            install_guide: v.install_guide,
            install_commands: v.install_commands.into_iter().map(Into::into).collect(),
            error: v.error,
        }
    }
}

impl From<ToolchainStatusJs> for xcpc_app::ToolchainStatusJs {
    fn from(v: ToolchainStatusJs) -> Self {
        Self {
            compilers: v.compilers.into_iter().map(Into::into).collect(),
            active: v.active.map(Into::into),
            override_path: v.override_path,
            env_override: v.env_override,
            install_guide: v.install_guide,
            install_commands: v.install_commands.into_iter().map(Into::into).collect(),
            error: v.error,
        }
    }
}

impl From<xcpc_app::InstallCommandJs> for InstallCommandJs {
    fn from(v: xcpc_app::InstallCommandJs) -> Self {
        Self {
            label: v.label,
            command: v.command,
        }
    }
}

impl From<InstallCommandJs> for xcpc_app::InstallCommandJs {
    fn from(v: InstallCommandJs) -> Self {
        Self {
            label: v.label,
            command: v.command,
        }
    }
}

impl From<xcpc_app::PackageInspectionResultJs> for PackageInspectionResultJs {
    fn from(v: xcpc_app::PackageInspectionResultJs) -> Self {
        Self {
            ok: v.ok,
            has_package: v.has_package,
            freshness_status: v.freshness_status,
            inspection_json: v.inspection_json,
            error: v.error,
        }
    }
}

impl From<PackageInspectionResultJs> for xcpc_app::PackageInspectionResultJs {
    fn from(v: PackageInspectionResultJs) -> Self {
        Self {
            ok: v.ok,
            has_package: v.has_package,
            freshness_status: v.freshness_status,
            inspection_json: v.inspection_json,
            error: v.error,
        }
    }
}

impl From<xcpc_app::PackageInitResultJs> for PackageInitResultJs {
    fn from(v: xcpc_app::PackageInitResultJs) -> Self {
        Self {
            ok: v.ok,
            created_files: v.created_files,
            skipped_files: v.skipped_files,
            updated_problem_toml: v.updated_problem_toml,
            inspection_json: v.inspection_json,
            error: v.error,
            code: v.code,
        }
    }
}

impl From<PackageInitResultJs> for xcpc_app::PackageInitResultJs {
    fn from(v: PackageInitResultJs) -> Self {
        Self {
            ok: v.ok,
            created_files: v.created_files,
            skipped_files: v.skipped_files,
            updated_problem_toml: v.updated_problem_toml,
            inspection_json: v.inspection_json,
            error: v.error,
            code: v.code,
        }
    }
}

impl From<xcpc_app::PackageBuildStatusJs> for PackageBuildStatusJs {
    fn from(v: xcpc_app::PackageBuildStatusJs) -> Self {
        Self {
            seq: v.seq,
            running: v.running,
            slug: v.slug,
            mode: v.mode,
            cancelled: v.cancelled,
            phase: v.phase,
            detail: v.detail,
            completed: v.completed,
            total: v.total,
            report_json: v.report_json,
            live_cells_json: v.live_cells_json,
            error: v.error,
        }
    }
}

impl From<PackageBuildStatusJs> for xcpc_app::PackageBuildStatusJs {
    fn from(v: PackageBuildStatusJs) -> Self {
        Self {
            seq: v.seq,
            running: v.running,
            slug: v.slug,
            mode: v.mode,
            cancelled: v.cancelled,
            phase: v.phase,
            detail: v.detail,
            completed: v.completed,
            total: v.total,
            report_json: v.report_json,
            live_cells_json: v.live_cells_json,
            error: v.error,
        }
    }
}

impl From<xcpc_app::TestCasePreviewJs> for TestCasePreviewJs {
    fn from(v: xcpc_app::TestCasePreviewJs) -> Self {
        Self {
            ok: v.ok,
            index: v.index,
            name: v.name,
            tag: v.tag,
            is_sample: v.is_sample,
            gen_cmd: v.gen_cmd,
            available_on_disk: v.available_on_disk,
            size_bytes: v.size_bytes,
            total_lines: v.total_lines,
            total_lines_exact: v.total_lines_exact,
            omitted_bytes: v.omitted_bytes,
            head: v.head,
            tail: v.tail,
            truncated: v.truncated,
            ans_available: v.ans_available,
            ans_source: v.ans_source,
            ans_size_bytes: v.ans_size_bytes,
            ans_head: v.ans_head,
            ans_tail: v.ans_tail,
            ans_truncated: v.ans_truncated,
            ans_total_lines: v.ans_total_lines,
            std_output_available: v.std_output_available,
            std_output_size_bytes: v.std_output_size_bytes,
            std_output_head: v.std_output_head,
            std_output_tail: v.std_output_tail,
            std_output_truncated: v.std_output_truncated,
            rule_index: v.rule_index,
            file_rel: v.file_rel,
            error: v.error,
        }
    }
}

impl From<TestCasePreviewJs> for xcpc_app::TestCasePreviewJs {
    fn from(v: TestCasePreviewJs) -> Self {
        Self {
            ok: v.ok,
            index: v.index,
            name: v.name,
            tag: v.tag,
            is_sample: v.is_sample,
            gen_cmd: v.gen_cmd,
            available_on_disk: v.available_on_disk,
            size_bytes: v.size_bytes,
            total_lines: v.total_lines,
            total_lines_exact: v.total_lines_exact,
            omitted_bytes: v.omitted_bytes,
            head: v.head,
            tail: v.tail,
            truncated: v.truncated,
            ans_available: v.ans_available,
            ans_source: v.ans_source,
            ans_size_bytes: v.ans_size_bytes,
            ans_head: v.ans_head,
            ans_tail: v.ans_tail,
            ans_truncated: v.ans_truncated,
            ans_total_lines: v.ans_total_lines,
            std_output_available: v.std_output_available,
            std_output_size_bytes: v.std_output_size_bytes,
            std_output_head: v.std_output_head,
            std_output_tail: v.std_output_tail,
            std_output_truncated: v.std_output_truncated,
            rule_index: v.rule_index,
            file_rel: v.file_rel,
            error: v.error,
        }
    }
}

impl From<xcpc_app::PackageFileContentJs> for PackageFileContentJs {
    fn from(v: xcpc_app::PackageFileContentJs) -> Self {
        Self {
            ok: v.ok,
            content: v.content,
            content_hash: v.content_hash,
            size_bytes: v.size_bytes,
            too_large: v.too_large,
            error: v.error,
            code: v.code,
        }
    }
}

impl From<PackageFileContentJs> for xcpc_app::PackageFileContentJs {
    fn from(v: PackageFileContentJs) -> Self {
        Self {
            ok: v.ok,
            content: v.content,
            content_hash: v.content_hash,
            size_bytes: v.size_bytes,
            too_large: v.too_large,
            error: v.error,
            code: v.code,
        }
    }
}

impl From<xcpc_app::SavePackageFileResultJs> for SavePackageFileResultJs {
    fn from(v: xcpc_app::SavePackageFileResultJs) -> Self {
        Self {
            ok: v.ok,
            conflict: v.conflict,
            content_hash: v.content_hash,
            error: v.error,
            code: v.code,
        }
    }
}

impl From<SavePackageFileResultJs> for xcpc_app::SavePackageFileResultJs {
    fn from(v: SavePackageFileResultJs) -> Self {
        Self {
            ok: v.ok,
            conflict: v.conflict,
            content_hash: v.content_hash,
            error: v.error,
            code: v.code,
        }
    }
}

impl From<xcpc_app::PackageCreateResultJs> for PackageCreateResultJs {
    fn from(v: xcpc_app::PackageCreateResultJs) -> Self {
        Self {
            ok: v.ok,
            rel_path: v.rel_path,
            error: v.error,
            code: v.code,
        }
    }
}

impl From<PackageCreateResultJs> for xcpc_app::PackageCreateResultJs {
    fn from(v: PackageCreateResultJs) -> Self {
        Self {
            ok: v.ok,
            rel_path: v.rel_path,
            error: v.error,
            code: v.code,
        }
    }
}

impl From<xcpc_app::AddedTestResultJs> for AddedTestResultJs {
    fn from(v: xcpc_app::AddedTestResultJs) -> Self {
        Self {
            ok: v.ok,
            rule_index: v.rule_index,
            file_rel: v.file_rel,
            name: v.name,
            tag: v.tag,
            error: v.error,
            code: v.code,
        }
    }
}

impl From<AddedTestResultJs> for xcpc_app::AddedTestResultJs {
    fn from(v: AddedTestResultJs) -> Self {
        Self {
            ok: v.ok,
            rule_index: v.rule_index,
            file_rel: v.file_rel,
            name: v.name,
            tag: v.tag,
            error: v.error,
            code: v.code,
        }
    }
}

impl From<xcpc_app::NormalizeDirtyResultJs> for NormalizeDirtyResultJs {
    fn from(v: xcpc_app::NormalizeDirtyResultJs) -> Self {
        Self {
            ok: v.ok,
            fixed: v.fixed,
            error: v.error,
            code: v.code,
        }
    }
}

impl From<NormalizeDirtyResultJs> for xcpc_app::NormalizeDirtyResultJs {
    fn from(v: NormalizeDirtyResultJs) -> Self {
        Self {
            ok: v.ok,
            fixed: v.fixed,
            error: v.error,
            code: v.code,
        }
    }
}

impl From<xcpc_app::PackageWarningJs> for PackageWarningJs {
    fn from(v: xcpc_app::PackageWarningJs) -> Self {
        Self {
            code: v.code,
            message: v.message,
            path: v.path,
        }
    }
}

impl From<PackageWarningJs> for xcpc_app::PackageWarningJs {
    fn from(v: PackageWarningJs) -> Self {
        Self {
            code: v.code,
            message: v.message,
            path: v.path,
        }
    }
}

impl From<xcpc_app::PolygonImportResultJs> for PolygonImportResultJs {
    fn from(v: xcpc_app::PolygonImportResultJs) -> Self {
        Self {
            ok: v.ok,
            cancelled: v.cancelled,
            kind: v.kind,
            id: v.id,
            title: v.title,
            problem_ids: v.problem_ids,
            warnings: v.warnings.into_iter().map(Into::into).collect(),
            error: v.error,
            code: v.code,
            detail: v.detail,
        }
    }
}

impl From<PolygonImportResultJs> for xcpc_app::PolygonImportResultJs {
    fn from(v: PolygonImportResultJs) -> Self {
        Self {
            ok: v.ok,
            cancelled: v.cancelled,
            kind: v.kind,
            id: v.id,
            title: v.title,
            problem_ids: v.problem_ids,
            warnings: v.warnings.into_iter().map(Into::into).collect(),
            error: v.error,
            code: v.code,
            detail: v.detail,
        }
    }
}

impl From<xcpc_app::ExportOjResultJs> for ExportOjResultJs {
    fn from(v: xcpc_app::ExportOjResultJs) -> Self {
        Self {
            ok: v.ok,
            format: v.format,
            paths: v.paths,
            reveal_path: v.reveal_path,
            warnings: v.warnings.into_iter().map(Into::into).collect(),
            error: v.error,
            code: v.code,
            detail: v.detail,
        }
    }
}

impl From<ExportOjResultJs> for xcpc_app::ExportOjResultJs {
    fn from(v: ExportOjResultJs) -> Self {
        Self {
            ok: v.ok,
            format: v.format,
            paths: v.paths,
            reveal_path: v.reveal_path,
            warnings: v.warnings.into_iter().map(Into::into).collect(),
            error: v.error,
            code: v.code,
            detail: v.detail,
        }
    }
}

impl From<xcpc_app::BatchItemResultJs> for BatchItemResultJs {
    fn from(v: xcpc_app::BatchItemResultJs) -> Self {
        Self {
            id: v.id,
            status: v.status,
            code: v.code,
            message: v.message,
        }
    }
}

impl From<BatchItemResultJs> for xcpc_app::BatchItemResultJs {
    fn from(v: BatchItemResultJs) -> Self {
        Self {
            id: v.id,
            status: v.status,
            code: v.code,
            message: v.message,
        }
    }
}

impl From<xcpc_app::BatchMutationResultJs> for BatchMutationResultJs {
    fn from(v: xcpc_app::BatchMutationResultJs) -> Self {
        Self {
            ok: v.ok,
            total: v.total,
            succeeded: v.succeeded,
            skipped: v.skipped,
            failed: v.failed,
            items: v.items.into_iter().map(Into::into).collect(),
            error: v.error,
            code: v.code,
        }
    }
}

impl From<BatchMutationResultJs> for xcpc_app::BatchMutationResultJs {
    fn from(v: BatchMutationResultJs) -> Self {
        Self {
            ok: v.ok,
            total: v.total,
            succeeded: v.succeeded,
            skipped: v.skipped,
            failed: v.failed,
            items: v.items.into_iter().map(Into::into).collect(),
            error: v.error,
            code: v.code,
        }
    }
}

impl From<xcpc_app::BatchDeleteItemJs> for BatchDeleteItemJs {
    fn from(v: xcpc_app::BatchDeleteItemJs) -> Self {
        Self {
            id: v.id,
            status: v.status,
            code: v.code,
            message: v.message,
            referencing_contests: v.referencing_contests,
        }
    }
}

impl From<BatchDeleteItemJs> for xcpc_app::BatchDeleteItemJs {
    fn from(v: BatchDeleteItemJs) -> Self {
        Self {
            id: v.id,
            status: v.status,
            code: v.code,
            message: v.message,
            referencing_contests: v.referencing_contests,
        }
    }
}

impl From<xcpc_app::BatchDeleteResultJs> for BatchDeleteResultJs {
    fn from(v: xcpc_app::BatchDeleteResultJs) -> Self {
        Self {
            ok: v.ok,
            total: v.total,
            deleted: v.deleted,
            skipped: v.skipped,
            failed: v.failed,
            items: v.items.into_iter().map(Into::into).collect(),
            error: v.error,
            code: v.code,
        }
    }
}

impl From<BatchDeleteResultJs> for xcpc_app::BatchDeleteResultJs {
    fn from(v: BatchDeleteResultJs) -> Self {
        Self {
            ok: v.ok,
            total: v.total,
            deleted: v.deleted,
            skipped: v.skipped,
            failed: v.failed,
            items: v.items.into_iter().map(Into::into).collect(),
            error: v.error,
            code: v.code,
        }
    }
}

impl From<xcpc_app::BatchExportRequestJs> for BatchExportRequestJs {
    fn from(v: xcpc_app::BatchExportRequestJs) -> Self {
        Self {
            format: v.format,
            problem_ids: v.problem_ids,
            title: v.title,
            language: v.language,
            book: v.book,
            output_path: v.output_path,
        }
    }
}

impl From<BatchExportRequestJs> for xcpc_app::BatchExportRequestJs {
    fn from(v: BatchExportRequestJs) -> Self {
        Self {
            format: v.format,
            problem_ids: v.problem_ids,
            title: v.title,
            language: v.language,
            book: v.book,
            output_path: v.output_path,
        }
    }
}

impl From<xcpc_app::BatchStartResultJs> for BatchStartResultJs {
    fn from(v: xcpc_app::BatchStartResultJs) -> Self {
        Self {
            ok: v.ok,
            task_id: v.task_id,
            error: v.error,
            code: v.code,
        }
    }
}

impl From<BatchStartResultJs> for xcpc_app::BatchStartResultJs {
    fn from(v: BatchStartResultJs) -> Self {
        Self {
            ok: v.ok,
            task_id: v.task_id,
            error: v.error,
            code: v.code,
        }
    }
}

impl From<xcpc_app::BatchTaskItemJs> for BatchTaskItemJs {
    fn from(v: xcpc_app::BatchTaskItemJs) -> Self {
        Self {
            problem_id: v.problem_id,
            status: v.status,
            verified: v.verified,
            elapsed_ms: v.elapsed_ms,
            phase: v.phase,
            detail: v.detail,
            code: v.code,
            message: v.message,
            output_path: v.output_path,
        }
    }
}

impl From<BatchTaskItemJs> for xcpc_app::BatchTaskItemJs {
    fn from(v: BatchTaskItemJs) -> Self {
        Self {
            problem_id: v.problem_id,
            status: v.status,
            verified: v.verified,
            elapsed_ms: v.elapsed_ms,
            phase: v.phase,
            detail: v.detail,
            code: v.code,
            message: v.message,
            output_path: v.output_path,
        }
    }
}

impl From<xcpc_app::BatchTaskSnapshotJs> for BatchTaskSnapshotJs {
    fn from(v: xcpc_app::BatchTaskSnapshotJs) -> Self {
        Self {
            task_id: v.task_id,
            running: v.running,
            kind: v.kind,
            cancelled: v.cancelled,
            total: v.total,
            completed: v.completed,
            succeeded: v.succeeded,
            failed: v.failed,
            skipped: v.skipped,
            cancelled_count: v.cancelled_count,
            elapsed_ms: v.elapsed_ms,
            current_problem_id: v.current_problem_id,
            current_phase: v.current_phase,
            current_detail: v.current_detail,
            output_path: v.output_path,
            items: v.items.into_iter().map(Into::into).collect(),
            error: v.error,
            code: v.code,
        }
    }
}

impl From<BatchTaskSnapshotJs> for xcpc_app::BatchTaskSnapshotJs {
    fn from(v: BatchTaskSnapshotJs) -> Self {
        Self {
            task_id: v.task_id,
            running: v.running,
            kind: v.kind,
            cancelled: v.cancelled,
            total: v.total,
            completed: v.completed,
            succeeded: v.succeeded,
            failed: v.failed,
            skipped: v.skipped,
            cancelled_count: v.cancelled_count,
            elapsed_ms: v.elapsed_ms,
            current_problem_id: v.current_problem_id,
            current_phase: v.current_phase,
            current_detail: v.current_detail,
            output_path: v.output_path,
            items: v.items.into_iter().map(Into::into).collect(),
            error: v.error,
            code: v.code,
        }
    }
}

impl From<xcpc_app::BatchTaskEventJs> for BatchTaskEventJs {
    fn from(v: xcpc_app::BatchTaskEventJs) -> Self {
        Self {
            kind: v.kind,
            task_id: v.task_id,
            problem_id: v.problem_id,
            index: v.index,
            total: v.total,
            phase: v.phase,
            detail: v.detail,
            item: v.item.map(Into::into),
            snapshot: v.snapshot.into(),
            error: v.error,
            code: v.code,
        }
    }
}

impl From<BatchTaskEventJs> for xcpc_app::BatchTaskEventJs {
    fn from(v: BatchTaskEventJs) -> Self {
        Self {
            kind: v.kind,
            task_id: v.task_id,
            problem_id: v.problem_id,
            index: v.index,
            total: v.total,
            phase: v.phase,
            detail: v.detail,
            item: v.item.map(Into::into),
            snapshot: v.snapshot.into(),
            error: v.error,
            code: v.code,
        }
    }
}

impl From<xcpc_app::PickedPathsJs> for PickedPathsJs {
    fn from(v: xcpc_app::PickedPathsJs) -> Self {
        Self {
            cancelled: v.cancelled,
            paths: v.paths,
            error: v.error,
        }
    }
}

impl From<PickedPathsJs> for xcpc_app::PickedPathsJs {
    fn from(v: PickedPathsJs) -> Self {
        Self {
            cancelled: v.cancelled,
            paths: v.paths,
            error: v.error,
        }
    }
}

impl From<xcpc_app::JsonReportResultJs> for JsonReportResultJs {
    fn from(v: xcpc_app::JsonReportResultJs) -> Self {
        Self {
            ok: v.ok,
            report_json: v.report_json,
            markdown: v.markdown,
            error: v.error,
            code: v.code,
        }
    }
}

impl From<JsonReportResultJs> for xcpc_app::JsonReportResultJs {
    fn from(v: JsonReportResultJs) -> Self {
        Self {
            ok: v.ok,
            report_json: v.report_json,
            markdown: v.markdown,
            error: v.error,
            code: v.code,
        }
    }
}

impl From<xcpc_app::AdoptSolutionResultJs> for AdoptSolutionResultJs {
    fn from(v: xcpc_app::AdoptSolutionResultJs) -> Self {
        Self {
            ok: v.ok,
            problem_id: v.problem_id,
            solution_name: v.solution_name,
            target_rel_path: v.target_rel_path,
            updated_problem_toml: v.updated_problem_toml,
            error: v.error,
            code: v.code,
        }
    }
}

impl From<AdoptSolutionResultJs> for xcpc_app::AdoptSolutionResultJs {
    fn from(v: AdoptSolutionResultJs) -> Self {
        Self {
            ok: v.ok,
            problem_id: v.problem_id,
            solution_name: v.solution_name,
            target_rel_path: v.target_rel_path,
            updated_problem_toml: v.updated_problem_toml,
            error: v.error,
            code: v.code,
        }
    }
}

impl From<xcpc_app::SaveReportResultJs> for SaveReportResultJs {
    fn from(v: xcpc_app::SaveReportResultJs) -> Self {
        Self {
            ok: v.ok,
            cancelled: v.cancelled,
            path: v.path,
            error: v.error,
        }
    }
}

impl From<SaveReportResultJs> for xcpc_app::SaveReportResultJs {
    fn from(v: SaveReportResultJs) -> Self {
        Self {
            ok: v.ok,
            cancelled: v.cancelled,
            path: v.path,
            error: v.error,
        }
    }
}

impl From<xcpc_app::ResolvedProblemUrlJs> for ResolvedProblemUrlJs {
    fn from(v: xcpc_app::ResolvedProblemUrlJs) -> Self {
        Self {
            ok: v.ok,
            platform: v.platform,
            problem_key: v.problem_key,
            suggested_slug: v.suggested_slug,
            title: v.title,
            tags: v.tags,
            rating: v.rating,
            url: v.url,
            existing_problem_id: v.existing_problem_id,
            error: v.error,
        }
    }
}

impl From<ResolvedProblemUrlJs> for xcpc_app::ResolvedProblemUrlJs {
    fn from(v: ResolvedProblemUrlJs) -> Self {
        Self {
            ok: v.ok,
            platform: v.platform,
            problem_key: v.problem_key,
            suggested_slug: v.suggested_slug,
            title: v.title,
            tags: v.tags,
            rating: v.rating,
            url: v.url,
            existing_problem_id: v.existing_problem_id,
            error: v.error,
        }
    }
}

impl From<xcpc_app::CreateProblemFromUrlResultJs> for CreateProblemFromUrlResultJs {
    fn from(v: xcpc_app::CreateProblemFromUrlResultJs) -> Self {
        Self {
            ok: v.ok,
            problem_id: v.problem_id,
            existed: v.existed,
            code: v.code,
            error: v.error,
        }
    }
}

impl From<CreateProblemFromUrlResultJs> for xcpc_app::CreateProblemFromUrlResultJs {
    fn from(v: CreateProblemFromUrlResultJs) -> Self {
        Self {
            ok: v.ok,
            problem_id: v.problem_id,
            existed: v.existed,
            code: v.code,
            error: v.error,
        }
    }
}

impl From<xcpc_app::PluginCredentialFieldJs> for PluginCredentialFieldJs {
    fn from(v: xcpc_app::PluginCredentialFieldJs) -> Self {
        Self {
            name: v.name,
            label: v.label,
            secret: v.secret,
            has_value: v.has_value,
            description: v.description,
            placeholder: v.placeholder,
        }
    }
}

impl From<PluginCredentialFieldJs> for xcpc_app::PluginCredentialFieldJs {
    fn from(v: PluginCredentialFieldJs) -> Self {
        Self {
            name: v.name,
            label: v.label,
            secret: v.secret,
            has_value: v.has_value,
            description: v.description,
            placeholder: v.placeholder,
        }
    }
}

impl From<xcpc_app::PluginSummaryJs> for PluginSummaryJs {
    fn from(v: xcpc_app::PluginSummaryJs) -> Self {
        Self {
            id: v.id,
            name: v.name,
            version: v.version,
            description: v.description,
            origin: v.origin,
            enabled: v.enabled,
            pending_trust: v.pending_trust,
            overrides_builtin: v.overrides_builtin,
            has_network: v.has_network,
            capabilities: v.capabilities,
            domains: v.domains,
            workspace_read: v.workspace_read,
            has_ui: v.has_ui,
            is_linked: v.is_linked,
            link_target: v.link_target,
            source_repo: v.source_repo,
            credentials: v.credentials.into_iter().map(Into::into).collect(),
            handle: v.handle,
            last_error: v.last_error,
        }
    }
}

impl From<PluginSummaryJs> for xcpc_app::PluginSummaryJs {
    fn from(v: PluginSummaryJs) -> Self {
        Self {
            id: v.id,
            name: v.name,
            version: v.version,
            description: v.description,
            origin: v.origin,
            enabled: v.enabled,
            pending_trust: v.pending_trust,
            overrides_builtin: v.overrides_builtin,
            has_network: v.has_network,
            capabilities: v.capabilities,
            domains: v.domains,
            workspace_read: v.workspace_read,
            has_ui: v.has_ui,
            is_linked: v.is_linked,
            link_target: v.link_target,
            source_repo: v.source_repo,
            credentials: v.credentials.into_iter().map(Into::into).collect(),
            handle: v.handle,
            last_error: v.last_error,
        }
    }
}

impl From<xcpc_app::PluginRepoConfigJs> for PluginRepoConfigJs {
    fn from(v: xcpc_app::PluginRepoConfigJs) -> Self {
        Self {
            url: v.url,
            name: v.name,
        }
    }
}

impl From<PluginRepoConfigJs> for xcpc_app::PluginRepoConfigJs {
    fn from(v: PluginRepoConfigJs) -> Self {
        Self {
            url: v.url,
            name: v.name,
        }
    }
}

impl From<xcpc_app::PluginsOverviewJs> for PluginsOverviewJs {
    fn from(v: xcpc_app::PluginsOverviewJs) -> Self {
        Self {
            plugins: v.plugins.into_iter().map(Into::into).collect(),
            repos: v.repos.into_iter().map(Into::into).collect(),
            auto_sync_on_startup: v.auto_sync_on_startup,
            last_synced_at: v.last_synced_at,
            error: v.error,
        }
    }
}

impl From<PluginsOverviewJs> for xcpc_app::PluginsOverviewJs {
    fn from(v: PluginsOverviewJs) -> Self {
        Self {
            plugins: v.plugins.into_iter().map(Into::into).collect(),
            repos: v.repos.into_iter().map(Into::into).collect(),
            auto_sync_on_startup: v.auto_sync_on_startup,
            last_synced_at: v.last_synced_at,
            error: v.error,
        }
    }
}

impl From<xcpc_app::ExportPluginResultJs> for ExportPluginResultJs {
    fn from(v: xcpc_app::ExportPluginResultJs) -> Self {
        Self {
            ok: v.ok,
            path: v.path,
            error: v.error,
        }
    }
}

impl From<ExportPluginResultJs> for xcpc_app::ExportPluginResultJs {
    fn from(v: ExportPluginResultJs) -> Self {
        Self {
            ok: v.ok,
            path: v.path,
            error: v.error,
        }
    }
}

impl From<xcpc_app::InstallPluginResultJs> for InstallPluginResultJs {
    fn from(v: xcpc_app::InstallPluginResultJs) -> Self {
        Self {
            ok: v.ok,
            cancelled: v.cancelled,
            plugin_id: v.plugin_id,
            overview: v.overview.map(Into::into),
            error: v.error,
        }
    }
}

impl From<InstallPluginResultJs> for xcpc_app::InstallPluginResultJs {
    fn from(v: InstallPluginResultJs) -> Self {
        Self {
            ok: v.ok,
            cancelled: v.cancelled,
            plugin_id: v.plugin_id,
            overview: v.overview.map(Into::into),
            error: v.error,
        }
    }
}

impl From<xcpc_app::RepoPluginPermissionJs> for RepoPluginPermissionJs {
    fn from(v: xcpc_app::RepoPluginPermissionJs) -> Self {
        Self {
            domains: v.domains,
            workspace_read: v.workspace_read,
        }
    }
}

impl From<RepoPluginPermissionJs> for xcpc_app::RepoPluginPermissionJs {
    fn from(v: RepoPluginPermissionJs) -> Self {
        Self {
            domains: v.domains,
            workspace_read: v.workspace_read,
        }
    }
}

impl From<xcpc_app::RepoPluginEntryJs> for RepoPluginEntryJs {
    fn from(v: xcpc_app::RepoPluginEntryJs) -> Self {
        Self {
            id: v.id,
            name: v.name,
            version: v.version,
            description: v.description,
            download_url: v.download_url,
            sha256: v.sha256,
            capabilities: v.capabilities,
            has_ui: v.has_ui,
            official: v.official,
            permissions: v.permissions.map(Into::into),
        }
    }
}

impl From<RepoPluginEntryJs> for xcpc_app::RepoPluginEntryJs {
    fn from(v: RepoPluginEntryJs) -> Self {
        Self {
            id: v.id,
            name: v.name,
            version: v.version,
            description: v.description,
            download_url: v.download_url,
            sha256: v.sha256,
            capabilities: v.capabilities,
            has_ui: v.has_ui,
            official: v.official,
            permissions: v.permissions.map(Into::into),
        }
    }
}

impl From<xcpc_app::RepoCatalogResultJs> for RepoCatalogResultJs {
    fn from(v: xcpc_app::RepoCatalogResultJs) -> Self {
        Self {
            ok: v.ok,
            name: v.name,
            plugins: v.plugins.into_iter().map(Into::into).collect(),
            error: v.error,
        }
    }
}

impl From<RepoCatalogResultJs> for xcpc_app::RepoCatalogResultJs {
    fn from(v: RepoCatalogResultJs) -> Self {
        Self {
            ok: v.ok,
            name: v.name,
            plugins: v.plugins.into_iter().map(Into::into).collect(),
            error: v.error,
        }
    }
}

impl From<xcpc_app::RepoFetchErrorJs> for RepoFetchErrorJs {
    fn from(v: xcpc_app::RepoFetchErrorJs) -> Self {
        Self {
            repo_url: v.repo_url,
            repo_name: v.repo_name,
            error: v.error,
        }
    }
}

impl From<RepoFetchErrorJs> for xcpc_app::RepoFetchErrorJs {
    fn from(v: RepoFetchErrorJs) -> Self {
        Self {
            repo_url: v.repo_url,
            repo_name: v.repo_name,
            error: v.error,
        }
    }
}

impl From<xcpc_app::AggregatedRepoPluginJs> for AggregatedRepoPluginJs {
    fn from(v: xcpc_app::AggregatedRepoPluginJs) -> Self {
        Self {
            repo_url: v.repo_url,
            repo_name: v.repo_name,
            entry: v.entry.into(),
        }
    }
}

impl From<AggregatedRepoPluginJs> for xcpc_app::AggregatedRepoPluginJs {
    fn from(v: AggregatedRepoPluginJs) -> Self {
        Self {
            repo_url: v.repo_url,
            repo_name: v.repo_name,
            entry: v.entry.into(),
        }
    }
}

impl From<xcpc_app::PluginRepoCatalogJs> for PluginRepoCatalogJs {
    fn from(v: xcpc_app::PluginRepoCatalogJs) -> Self {
        Self {
            ok: v.ok,
            plugins: v.plugins.into_iter().map(Into::into).collect(),
            errors: v.errors.into_iter().map(Into::into).collect(),
            error: v.error,
        }
    }
}

impl From<PluginRepoCatalogJs> for xcpc_app::PluginRepoCatalogJs {
    fn from(v: PluginRepoCatalogJs) -> Self {
        Self {
            ok: v.ok,
            plugins: v.plugins.into_iter().map(Into::into).collect(),
            errors: v.errors.into_iter().map(Into::into).collect(),
            error: v.error,
        }
    }
}

impl From<xcpc_app::VerifyPluginResultJs> for VerifyPluginResultJs {
    fn from(v: xcpc_app::VerifyPluginResultJs) -> Self {
        Self {
            ok: v.ok,
            message: v.message,
            error: v.error,
        }
    }
}

impl From<VerifyPluginResultJs> for xcpc_app::VerifyPluginResultJs {
    fn from(v: VerifyPluginResultJs) -> Self {
        Self {
            ok: v.ok,
            message: v.message,
            error: v.error,
        }
    }
}

impl From<xcpc_app::PlatformSyncReportJs> for PlatformSyncReportJs {
    fn from(v: xcpc_app::PlatformSyncReportJs) -> Self {
        Self {
            platform: v.platform,
            handle: v.handle,
            ok: v.ok,
            account_synced: v.account_synced,
            summary_synced: v.summary_synced,
            submissions_synced: v.submissions_synced,
            rating_history_synced: v.rating_history_synced,
            contests_synced: v.contests_synced,
            error: v.error,
        }
    }
}

impl From<PlatformSyncReportJs> for xcpc_app::PlatformSyncReportJs {
    fn from(v: PlatformSyncReportJs) -> Self {
        Self {
            platform: v.platform,
            handle: v.handle,
            ok: v.ok,
            account_synced: v.account_synced,
            summary_synced: v.summary_synced,
            submissions_synced: v.submissions_synced,
            rating_history_synced: v.rating_history_synced,
            contests_synced: v.contests_synced,
            error: v.error,
        }
    }
}

impl From<xcpc_app::SyncReportJs> for SyncReportJs {
    fn from(v: xcpc_app::SyncReportJs) -> Self {
        Self {
            ok: v.ok,
            synced_at: v.synced_at,
            platforms: v.platforms.into_iter().map(Into::into).collect(),
            error: v.error,
        }
    }
}

impl From<SyncReportJs> for xcpc_app::SyncReportJs {
    fn from(v: SyncReportJs) -> Self {
        Self {
            ok: v.ok,
            synced_at: v.synced_at,
            platforms: v.platforms.into_iter().map(Into::into).collect(),
            error: v.error,
        }
    }
}

impl From<xcpc_app::TrainingSubmissionsQueryJs> for TrainingSubmissionsQueryJs {
    fn from(v: xcpc_app::TrainingSubmissionsQueryJs) -> Self {
        Self {
            by_problem: v.by_problem,
            platform: v.platform,
            verdict: v.verdict,
            unsolved_only: v.unsolved_only,
            min_rating: v.min_rating,
            max_rating: v.max_rating,
            tag: v.tag,
            search: v.search,
            limit: v.limit,
            offset: v.offset,
        }
    }
}

impl From<TrainingSubmissionsQueryJs> for xcpc_app::TrainingSubmissionsQueryJs {
    fn from(v: TrainingSubmissionsQueryJs) -> Self {
        Self {
            by_problem: v.by_problem,
            platform: v.platform,
            verdict: v.verdict,
            unsolved_only: v.unsolved_only,
            min_rating: v.min_rating,
            max_rating: v.max_rating,
            tag: v.tag,
            search: v.search,
            limit: v.limit,
            offset: v.offset,
        }
    }
}

impl From<xcpc_app::IngestProblemRequestJs> for IngestProblemRequestJs {
    fn from(v: xcpc_app::IngestProblemRequestJs) -> Self {
        Self {
            url: v.url,
            platform: v.platform,
            problem_key: v.problem_key,
            id: v.id,
            title: v.title,
            tags: v.tags,
            rating: v.rating,
            initial_status: v.initial_status,
        }
    }
}

impl From<IngestProblemRequestJs> for xcpc_app::IngestProblemRequestJs {
    fn from(v: IngestProblemRequestJs) -> Self {
        Self {
            url: v.url,
            platform: v.platform,
            problem_key: v.problem_key,
            id: v.id,
            title: v.title,
            tags: v.tags,
            rating: v.rating,
            initial_status: v.initial_status,
        }
    }
}

impl From<xcpc_app::IngestProblemResultJs> for IngestProblemResultJs {
    fn from(v: xcpc_app::IngestProblemResultJs) -> Self {
        Self {
            ok: v.ok,
            problem_id: v.problem_id,
            created: v.created,
            title: v.title,
            code: v.code,
            error: v.error,
        }
    }
}

impl From<IngestProblemResultJs> for xcpc_app::IngestProblemResultJs {
    fn from(v: IngestProblemResultJs) -> Self {
        Self {
            ok: v.ok,
            problem_id: v.problem_id,
            created: v.created,
            title: v.title,
            code: v.code,
            error: v.error,
        }
    }
}

impl From<xcpc_app::ManualLogResultJs> for ManualLogResultJs {
    fn from(v: xcpc_app::ManualLogResultJs) -> Self {
        Self {
            ok: v.ok,
            library_problem_id: v.library_problem_id,
            created_in_library: v.created_in_library,
            error: v.error,
        }
    }
}

impl From<ManualLogResultJs> for xcpc_app::ManualLogResultJs {
    fn from(v: ManualLogResultJs) -> Self {
        Self {
            ok: v.ok,
            library_problem_id: v.library_problem_id,
            created_in_library: v.created_in_library,
            error: v.error,
        }
    }
}

impl From<xcpc_app::EditorialSummaryJs> for EditorialSummaryJs {
    fn from(v: xcpc_app::EditorialSummaryJs) -> Self {
        Self {
            problem_id: v.problem_id,
            title: v.title,
            rating: v.rating,
            tags: v.tags,
            sources: v.sources.into_iter().map(Into::into).collect(),
            excerpt: v.excerpt,
            updated_at: v.updated_at,
        }
    }
}

impl From<EditorialSummaryJs> for xcpc_app::EditorialSummaryJs {
    fn from(v: EditorialSummaryJs) -> Self {
        Self {
            problem_id: v.problem_id,
            title: v.title,
            rating: v.rating,
            tags: v.tags,
            sources: v.sources.into_iter().map(Into::into).collect(),
            excerpt: v.excerpt,
            updated_at: v.updated_at,
        }
    }
}

impl From<xcpc_app::EditorialListResultJs> for EditorialListResultJs {
    fn from(v: xcpc_app::EditorialListResultJs) -> Self {
        Self {
            items: v.items.into_iter().map(Into::into).collect(),
            total: v.total,
            available_tags: v.available_tags,
            error: v.error,
        }
    }
}

impl From<EditorialListResultJs> for xcpc_app::EditorialListResultJs {
    fn from(v: EditorialListResultJs) -> Self {
        Self {
            items: v.items.into_iter().map(Into::into).collect(),
            total: v.total,
            available_tags: v.available_tags,
            error: v.error,
        }
    }
}

impl From<xcpc_app::PluginUiHtmlJs> for PluginUiHtmlJs {
    fn from(v: xcpc_app::PluginUiHtmlJs) -> Self {
        Self {
            ok: v.ok,
            html: v.html,
            plugin_id: v.plugin_id,
            workspace_read: v.workspace_read,
            error: v.error,
            error_code: v.error_code,
        }
    }
}

impl From<PluginUiHtmlJs> for xcpc_app::PluginUiHtmlJs {
    fn from(v: PluginUiHtmlJs) -> Self {
        Self {
            ok: v.ok,
            html: v.html,
            plugin_id: v.plugin_id,
            workspace_read: v.workspace_read,
            error: v.error,
            error_code: v.error_code,
        }
    }
}

impl From<xcpc_app::PluginWorkspaceResultJs> for PluginWorkspaceResultJs {
    fn from(v: xcpc_app::PluginWorkspaceResultJs) -> Self {
        Self {
            ok: v.ok,
            data_json: v.data_json,
            error: v.error,
            error_code: v.error_code,
        }
    }
}

impl From<PluginWorkspaceResultJs> for xcpc_app::PluginWorkspaceResultJs {
    fn from(v: PluginWorkspaceResultJs) -> Self {
        Self {
            ok: v.ok,
            data_json: v.data_json,
            error: v.error,
            error_code: v.error_code,
        }
    }
}

impl From<xcpc_app::PluginRenderPngResultJs> for PluginRenderPngResultJs {
    fn from(v: xcpc_app::PluginRenderPngResultJs) -> Self {
        Self {
            ok: v.ok,
            png_base64: v.png_base64,
            error: v.error,
        }
    }
}

impl From<PluginRenderPngResultJs> for xcpc_app::PluginRenderPngResultJs {
    fn from(v: PluginRenderPngResultJs) -> Self {
        Self {
            ok: v.ok,
            png_base64: v.png_base64,
            error: v.error,
        }
    }
}

impl From<xcpc_app::PluginKvResultJs> for PluginKvResultJs {
    fn from(v: xcpc_app::PluginKvResultJs) -> Self {
        Self {
            ok: v.ok,
            found: v.found,
            value_json: v.value_json,
            error: v.error,
        }
    }
}

impl From<PluginKvResultJs> for xcpc_app::PluginKvResultJs {
    fn from(v: PluginKvResultJs) -> Self {
        Self {
            ok: v.ok,
            found: v.found,
            value_json: v.value_json,
            error: v.error,
        }
    }
}

impl From<xcpc_app::PluginWatchEventJs> for PluginWatchEventJs {
    fn from(v: xcpc_app::PluginWatchEventJs) -> Self {
        Self {
            kind: v.kind,
            paths: v.paths,
            changed_plugin_ids: v.changed_plugin_ids,
        }
    }
}

impl From<PluginWatchEventJs> for xcpc_app::PluginWatchEventJs {
    fn from(v: PluginWatchEventJs) -> Self {
        Self {
            kind: v.kind,
            paths: v.paths,
            changed_plugin_ids: v.changed_plugin_ids,
        }
    }
}

impl From<xcpc_app::PluginPickFileResultJs> for PluginPickFileResultJs {
    fn from(v: xcpc_app::PluginPickFileResultJs) -> Self {
        Self {
            ok: v.ok,
            cancelled: v.cancelled,
            name: v.name,
            text: v.text,
            content_base64: v.content_base64,
            error: v.error,
        }
    }
}

impl From<PluginPickFileResultJs> for xcpc_app::PluginPickFileResultJs {
    fn from(v: PluginPickFileResultJs) -> Self {
        Self {
            ok: v.ok,
            cancelled: v.cancelled,
            name: v.name,
            text: v.text,
            content_base64: v.content_base64,
            error: v.error,
        }
    }
}

impl From<xcpc_app::ThemeColorEntryJs> for ThemeColorEntryJs {
    fn from(v: xcpc_app::ThemeColorEntryJs) -> Self {
        Self {
            key: v.key,
            value: v.value,
        }
    }
}

impl From<ThemeColorEntryJs> for xcpc_app::ThemeColorEntryJs {
    fn from(v: ThemeColorEntryJs) -> Self {
        Self {
            key: v.key,
            value: v.value,
        }
    }
}

impl From<xcpc_app::ThemeOverridesJs> for ThemeOverridesJs {
    fn from(v: xcpc_app::ThemeOverridesJs) -> Self {
        Self {
            light_colors: v.light_colors.into_iter().map(Into::into).collect(),
            dark_colors: v.dark_colors.into_iter().map(Into::into).collect(),
            radius: v.radius,
            editor_font_family: v.editor_font_family,
            editor_font_size: v.editor_font_size,
            editor_line_height: v.editor_line_height,
            active_path: v.active_path,
        }
    }
}

impl From<ThemeOverridesJs> for xcpc_app::ThemeOverridesJs {
    fn from(v: ThemeOverridesJs) -> Self {
        Self {
            light_colors: v.light_colors.into_iter().map(Into::into).collect(),
            dark_colors: v.dark_colors.into_iter().map(Into::into).collect(),
            radius: v.radius,
            editor_font_family: v.editor_font_family,
            editor_font_size: v.editor_font_size,
            editor_line_height: v.editor_line_height,
            active_path: v.active_path,
        }
    }
}

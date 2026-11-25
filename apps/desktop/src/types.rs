//! JavaScript-facing data transfer objects (`#[derive(JsObject)]` and tri-state patch
//! types) for `xcpc-workstation/native`.
//!
//! These types are reflected directly into `.nsh/types/xcpc-workstation/native.d.ts` by
//! `nsh check`.

use std::collections::BTreeMap;

use nsh_core::element::{JsHost, JsIn, JsKind, JsOwned, JsValue, PropError, TsTypes};
use nsh_core::prelude::*;
use xcpc_convert::publish::{self, Kind, Target};
use xcpc_domain::{
    BatchItemStatus, BatchReport, ContestMetaPatch, DiffKind, Language, Limits, LimitsPatch,
    OverridePatch, Patch, ProblemMetaPatch, RatingTier, Source, TemplateOptionValue,
    TemplateOptionsPatch,
};

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

impl PreviewRequest {
    pub(crate) fn target(&self) -> Target {
        match self.target.as_str() {
            "contest" => Target::Contest(self.id.clone()),
            _ => Target::Problem(self.id.clone()),
        }
    }

    pub(crate) fn kind(&self) -> Kind {
        if self.book == "editorial" {
            Kind::Editorial
        } else {
            Kind::Statements
        }
    }

    pub(crate) fn unsaved(&self) -> Option<(&str, &str)> {
        (!self.path.is_empty()).then_some((self.path.as_str(), self.text.as_str()))
    }
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
            error: None,
            code: None,
            referencing_contests: Vec::new(),
        }
    }

    pub(crate) fn err(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            ok: false,
            error: Some(message.into()),
            code: Some(code.into()),
            referencing_contests: Vec::new(),
        }
    }

    pub(crate) fn from_storage_err(err: xcpc_storage::Error) -> Self {
        let code = err.code().to_owned();
        let message = err.human_message();
        let referencing_contests = match err {
            xcpc_storage::Error::Referenced { contests, .. } => contests,
            _ => Vec::new(),
        };
        Self {
            ok: false,
            error: Some(message),
            code: Some(code),
            referencing_contests,
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

impl ProblemMetaPatchJs {
    pub(crate) fn into_core(self) -> Result<ProblemMetaPatch, String> {
        let template_options =
            parse_namespaced_options_json(self.template_options_json.as_deref())?;
        Ok(ProblemMetaPatch {
            title: self.title,
            group: Patch::Keep,
            rating: self.rating,
            tags: self.tags,
            author: self.author,
            limits: LimitsPatch {
                time: self.time,
                memory: self.memory,
                input: self.input,
                output: self.output,
            },
            sources: self.sources.map(|list| {
                list.into_iter()
                    .map(|s| Source {
                        platform: s.platform,
                        id: s.id,
                        url: s.url,
                    })
                    .collect()
            }),
            template_options,
        })
    }
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

impl ContestMetaPatchJs {
    pub(crate) fn into_core(self) -> Result<ContestMetaPatch, String> {
        let language =
            match self.language {
                None => None,
                Some(ref s) => Some(Language::parse(s).ok_or_else(|| {
                    format!("invalid language {s:?}: expected chinese or english")
                })?),
            };
        let template_options =
            parse_namespaced_options_json(self.template_options_json.as_deref())?;
        Ok(ContestMetaPatch {
            name: self.name,
            location: self.location,
            date: self.date,
            author: self.author,
            language,
            show_metadata: self.show_metadata,
            cover: self.cover,
            blank_pages: self.blank_pages,
            template: self.template,
            template_options,
        })
    }
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

impl OverridePatchJs {
    pub(crate) fn into_core(self) -> Result<OverridePatch, String> {
        let options = parse_namespaced_options_json(self.options_json.as_deref())?;
        Ok(OverridePatch {
            title: self.title,
            limits: LimitsPatch {
                time: self.time,
                memory: self.memory,
                input: self.input,
                output: self.output,
            },
            show_metadata: self.show_metadata,
            options,
        })
    }
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

/// Parses a JSON object `{ "<template-id>": { "<key>": <value | null> } }` into Core patches.
pub(crate) fn parse_namespaced_options_json(
    raw: Option<&str>,
) -> Result<BTreeMap<String, TemplateOptionsPatch>, String> {
    let Some(raw) = raw.filter(|s| !s.trim().is_empty()) else {
        return Ok(BTreeMap::new());
    };
    let root: serde_json::Value =
        serde_json::from_str(raw).map_err(|e| format!("invalid template options JSON: {e}"))?;
    let Some(outer) = root.as_object() else {
        return Err("template options JSON must be an object".into());
    };
    let mut out = BTreeMap::new();
    for (tpl, val) in outer {
        let Some(inner) = val.as_object() else {
            return Err(format!("options for template {tpl:?} must be an object"));
        };
        let mut fields = TemplateOptionsPatch::new();
        for (key, fv) in inner {
            if fv.is_null() {
                fields.insert(key.clone(), Patch::Clear);
            } else {
                let parsed = match fv {
                    serde_json::Value::Bool(b) => TemplateOptionValue::Bool(*b),
                    serde_json::Value::Number(n) => {
                        if let Some(i) = n.as_i64() {
                            TemplateOptionValue::Int(i)
                        } else if let Some(f) = n.as_f64() {
                            TemplateOptionValue::Float(f)
                        } else {
                            return Err(format!("invalid numeric value for {key:?}"));
                        }
                    }
                    serde_json::Value::String(s) => TemplateOptionValue::String(s.clone()),
                    serde_json::Value::Array(arr) => {
                        let mut lines = Vec::with_capacity(arr.len());
                        for item in arr {
                            let s = item.as_str().ok_or_else(|| {
                                format!("array option {key:?} must contain strings")
                            })?;
                            lines.push(s.to_owned());
                        }
                        TemplateOptionValue::Lines(lines)
                    }
                    _ => return Err(format!("unsupported option value type for {key:?}")),
                };
                fields.insert(key.clone(), Patch::Set(parsed));
            }
        }
        out.insert(tpl.clone(), fields);
    }
    Ok(out)
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

// ---- Conversion helpers ----------------------------------------------------------------

pub(crate) fn limits_to_js(limits: &Limits) -> LimitsJs {
    LimitsJs {
        time: limits.time.clone(),
        memory: limits.memory.clone(),
        input: limits.input.clone(),
        output: limits.output.clone(),
    }
}

pub(crate) fn source_to_js(source: &Source) -> ProblemSourceJs {
    ProblemSourceJs {
        platform: source.platform.clone(),
        id: source.id.clone(),
        url: source.url.clone(),
    }
}

pub(crate) fn rating_tier_str(tier: RatingTier) -> &'static str {
    match tier {
        RatingTier::Newbie => "newbie",
        RatingTier::Pupil => "pupil",
        RatingTier::Specialist => "specialist",
        RatingTier::Expert => "expert",
        RatingTier::CandidateMaster => "candidate_master",
        RatingTier::Master => "master",
        RatingTier::Grandmaster => "grandmaster",
        RatingTier::Unrated => "unrated",
    }
}

pub(crate) fn diff_kind_str(kind: DiffKind) -> &'static str {
    match kind {
        DiffKind::StatementChanged => "statement_changed",
        DiffKind::SamplesChanged => "samples_changed",
        DiffKind::EditorialChanged => "editorial_changed",
        DiffKind::AssetsChanged => "assets_changed",
        DiffKind::MetaChanged => "meta_changed",
        DiffKind::ProblemDeleted => "problem_deleted",
    }
}

pub(crate) fn extract_report(
    report: &publish::Report,
) -> (Option<String>, Vec<PreviewDiagnostic>, Vec<PreviewStart>) {
    let diagnostics = report
        .diagnostics
        .iter()
        .map(|d| PreviewDiagnostic {
            severity: d.severity.into(),
            code: d.code.clone(),
            message: d.message.clone(),
            file: d.file.clone(),
            line: d.line.map(|l| l as u32),
            column: d.column.map(|c| c as u32),
            page: d.page.map(|p| p.saturating_sub(1) as u32),
        })
        .collect();
    let starts = report
        .problems
        .iter()
        .map(|p| PreviewStart {
            label: p.label.clone(),
            id: p.id.clone(),
            page: p.first.saturating_sub(1) as u32,
            first: p.first as u32,
            count: p.count as u32,
        })
        .collect();
    let error = first_error(report);
    (error, diagnostics, starts)
}

/// The first error, `file:line: message`.
pub(crate) fn first_error(report: &publish::Report) -> Option<String> {
    let d = report.diagnostics.iter().find(|d| d.is_error())?;
    Some(match (&d.file, d.line) {
        (Some(f), Some(l)) => format!("{f}:{l}: {}", d.message),
        _ => d.message.clone(),
    })
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

impl From<xcpc_packages::PackageWarning> for PackageWarningJs {
    fn from(w: xcpc_packages::PackageWarning) -> Self {
        Self {
            code: w.code,
            message: w.message,
            path: w.path,
        }
    }
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

impl From<xcpc_judge::DetectedCompiler> for DetectedCompilerJs {
    fn from(c: xcpc_judge::DetectedCompiler) -> Self {
        let kind = match c.kind {
            xcpc_judge::CompilerKind::Gpp => "gpp",
            xcpc_judge::CompilerKind::Clangpp => "clangpp",
            xcpc_judge::CompilerKind::Other => "other",
        };
        Self {
            name: c.name,
            path: c.path,
            version: c.version,
            kind: kind.to_owned(),
        }
    }
}

impl From<xcpc_judge::ToolchainStatus> for ToolchainStatusJs {
    fn from(s: xcpc_judge::ToolchainStatus) -> Self {
        Self {
            compilers: s.compilers.into_iter().map(Into::into).collect(),
            active: s.active.map(Into::into),
            override_path: s.override_path,
            env_override: s.env_override,
            install_guide: s.install_guide,
            install_commands: s
                .install_commands
                .into_iter()
                .map(|c| InstallCommandJs {
                    label: c.label,
                    command: c.command,
                })
                .collect(),
            error: None,
        }
    }
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

impl BatchMutationResultJs {
    pub(crate) fn from_report(report: BatchReport) -> Self {
        let items = report
            .items
            .into_iter()
            .map(|item| BatchItemResultJs {
                id: item.id,
                status: batch_item_status_str(item.status).to_owned(),
                code: item.code,
                message: item.message,
            })
            .collect();
        Self {
            ok: report.ok,
            total: report.total as u32,
            succeeded: report.succeeded as u32,
            skipped: report.skipped as u32,
            failed: report.failed as u32,
            items,
            error: None,
            code: None,
        }
    }

    pub(crate) fn err(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            ok: false,
            error: Some(message.into()),
            code: Some(code.into()),
            ..Default::default()
        }
    }

    pub(crate) fn from_storage_err(err: xcpc_storage::Error) -> Self {
        Self::err(err.code(), err.human_message())
    }
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

impl BatchDeleteResultJs {
    pub(crate) fn from_report(report: BatchReport) -> Self {
        let items = report
            .items
            .into_iter()
            .map(|item| BatchDeleteItemJs {
                id: item.id,
                status: batch_item_status_str(item.status).to_owned(),
                code: item.code,
                message: item.message,
                referencing_contests: item.referencing_contests,
            })
            .collect();
        Self {
            ok: report.ok,
            total: report.total as u32,
            deleted: report.succeeded as u32,
            skipped: report.skipped as u32,
            failed: report.failed as u32,
            items,
            error: None,
            code: None,
        }
    }

    pub(crate) fn err(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            ok: false,
            error: Some(message.into()),
            code: Some(code.into()),
            ..Default::default()
        }
    }

    pub(crate) fn from_storage_err(err: xcpc_storage::Error) -> Self {
        Self::err(err.code(), err.human_message())
    }
}

fn batch_item_status_str(status: BatchItemStatus) -> &'static str {
    match status {
        BatchItemStatus::Ok => "ok",
        BatchItemStatus::Skipped => "skipped",
        BatchItemStatus::Failed => "failed",
    }
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

impl BatchStartResultJs {
    pub(crate) fn ok(task_id: u32) -> Self {
        Self {
            ok: true,
            task_id,
            error: None,
            code: None,
        }
    }

    pub(crate) fn err(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            ok: false,
            task_id: 0,
            error: Some(message.into()),
            code: Some(code.into()),
        }
    }
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

impl BatchTaskEventJs {
    pub(crate) fn closed(task_id: u32, snapshot: BatchTaskSnapshotJs) -> Self {
        Self {
            kind: "closed".to_owned(),
            task_id,
            snapshot,
            ..Default::default()
        }
    }
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

impl JsonReportResultJs {
    pub(crate) fn ok(report_json: Option<String>, markdown: Option<String>) -> Self {
        Self {
            ok: true,
            report_json,
            markdown,
            error: None,
            code: None,
        }
    }

    pub(crate) fn err(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            ok: false,
            report_json: None,
            markdown: None,
            error: Some(message.into()),
            code: Some(code.into()),
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

impl AdoptSolutionResultJs {
    pub(crate) fn err(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            ok: false,
            error: Some(message.into()),
            code: Some(code.into()),
            ..Default::default()
        }
    }
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
    pub capabilities: Vec<String>,
    pub domains: Vec<String>,
    pub workspace_read: bool,
    pub has_unsandboxed_ui: bool,
    #[prop(default)]
    pub is_linked: bool,
    #[prop(skip_none)]
    pub link_target: Option<String>,
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
    pub has_unsandboxed_ui: bool,
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

/// Result of reading a plugin's desktop `ui.js` source (`pluginUiSource`).
#[derive(Debug, Clone, Default, PartialEq, Eq, JsObject)]
pub struct PluginUiSourceResultJs {
    pub ok: bool,
    #[prop(skip_none)]
    pub source: Option<String>,
    #[prop(skip_none)]
    pub error: Option<String>,
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

/// Result of synchronous plugin KV read/delete (`pluginKvGet` / `pluginKvDelete`).
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

/// Serialized VNode frame & effects returned by a sandboxed Core QuickJS UI session.
#[derive(Debug, Clone, Default, PartialEq, Eq, JsObject)]
pub struct PluginUiFrameJs {
    pub ok: bool,
    #[prop(skip_none)]
    pub session_id: Option<u32>,
    #[prop(skip_none)]
    pub frame_json: Option<String>,
    #[prop(skip_none)]
    pub error: Option<String>,
}

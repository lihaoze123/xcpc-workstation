//! JavaScript-facing data transfer objects (`#[derive(JsObject)]` and tri-state patch
//! types) for `xcpc-workstation/native`.
//!
//! These types are reflected directly into `.nsh/types/xcpc-workstation/native.d.ts` by
//! `nsh check`.

use nsh_core::element::{JsHost, JsIn, JsKind, JsOwned, JsValue, PropError, TsTypes};
use nsh_core::prelude::*;
use xcpc_convert::publish::{self, Kind, Target};
use xcpc_domain::{
    ContestMetaPatch, DiffKind, Language, Limits, LimitsPatch, OverridePatch, Patch,
    ProblemMetaPatch, RatingTier, Source,
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
    pub items: Vec<WorkspaceItem>,
}

/// The open workspace.
#[derive(Debug, Clone, Default, PartialEq, JsObject)]
pub struct WorkspaceInfo {
    /// Its directory; empty when none is open.
    pub root: String,
    pub read_only: bool,
    /// What could not be read, or why the folder picked is no workspace.
    #[prop(skip_none)]
    pub error: Option<String>,
    pub problems: Vec<WorkspaceProblem>,
    pub contests: Vec<WorkspaceContest>,
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
        let message = err.to_string();
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
}

/// Indexed problem summary returned by `searchProblems` (without statement body).
#[derive(Debug, Clone, Default, PartialEq, Eq, JsObject)]
pub struct ProblemSummaryJs {
    pub id: String,
    pub title: String,
    #[prop(skip_none)]
    pub rating: Option<u32>,
    pub tags: Vec<String>,
    pub sources: Vec<ProblemSourceJs>,
    pub has_editorial: bool,
    pub has_solutions: bool,
    #[prop(skip_none)]
    pub statement_path: Option<String>,
}

/// Distinct tags and external platforms across the workspace problem library, sorted.
#[derive(Debug, Clone, Default, PartialEq, Eq, JsObject)]
pub struct WorkspaceFacets {
    pub tags: Vec<String>,
    pub platforms: Vec<String>,
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
    pub show_metadata: bool,
    /// Explicit `cover` value (`null` means auto: enabled when `items.length >= 2`).
    #[prop(skip_none)]
    pub cover: Option<bool>,
    /// Effective cover flag (`cover ?? items.length >= 2`).
    pub has_cover: bool,
    pub blank_pages: bool,
    pub frozen: bool,
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
        }
    }
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
}

impl ProblemMetaPatchJs {
    pub(crate) fn into_core(self) -> ProblemMetaPatch {
        ProblemMetaPatch {
            title: self.title,
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
        }
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
  sources?: ({source_ty})[];\n\
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
        Ok(owned)
    }
}

/// Partial update for `contest.toml` metadata via `toml_edit`.
///
/// Omitting a nullable field (`location`, `date`, `author`, `cover`) leaves it untouched;
/// passing `null` removes the key (e.g. resetting `cover` to auto).
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
        Ok(ContestMetaPatch {
            name: self.name,
            location: self.location,
            date: self.date,
            author: self.author,
            language,
            show_metadata: self.show_metadata,
            cover: self.cover,
            blank_pages: self.blank_pages,
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
  blankPages?: boolean;\n\
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
}

impl OverridePatchJs {
    pub(crate) fn into_core(self) -> OverridePatch {
        OverridePatch {
            title: self.title,
            limits: LimitsPatch {
                time: self.time,
                memory: self.memory,
                input: self.input,
                output: self.output,
            },
            show_metadata: self.show_metadata,
        }
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
  showMetadata?: boolean | null;\n\
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

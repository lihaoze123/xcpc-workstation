//! The workspace's model: problems, contests and how a contest's overrides apply.
//!
//! These are the shapes of `problem.toml` and `contest.toml` (docs/product.md §2–3) and
//! of a problem as loaded from its directory. Reading and writing them is
//! `xcpc-storage`'s; turning them into Typst is `xcpc-convert`'s.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// The languages a statement is set in: they pick the template's keywords.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    #[default]
    Chinese,
    English,
}

impl Language {
    /// The suffix of its statement file (`statement.zh.md`).
    pub fn suffix(self) -> &'static str {
        match self {
            Language::Chinese => "zh",
            Language::English => "en",
        }
    }

    /// The name used in `contest.toml` and by the template.
    pub fn name(self) -> &'static str {
        match self {
            Language::Chinese => "chinese",
            Language::English => "english",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "chinese" | "zh" => Some(Language::Chinese),
            "english" | "en" => Some(Language::English),
            _ => None,
        }
    }
}

/// `problem.toml`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct ProblemMeta {
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rating: Option<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(default)]
    pub limits: Limits,
    /// External identities (a Codeforces original, a Luogu copy); none for an original.
    #[serde(default, rename = "source", skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<Source>,
    /// Phase 5 (a declarative local Polygon): parsed and kept, not used yet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subtasks: Option<toml::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generators: Option<toml::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub validators: Option<toml::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checkers: Option<toml::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub solutions: Option<toml::Value>,
}

/// Time and memory limits and file names, as written (`"1s"`, `"256MB"`). Empty or
/// missing file names mean standard input and output.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Limits {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
}

impl Limits {
    /// These limits with the fields `over` sets replacing them.
    pub fn merged(&self, over: &Limits) -> Limits {
        Limits {
            time: over.time.clone().or_else(|| self.time.clone()),
            memory: over.memory.clone().or_else(|| self.memory.clone()),
            input: over.input.clone().or_else(|| self.input.clone()),
            output: over.output.clone().or_else(|| self.output.clone()),
        }
    }
}

/// One external identity of a problem (`[[source]]`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub platform: String,
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

/// `contest.toml`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct ContestMeta {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(default)]
    pub language: Language,
    /// Show the limits table (each problem can override it).
    #[serde(default = "yes")]
    pub show_metadata: bool,
    /// A cover with the problem list; by default when there are two problems or more.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cover: Option<bool>,
    /// Start every problem on an odd page, with "this page is intentionally left blank".
    #[serde(default)]
    pub blank_pages: bool,
    /// Archived: the contest renders from `snapshot/` and nothing in it may be written.
    #[serde(default)]
    pub frozen: bool,
    /// The problems, in order.
    #[serde(default, rename = "problem")]
    pub problems: Vec<ContestItem>,
}

fn yes() -> bool {
    true
}

impl ContestMeta {
    /// Whether the book gets a cover.
    pub fn has_cover(&self) -> bool {
        self.cover.unwrap_or(self.problems.len() >= 2)
    }
}

/// One problem of a contest (`[[problem]]`): a live reference by ID, with metadata
/// overrides only. Statements, samples and pictures cannot be overridden.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct ContestItem {
    pub id: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limits: Option<Limits>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub show_metadata: Option<bool>,
}

impl ContestItem {
    /// The problem's metadata with this item's overrides over it.
    pub fn apply(&self, meta: &ProblemMeta) -> ProblemMeta {
        let mut out = meta.clone();
        if let Some(title) = &self.title {
            out.title = title.clone();
        }
        if let Some(limits) = &self.limits {
            out.limits = meta.limits.merged(limits);
        }
        out
    }
}

/// A problem as loaded from its directory.
#[derive(Debug, Clone, PartialEq)]
pub struct Problem {
    /// The directory's name.
    pub id: String,
    /// Its directory relative to the workspace, with `/` (`problems/cf-1999a`, or a
    /// frozen contest's `contests/x/snapshot/A`).
    pub dir: String,
    pub meta: ProblemMeta,
    /// `statement.md`, `statement.zh.md`, `statement.en.md`: those that exist.
    pub statements: Vec<Statement>,
    /// `samples/*.in` with their `.ans` (or `.out`), by name.
    pub samples: Vec<Sample>,
    pub editorial: Option<TextFile>,
}

/// A statement file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Statement {
    /// `None` for `statement.md`, whose language is the one asked for.
    pub language: Option<Language>,
    pub file: TextFile,
}

/// A text file and where it is, relative to the workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextFile {
    pub path: String,
    pub text: String,
}

/// One sample test.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sample {
    /// Its name without the extension (`01`).
    pub name: String,
    pub input: String,
    pub output: String,
}

impl Problem {
    /// The statement to set in `language`: the one in that language, else
    /// `statement.md`, else any.
    pub fn statement(&self, language: Language) -> Option<&Statement> {
        self.statements
            .iter()
            .find(|s| s.language == Some(language))
            .or_else(|| self.statements.iter().find(|s| s.language.is_none()))
            .or_else(|| self.statements.first())
    }

    /// The language a problem is set in on its own: `statement.md`'s is Chinese, the
    /// template's default; a problem with only an English statement is English.
    pub fn default_language(&self) -> Language {
        if self.statements.iter().any(|s| s.language.is_none()) {
            return Language::Chinese;
        }
        self.statements
            .first()
            .and_then(|s| s.language)
            .unwrap_or_default()
    }
}

/// Why an ID is not one.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error(
    "invalid id {0:?}: use lowercase letters, digits, '-' and '_', starting with a letter or digit"
)]
pub struct InvalidId(pub String);

/// Checks a problem or contest ID: a slug that is also a safe directory name.
pub fn check_id(id: &str) -> Result<(), InvalidId> {
    let mut chars = id.chars();
    let ok = chars
        .next()
        .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
        && id.len() <= 64;
    if ok {
        Ok(())
    } else {
        Err(InvalidId(id.to_owned()))
    }
}

/// Maximum number of problems in a single contest (`A` through `Z`).
pub const MAX_CONTEST_PROBLEMS: usize = 26;

/// Returns the contest label (`"A"`..=`"Z"`) for a 0-based index, or `None` if `idx >= 26`.
pub fn label_for_index(idx: usize) -> Option<String> {
    if idx < MAX_CONTEST_PROBLEMS {
        Some(((b'A' + idx as u8) as char).to_string())
    } else {
        None
    }
}

/// What a statement's second-level headings mean (docs/product.md §2.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SectionKind {
    /// The template's section with this key (`input`, `note`, …).
    Known(&'static str),
    /// The samples go here (`## 样例`).
    Samples,
    /// The description, which has no heading (`## 题目描述`): dropped.
    Legend,
}

/// Recognizes a statement heading by its words, Chinese or English.
pub fn recognize_section(title: &str) -> Option<SectionKind> {
    let t = title
        .trim()
        .trim_end_matches([':', '：'])
        .trim()
        .to_lowercase();
    let known = |key| Some(SectionKind::Known(key));
    match t.as_str() {
        "输入" | "输入格式" | "输入描述" | "输入数据" | "input" | "input format" | "input data" => {
            known("input")
        }
        "输出" | "输出格式" | "输出描述" | "输出数据" | "output" | "output format"
        | "output data" => known("output"),
        "交互"
        | "交互格式"
        | "交互协议"
        | "交互方式"
        | "interaction"
        | "interaction protocol"
        | "interactive protocol" => known("interaction"),
        "提示" | "说明" | "备注" | "注释" | "注意" | "note" | "notes" | "hint" | "hints" => {
            known("note")
        }
        "样例解释" | "样例说明" | "explanation" | "explanations" | "sample explanation" => {
            known("explanation")
        }
        "评分" | "评分方式" | "计分方式" | "scoring" => known("scoring"),
        "约束" | "数据范围" | "数据范围与约定" | "数据规模与约定" | "constraints" => {
            known("constraints")
        }
        "背景" | "题目背景" | "背景知识" | "background" => known("background"),
        "题目要求" | "specification" => known("specification"),
        "图表说明" | "illustration" => known("illustration"),
        "子任务" | "subtasks" | "subtask" => known("subtasks"),
        "样例" | "样例输入输出" | "示例" | "example" | "examples" | "sample" | "samples" => {
            Some(SectionKind::Samples)
        }
        "题目描述" | "题面" | "描述" | "description" | "legend" | "statement" => {
            Some(SectionKind::Legend)
        }
        _ => None,
    }
}

/// A tri-state field update: leave untouched, replace with a value, or remove.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Patch<T> {
    #[default]
    Keep,
    Set(T),
    Clear,
}

impl<T> Patch<T> {
    pub fn is_keep(&self) -> bool {
        matches!(self, Patch::Keep)
    }

    pub fn from_option(value: Option<T>) -> Self {
        match value {
            Some(v) => Patch::Set(v),
            None => Patch::Clear,
        }
    }

    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Patch<U> {
        match self {
            Patch::Keep => Patch::Keep,
            Patch::Set(v) => Patch::Set(f(v)),
            Patch::Clear => Patch::Clear,
        }
    }
}

/// Partial update for `[limits]` or `[problem.limits]`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LimitsPatch {
    #[serde(default)]
    pub time: Patch<String>,
    #[serde(default)]
    pub memory: Patch<String>,
    #[serde(default)]
    pub input: Patch<String>,
    #[serde(default)]
    pub output: Patch<String>,
}

impl LimitsPatch {
    pub fn is_empty(&self) -> bool {
        self.time.is_keep()
            && self.memory.is_keep()
            && self.input.is_keep()
            && self.output.is_keep()
    }

    /// Clears all four limit fields.
    pub fn clear_all() -> Self {
        Self {
            time: Patch::Clear,
            memory: Patch::Clear,
            input: Patch::Clear,
            output: Patch::Clear,
        }
    }
}

/// Partial update for `problem.toml` (written via `toml_edit` to preserve comments and unknown keys).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ProblemMetaPatch {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub rating: Patch<u32>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    #[serde(default)]
    pub author: Patch<String>,
    #[serde(default)]
    pub limits: LimitsPatch,
    #[serde(default)]
    pub sources: Option<Vec<Source>>,
}

/// Partial update for `contest.toml` metadata (written via `toml_edit`; excludes `frozen` and `[[problem]]`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContestMetaPatch {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub location: Patch<String>,
    #[serde(default)]
    pub date: Patch<String>,
    #[serde(default)]
    pub author: Patch<String>,
    #[serde(default)]
    pub language: Option<Language>,
    #[serde(default)]
    pub show_metadata: Option<bool>,
    #[serde(default)]
    pub cover: Patch<bool>,
    #[serde(default)]
    pub blank_pages: Option<bool>,
}

/// Partial update for one `[[problem]]` item's metadata overrides inside `contest.toml`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OverridePatch {
    #[serde(default)]
    pub title: Patch<String>,
    #[serde(default)]
    pub limits: LimitsPatch,
    #[serde(default)]
    pub show_metadata: Patch<bool>,
}

/// Inspection result of a `samples/` directory.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SampleInventory {
    /// Sorted stems of valid paired samples (e.g. `["01", "02"]`).
    pub pairs: Vec<String>,
    /// Sorted filenames of `.in` files without a matching `.ans` or `.out` (e.g. `["03.in"]`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub orphan_inputs: Vec<String>,
    /// Sorted filenames of `.ans`/`.out` files without a matching `.in` (e.g. `["04.ans"]`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub orphan_outputs: Vec<String>,
    /// Whether `pairs` is non-empty and not numbered contiguously `01, 02, …, NN`.
    pub non_contiguous: bool,
}

/// Metadata summary of a problem for listings and search results (no statement body).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProblemSummary {
    pub id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rating: Option<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<Source>,
    pub has_editorial: bool,
    pub has_solutions: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub statement_path: Option<String>,
}

/// Structured filter + normalized text query for searching the problem library.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchQuery {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rating_min: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rating_max: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub platform: Option<String>,
}

/// Difficulty tier matching `docs/design-system.md` §3.3, plus `Unrated`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RatingTier {
    Newbie,
    Pupil,
    Specialist,
    Expert,
    CandidateMaster,
    Master,
    Grandmaster,
    Unrated,
}

impl RatingTier {
    /// All 8 tiers in display order (`Newbie` .. `Grandmaster`, then `Unrated`).
    pub const ALL: [RatingTier; 8] = [
        RatingTier::Newbie,
        RatingTier::Pupil,
        RatingTier::Specialist,
        RatingTier::Expert,
        RatingTier::CandidateMaster,
        RatingTier::Master,
        RatingTier::Grandmaster,
        RatingTier::Unrated,
    ];

    /// Maps an optional numeric rating into one of the 7 tiers or `Unrated`.
    pub fn from_rating(rating: Option<u32>) -> Self {
        match rating {
            None => RatingTier::Unrated,
            Some(r) if r < 1200 => RatingTier::Newbie,
            Some(r) if r < 1400 => RatingTier::Pupil,
            Some(r) if r < 1600 => RatingTier::Specialist,
            Some(r) if r < 1900 => RatingTier::Expert,
            Some(r) if r < 2100 => RatingTier::CandidateMaster,
            Some(r) if r < 2400 => RatingTier::Master,
            Some(_) => RatingTier::Grandmaster,
        }
    }
}

/// Composition statistics for a contest: difficulty distribution and tag counts.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContestStats {
    pub problem_count: usize,
    /// Counts per rating tier (all 8 tiers populated for deterministic layout).
    pub rating_distribution: BTreeMap<RatingTier, usize>,
    /// Tag counts sorted by count descending, then tag name ascending.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tag_counts: Vec<(String, usize)>,
}

/// Kind of difference between a frozen contest's snapshot and the live problem library.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiffKind {
    StatementChanged,
    SamplesChanged,
    EditorialChanged,
    AssetsChanged,
    MetaChanged,
    ProblemDeleted,
}

/// Differences for one problem in a frozen contest when preparing to unfreeze.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnfreezeProblemDiff {
    pub label: String,
    pub problem_id: String,
    pub kinds: Vec<DiffKind>,
    /// Relative paths inside the problem directory whose bytes differ, sorted.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub changed_files: Vec<String>,
}

/// Kind of entity returned by `Cmd+K` fuzzy search.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FuzzyMatchKind {
    Problem,
    Contest,
}

/// One ranked match from fuzzy search (with Frecency boost).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FuzzyHit {
    pub kind: FuzzyMatchKind,
    pub id: String,
    pub title: String,
    pub score: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn problem_toml_round_trips_with_sources_and_reserved_fields() {
        let text = r#"
title = "Two Sum"
rating = 800
tags = ["greedy"]

[limits]
time = "1s"
memory = "256MB"
input = ""

[[source]]
platform = "codeforces"
id = "1999A"
url = "https://codeforces.com/problemset/problem/1999/A"

[[source]]
platform = "luogu"
id = "CF1999A"

[[subtasks]]
score = 30
"#;
        let meta: ProblemMeta = toml::from_str(text).unwrap();
        assert_eq!(meta.sources.len(), 2);
        assert_eq!(meta.limits.time.as_deref(), Some("1s"));
        assert!(meta.subtasks.is_some());
        let again: ProblemMeta = toml::from_str(&toml::to_string(&meta).unwrap()).unwrap();
        assert_eq!(meta, again);
    }

    #[test]
    fn unknown_keys_are_tolerated_on_read_and_training_status_is_never_written() {
        let text = r#"
title = "Two Sum"
custom_note = "keep me via toml_edit"
status = "ac"

[limits]
time = "1s"
extra_limit = 42

[[test]]
gen = "random"
args = ["--n", "100000"]
"#;
        let meta: ProblemMeta = toml::from_str(text).unwrap();
        assert_eq!(meta.title, "Two Sum");
        assert_eq!(meta.limits.time.as_deref(), Some("1s"));
        let serialized = toml::to_string(&meta).unwrap();
        assert!(
            !serialized.contains("status"),
            "training status must never be serialized: {serialized}"
        );

        let contest_text = r#"
name = "ICPC"
custom_field = true

[[problem]]
id = "two-sum"
label = "A"
custom_item_key = "preserved"
[problem.limits]
time = "2s"
custom_limit = true
"#;
        let contest: ContestMeta = toml::from_str(contest_text).unwrap();
        assert_eq!(contest.name, "ICPC");
        assert_eq!(contest.problems.len(), 1);
        assert_eq!(
            contest.problems[0].limits.as_ref().unwrap().time.as_deref(),
            Some("2s")
        );
    }

    #[test]
    fn overrides_touch_metadata_only() {
        let meta = ProblemMeta {
            title: "Old".into(),
            limits: Limits {
                time: Some("1s".into()),
                memory: Some("256MB".into()),
                ..Default::default()
            },
            ..Default::default()
        };
        let item = ContestItem {
            id: "p".into(),
            label: "A".into(),
            title: Some("New".into()),
            limits: Some(Limits {
                time: Some("2s".into()),
                ..Default::default()
            }),
            show_metadata: Some(false),
        };
        let out = item.apply(&meta);
        assert_eq!(out.title, "New");
        assert_eq!(out.limits.time.as_deref(), Some("2s"));
        assert_eq!(out.limits.memory.as_deref(), Some("256MB"));
    }

    #[test]
    fn contest_defaults() {
        let meta: ContestMeta = toml::from_str(
            "name = \"X\"\n[[problem]]\nid = \"a\"\nlabel = \"A\"\n[[problem]]\nid = \"b\"\nlabel = \"B\"\n",
        )
        .unwrap();
        assert!(meta.show_metadata && meta.has_cover() && !meta.frozen);
        assert_eq!(meta.language, Language::Chinese);
    }

    #[test]
    fn ids() {
        assert!(check_id("cf-1999a").is_ok());
        for bad in ["", "..", "a/b", "A", "-a", "a b"] {
            assert!(check_id(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn contest_labels_cover_a_through_z() {
        assert_eq!(MAX_CONTEST_PROBLEMS, 26);
        assert_eq!(label_for_index(0).as_deref(), Some("A"));
        assert_eq!(label_for_index(1).as_deref(), Some("B"));
        assert_eq!(label_for_index(25).as_deref(), Some("Z"));
        assert_eq!(label_for_index(26), None);
        assert_eq!(label_for_index(100), None);
    }

    #[test]
    fn section_recognition_matches_spec() {
        assert_eq!(
            recognize_section("输入格式"),
            Some(SectionKind::Known("input"))
        );
        assert_eq!(
            recognize_section(" Output: "),
            Some(SectionKind::Known("output"))
        );
        assert_eq!(
            recognize_section("交互协议："),
            Some(SectionKind::Known("interaction"))
        );
        assert_eq!(recognize_section("Hints"), Some(SectionKind::Known("note")));
        assert_eq!(
            recognize_section("样例解释"),
            Some(SectionKind::Known("explanation"))
        );
        assert_eq!(
            recognize_section("Scoring"),
            Some(SectionKind::Known("scoring"))
        );
        assert_eq!(
            recognize_section("数据范围与约定"),
            Some(SectionKind::Known("constraints"))
        );
        assert_eq!(
            recognize_section("题目背景"),
            Some(SectionKind::Known("background"))
        );
        assert_eq!(
            recognize_section("Specification"),
            Some(SectionKind::Known("specification"))
        );
        assert_eq!(
            recognize_section("图表说明"),
            Some(SectionKind::Known("illustration"))
        );
        assert_eq!(
            recognize_section("Subtasks"),
            Some(SectionKind::Known("subtasks"))
        );
        assert_eq!(recognize_section("样例"), Some(SectionKind::Samples));
        assert_eq!(recognize_section("Examples:"), Some(SectionKind::Samples));
        assert_eq!(recognize_section("题目描述"), Some(SectionKind::Legend));
        assert_eq!(recognize_section("Legend"), Some(SectionKind::Legend));
        assert_eq!(recognize_section("其他"), None);
    }

    #[test]
    fn rating_tier_boundaries() {
        assert_eq!(RatingTier::from_rating(None), RatingTier::Unrated);
        assert_eq!(RatingTier::from_rating(Some(0)), RatingTier::Newbie);
        assert_eq!(RatingTier::from_rating(Some(1199)), RatingTier::Newbie);
        assert_eq!(RatingTier::from_rating(Some(1200)), RatingTier::Pupil);
        assert_eq!(RatingTier::from_rating(Some(1399)), RatingTier::Pupil);
        assert_eq!(RatingTier::from_rating(Some(1400)), RatingTier::Specialist);
        assert_eq!(RatingTier::from_rating(Some(1599)), RatingTier::Specialist);
        assert_eq!(RatingTier::from_rating(Some(1600)), RatingTier::Expert);
        assert_eq!(RatingTier::from_rating(Some(1899)), RatingTier::Expert);
        assert_eq!(
            RatingTier::from_rating(Some(1900)),
            RatingTier::CandidateMaster
        );
        assert_eq!(
            RatingTier::from_rating(Some(2099)),
            RatingTier::CandidateMaster
        );
        assert_eq!(RatingTier::from_rating(Some(2100)), RatingTier::Master);
        assert_eq!(RatingTier::from_rating(Some(2399)), RatingTier::Master);
        assert_eq!(RatingTier::from_rating(Some(2400)), RatingTier::Grandmaster);
        assert_eq!(RatingTier::from_rating(Some(3500)), RatingTier::Grandmaster);
    }
}

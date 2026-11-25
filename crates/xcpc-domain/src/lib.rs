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

/// One scalar or list value for a template schema option.
///
/// Serializes naturally in both TOML and JSON (booleans, integers, floats, strings,
/// and string arrays for multiline/list options).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum TemplateOptionValue {
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Lines(Vec<String>),
}

/// Template options namespaced by template ID: `<template-id> -> (<key> -> <value>)`.
pub type TemplateOptionsMap = BTreeMap<String, BTreeMap<String, TemplateOptionValue>>;

/// Supported control types in a template's `#let schema = (...)` declaration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TemplateFieldType {
    Bool,
    Enum,
    #[default]
    String,
    Lines,
    Number,
    Length,
    Color,
    Font,
    Image,
}

/// Which level a template schema field applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TemplateFieldScope {
    #[default]
    Book,
    Problem,
}

/// Polymorphic user-facing text in template `#let schema` and template/preset metadata.
///
/// Supports either a plain single-language string (`"Cover"`) or a BCP 47 language
/// dictionary (`(zh: "封面", en: "Cover")`). Serializes transparently via `#[serde(untagged)]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum LocalizedText {
    Plain(String),
    Map(BTreeMap<String, String>),
}

impl Default for LocalizedText {
    fn default() -> Self {
        Self::Plain(String::new())
    }
}

impl From<String> for LocalizedText {
    fn from(s: String) -> Self {
        Self::Plain(s)
    }
}

impl From<&str> for LocalizedText {
    fn from(s: &str) -> Self {
        Self::Plain(s.to_owned())
    }
}

impl LocalizedText {
    /// Constructs a bilingual `(zh, en)` localized text map.
    pub fn bilingual(zh: impl Into<String>, en: impl Into<String>) -> Self {
        let mut map = BTreeMap::new();
        map.insert("zh".to_owned(), zh.into());
        map.insert("en".to_owned(), en.into());
        Self::Map(map)
    }

    /// Resolves this text for `locale` (e.g. `"zh-CN"` or `"en"`) using the 6-step
    /// fallback chain specified in `docs/specs/i18n.md` §3.2:
    /// 1. Plain string → returned directly.
    /// 2. Exact tag match (case-insensitive, `_` ≡ `-`).
    /// 3. Primary subtag match (exact primary subtag first, then first sorted key with same primary subtag).
    /// 4. Fallback to `"en"` (or `en-*`).
    /// 5. Fallback to `"zh"` / `"zh-CN"` (or `zh-*`).
    /// 6. First non-empty value sorted by key, else `""`.
    pub fn resolve(&self, locale: &str) -> &str {
        match self {
            Self::Plain(s) => s.as_str(),
            Self::Map(map) => {
                if map.is_empty() {
                    return "";
                }
                let normalize = |tag: &str| tag.trim().replace('_', "-").to_ascii_lowercase();
                fn primary(norm: &str) -> &str {
                    norm.split('-').next().unwrap_or("")
                }

                let target_norm = normalize(locale);
                let target_primary = primary(&target_norm);

                let find_exact = |needle: &str| -> Option<&str> {
                    if needle.is_empty() {
                        return None;
                    }
                    map.iter().find_map(|(k, v)| {
                        (!v.is_empty() && normalize(k) == needle).then_some(v.as_str())
                    })
                };

                let find_primary = |needle_primary: &str| -> Option<&str> {
                    if needle_primary.is_empty() {
                        return None;
                    }
                    if let Some(exact) = find_exact(needle_primary) {
                        return Some(exact);
                    }
                    map.iter().find_map(|(k, v)| {
                        if v.is_empty() {
                            return None;
                        }
                        let k_norm = normalize(k);
                        (primary(&k_norm) == needle_primary).then_some(v.as_str())
                    })
                };

                // 1. Exact match (case-insensitive, `_` == `-`).
                if let Some(v) = find_exact(&target_norm) {
                    return v;
                }
                // 2. Primary subtag match (e.g. "zh-CN" -> "zh", or first same-primary key like "zh-Hans").
                if let Some(v) = find_primary(target_primary) {
                    return v;
                }
                // 3. Fallback to "en".
                if let Some(v) = find_primary("en") {
                    return v;
                }
                // 4. Fallback to "zh" / "zh-CN".
                if let Some(v) = find_exact("zh")
                    .or_else(|| find_exact("zh-cn"))
                    .or_else(|| find_primary("zh"))
                {
                    return v;
                }
                // 5. First non-empty value sorted by key (BTreeMap is already key-sorted).
                if let Some(v) = map.values().find(|v| !v.is_empty()) {
                    return v.as_str();
                }
                // 6. Empty fallback.
                ""
            }
        }
    }
}

/// One selectable choice for an `enum` schema field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TemplateChoice {
    pub value: String,
    pub label: LocalizedText,
}

/// A named visual group in a template's schema.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TemplateGroup {
    pub id: String,
    pub label: LocalizedText,
}

/// One parameter declared in a template's `#let schema = (...)`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct TemplateField {
    pub key: String,
    pub label: LocalizedText,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<LocalizedText>,
    #[serde(rename = "type", default)]
    pub field_type: TemplateFieldType,
    #[serde(default)]
    pub scope: TemplateFieldScope,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    pub default: TemplateOptionValue,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub choices: Vec<TemplateChoice>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub presets: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placeholder: Option<LocalizedText>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub when: BTreeMap<String, TemplateOptionValue>,
}

/// Parsed `#let schema = (...)` exported by a Typst template.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct TemplateSchema {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<LocalizedText>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<LocalizedText>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub groups: Vec<TemplateGroup>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<TemplateField>,
}

impl TemplateSchema {
    pub fn has_book_options(&self) -> bool {
        self.fields
            .iter()
            .any(|f| f.scope == TemplateFieldScope::Book)
    }

    pub fn has_problem_options(&self) -> bool {
        self.fields
            .iter()
            .any(|f| f.scope == TemplateFieldScope::Problem)
    }
}

/// Metadata for one bundled template seed (`olymp-classic`, `cnoi-classic`, `minimal`).
///
/// Presets have no runtime privilege or implicit fallback; they are only used when
/// initializing a workspace or installing a new editable template into `templates/`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PresetInfo {
    pub id: String,
    pub name: LocalizedText,
    pub description: LocalizedText,
}

/// Metadata and declarative schema for one template file on disk.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TemplateInfo {
    pub id: String,
    /// Workspace-relative path (e.g. `"templates/olymp-classic.typ"` or `"contests/c1/template.typ"`).
    pub path: String,
    pub name: LocalizedText,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<LocalizedText>,
    pub schema: TemplateSchema,
}

/// Partial update for one template's option keys (`Set` writes, `Clear` removes).
pub type TemplateOptionsPatch = BTreeMap<String, Patch<TemplateOptionValue>>;

/// `[package]` section in `problem.toml` (Phase 3 local Polygon).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct PackageConfig {
    /// Checker name (`wcmp`, `ncmp`, `fcmp`, `lcmp`, `rcmp4`, `rcmp6`, `rcmp9`, `yesno`,
    /// `nyesno`, or `"checker.cpp"`). When omitted, `checker.cpp` is used if present,
    /// otherwise `wcmp`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checker: Option<String>,
    /// Number of sequential stdio passes (`runs >= 2` enables multi-run communication mode;
    /// omitted or `1` means single-run).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runs: Option<u32>,
}

impl PackageConfig {
    /// Effective run count (minimum 1).
    pub fn run_count(&self) -> u32 {
        self.runs.unwrap_or(1).max(1)
    }
}

/// Scoring policy for an OI subtask (`[[subtask]].scoring`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SubtaskScoring {
    /// Bundled scoring (default): any failure in this subtask or its transitive dependencies
    /// yields 0; otherwise `score * min(test_ratio)`.
    #[default]
    Min,
    /// Even split across tests in this subtask: `sum((score / count) * test_ratio)`,
    /// or 0 if any transitive dependency did not achieve full score.
    Sum,
}

impl SubtaskScoring {
    pub fn as_str(self) -> &'static str {
        match self {
            SubtaskScoring::Min => "min",
            SubtaskScoring::Sum => "sum",
        }
    }
}

/// One `[[subtask]]` entry in `problem.toml` (Phase 3C OI subtasks / partial scoring).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct SubtaskSpec {
    /// Unique subtask identifier (e.g. `"1"`, `"small"`, `"chain"`).
    pub id: String,
    /// Integer point value for this subtask (`>= 0`).
    pub score: u32,
    /// Optional prerequisite subtask IDs forming a DAG.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub depends: Vec<String>,
    /// Scoring policy: `"min"` (default, bundled) or `"sum"` (per-test even split).
    #[serde(default)]
    pub scoring: SubtaskScoring,
}

/// One `[[test]]` entry in `problem.toml`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct TestSpec {
    /// Hand-written input file relative to the problem directory (e.g. `"tests/hand-01.in"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    /// Generator stem under `generators/` (e.g. `"gen_tree"` -> `generators/gen_tree.cpp`).
    #[serde(default, rename = "gen", skip_serializing_if = "Option::is_none")]
    pub generator: Option<String>,
    /// Command-line arguments passed to the generator (`{i}` expands to `1..=repeat`).
    /// Accepts either a single shell-like string or a legacy list of strings in TOML.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_opt_args"
    )]
    pub args: Option<String>,
    /// Number of generated cases to expand from this spec (default 1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repeat: Option<u32>,
    /// Optional display and filtering tag (e.g. `"edge"`, `"small"`, `"max"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    /// Optional human-readable name for the test.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// When `true`, write the generated input/answer back into `samples/`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sample: Option<bool>,
    /// Optional subtask ID this test belongs to (single ownership; reuse via `[[subtask]].depends`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subtask: Option<String>,
}

fn deserialize_opt_args<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum StringOrVec {
        Str(String),
        Vec(Vec<String>),
    }
    let val = Option::<StringOrVec>::deserialize(deserializer)?;
    Ok(val.map(|v| match v {
        StringOrVec::Str(s) => s,
        StringOrVec::Vec(list) => list.join(" "),
    }))
}

/// Expected verdict(s) for a solution (`"AC"` or `["WA", "TLE"]`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ExpectVerdict {
    Single(String),
    Multiple(Vec<String>),
}

impl ExpectVerdict {
    /// Returns all expected verdicts normalized to uppercase ASCII.
    pub fn normalized(&self) -> Vec<String> {
        match self {
            ExpectVerdict::Single(s) => vec![s.trim().to_ascii_uppercase()],
            ExpectVerdict::Multiple(v) => v
                .iter()
                .map(|s| s.trim().to_ascii_uppercase())
                .filter(|s| !s.is_empty())
                .collect(),
        }
    }

    /// Returns `true` if `actual` (e.g. `"AC"`, `"WA"`, `"TLE"`) satisfies this expectation.
    pub fn matches(&self, actual: &str) -> bool {
        let actual = actual.trim().to_ascii_uppercase();
        self.normalized().iter().any(|e| e == "ANY" || e == &actual)
    }
}

/// Expected total score for a solution in an OI subtask problem:
/// an exact integer (`score = 30`) or an inclusive range (`score = [20, 40]`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ScoreExpect {
    Exact(u32),
    Range([u32; 2]),
}

impl ScoreExpect {
    /// Lower bound of the expected score (inclusive).
    pub fn min_score(self) -> u32 {
        match self {
            ScoreExpect::Exact(v) => v,
            ScoreExpect::Range([lo, hi]) => lo.min(hi),
        }
    }

    /// Upper bound of the expected score (inclusive).
    pub fn max_score(self) -> u32 {
        match self {
            ScoreExpect::Exact(v) => v,
            ScoreExpect::Range([lo, hi]) => lo.max(hi),
        }
    }

    /// Returns `true` if `actual` satisfies this score expectation (within `1e-6` tolerance).
    pub fn matches(self, actual: f64) -> bool {
        const EPS: f64 = 1e-6;
        match self {
            ScoreExpect::Exact(v) => (actual - f64::from(v)).abs() <= EPS,
            ScoreExpect::Range([lo, hi]) => {
                let (lo, hi) = if lo <= hi { (lo, hi) } else { (hi, lo) };
                actual >= f64::from(lo) - EPS && actual <= f64::from(hi) + EPS
            }
        }
    }
}

/// One `[solutions.<name>]` entry in `problem.toml`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct SolutionSpec {
    /// Marks the main/reference solution (`std`). Exactly one solution should have `main = true`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub main: Option<bool>,
    /// Expected overall verdict(s) across the tests this solution runs on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expect: Option<ExpectVerdict>,
    /// Optional tag filter: run this solution only on tests whose tag is in `tags`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    /// Optional expected total score (exact or `[min, max]` range) for OI subtask problems.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score: Option<ScoreExpect>,
    /// Optional per-subtask expected verdict assertions (`subtasks = { "1" = "AC", "2" = ["WA", "TLE"] }`).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub subtasks: BTreeMap<String, ExpectVerdict>,
}

/// `problem.toml`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct ProblemMeta {
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
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
    /// Problem-scoped template options, namespaced by template ID (`[template-options.<tpl>]`).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub template_options: TemplateOptionsMap,
    /// Local Polygon package configuration (`[package]`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package: Option<PackageConfig>,
    /// Ordered OI subtask specifications (`[[subtask]]`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub subtask: Vec<SubtaskSpec>,
    /// Ordered test specifications (`[[test]]`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub test: Vec<TestSpec>,
    /// Declared solutions (`[solutions.<name>]`).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub solutions: BTreeMap<String, SolutionSpec>,
    /// Reserved for future extensions: parsed and kept, not used yet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subtasks: Option<toml::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generators: Option<toml::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub validators: Option<toml::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checkers: Option<toml::Value>,
}

impl ProblemMeta {
    /// Effective multi-run pass count (`[package].runs`, default 1).
    pub fn run_count(&self) -> u32 {
        self.package.as_ref().map_or(1, PackageConfig::run_count)
    }

    /// Sum of all declared `[[subtask]].score` values.
    pub fn total_subtask_score(&self) -> u32 {
        self.subtask
            .iter()
            .fold(0u32, |acc, s| acc.saturating_add(s.score))
    }

    /// Returns `true` if this problem is a Polygon package (either `solutions/` contains files,
    /// or `problem.toml` declares `[[test]]`, `[[subtask]]`, or `[solutions.*]`).
    pub fn has_package(&self, has_solution_files: bool) -> bool {
        has_solution_files
            || !self.test.is_empty()
            || !self.solutions.is_empty()
            || !self.subtask.is_empty()
    }

    /// Checks on disk whether `problem_dir` has a Polygon package (`solutions/` directory
    /// containing at least one file, `interactor.cpp`, or `!self.test.is_empty()`,
    /// `!self.solutions.is_empty()`, `!self.subtask.is_empty()`).
    pub fn has_package_in_dir(&self, problem_dir: &std::path::Path) -> bool {
        if !self.test.is_empty() || !self.solutions.is_empty() || !self.subtask.is_empty() {
            return true;
        }
        if problem_dir.join("interactor.cpp").is_file() {
            return true;
        }
        has_solution_files_in_dir(problem_dir)
    }
}

/// Returns `true` if `problem_dir/solutions` exists and contains at least one file.
pub fn has_solution_files_in_dir(problem_dir: &std::path::Path) -> bool {
    let sol_dir = problem_dir.join("solutions");
    let Ok(entries) = std::fs::read_dir(sol_dir) else {
        return false;
    };
    entries
        .flatten()
        .any(|e| e.file_type().is_ok_and(|ft| ft.is_file()))
}

/// Checks whether a problem at `problem_dir` with parsed `meta` has a Polygon package.
pub fn has_package(problem_dir: &std::path::Path, meta: &ProblemMeta) -> bool {
    meta.has_package_in_dir(problem_dir)
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

/// Alias for [`Source`] used in training logs and OJ ingestion APIs.
pub type ProblemSource = Source;

impl Source {
    /// Returns `true` if `platform` and `id` match case-insensitively (ignoring surrounding whitespace).
    pub fn matches(&self, platform: &str, id: &str) -> bool {
        let p = self.platform.trim();
        let i = self.id.trim();
        let tp = platform.trim();
        let ti = id.trim();
        !p.is_empty() && !i.is_empty() && p.eq_ignore_ascii_case(tp) && i.eq_ignore_ascii_case(ti)
    }

    /// Returns `true` if two sources identify the same external problem (case-insensitive platform & id).
    pub fn matches_source(&self, other: &Source) -> bool {
        self.matches(&other.platform, &other.id)
    }
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
    /// Optional template name (e.g. `"olymp-classic"`, `"cnoi-classic"`, or a custom
    /// `templates/<name>.typ` in the workspace).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template: Option<String>,
    /// Contest-level template options, namespaced by template ID (`[template-options.<tpl>]`).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub template_options: TemplateOptionsMap,
    /// Archived: the contest renders from `snapshot/` and nothing in it may be written.
    #[serde(default)]
    pub frozen: bool,
    /// Archived (hidden in the archived section): read-only until unarchived; orthogonal to `frozen`.
    #[serde(default, skip_serializing_if = "is_false")]
    pub archived: bool,
    /// The problems, in order.
    #[serde(default, rename = "problem")]
    pub problems: Vec<ContestItem>,
}

fn yes() -> bool {
    true
}

fn is_false(b: &bool) -> bool {
    !*b
}

impl ContestMeta {
    /// Whether the book gets a cover.
    pub fn has_cover(&self) -> bool {
        self.cover.unwrap_or(self.problems.len() >= 2)
    }
}

/// One problem of a contest (`[[problem]]`): a live reference by ID, with metadata
/// and problem-scoped template option overrides only. Statements, samples and pictures
/// cannot be overridden.
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
    /// Problem-scoped template option overrides, namespaced by template ID (`[problem.options.<tpl>]`).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub options: TemplateOptionsMap,
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
        for (tpl, opts) in &self.options {
            let entry = out.template_options.entry(tpl.clone()).or_default();
            for (k, v) in opts {
                entry.insert(k.clone(), v.clone());
            }
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

/// Maximum number of Unicode scalar values in a persistent group name.
pub const MAX_GROUP_NAME_CHARS: usize = 64;

/// Why a group name is not valid.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error(
    "invalid group name {0:?}: must be non-empty, contain no control characters, and be at most {MAX_GROUP_NAME_CHARS} characters"
)]
pub struct InvalidGroupName(pub String);

/// Trims and validates a persistent group name.
pub fn normalize_group_name(s: &str) -> Result<String, InvalidGroupName> {
    let trimmed = s.trim();
    if trimmed.is_empty()
        || trimmed.chars().count() > MAX_GROUP_NAME_CHARS
        || trimmed.chars().any(char::is_control)
    {
        return Err(InvalidGroupName(s.to_owned()));
    }
    Ok(trimmed.to_owned())
}

/// Maximum number of problems in a single contest (`A` through `Z`).
pub const MAX_CONTEST_PROBLEMS: usize = 26;

/// Markdown files inside a problem directory that may reference `assets/` or affect
/// problem rendering and snapshots.
pub const ASSET_MARKDOWN_DOCS: &[&str] = &[
    "statement.md",
    "statement.zh.md",
    "statement.en.md",
    "editorial.md",
    "editorial.zh.md",
    "editorial.en.md",
];

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
    pub group: Patch<String>,
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
    #[serde(default)]
    pub template_options: BTreeMap<String, TemplateOptionsPatch>,
}

/// Partial update for `contest.toml` metadata (written via `toml_edit`; excludes `frozen` and `[[problem]]`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
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
    #[serde(default)]
    pub template: Patch<String>,
    #[serde(default)]
    pub template_options: BTreeMap<String, TemplateOptionsPatch>,
}

/// Partial update for one `[[problem]]` item's metadata and template option overrides inside `contest.toml`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct OverridePatch {
    #[serde(default)]
    pub title: Patch<String>,
    #[serde(default)]
    pub limits: LimitsPatch,
    #[serde(default)]
    pub show_metadata: Patch<bool>,
    #[serde(default)]
    pub options: BTreeMap<String, TemplateOptionsPatch>,
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
    pub group: Option<String>,
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

/// Filter by persistent problem group.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "name", rename_all = "snake_case")]
pub enum GroupFilter {
    Named(String),
    Ungrouped,
}

/// Summary of one persistent problem group.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupSummary {
    pub name: String,
    pub count: usize,
    /// `true` when declared in `workspace.toml`; `false` when only referenced by `problem.toml`.
    pub declared: bool,
}

/// Outcome of renaming a persistent problem group.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenameGroupReport {
    pub old_name: String,
    pub new_name: String,
    pub updated_problems: usize,
}

/// Outcome of deleting a persistent problem group.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeleteGroupReport {
    pub name: String,
    pub ungrouped_problems: usize,
}

/// Per-item status in a batch operation report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BatchItemStatus {
    Ok,
    Skipped,
    Failed,
}

/// Per-item outcome in a batch operation report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatchItemOutcome {
    pub id: String,
    pub status: BatchItemStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub referencing_contests: Vec<String>,
}

/// Unified report returned by batch operations.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatchReport {
    pub ok: bool,
    pub total: usize,
    pub succeeded: usize,
    pub skipped: usize,
    pub failed: usize,
    pub items: Vec<BatchItemOutcome>,
}

impl BatchReport {
    /// Builds a [`BatchReport`] from per-item outcomes, computing summary counts.
    pub fn from_items(items: Vec<BatchItemOutcome>) -> Self {
        let mut succeeded = 0usize;
        let mut skipped = 0usize;
        let mut failed = 0usize;
        for item in &items {
            match item.status {
                BatchItemStatus::Ok => succeeded += 1,
                BatchItemStatus::Skipped => skipped += 1,
                BatchItemStatus::Failed => failed += 1,
            }
        }
        Self {
            ok: failed == 0,
            total: items.len(),
            succeeded,
            skipped,
            failed,
            items,
        }
    }
}

/// Outcome of adding multiple problems to a contest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatchAddToContestResult {
    pub contest_id: String,
    pub added_ids: Vec<String>,
    pub problem_count: usize,
    pub report: BatchReport,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<GroupFilter>,
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

// ---- Training status & logs (Phase 4A, docs/specs/phase-4-training-and-ecosystem.md §3.2) ----

/// Effective training status of a problem, derived at runtime by merging synced OJ
/// submissions with manual entries in `training/logs.jsonl`. Never persisted in `problem.toml`.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum TrainingStatus {
    #[default]
    None,
    Todo,
    Attempted,
    Ac,
    Upsolved,
}

impl TrainingStatus {
    /// Canonical lowercase string representation (`"none"`, `"todo"`, `"attempted"`, `"ac"`, `"upsolved"`).
    pub fn as_str(self) -> &'static str {
        match self {
            TrainingStatus::None => "none",
            TrainingStatus::Todo => "todo",
            TrainingStatus::Attempted => "attempted",
            TrainingStatus::Ac => "ac",
            TrainingStatus::Upsolved => "upsolved",
        }
    }

    /// Parses a training status string (case-insensitive, trimmed).
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "none" => Some(TrainingStatus::None),
            "todo" => Some(TrainingStatus::Todo),
            "attempted" => Some(TrainingStatus::Attempted),
            "ac" => Some(TrainingStatus::Ac),
            "upsolved" => Some(TrainingStatus::Upsolved),
            _ => None,
        }
    }
}

impl std::fmt::Display for TrainingStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for TrainingStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s).ok_or_else(|| format!("invalid training status {s:?}"))
    }
}

/// Status event written to `training/logs.jsonl` (`clear` clears any prior manual override).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TrainingLogStatus {
    Todo,
    Attempted,
    Ac,
    Upsolved,
    Clear,
}

impl TrainingLogStatus {
    /// Canonical lowercase string representation (`"todo"`, `"attempted"`, `"ac"`, `"upsolved"`, `"clear"`).
    pub fn as_str(self) -> &'static str {
        match self {
            TrainingLogStatus::Todo => "todo",
            TrainingLogStatus::Attempted => "attempted",
            TrainingLogStatus::Ac => "ac",
            TrainingLogStatus::Upsolved => "upsolved",
            TrainingLogStatus::Clear => "clear",
        }
    }

    /// Parses a training log status string (case-insensitive, trimmed).
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "todo" => Some(TrainingLogStatus::Todo),
            "attempted" => Some(TrainingLogStatus::Attempted),
            "ac" => Some(TrainingLogStatus::Ac),
            "upsolved" => Some(TrainingLogStatus::Upsolved),
            "clear" => Some(TrainingLogStatus::Clear),
            _ => None,
        }
    }

    /// Converts a log event into the corresponding manual [`TrainingStatus`] override
    /// (`Clear` yields `None`).
    pub fn to_training_status(self) -> Option<TrainingStatus> {
        match self {
            TrainingLogStatus::Todo => Some(TrainingStatus::Todo),
            TrainingLogStatus::Attempted => Some(TrainingStatus::Attempted),
            TrainingLogStatus::Ac => Some(TrainingStatus::Ac),
            TrainingLogStatus::Upsolved => Some(TrainingStatus::Upsolved),
            TrainingLogStatus::Clear => None,
        }
    }
}

impl std::fmt::Display for TrainingLogStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for TrainingLogStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s).ok_or_else(|| format!("invalid training log status {s:?}"))
    }
}

/// One append-only status event line in `training/logs.jsonl`.
///
/// Supports both workspace problem check-ins (`problem_id` and/or `source`) and
/// standalone manual training records (`title`, with optional `url`, `platform`,
/// `rating`, `tags`, `note`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrainingLogEntry {
    pub at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub problem_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<ProblemSource>,
    pub status: TrainingLogStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub platform: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rating: Option<i32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

impl TrainingLogEntry {
    /// Constructs a workspace problem check-in entry with all standalone metadata fields empty.
    pub fn for_problem(
        at: impl Into<String>,
        problem_id: Option<String>,
        source: Option<ProblemSource>,
        status: TrainingLogStatus,
    ) -> Self {
        Self {
            at: at.into(),
            problem_id,
            source,
            status,
            title: None,
            url: None,
            platform: None,
            rating: None,
            tags: Vec::new(),
            note: None,
        }
    }

    /// Validates that this entry identifies a target via at least one of:
    /// non-empty `problem_id`, valid `source` (non-empty `platform` and `id`), or non-empty `title`.
    pub fn validate(&self) -> Result<(), String> {
        let has_problem_id = self
            .problem_id
            .as_deref()
            .is_some_and(|s| !s.trim().is_empty());
        let has_source = self
            .source
            .as_ref()
            .is_some_and(|s| !s.platform.trim().is_empty() && !s.id.trim().is_empty());
        let has_title = self.title.as_deref().is_some_and(|s| !s.trim().is_empty());
        if !has_problem_id && !has_source && !has_title {
            return Err(
                "training log entry must specify at least one of `problem_id`, `source`, or `title`"
                    .to_owned(),
            );
        }
        Ok(())
    }

    /// Serializes this entry to a single compact JSON line (no trailing newline).
    pub fn to_json_line(&self) -> Result<String, String> {
        serde_json::to_string(self).map_err(|e| e.to_string())
    }

    /// Parses a single JSON line from `training/logs.jsonl` and validates that it has
    /// a non-empty `problem_id`, valid `source`, or non-empty `title`.
    pub fn from_json_line(line: &str) -> Result<Self, String> {
        let entry: Self = serde_json::from_str(line).map_err(|e| e.to_string())?;
        entry.validate()?;
        Ok(entry)
    }

    /// Returns `true` if this log entry refers to `problem_id` or any external identity in `sources`.
    pub fn matches_problem(&self, problem_id: &str, sources: &[ProblemSource]) -> bool {
        if !problem_id.is_empty() && self.problem_id.as_deref() == Some(problem_id) {
            return true;
        }
        self.source
            .as_ref()
            .is_some_and(|entry_src| sources.iter().any(|src| src.matches_source(entry_src)))
    }
}

/// Summary metadata for a workspace problem with a non-empty `editorial.md`,
/// used to populate the Training Dashboard's Editorials (`题解`) tab.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditorialSummary {
    pub problem_id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rating: Option<i32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<ProblemSource>,
    pub excerpt: String,
    pub updated_at: String,
}

/// Synced external OJ status for a problem (derived from `<dataDir>/oj_sync.db`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncedProblemStatus {
    pub ac: bool,
    pub attempted: bool,
}

/// Scans `entries` chronologically to resolve the latest manual [`TrainingStatus`] override
/// for a problem identified by `problem_id` and/or `sources` (case-insensitive platform & id).
///
/// A [`TrainingLogStatus::Clear`] entry clears any prior manual override.
pub fn resolve_manual_status(
    entries: &[TrainingLogEntry],
    problem_id: &str,
    sources: &[ProblemSource],
) -> Option<TrainingStatus> {
    let mut manual = None;
    for entry in entries {
        if entry.matches_problem(problem_id, sources) {
            manual = entry.status.to_training_status();
        }
    }
    manual
}

/// Derives the effective [`TrainingStatus`] by merging synced OJ submission status with the
/// latest manual override from `training/logs.jsonl`.
///
/// Priority: `ac` / `upsolved` > `attempted` > `todo` > `none`.
/// - External AC (`synced.ac == true`) + manual `Upsolved` -> `Upsolved`; otherwise `Ac`.
/// - External attempted (`synced.attempted == true`) + manual `Ac`/`Upsolved` -> manual wins;
///   otherwise `Attempted`.
/// - Otherwise manual status (or `None`).
pub fn derive_training_status(
    synced: SyncedProblemStatus,
    manual: Option<TrainingStatus>,
) -> TrainingStatus {
    let manual = manual.unwrap_or(TrainingStatus::None);
    if synced.ac {
        if manual == TrainingStatus::Upsolved {
            TrainingStatus::Upsolved
        } else {
            TrainingStatus::Ac
        }
    } else if synced.attempted {
        match manual {
            TrainingStatus::Ac | TrainingStatus::Upsolved => manual,
            TrainingStatus::None | TrainingStatus::Todo | TrainingStatus::Attempted => {
                TrainingStatus::Attempted
            }
        }
    } else {
        manual
    }
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
            ..Default::default()
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

    #[test]
    fn localized_text_serde_roundtrip() {
        let plain = LocalizedText::Plain("Cover".to_owned());
        let plain_json = serde_json::to_string(&plain).unwrap();
        assert_eq!(plain_json, "\"Cover\"");
        let plain_back: LocalizedText = serde_json::from_str(&plain_json).unwrap();
        assert_eq!(plain_back, plain);

        let bilingual = LocalizedText::bilingual("封面", "Cover");
        let map_json = serde_json::to_string(&bilingual).unwrap();
        assert_eq!(map_json, "{\"en\":\"Cover\",\"zh\":\"封面\"}");
        let map_back: LocalizedText = serde_json::from_str(&map_json).unwrap();
        assert_eq!(map_back, bilingual);
    }

    #[test]
    fn localized_text_resolve_fallback_chain() {
        // 1. Plain string.
        assert_eq!(LocalizedText::from("Plain").resolve("en"), "Plain");
        assert_eq!(LocalizedText::from("Plain").resolve("zh-CN"), "Plain");

        // 2. Exact match (case-insensitive, `_` == `-`).
        let exact = LocalizedText::Map(BTreeMap::from([
            ("zh_cn".to_owned(), "简体".to_owned()),
            ("zh-TW".to_owned(), "繁體".to_owned()),
            ("en".to_owned(), "English".to_owned()),
        ]));
        assert_eq!(exact.resolve("zh-CN"), "简体");
        assert_eq!(exact.resolve("ZH_tw"), "繁體");
        assert_eq!(exact.resolve("en"), "English");

        // 3. Primary subtag match (exact primary first, then first sorted same-primary key).
        let primary_exact = LocalizedText::Map(BTreeMap::from([
            ("zh".to_owned(), "中文".to_owned()),
            ("zh-Hans".to_owned(), "简体中文".to_owned()),
        ]));
        assert_eq!(primary_exact.resolve("zh-CN"), "中文");

        let primary_variant = LocalizedText::Map(BTreeMap::from([
            ("en".to_owned(), "English".to_owned()),
            ("zh-Hans".to_owned(), "简体中文".to_owned()),
            ("zh-Hant".to_owned(), "繁體中文".to_owned()),
        ]));
        assert_eq!(primary_variant.resolve("zh-CN"), "简体中文");

        // 4. Fallback to "en".
        let en_fallback = LocalizedText::Map(BTreeMap::from([
            ("de".to_owned(), "Deutsch".to_owned()),
            ("en".to_owned(), "English".to_owned()),
            ("zh".to_owned(), "中文".to_owned()),
        ]));
        assert_eq!(en_fallback.resolve("ja-JP"), "English");

        // 5. Fallback to "zh" / "zh-CN".
        let zh_fallback = LocalizedText::Map(BTreeMap::from([
            ("fr".to_owned(), "Français".to_owned()),
            ("zh-CN".to_owned(), "中文".to_owned()),
        ]));
        assert_eq!(zh_fallback.resolve("ja-JP"), "中文");

        // 6. First non-empty sorted key.
        let sorted_fallback = LocalizedText::Map(BTreeMap::from([
            ("aa".to_owned(), String::new()),
            ("de".to_owned(), "Deutsch".to_owned()),
            ("fr".to_owned(), "Français".to_owned()),
        ]));
        assert_eq!(sorted_fallback.resolve("ja"), "Deutsch");

        // 7. Empty map or all-empty values -> "".
        assert_eq!(LocalizedText::Map(BTreeMap::new()).resolve("en"), "");
        let all_empty = LocalizedText::Map(BTreeMap::from([
            ("en".to_owned(), String::new()),
            ("zh".to_owned(), String::new()),
        ]));
        assert_eq!(all_empty.resolve("en"), "");
    }

    #[test]
    fn package_toml_round_trip_and_expect_verdict() {
        let text = r#"
title = "Tree Query"

[limits]
time = "2s"
memory = "256MB"

[package]
checker = "ncmp"

[[test]]
file = "tests/hand-01.in"
tag = "edge"
name = "min-bound"

[[test]]
gen = "gen_tree"
args = "-n 200000 -w 5 {i}"
repeat = 10
tag = "max"
sample = false

[solutions.std]
main = true

[solutions.brute]
expect = "AC"
tags = ["sample", "small"]

[solutions.greedy_wa]
expect = ["WA", "TLE"]
"#;
        let meta: ProblemMeta = toml::from_str(text).unwrap();
        assert_eq!(
            meta.package.as_ref().and_then(|p| p.checker.as_deref()),
            Some("ncmp")
        );
        assert_eq!(meta.test.len(), 2);
        assert_eq!(meta.test[0].file.as_deref(), Some("tests/hand-01.in"));
        assert_eq!(meta.test[0].tag.as_deref(), Some("edge"));
        assert_eq!(meta.test[0].name.as_deref(), Some("min-bound"));
        assert_eq!(meta.test[1].generator.as_deref(), Some("gen_tree"));
        assert_eq!(meta.test[1].args.as_deref(), Some("-n 200000 -w 5 {i}"));
        assert_eq!(meta.test[1].repeat, Some(10));
        assert_eq!(meta.test[1].sample, Some(false));

        assert_eq!(meta.solutions.len(), 3);
        assert_eq!(meta.solutions["std"].main, Some(true));
        assert_eq!(
            meta.solutions["brute"].expect,
            Some(ExpectVerdict::Single("AC".into()))
        );
        assert_eq!(
            meta.solutions["brute"].tags.as_deref(),
            Some(&["sample".to_string(), "small".to_string()][..])
        );
        let greedy_expect = meta.solutions["greedy_wa"].expect.as_ref().unwrap();
        assert!(greedy_expect.matches("wa"));
        assert!(greedy_expect.matches("TLE"));
        assert!(!greedy_expect.matches("AC"));

        let any_expect = ExpectVerdict::Single("any".into());
        assert!(any_expect.matches("RE"));
        assert!(any_expect.matches("AC"));

        assert!(meta.has_package(false));
        let empty = ProblemMeta::default();
        assert!(!empty.has_package(false));
        assert!(empty.has_package(true));

        let serialized = toml::to_string(&meta).unwrap();
        let again: ProblemMeta = toml::from_str(&serialized).unwrap();
        assert_eq!(meta, again);
    }

    #[test]
    fn normalize_group_name_boundaries() {
        assert_eq!(normalize_group_name("  省赛候选  ").unwrap(), "省赛候选");
        assert_eq!(normalize_group_name("ungrouped").unwrap(), "ungrouped");

        // Exact 64 Chinese characters allowed.
        let exact_64 = "题".repeat(MAX_GROUP_NAME_CHARS);
        assert_eq!(normalize_group_name(&exact_64).unwrap(), exact_64);

        // 65 characters rejected.
        let over_64 = "题".repeat(MAX_GROUP_NAME_CHARS + 1);
        assert!(normalize_group_name(&over_64).is_err());

        // Empty / whitespace-only rejected.
        for bad in ["", "   ", "\t\n"] {
            assert!(normalize_group_name(bad).is_err(), "{bad:?}");
        }

        // Control characters rejected.
        for bad in ["a\nb", "a\rb", "a\tb", "a\0b", "\x1bfoo"] {
            assert!(normalize_group_name(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn group_filter_and_batch_types_serde_roundtrip() {
        let named = GroupFilter::Named("ungrouped".into());
        let named_json = serde_json::to_string(&named).unwrap();
        assert_eq!(named_json, r#"{"kind":"named","name":"ungrouped"}"#);
        assert_eq!(
            serde_json::from_str::<GroupFilter>(&named_json).unwrap(),
            named
        );

        let ungrouped = GroupFilter::Ungrouped;
        let ungrouped_json = serde_json::to_string(&ungrouped).unwrap();
        assert_eq!(ungrouped_json, r#"{"kind":"ungrouped"}"#);
        assert_eq!(
            serde_json::from_str::<GroupFilter>(&ungrouped_json).unwrap(),
            ungrouped
        );

        let report = BatchReport::from_items(vec![
            BatchItemOutcome {
                id: "p1".into(),
                status: BatchItemStatus::Ok,
                code: None,
                message: None,
                referencing_contests: Vec::new(),
            },
            BatchItemOutcome {
                id: "p2".into(),
                status: BatchItemStatus::Skipped,
                code: Some("problem_referenced".into()),
                message: Some("referenced".into()),
                referencing_contests: vec!["c1".into()],
            },
        ]);
        assert!(report.ok);
        assert_eq!(report.total, 2);
        assert_eq!(report.succeeded, 1);
        assert_eq!(report.skipped, 1);
        assert_eq!(report.failed, 0);
        let report_json = serde_json::to_string(&report).unwrap();
        assert_eq!(
            serde_json::from_str::<BatchReport>(&report_json).unwrap(),
            report
        );

        let meta_toml = "title = \"Grouped\"\ngroup = \"省赛候选\"\n";
        let meta: ProblemMeta = toml::from_str(meta_toml).unwrap();
        assert_eq!(meta.group.as_deref(), Some("省赛候选"));
        let back: ProblemMeta = toml::from_str(&toml::to_string(&meta).unwrap()).unwrap();
        assert_eq!(meta, back);
    }

    #[test]
    fn phase_3c_runs_subtasks_and_solution_score_round_trip() {
        let text = r#"
title = "Interactive Subtask Communication"

[limits]
time = "2s"
memory = "512MB"

[package]
checker = "checker.cpp"
runs = 2

[[subtask]]
id = "1"
score = 30
scoring = "min"

[[subtask]]
id = "2"
score = 70
depends = ["1"]
scoring = "sum"

[[test]]
gen = "gen_small"
args = "{i}"
repeat = 5
subtask = "1"
tag = "small"

[[test]]
gen = "gen_full"
args = "{i}"
repeat = 10
subtask = "2"
tag = "max"

[solutions.std]
main = true

[solutions.brute]
score = 30
subtasks = { "1" = "AC", "2" = ["WA", "TLE", "ILE"] }

[solutions.partial]
score = [20, 40]
"#;
        let meta: ProblemMeta = toml::from_str(text).unwrap();
        assert_eq!(meta.run_count(), 2);
        assert_eq!(meta.total_subtask_score(), 100);
        assert_eq!(meta.subtask.len(), 2);
        assert_eq!(meta.subtask[0].id, "1");
        assert_eq!(meta.subtask[0].score, 30);
        assert_eq!(meta.subtask[0].scoring, SubtaskScoring::Min);
        assert!(meta.subtask[0].depends.is_empty());
        assert_eq!(meta.subtask[1].id, "2");
        assert_eq!(meta.subtask[1].score, 70);
        assert_eq!(meta.subtask[1].scoring, SubtaskScoring::Sum);
        assert_eq!(meta.subtask[1].depends, vec!["1"]);

        assert_eq!(meta.test[0].subtask.as_deref(), Some("1"));
        assert_eq!(meta.test[1].subtask.as_deref(), Some("2"));

        let brute = &meta.solutions["brute"];
        assert_eq!(brute.score, Some(ScoreExpect::Exact(30)));
        assert!(brute.score.unwrap().matches(30.0));
        assert!(!brute.score.unwrap().matches(29.0));
        assert!(brute.subtasks["1"].matches("AC"));
        assert!(brute.subtasks["2"].matches("ILE"));
        assert!(brute.subtasks["2"].matches("wa"));
        assert!(!brute.subtasks["2"].matches("AC"));

        let partial = &meta.solutions["partial"];
        assert_eq!(partial.score, Some(ScoreExpect::Range([20, 40])));
        assert!(partial.score.unwrap().matches(20.0));
        assert!(partial.score.unwrap().matches(35.5));
        assert!(partial.score.unwrap().matches(40.0));
        assert!(!partial.score.unwrap().matches(41.0));

        let serialized = toml::to_string(&meta).unwrap();
        let again: ProblemMeta = toml::from_str(&serialized).unwrap();
        assert_eq!(meta, again);
    }

    #[test]
    fn training_status_and_log_status_serde_and_parse() {
        assert_eq!(TrainingStatus::default(), TrainingStatus::None);
        for (status, text) in [
            (TrainingStatus::None, "none"),
            (TrainingStatus::Todo, "todo"),
            (TrainingStatus::Attempted, "attempted"),
            (TrainingStatus::Ac, "ac"),
            (TrainingStatus::Upsolved, "upsolved"),
        ] {
            assert_eq!(status.as_str(), text);
            assert_eq!(status.to_string(), text);
            assert_eq!(TrainingStatus::parse(text), Some(status));
            assert_eq!(TrainingStatus::parse(&text.to_uppercase()), Some(status));
            assert_eq!(text.parse::<TrainingStatus>().unwrap(), status);
            let json = serde_json::to_string(&status).unwrap();
            assert_eq!(json, format!("\"{text}\""));
            assert_eq!(
                serde_json::from_str::<TrainingStatus>(&json).unwrap(),
                status
            );
        }
        assert_eq!(TrainingStatus::parse("unknown"), None);
        assert!("unknown".parse::<TrainingStatus>().is_err());

        for (status, text, expected_manual) in [
            (TrainingLogStatus::Todo, "todo", Some(TrainingStatus::Todo)),
            (
                TrainingLogStatus::Attempted,
                "attempted",
                Some(TrainingStatus::Attempted),
            ),
            (TrainingLogStatus::Ac, "ac", Some(TrainingStatus::Ac)),
            (
                TrainingLogStatus::Upsolved,
                "upsolved",
                Some(TrainingStatus::Upsolved),
            ),
            (TrainingLogStatus::Clear, "clear", None),
        ] {
            assert_eq!(status.as_str(), text);
            assert_eq!(status.to_string(), text);
            assert_eq!(TrainingLogStatus::parse(text), Some(status));
            assert_eq!(TrainingLogStatus::parse(&format!(" {text} ")), Some(status));
            assert_eq!(status.to_training_status(), expected_manual);
            let json = serde_json::to_string(&status).unwrap();
            assert_eq!(json, format!("\"{text}\""));
            assert_eq!(
                serde_json::from_str::<TrainingLogStatus>(&json).unwrap(),
                status
            );
        }
        assert_eq!(TrainingLogStatus::parse("none"), None);
    }

    #[test]
    fn training_log_entry_omits_none_fields_and_roundtrips() {
        let entry = TrainingLogEntry::for_problem(
            "2026-11-24T12:00:00Z",
            Some("cf-1999a".into()),
            Some(ProblemSource {
                platform: "codeforces".into(),
                id: "1999A".into(),
                url: None,
            }),
            TrainingLogStatus::Upsolved,
        );
        let line = entry.to_json_line().unwrap();
        let val: serde_json::Value = serde_json::from_str(&line).unwrap();
        let obj = val.as_object().unwrap();
        assert!(!line.contains("\"url\""));
        assert!(!obj.contains_key("title"));
        assert!(!obj.contains_key("platform"));
        assert!(!obj.contains_key("rating"));
        assert!(!obj.contains_key("tags"));
        assert!(!obj.contains_key("note"));
        assert!(line.contains("\"problem_id\":\"cf-1999a\""));
        assert!(line.contains("\"status\":\"upsolved\""));
        assert_eq!(TrainingLogEntry::from_json_line(&line).unwrap(), entry);

        // Legacy JSONL line without new fields parses identically.
        let legacy = r#"{"at":"2026-11-24T12:00:00Z","problem_id":"cf-1999a","source":{"platform":"codeforces","id":"1999A"},"status":"upsolved"}"#;
        assert_eq!(TrainingLogEntry::from_json_line(legacy).unwrap(), entry);

        // Standalone manual record (no problem_id, no source) round-trips cleanly.
        let manual = TrainingLogEntry {
            at: "2026-11-25T08:30:00Z".into(),
            problem_id: None,
            source: None,
            status: TrainingLogStatus::Ac,
            title: Some("P1001 A+B Problem".into()),
            url: Some("https://www.luogu.com.cn/problem/P1001".into()),
            platform: Some("luogu".into()),
            rating: Some(800),
            tags: vec!["implementation".into(), "math".into()],
            note: Some("warm-up".into()),
        };
        let manual_line = manual.to_json_line().unwrap();
        assert!(!manual_line.contains("\"problem_id\""));
        assert!(!manual_line.contains("\"source\""));
        assert!(manual_line.contains("\"title\":\"P1001 A+B Problem\""));
        assert!(manual_line.contains("\"rating\":800"));
        assert_eq!(
            TrainingLogEntry::from_json_line(&manual_line).unwrap(),
            manual
        );

        // Entries missing all of problem_id, source, and title (or blank) are rejected.
        let missing_all = r#"{"at":"2026-11-24T12:01:00Z","status":"clear"}"#;
        assert!(TrainingLogEntry::from_json_line(missing_all).is_err());

        let blank_only =
            r#"{"at":"2026-11-24T12:01:00Z","problem_id":"  ","title":"   ","status":"ac"}"#;
        assert!(TrainingLogEntry::from_json_line(blank_only).is_err());
    }

    #[test]
    fn resolve_manual_status_chronological_clear_and_case_insensitive_source() {
        let sources = vec![ProblemSource {
            platform: "CodeForces".into(),
            id: "1999a".into(),
            url: None,
        }];
        let entries = vec![
            TrainingLogEntry::for_problem(
                "2026-11-20T00:00:00Z",
                Some("other-problem".into()),
                None,
                TrainingLogStatus::Ac,
            ),
            // Standalone manual entry without problem_id or source must not affect workspace problems.
            TrainingLogEntry {
                at: "2026-11-20T12:00:00Z".into(),
                problem_id: None,
                source: None,
                status: TrainingLogStatus::Ac,
                title: Some("Offline Paper Problem".into()),
                url: None,
                platform: Some("manual".into()),
                rating: Some(1800),
                tags: vec!["dp".into()],
                note: None,
            },
            TrainingLogEntry::for_problem(
                "2026-11-21T00:00:00Z",
                Some("cf-1999a".into()),
                None,
                TrainingLogStatus::Todo,
            ),
            TrainingLogEntry::for_problem(
                "2026-11-22T00:00:00Z",
                None,
                Some(ProblemSource {
                    platform: "codeforces".into(),
                    id: "1999A".into(),
                    url: None,
                }),
                TrainingLogStatus::Upsolved,
            ),
            TrainingLogEntry::for_problem(
                "2026-11-23T00:00:00Z",
                Some("cf-1999a".into()),
                None,
                TrainingLogStatus::Clear,
            ),
        ];

        // Up to entry 3 -> Todo (standalone manual entry ignored).
        assert_eq!(
            resolve_manual_status(&entries[..3], "cf-1999a", &sources),
            Some(TrainingStatus::Todo)
        );
        // Up to entry 4 -> matched by case-insensitive source -> Upsolved.
        assert_eq!(
            resolve_manual_status(&entries[..4], "cf-1999a", &sources),
            Some(TrainingStatus::Upsolved)
        );
        // After Clear -> None.
        assert_eq!(resolve_manual_status(&entries, "cf-1999a", &sources), None);

        // Subsequent manual status after Clear takes effect.
        let mut extended = entries;
        extended.push(TrainingLogEntry::for_problem(
            "2026-11-24T00:00:00Z",
            None,
            Some(ProblemSource {
                platform: "CODEFORCES".into(),
                id: "1999A".into(),
                url: None,
            }),
            TrainingLogStatus::Attempted,
        ));
        assert_eq!(
            resolve_manual_status(&extended, "cf-1999a", &sources),
            Some(TrainingStatus::Attempted)
        );
    }

    #[test]
    fn derive_training_status_follows_priority_rules() {
        let none_synced = SyncedProblemStatus {
            ac: false,
            attempted: false,
        };
        let attempted_synced = SyncedProblemStatus {
            ac: false,
            attempted: true,
        };
        let ac_synced = SyncedProblemStatus {
            ac: true,
            attempted: true,
        };
        let ac_only_synced = SyncedProblemStatus {
            ac: true,
            attempted: false,
        };

        // External AC + manual Upsolved -> Upsolved; any other manual -> Ac.
        for synced in [ac_synced, ac_only_synced] {
            assert_eq!(
                derive_training_status(synced, Some(TrainingStatus::Upsolved)),
                TrainingStatus::Upsolved
            );
            for manual in [
                None,
                Some(TrainingStatus::None),
                Some(TrainingStatus::Todo),
                Some(TrainingStatus::Attempted),
                Some(TrainingStatus::Ac),
            ] {
                assert_eq!(
                    derive_training_status(synced, manual),
                    TrainingStatus::Ac,
                    "synced={synced:?}, manual={manual:?}"
                );
            }
        }

        // External Attempted: Ac / Upsolved win; None / Todo / Attempted -> Attempted.
        assert_eq!(
            derive_training_status(attempted_synced, Some(TrainingStatus::Ac)),
            TrainingStatus::Ac
        );
        assert_eq!(
            derive_training_status(attempted_synced, Some(TrainingStatus::Upsolved)),
            TrainingStatus::Upsolved
        );
        for manual in [
            None,
            Some(TrainingStatus::None),
            Some(TrainingStatus::Todo),
            Some(TrainingStatus::Attempted),
        ] {
            assert_eq!(
                derive_training_status(attempted_synced, manual),
                TrainingStatus::Attempted,
                "manual={manual:?}"
            );
        }

        // No external activity: manual wins (or None).
        assert_eq!(
            derive_training_status(none_synced, None),
            TrainingStatus::None
        );
        for manual in [
            TrainingStatus::None,
            TrainingStatus::Todo,
            TrainingStatus::Attempted,
            TrainingStatus::Ac,
            TrainingStatus::Upsolved,
        ] {
            assert_eq!(derive_training_status(none_synced, Some(manual)), manual);
        }
    }
}

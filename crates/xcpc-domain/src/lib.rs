//! The workspace's model: problems, contests and how a contest's overrides apply.
//!
//! These are the shapes of `problem.toml` and `contest.toml` (docs/product.md §2–3) and
//! of a problem as loaded from its directory. Reading and writing them is
//! `xcpc-storage`'s; turning them into Typst is `xcpc-convert`'s.

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
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
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
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
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
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
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
#[derive(Debug, Clone, PartialEq, Eq)]
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
    fn unknown_keys_are_errors() {
        assert!(toml::from_str::<ProblemMeta>("title = \"x\"\nstatus = \"ac\"").is_err());
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
}

//! Phase 5 domain model: Living TRD (Team Reference Document), algorithm snippets,
//! C++ single-file bundler, cumulative verification hashes, and Library Checker config.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{Patch, TemplateOptionsMap, TemplateOptionsPatch, TextFile, is_false};

/// Top-level `workspace.toml` configuration.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct WorkspaceMeta {
    /// Default statement template name (e.g. `"olymp-classic"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template: Option<String>,
    /// Persistent problem groups declared in `workspace.toml`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub groups: Vec<String>,
    /// Whether the Training workspace module is enabled (default `true` when omitted).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub training: Option<bool>,
    /// Whether the Snippets / TRD workspace module is enabled (default `true` when omitted).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snippets: Option<bool>,
    /// Workspace-level default template options (`[template-options.<tpl>]`).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub template_options: TemplateOptionsMap,
}

impl WorkspaceMeta {
    /// Returns `true` unless `training = false` is explicitly set.
    pub fn training_enabled(&self) -> bool {
        self.training.unwrap_or(true)
    }

    /// Returns `true` unless `snippets = false` is explicitly set.
    pub fn snippets_enabled(&self) -> bool {
        self.snippets.unwrap_or(true)
    }
}

/// Optional configuration for fetching `yosupo06/library-checker-problems` sources.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct LibraryCheckerConfig {
    /// Optional GitHub raw mirror URL prefix (e.g. `"https://ghproxy.net/https://raw.githubusercontent.com"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mirror_url: Option<String>,
    /// Optional local filesystem path to a `library-checker-problems` git clone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_repo_path: Option<String>,
}

// ---- Algorithm Snippets ----------------------------------------------------------------

/// One `[[verify]]` declaration in `snippets/<slug>/snippet.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SnippetVerifySpec {
    /// Standalone unit test or stress test inside the snippet directory (exit 0 = pass).
    #[serde(rename = "self")]
    SelfTest { file: String },
    /// Verify `adapter` against a workspace problem `problems/<problem>/`.
    #[serde(rename = "problem")]
    Problem { problem: String, adapter: String },
    /// Verify `adapter` against a `yosupo06/library-checker-problems` problem.
    #[serde(rename = "library_checker")]
    LibraryChecker { problem: String, adapter: String },
}

/// `snippets/<slug>/snippet.toml`.
///
/// Dependencies between snippets are NOT stored here; they are derived automatically
/// from `#include "<slug>/code.hpp"` directives in `code.hpp`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SnippetMeta {
    pub title: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(
        default,
        alias = "time-complexity",
        skip_serializing_if = "Option::is_none"
    )]
    pub time_complexity: Option<String>,
    #[serde(
        default,
        alias = "space-complexity",
        skip_serializing_if = "Option::is_none"
    )]
    pub space_complexity: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Optional workspace problem slug this snippet was extracted from.
    #[serde(
        default,
        alias = "source-problem",
        skip_serializing_if = "Option::is_none"
    )]
    pub source_problem: Option<String>,
    /// Verification sources (`[[verify]]`).
    #[serde(default, rename = "verify")]
    pub verify: Vec<SnippetVerifySpec>,
}

/// Partial update for `snippets/<slug>/snippet.toml` (applied via `toml_edit`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnippetMetaPatch {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    #[serde(default)]
    pub time_complexity: Patch<String>,
    #[serde(default)]
    pub space_complexity: Patch<String>,
    #[serde(default)]
    pub description: Patch<String>,
    #[serde(default)]
    pub source_problem: Patch<String>,
    #[serde(default)]
    pub verify: Option<Vec<SnippetVerifySpec>>,
}

/// Derived verification status of a snippet (never persisted in `snippet.toml`).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum SnippetVerifyStatus {
    #[default]
    Unverified,
    Passed,
    Failed,
    Stale,
}

impl SnippetVerifyStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            SnippetVerifyStatus::Unverified => "unverified",
            SnippetVerifyStatus::Passed => "passed",
            SnippetVerifyStatus::Failed => "failed",
            SnippetVerifyStatus::Stale => "stale",
        }
    }
}

impl std::fmt::Display for SnippetVerifyStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Result of running one `[[verify]]` entry for a snippet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnippetVerifyCaseResult {
    /// `"self"`, `"problem"`, or `"library_checker"`.
    pub kind: String,
    /// Filename (for `self`) or problem ID (for `problem` / `library_checker`).
    pub target: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adapter: Option<String>,
    pub passed: bool,
    /// Standard verdict string (`"AC"`, `"WA"`, `"TLE"`, `"MLE"`, `"RE"`, `"CE"`, `"JE"`).
    pub verdict: String,
    pub time_ms: u64,
    pub memory_kib: u64,
    pub passed_cases: usize,
    pub total_cases: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compile_stderr: Option<String>,
}

/// Cached/live verification report for one snippet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnippetVerifyReport {
    pub slug: String,
    /// Combined SHA-256 content hash of own `code.hpp`, all transitive deps' `code.hpp`,
    /// and all verify files / problem data.
    pub content_hash: String,
    pub status: SnippetVerifyStatus,
    pub verified_at_epoch_secs: u64,
    pub total_time_ms: u64,
    pub max_memory_kib: u64,
    #[serde(default)]
    pub cases: Vec<SnippetVerifyCaseResult>,
}

/// Dependency and reverse-reference graph slice for one snippet.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnippetReverseRefs {
    /// Direct workspace snippet dependencies parsed from `code.hpp`.
    #[serde(default)]
    pub deps: Vec<String>,
    /// All transitive workspace snippet dependencies in topological order (excluding self).
    #[serde(default)]
    pub transitive_deps: Vec<String>,
    /// Workspace snippets that directly or transitively depend on this snippet, sorted.
    #[serde(default)]
    pub reverse_deps: Vec<String>,
    /// Workspace problem slugs linked via `[[verify]] kind = "problem"`, `source_problem`, or notes.
    #[serde(default)]
    pub related_problems: Vec<String>,
}

/// Lightweight metadata summary of a snippet for listings and search.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnippetSummary {
    pub slug: String,
    pub title: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_complexity: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub space_complexity: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// `false` for pure formula/conclusion cards (no `code.hpp`).
    pub has_code: bool,
    pub has_notes: bool,
    pub verify_count: usize,
    #[serde(default)]
    pub deps: Vec<String>,
    pub code_lines: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code_hash: Option<String>,
}

/// Full snippet detail loaded from `snippets/<slug>/`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnippetDetail {
    pub slug: String,
    /// Workspace-relative directory (`snippets/<slug>` or snapshot path).
    pub dir: String,
    pub meta: SnippetMeta,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<TextFile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<TextFile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub test_file: Option<TextFile>,
    #[serde(default)]
    pub verify_files: Vec<TextFile>,
    pub reverse_refs: SnippetReverseRefs,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub printed_code: Option<PrintedSnippetCode>,
}

// ---- C++ Single-File Bundler -----------------------------------------------------------

/// Options controlling C++ single-file bundling (`#include "..."` expansion).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BundleOptions {
    /// Additional `-I` header search directories.
    #[serde(default)]
    pub include_dirs: Vec<String>,
    /// When `true`, strip C/C++ comments and collapse excess blank lines.
    #[serde(default)]
    pub compact: bool,
    /// Emit `// --- <rel> ---` boundary comments around inlined files (ignored when `compact = true`).
    #[serde(default = "default_true")]
    pub include_boundary_comments: bool,
}

impl Default for BundleOptions {
    fn default() -> Self {
        Self {
            include_dirs: Vec::new(),
            compact: false,
            include_boundary_comments: true,
        }
    }
}

fn default_true() -> bool {
    true
}

/// One resolved local `#include "..."` edge encountered during bundling.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BundleIncludeEdge {
    pub from_file: String,
    pub line: usize,
    pub raw_include: String,
    pub resolved_file: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snippet_slug: Option<String>,
}

/// Result of expanding a C++ source or snippet into a single compilable file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BundleResult {
    pub code: String,
    #[serde(default)]
    pub system_includes: Vec<String>,
    #[serde(default)]
    pub inlined_files: Vec<String>,
    #[serde(default)]
    pub inlined_snippets: Vec<String>,
    #[serde(default)]
    pub edges: Vec<BundleIncludeEdge>,
}

// ---- KACTL++ Cumulative Code Verification Hash -----------------------------------------

/// One cumulative 6-hex-char MD5 checkpoint emitted in the TRD code margin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeHashBlock {
    /// 1-based line number in [`PrintedSnippetCode::code`].
    pub line: usize,
    /// Lowercase 6-character hex prefix of MD5 over preprocessed lines `1..=line`.
    pub hash: String,
    /// `true` for the final line's total hash.
    pub is_final: bool,
}

/// Printed-view code of a snippet (with `#pragma once` and workspace snippet `#include`s stripped)
/// together with its KACTL++ cumulative verification hashes.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrintedSnippetCode {
    pub code: String,
    #[serde(default)]
    pub lines: Vec<String>,
    /// Workspace snippet slugs stripped from `#include "<slug>/code.hpp"` lines.
    #[serde(default)]
    pub stripped_deps: Vec<String>,
    #[serde(default)]
    pub hash_blocks: Vec<CodeHashBlock>,
    /// Total 6-character cumulative MD5 hash (empty when `code` has no lines).
    pub total_hash: String,
}

// ---- TRDs (Team Reference Documents) ---------------------------------------------------

/// One `[[chapter]]` entry in `trds/<id>/trd.toml`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrdChapter {
    pub title: String,
    #[serde(default)]
    pub items: Vec<String>,
}

/// `trds/<id>/trd.toml`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct TrdMeta {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    /// Template name (`"trd-compact"`, `"trd-book"`, or custom `templates/<name>.typ`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template: Option<String>,
    /// Frozen TRDs render from `snapshot/` and refuse all writes until unfrozen.
    #[serde(default)]
    pub frozen: bool,
    /// Archived (hidden in archived section); orthogonal to `frozen`.
    #[serde(default, skip_serializing_if = "is_false")]
    pub archived: bool,
    /// TRD-level template options (`[template-options.<tpl>]`).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub template_options: TemplateOptionsMap,
    /// Ordered chapters referencing snippet slugs (`[[chapter]]`).
    #[serde(default, rename = "chapter")]
    pub chapters: Vec<TrdChapter>,
}

/// Partial update for `trds/<id>/trd.toml` metadata (applied via `toml_edit`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TrdMetaPatch {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub author: Patch<String>,
    #[serde(default)]
    pub date: Patch<String>,
    #[serde(default)]
    pub template: Patch<String>,
    #[serde(default)]
    pub template_options: BTreeMap<String, TemplateOptionsPatch>,
}

/// Lightweight summary of a TRD for listings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrdSummary {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template: Option<String>,
    pub frozen: bool,
    pub archived: bool,
    pub chapter_count: usize,
    pub item_count: usize,
}

/// Loaded TRD record from `trds/<id>/`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrdDetail {
    pub id: String,
    /// Workspace-relative directory (`trds/<id>`).
    pub dir: String,
    pub meta: TrdMeta,
    pub has_local_template: bool,
    pub has_snapshot: bool,
}

/// Severity of a TRD preflight finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrdPreflightSeverity {
    Error,
    Warning,
}

/// Category of a TRD preflight finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrdPreflightKind {
    MissingSnippet,
    MissingDependency,
    DependencyCycle,
    UnverifiedSnippet,
    StaleSnippet,
    FailedSnippet,
}

/// One diagnostic produced by TRD preflight validation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrdPreflightIssue {
    pub severity: TrdPreflightSeverity,
    pub kind: TrdPreflightKind,
    pub snippet: String,
    #[serde(default)]
    pub missing_deps: Vec<String>,
    pub message: String,
}

/// Outcome of freezing a TRD into `trds/<id>/snapshot/`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrdFreezeResult {
    pub id: String,
    pub snapshot_dir: String,
    pub snippet_count: usize,
    #[serde(default)]
    pub warnings: Vec<TrdPreflightIssue>,
    pub pdf_archived: bool,
}

/// Kind of difference between a frozen TRD's snapshot and live `snippets/<slug>/`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrdSnippetDiffKind {
    CodeChanged,
    NotesChanged,
    MetaChanged,
    SnippetDeleted,
}

/// Difference for one snippet referenced by a frozen TRD.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrdUnfreezeSnippetDiff {
    pub slug: String,
    pub kinds: Vec<TrdSnippetDiffKind>,
    #[serde(default)]
    pub changed_files: Vec<String>,
}

/// Full diff report comparing a frozen TRD's `snapshot/` against the live snippet library.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrdUnfreezeDiff {
    pub id: String,
    #[serde(default)]
    pub diffs: Vec<TrdUnfreezeSnippetDiff>,
    pub has_changes: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_meta_defaults_snippets_to_true() {
        let empty: WorkspaceMeta = toml::from_str("").unwrap();
        assert_eq!(empty.snippets, None);
        assert!(empty.snippets_enabled());
        assert!(empty.training_enabled());

        let disabled: WorkspaceMeta =
            toml::from_str("snippets = false\ntraining = false\n").unwrap();
        assert_eq!(disabled.snippets, Some(false));
        assert!(!disabled.snippets_enabled());
        assert!(!disabled.training_enabled());
    }

    #[test]
    fn snippet_meta_round_trip_with_all_verify_kinds() {
        let raw = r#"
title = "快速数论变换（NTT）"
tags = ["poly", "math"]
time_complexity = "O(n log n)"
space_complexity = "O(n)"
description = "998244353 模数下的多项式乘法"
source_problem = "poly-mul"

[[verify]]
kind = "self"
file = "test.cpp"

[[verify]]
kind = "problem"
problem = "my-poly-mul"
adapter = "verify_local.cpp"

[[verify]]
kind = "library_checker"
problem = "convolution_mod"
adapter = "verify_conv.cpp"
"#;
        let meta: SnippetMeta = toml::from_str(raw).unwrap();
        assert_eq!(meta.title, "快速数论变换（NTT）");
        assert_eq!(meta.tags, vec!["poly", "math"]);
        assert_eq!(meta.verify.len(), 3);
        assert!(matches!(meta.verify[0], SnippetVerifySpec::SelfTest { .. }));
        assert!(matches!(meta.verify[1], SnippetVerifySpec::Problem { .. }));
        assert!(matches!(
            meta.verify[2],
            SnippetVerifySpec::LibraryChecker { .. }
        ));
        let back: SnippetMeta = toml::from_str(&toml::to_string(&meta).unwrap()).unwrap();
        assert_eq!(meta, back);
    }

    #[test]
    fn trd_meta_round_trip() {
        let raw = r#"
name = "2026 ICPC 区域赛模板册"
author = "队名"
date = "2026 年 11 月"
template = "trd-compact"
frozen = false
archived = false

[template-options.trd-compact]
columns = 3
hash-interval = 10
strip-comments = false

[[chapter]]
title = "数学与多项式"
items = ["modint", "ntt", "number-theory-facts"]

[[chapter]]
title = "数据结构"
items = ["dsu"]
"#;
        let meta: TrdMeta = toml::from_str(raw).unwrap();
        assert_eq!(meta.name, "2026 ICPC 区域赛模板册");
        assert_eq!(meta.chapters.len(), 2);
        assert_eq!(meta.chapters[0].items.len(), 3);
        assert!(meta.template_options.contains_key("trd-compact"));
        let back: TrdMeta = toml::from_str(&toml::to_string(&meta).unwrap()).unwrap();
        assert_eq!(meta, back);
    }
}

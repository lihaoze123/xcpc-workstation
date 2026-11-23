//! Public request, result, and warning types for `xcpc-packages`.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use xcpc_domain::Language;

/// Structured non-fatal warning emitted during import or export.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageWarning {
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

impl PackageWarning {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            path: None,
        }
    }

    pub fn with_path(
        code: impl Into<String>,
        message: impl Into<String>,
        path: impl Into<String>,
    ) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            path: Some(path.into()),
        }
    }
}

/// Whether a Polygon ZIP was detected as a single problem or a contest archive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PolygonImportKind {
    Problem,
    Contest,
}

/// Options controlling [`crate::import_polygon`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolygonImportOptions {
    /// Explicit target slug (for single problem) or contest ID (for contest).
    /// When `None`, derived from the archive's short name / XML metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_id: Option<String>,
    /// Optional persistent group name to assign to imported problem(s).
    /// Takes precedence over automatic contest-name grouping when set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    /// When `true`, disables automatic grouping by contest name for Polygon contest imports.
    #[serde(default)]
    pub no_auto_group: bool,
    /// Maximum total uncompressed bytes allowed from the ZIP (default: 512 MiB).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_uncompressed_bytes: Option<u64>,
    /// Maximum number of entries allowed in the ZIP (default: 8192).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_entry_count: Option<usize>,
}

/// Summary of one imported problem.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportedProblemSummary {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub title: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub languages: Vec<String>,
    pub samples_count: usize,
    pub tests_count: usize,
    pub generators_count: usize,
    pub solutions_count: usize,
    pub has_validator: bool,
    #[serde(default)]
    pub has_interactor: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runs: Option<u32>,
    #[serde(default)]
    pub subtasks_count: usize,
    pub checker: String,
    pub assets_count: usize,
}

/// Result returned by [`crate::import_polygon`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolygonImportResult {
    pub kind: PolygonImportKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contest_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    pub problems: Vec<ImportedProblemSummary>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<PackageWarning>,
}

/// Supported OJ export package formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OjFormat {
    Domjudge,
    Hydro,
}

impl OjFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            OjFormat::Domjudge => "domjudge",
            OjFormat::Hydro => "hydro",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "domjudge" | "dj" => Some(OjFormat::Domjudge),
            "hydro" | "hydrooj" => Some(OjFormat::Hydro),
            _ => None,
        }
    }
}

/// Target of [`crate::export_oj_package`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "id", rename_all = "lowercase")]
pub enum ExportTarget {
    Problem(String),
    Contest(String),
}

/// Options controlling [`crate::export_oj_package`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportOjOptions {
    pub format: OjFormat,
    /// For a single problem: target `.zip` file path (or an existing directory where `<slug>.zip` is created).
    /// For a contest: target directory where `A-<slug>.zip`, `B-<slug>.zip`, ... are created.
    pub output_path: PathBuf,
    /// Bypass only the `verified == true` gate; built testdata in `build/problems/<slug>/` is still required.
    #[serde(default)]
    pub force: bool,
    /// Optional statement language override (used for DOMjudge `problem.pdf` and Hydro primary title).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<Language>,
}

/// One exported ZIP artifact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportedArtifact {
    pub problem_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub zip_path: String,
    pub sample_count: usize,
    pub secret_count: usize,
    pub size_bytes: u64,
    pub verified: bool,
    pub forced: bool,
}

/// Result returned by [`crate::export_oj_package`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportOjResult {
    pub format: OjFormat,
    pub artifacts: Vec<ExportedArtifact>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<PackageWarning>,
}

/// Per-problem outcome status for [`crate::export_oj_packages_batch`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BatchExportItemStatus {
    Exported,
    Skipped,
    Failed,
    Cancelled,
}

/// Per-problem result entry in [`BatchExportOjResult`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatchExportItemResult {
    pub problem_id: String,
    pub status: BatchExportItemStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zip_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
    #[serde(default)]
    pub sample_count: usize,
    #[serde(default)]
    pub secret_count: usize,
    #[serde(default)]
    pub verified: bool,
    #[serde(default)]
    pub forced: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// Streaming progress event emitted by [`crate::export_oj_packages_batch`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BatchExportEvent {
    ProblemStarted {
        problem_id: String,
        index: usize,
        total: usize,
    },
    ProblemFinished {
        item: BatchExportItemResult,
        index: usize,
        total: usize,
    },
    Cancelled {
        completed: usize,
        total: usize,
    },
    AllFinished {
        exported: usize,
        skipped: usize,
        failed: usize,
        cancelled: usize,
        total: usize,
        output_dir: String,
    },
}

/// Aggregated result returned by [`crate::export_oj_packages_batch`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatchExportOjResult {
    pub ok: bool,
    pub format: OjFormat,
    pub output_dir: String,
    pub cancelled: bool,
    pub total: usize,
    pub exported: usize,
    pub skipped: usize,
    pub failed: usize,
    pub cancelled_count: usize,
    pub items: Vec<BatchExportItemResult>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<PackageWarning>,
}

//! `xcpc-packages`: Polygon ZIP import and DOMjudge / Hydro minimal-set OJ export (Phase 3B).

pub mod error;
pub mod export;
pub mod latex;
pub mod polygon;
pub mod types;
pub mod zip_util;

pub use error::{PackageError, Result};
pub use export::{default_batch_export_dir, export_oj_package, export_oj_packages_batch};
pub use polygon::import_polygon;
pub use types::{
    BatchExportEvent, BatchExportItemResult, BatchExportItemStatus, BatchExportOjResult,
    ExportOjOptions, ExportOjResult, ExportTarget, ExportedArtifact, ImportedProblemSummary,
    OjFormat, PackageWarning, PolygonImportKind, PolygonImportOptions, PolygonImportResult,
};

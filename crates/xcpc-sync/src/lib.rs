pub mod analytics;
pub mod config;
pub mod db;
pub mod error;
pub mod ingest;
pub mod scheduler;

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

pub use analytics::{
    ContestCalendarItem, ContestsQuery, DifficultyBucket, DifficultyTrendPoint, EditorialQuery,
    EditorialQueryResult, HeatmapCell, PlatformAccountMetric, PlatformRatingSeries, RatingPoint,
    SubmissionViewRow, SubmissionsPage, SubmissionsQuery, TagStat, TimeWindow, TrainingDashboard,
    TrainingDashboardQuery, TrainingKpi, WorkspaceBacklogItem, compute_training_dashboard,
    query_contests, query_submissions, query_training_editorials,
};
pub use config::{CONFIG_FILENAME, SyncConfig};
pub use db::{
    CachedAccount, CachedContest, CachedProblemMeta, CachedRatingChange, CachedSubmission,
    CachedSummary, DB_FILENAME, SyncDb,
};
pub use error::{Result, SyncError};
pub use ingest::{
    IngestProblemRequest, IngestProblemResult, ManualLogRequest, ManualLogResult, ingest_problem,
    log_training_record,
};
pub use scheduler::{
    PlatformSyncReport, ResolvedProblemUrl, SyncEngine, SyncOptions, SyncReport,
    suggest_problem_slug,
};
pub use xcpc_domain::EditorialSummary;

/// Desktop application ID from `apps/desktop/nsh.json`, used to share `<dataDir>`
/// seamlessly between the CLI and the NSH desktop application.
pub const APP_ID: &str = "dev.xcpc.workstation";

/// Resolves the global XCPC Workstation data directory (`<dataDir>`).
///
/// Precedence:
/// 1. `XCPC_DATA_DIR` environment variable (when set and non-empty).
/// 2. Platform standard app data directory matching NSH's `<support_dir>/dev.xcpc.workstation`:
///    - macOS / iOS: `$HOME/Library/Application Support/dev.xcpc.workstation`
///    - Windows: `%APPDATA%\dev.xcpc.workstation`
///    - Linux / other Unix: `$XDG_DATA_HOME/dev.xcpc.workstation` or `$HOME/.local/share/dev.xcpc.workstation`
pub fn default_data_dir() -> Option<PathBuf> {
    default_data_dir_with(std::env::consts::OS, |name| {
        std::env::var_os(name).map(PathBuf::from)
    })
}

pub(crate) fn default_data_dir_with(
    os: &str,
    env: impl Fn(&str) -> Option<PathBuf>,
) -> Option<PathBuf> {
    let get = |name: &str| env(name).filter(|v| !v.as_os_str().is_empty());
    if let Some(override_dir) = get("XCPC_DATA_DIR") {
        return Some(override_dir);
    }
    let support = match os {
        "android" => None,
        "macos" | "ios" => get("HOME").map(|home| home.join("Library/Application Support")),
        "windows" => get("APPDATA"),
        _ => get("XDG_DATA_HOME").or_else(|| get("HOME").map(|home| home.join(".local/share"))),
    }?;
    Some(support.join(APP_ID))
}

/// Returns the current UTC wall-clock timestamp formatted as RFC 3339 (`YYYY-MM-DDTHH:MM:SSZ`).
pub fn now_rfc3339_utc() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    unix_secs_to_rfc3339_utc(secs)
}

/// Formats a Unix timestamp in seconds as RFC 3339 UTC (`YYYY-MM-DDTHH:MM:SSZ`).
pub fn unix_secs_to_rfc3339_utc(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let hour = rem / 3_600;
    let minute = (rem % 3_600) / 60;
    let second = rem % 60;

    // Howard Hinnant's civil-from-days algorithm.
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = y + i64::from(m <= 2);

    format!("{year:04}-{m:02}-{d:02}T{hour:02}:{minute:02}:{second:02}Z")
}

#[cfg(test)]
mod analytics_tests;
#[cfg(test)]
mod tests;

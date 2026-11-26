use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt;
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use xcpc_domain::{
    EditorialSummary, ProblemMeta, ProblemSource, TrainingLogEntry, TrainingLogStatus,
    TrainingStatus,
};
use xcpc_storage::Workspace;

use crate::config::SyncConfig;
use crate::db::{CachedSubmission, SyncDb};
use crate::error::Result;

const SECS_PER_DAY: i64 = 86_400;
const MAX_UNSOLVED_DASHBOARD_ITEMS: usize = 50;
const DEFAULT_SUBMISSIONS_LIMIT: usize = 100;

/// Global time window for training dashboard analytics.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TimeWindow {
    #[serde(rename = "30d")]
    Days30,
    #[serde(rename = "90d")]
    Days90,
    #[serde(rename = "year")]
    Year,
    #[default]
    #[serde(rename = "all")]
    All,
}

impl TimeWindow {
    pub fn as_str(self) -> &'static str {
        match self {
            TimeWindow::Days30 => "30d",
            TimeWindow::Days90 => "90d",
            TimeWindow::Year => "year",
            TimeWindow::All => "all",
        }
    }

    pub fn cutoff_unix(self, now_unix: i64) -> Option<i64> {
        match self {
            TimeWindow::Days30 => Some(now_unix - 30 * SECS_PER_DAY),
            TimeWindow::Days90 => Some(now_unix - 90 * SECS_PER_DAY),
            TimeWindow::Year => {
                let (year, _, _) = unix_secs_to_ymd(now_unix);
                Some(ymd_to_unix_secs(year, 1, 1))
            }
            TimeWindow::All => None,
        }
    }
}

impl fmt::Display for TimeWindow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for TimeWindow {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "30d" | "days30" => Ok(TimeWindow::Days30),
            "90d" | "days90" => Ok(TimeWindow::Days90),
            "year" | "1y" => Ok(TimeWindow::Year),
            "all" => Ok(TimeWindow::All),
            other => Err(format!(
                "invalid time window {other:?}; expected one of: 30d, 90d, year, all"
            )),
        }
    }
}

/// Query parameters for [`compute_training_dashboard`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrainingDashboardQuery {
    #[serde(default)]
    pub window: TimeWindow,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heatmap_year: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag_min_rating: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub now_unix: Option<i64>,
}

/// Per-platform account summary card on the training dashboard.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlatformAccountMetric {
    pub platform: String,
    pub handle: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rating: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_rating: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rank: Option<String>,
    pub solved_count: u64,
    pub submission_count: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

/// High-level KPI metrics for the training dashboard.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrainingKpi {
    pub today_ac: u64,
    pub today_submissions: u64,
    pub week_ac: u64,
    pub week_submissions: u64,
    pub month_ac: u64,
    pub month_submissions: u64,
    pub window_ac: u64,
    pub window_submissions: u64,
    pub total_ac: u64,
    pub total_submissions: u64,
    pub streak_days: u64,
    pub longest_streak_days: u64,
    pub unsolved_attempted_count: u64,
    pub library_todo_count: u64,
    pub library_attempted_count: u64,
}

/// One active day cell for the dual-mode heatmap (AC count & max solved rating).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeatmapCell {
    pub date: String,
    pub ac_count: u64,
    pub submission_count: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_rating: Option<i64>,
}

/// One contest point in a platform's rating trajectory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RatingPoint {
    pub contest_id: String,
    pub contest_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rating_before: Option<i64>,
    pub rating_after: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delta: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rank: Option<i64>,
    pub at: i64,
}

/// Chronological rating series for a single OJ platform.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlatformRatingSeries {
    pub platform: String,
    pub points: Vec<RatingPoint>,
}

/// Effective difficulty progression (median / P75 / P90 of solved problems) for one ISO week or month.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DifficultyTrendPoint {
    pub period: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub p75: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub p90: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub median: Option<i64>,
    pub ac_count: u64,
    pub rated_ac_count: u64,
}

/// Solved problem count in a unified Rating bucket.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DifficultyBucket {
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_rating: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_rating: Option<i64>,
    pub ac_count: u64,
}

/// Per-tag AC count and average unified Rating.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TagStat {
    pub tag: String,
    pub ac_count: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avg_rating: Option<i64>,
}

/// Workspace problem currently in `todo` or `attempted` derived training state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceBacklogItem {
    pub problem_id: String,
    pub title: String,
    pub status: TrainingStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rating: Option<u32>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub sources: Vec<ProblemSource>,
}

/// Complete payload for the Phase 4B training dashboard (GUI, CLI `--json`, MCP).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrainingDashboard {
    pub window: TimeWindow,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_synced_at: Option<String>,
    #[serde(default)]
    pub has_local_records: bool,
    #[serde(default)]
    pub local_solved_count: u64,
    pub accounts: Vec<PlatformAccountMetric>,
    pub kpi: TrainingKpi,
    pub available_years: Vec<i32>,
    pub heatmap_year: i32,
    pub heatmap: Vec<HeatmapCell>,
    pub rating_series: Vec<PlatformRatingSeries>,
    pub difficulty_trend: Vec<DifficultyTrendPoint>,
    pub difficulty_buckets: Vec<DifficultyBucket>,
    pub tag_distribution: Vec<TagStat>,
    pub unsolved_problems: Vec<SubmissionViewRow>,
    pub workspace_backlog: Vec<WorkspaceBacklogItem>,
}

/// Query parameters for [`query_training_editorials`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditorialQuery {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub search: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_rating: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_rating: Option<i32>,
}

/// Response payload for [`query_training_editorials`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditorialQueryResult {
    pub items: Vec<EditorialSummary>,
    pub total: usize,
    #[serde(default)]
    pub available_tags: Vec<String>,
}

/// Query parameters for [`query_contests`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContestsQuery {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub platform: Option<String>,
    #[serde(default)]
    pub include_finished: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub now_unix: Option<i64>,
}

/// Aggregated contest entry for the contest calendar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContestCalendarItem {
    pub platform: String,
    pub contest_id: String,
    pub name: String,
    pub start_time: i64,
    pub duration_sec: i64,
    pub end_time: i64,
    /// `"running" | "upcoming" | "finished"`
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    pub updated_at: String,
}

/// Query parameters for [`query_submissions`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubmissionsQuery {
    #[serde(default)]
    pub by_problem: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub platform: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verdict: Option<String>,
    #[serde(default)]
    pub unsolved_only: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_rating: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_rating: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub search: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<usize>,
}

/// One row in the submissions table (either a raw submission or a per-problem aggregate).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubmissionViewRow {
    pub platform: String,
    pub submission_id: String,
    pub handle: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contest_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub problem_index: Option<String>,
    pub problem_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub problem_title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub problem_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_rating: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub normalized_rating: Option<i64>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub verdict: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    pub submitted_at: i64,
    pub attempt_count: u64,
    pub solved: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub library_problem_id: Option<String>,
}

/// Paginated response from [`query_submissions`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubmissionsPage {
    pub total: usize,
    pub offset: usize,
    pub limit: usize,
    pub items: Vec<SubmissionViewRow>,
}

/// Computes the complete training dashboard analytics payload from `oj_sync.db`, global config,
/// and an optional workspace.
pub fn compute_training_dashboard(
    db: &SyncDb,
    config: &SyncConfig,
    workspace: Option<&Workspace>,
    query: &TrainingDashboardQuery,
) -> Result<TrainingDashboard> {
    let now_unix = query.now_unix.unwrap_or_else(current_unix_secs);
    let today_day = now_unix.div_euclid(SECS_PER_DAY);
    let (current_year, _, _) = days_to_ymd(today_day);
    let heatmap_year = query.heatmap_year.unwrap_or(current_year);
    let window_cutoff = query.window.cutoff_unix(now_unix);
    let week_cutoff = now_unix - 7 * SECS_PER_DAY;
    let month_cutoff = now_unix - 30 * SECS_PER_DAY;

    let cached_accounts = db.list_accounts()?;
    let cached_summaries = db.list_summaries()?;
    let rating_history = db.list_rating_history(None)?;
    let submissions = db.list_submissions(None)?;

    let summary_solved_keys = build_summary_solved_set(&cached_summaries);

    // Pre-scan workspace once if provided (for library_problem_id mapping, manual logs, and backlog).
    let ws_info = if let Some(ws) = workspace {
        collect_workspace_training_info(db, ws)?
    } else {
        WorkspaceTrainingInfo::default()
    };
    let source_to_problem = ws_info.source_map;
    let problems_by_id = ws_info.problems_by_id;
    let manual_logs = ws_info.logs;
    let workspace_backlog = ws_info.backlog;
    let library_todo_count = ws_info.todo_count;
    let library_attempted_count = ws_info.attempted_count;

    // Track OJ keys & normalized URLs for cross-source deduplication with local manual logs.
    let mut oj_known_keys: HashSet<(String, String)> = summary_solved_keys.clone();
    let mut oj_url_to_key: HashMap<String, (String, String)> = HashMap::new();

    // Per-platform submission aggregates + global KPI sets.
    let mut platform_sub_counts: HashMap<String, u64> = HashMap::new();
    let mut platform_ac_keys: HashMap<String, HashSet<String>> = HashMap::new();
    let mut platform_first_handle: HashMap<String, String> = HashMap::new();

    let mut today_ac_set: HashSet<(String, String)> = HashSet::new();
    let mut today_submissions = 0u64;
    let mut week_ac_set: HashSet<(String, String)> = HashSet::new();
    let mut week_submissions = 0u64;
    let mut month_ac_set: HashSet<(String, String)> = HashSet::new();
    let mut month_submissions = 0u64;
    let mut window_ac_set: HashSet<(String, String)> = HashSet::new();
    let mut window_submissions = 0u64;
    let mut all_ac_set: HashSet<(String, String)> = HashSet::new();
    let mut all_ac_days: BTreeSet<i64> = BTreeSet::new();
    let mut years_set: BTreeSet<i32> = BTreeSet::new();
    years_set.insert(current_year);

    // Heatmap aggregation for `heatmap_year`.
    struct DayAgg {
        ac_keys: HashSet<(String, String)>,
        submission_count: u64,
        max_rating: Option<i64>,
    }
    let mut heatmap_days: BTreeMap<i64, DayAgg> = BTreeMap::new();

    // Window distinct AC problems with merged metadata (for difficulty_buckets & tag_distribution).
    struct WindowAcProblem {
        rating: Option<i64>,
        tags: Vec<String>,
    }
    let mut window_ac_problems: HashMap<(String, String), WindowAcProblem> = HashMap::new();

    // Difficulty trend periods -> distinct AC problems -> rating.
    let use_iso_week = matches!(query.window, TimeWindow::Days30 | TimeWindow::Days90);
    let mut trend_periods: BTreeMap<String, HashMap<(String, String), Option<i64>>> =
        BTreeMap::new();

    for sub in &submissions {
        let p_lower = sub.platform.trim().to_ascii_lowercase();
        let k_lower = sub.problem_key.trim().to_ascii_lowercase();
        let pk = (p_lower.clone(), k_lower.clone());
        oj_known_keys.insert(pk.clone());
        if let Some(ref url) = sub.problem_url {
            let norm = normalize_url(url);
            if !norm.is_empty() {
                oj_url_to_key.entry(norm).or_insert_with(|| pk.clone());
            }
        }
        let ac = is_ac_verdict(&sub.verdict);
        let sub_day = sub.submitted_at.div_euclid(SECS_PER_DAY);
        let (sub_year, sub_month, _) = days_to_ymd(sub_day);
        years_set.insert(sub_year);

        *platform_sub_counts.entry(p_lower.clone()).or_insert(0) += 1;
        platform_first_handle
            .entry(p_lower.clone())
            .or_insert_with(|| sub.handle.clone());

        if ac {
            platform_ac_keys
                .entry(p_lower.clone())
                .or_default()
                .insert(k_lower.clone());
            all_ac_set.insert(pk.clone());
            all_ac_days.insert(sub_day);
        }

        if sub_day == today_day {
            today_submissions += 1;
            if ac {
                today_ac_set.insert(pk.clone());
            }
        }
        if sub.submitted_at >= week_cutoff {
            week_submissions += 1;
            if ac {
                week_ac_set.insert(pk.clone());
            }
        }
        if sub.submitted_at >= month_cutoff {
            month_submissions += 1;
            if ac {
                month_ac_set.insert(pk.clone());
            }
        }

        let in_window = window_cutoff.is_none_or(|c| sub.submitted_at >= c);
        if in_window {
            window_submissions += 1;
            if ac {
                window_ac_set.insert(pk.clone());

                let entry =
                    window_ac_problems
                        .entry(pk.clone())
                        .or_insert_with(|| WindowAcProblem {
                            rating: None,
                            tags: Vec::new(),
                        });
                if entry.rating.is_none() && sub.normalized_rating.is_some() {
                    entry.rating = sub.normalized_rating;
                }
                if entry.tags.is_empty() && !sub.tags.is_empty() {
                    entry.tags = sub.tags.clone();
                }

                let period = if use_iso_week {
                    let (iy, iw) = iso_week_from_days(sub_day);
                    format!("{iy:04}-W{iw:02}")
                } else {
                    format!("{sub_year:04}-{sub_month:02}")
                };
                let period_entry = trend_periods
                    .entry(period)
                    .or_default()
                    .entry(pk.clone())
                    .or_insert(None);
                if period_entry.is_none() && sub.normalized_rating.is_some() {
                    *period_entry = sub.normalized_rating;
                }
            }
        }

        if sub_year == heatmap_year {
            let cell = heatmap_days.entry(sub_day).or_insert_with(|| DayAgg {
                ac_keys: HashSet::new(),
                submission_count: 0,
                max_rating: None,
            });
            cell.submission_count += 1;
            if ac {
                cell.ac_keys.insert(pk);
                if let Some(r) = sub.normalized_rating {
                    cell.max_rating = Some(cell.max_rating.map_or(r, |prev| prev.max(r)));
                }
            }
        }
    }

    // Build per-platform account metrics (merging oj_accounts, oj_summaries, config.handles, submissions).
    let mut account_map: HashMap<String, crate::db::CachedAccount> = HashMap::new();
    for acc in cached_accounts {
        account_map.insert(acc.platform.trim().to_ascii_lowercase(), acc);
    }
    let mut summary_map: HashMap<String, crate::db::CachedSummary> = HashMap::new();
    for sum in cached_summaries {
        summary_map.insert(sum.platform.trim().to_ascii_lowercase(), sum);
    }

    let mut all_platforms: BTreeSet<String> = BTreeSet::new();
    all_platforms.extend(account_map.keys().cloned());
    all_platforms.extend(summary_map.keys().cloned());
    all_platforms.extend(platform_sub_counts.keys().cloned());
    for (k, v) in &config.handles {
        let p = k.trim().to_ascii_lowercase();
        if !p.is_empty() && !v.trim().is_empty() {
            all_platforms.insert(p);
        }
    }

    let mut accounts = Vec::with_capacity(all_platforms.len());
    let mut total_ac = 0u64;
    let mut total_submissions = 0u64;

    for platform in all_platforms {
        let acc = account_map.get(&platform);
        let sum = summary_map.get(&platform);

        let handle = acc
            .map(|a| a.handle.clone())
            .or_else(|| sum.map(|s| s.handle.clone()))
            .or_else(|| config.handle_for(&platform).map(str::to_owned))
            .or_else(|| platform_first_handle.get(&platform).cloned())
            .unwrap_or_default();

        let rating = acc
            .and_then(|a| a.rating)
            .or_else(|| sum.and_then(|s| s.rating));
        let max_rating = acc.and_then(|a| a.max_rating).or(rating);
        let rank = acc.and_then(|a| a.rank.clone());

        let mut merged_ac_keys = platform_ac_keys.get(&platform).cloned().unwrap_or_default();
        if let Some(s) = sum {
            for k in &s.solved_keys {
                let norm = k.trim().to_ascii_lowercase();
                if !norm.is_empty() {
                    merged_ac_keys.insert(norm);
                }
            }
        }
        let solved_count = (merged_ac_keys.len() as u64).max(sum.map_or(0, |s| s.solved_count));

        let sub_count = platform_sub_counts.get(&platform).copied().unwrap_or(0);
        let submission_count = sub_count.max(sum.and_then(|s| s.submission_count).unwrap_or(0));

        let updated_at = match (
            acc.map(|a| a.updated_at.as_str()),
            sum.map(|s| s.updated_at.as_str()),
        ) {
            (Some(a), Some(b)) => Some(a.max(b).to_string()),
            (Some(a), None) => Some(a.to_string()),
            (None, Some(b)) => Some(b.to_string()),
            (None, None) => None,
        };

        total_ac += solved_count;
        total_submissions += submission_count;

        accounts.push(PlatformAccountMetric {
            platform,
            handle,
            rating,
            max_rating,
            rank,
            solved_count,
            submission_count,
            updated_at,
        });
    }

    // Merge local manual training logs from `training/logs.jsonl`, deduplicating against OJ solves.
    let mut oj_solved_keys: HashSet<(String, String)> = all_ac_set.clone();
    oj_solved_keys.extend(summary_solved_keys.iter().cloned());

    let mut has_local_records = false;
    let mut local_solved_count = 0u64;

    if !manual_logs.is_empty() {
        let resolved_manual = resolve_manual_log_problems(
            &manual_logs,
            &problems_by_id,
            &source_to_problem,
            &oj_known_keys,
            &oj_url_to_key,
        );
        for rec in resolved_manual.into_values() {
            if rec.latest_status == TrainingLogStatus::Clear {
                continue;
            }
            has_local_records = true;

            if !matches!(
                rec.latest_status,
                TrainingLogStatus::Ac | TrainingLogStatus::Upsolved
            ) {
                continue;
            }
            if oj_solved_keys.contains(&rec.canonical_key) {
                // Already solved in `oj_sync.db`; never double-count.
                continue;
            }

            local_solved_count += 1;
            total_ac += 1;
            total_submissions += 1;

            let pk = rec.canonical_key;
            let at = rec.effective_ac_at.unwrap_or(rec.latest_at);
            let ac_day = at.div_euclid(SECS_PER_DAY);
            let (ac_year, ac_month, _) = days_to_ymd(ac_day);
            years_set.insert(ac_year);
            all_ac_set.insert(pk.clone());
            all_ac_days.insert(ac_day);

            if ac_day == today_day {
                today_submissions += 1;
                today_ac_set.insert(pk.clone());
            }
            if at >= week_cutoff {
                week_submissions += 1;
                week_ac_set.insert(pk.clone());
            }
            if at >= month_cutoff {
                month_submissions += 1;
                month_ac_set.insert(pk.clone());
            }

            let in_window = window_cutoff.is_none_or(|c| at >= c);
            if in_window {
                window_submissions += 1;
                window_ac_set.insert(pk.clone());

                let entry =
                    window_ac_problems
                        .entry(pk.clone())
                        .or_insert_with(|| WindowAcProblem {
                            rating: None,
                            tags: Vec::new(),
                        });
                if entry.rating.is_none() && rec.rating.is_some() {
                    entry.rating = rec.rating;
                }
                if entry.tags.is_empty() && !rec.tags.is_empty() {
                    entry.tags = rec.tags.clone();
                }

                let period = if use_iso_week {
                    let (iy, iw) = iso_week_from_days(ac_day);
                    format!("{iy:04}-W{iw:02}")
                } else {
                    format!("{ac_year:04}-{ac_month:02}")
                };
                let period_entry = trend_periods
                    .entry(period)
                    .or_default()
                    .entry(pk.clone())
                    .or_insert(None);
                if period_entry.is_none() && rec.rating.is_some() {
                    *period_entry = rec.rating;
                }
            }

            if ac_year == heatmap_year {
                let cell = heatmap_days.entry(ac_day).or_insert_with(|| DayAgg {
                    ac_keys: HashSet::new(),
                    submission_count: 0,
                    max_rating: None,
                });
                cell.submission_count += 1;
                cell.ac_keys.insert(pk);
                if let Some(r) = rec.rating {
                    cell.max_rating = Some(cell.max_rating.map_or(r, |prev| prev.max(r)));
                }
            }
        }
    }

    // Compute streaks.
    let (streak_days, longest_streak_days) = compute_streaks(&all_ac_days, today_day);

    // Build heatmap cells (sorted ascending by date).
    let heatmap: Vec<HeatmapCell> = heatmap_days
        .into_iter()
        .map(|(day, agg)| {
            let (y, m, d) = days_to_ymd(day);
            HeatmapCell {
                date: format!("{y:04}-{m:02}-{d:02}"),
                ac_count: agg.ac_keys.len() as u64,
                submission_count: agg.submission_count,
                max_rating: agg.max_rating,
            }
        })
        .collect();

    // Available years descending.
    let available_years: Vec<i32> = years_set.into_iter().rev().collect();

    // Rating series grouped by platform, sorted by `at` asc.
    let mut series_map: BTreeMap<String, Vec<RatingPoint>> = BTreeMap::new();
    for item in rating_history {
        let p = item.platform.trim().to_ascii_lowercase();
        let delta = item.rating_before.map(|before| item.rating_after - before);
        series_map.entry(p).or_default().push(RatingPoint {
            contest_id: item.contest_id,
            contest_name: item.contest_name,
            rating_before: item.rating_before,
            rating_after: item.rating_after,
            delta,
            rank: item.rank,
            at: item.at,
        });
    }
    let rating_series: Vec<PlatformRatingSeries> = series_map
        .into_iter()
        .map(|(platform, mut points)| {
            points.sort_by(|a, b| {
                a.at.cmp(&b.at)
                    .then_with(|| a.contest_id.cmp(&b.contest_id))
            });
            PlatformRatingSeries { platform, points }
        })
        .collect();

    // Difficulty trend points (sorted chronologically by period).
    let difficulty_trend: Vec<DifficultyTrendPoint> = trend_periods
        .into_iter()
        .map(|(period, probs)| {
            let ac_count = probs.len() as u64;
            let mut ratings: Vec<i64> = probs.into_values().flatten().collect();
            ratings.sort_unstable();
            let rated_ac_count = ratings.len() as u64;
            DifficultyTrendPoint {
                period,
                p75: percentile_i64(&ratings, 0.75),
                p90: percentile_i64(&ratings, 0.90),
                median: percentile_i64(&ratings, 0.50),
                ac_count,
                rated_ac_count,
            }
        })
        .collect();

    // Difficulty buckets (unified Rating scale, filtered by global window).
    let mut difficulty_buckets = default_difficulty_buckets();
    for prob in window_ac_problems.values() {
        if let Some(r) = prob.rating {
            let idx = difficulty_bucket_index(r);
            difficulty_buckets[idx].ac_count += 1;
        }
    }

    // Tag distribution (filtered by global window + tag_min_rating).
    struct TagAgg {
        display: String,
        ac_count: u64,
        rating_sum: i64,
        rated_count: u64,
    }
    let mut tag_map: HashMap<String, TagAgg> = HashMap::new();
    for prob in window_ac_problems.values() {
        if let Some(min_r) = query.tag_min_rating
            && prob.rating.is_none_or(|r| r < min_r)
        {
            continue;
        }
        let mut seen_in_problem: HashSet<String> = HashSet::new();
        for raw_tag in &prob.tags {
            let trimmed = raw_tag.trim();
            if trimmed.is_empty() {
                continue;
            }
            let norm = trimmed.to_ascii_lowercase();
            if !seen_in_problem.insert(norm.clone()) {
                continue;
            }
            let entry = tag_map.entry(norm).or_insert_with(|| TagAgg {
                display: trimmed.to_string(),
                ac_count: 0,
                rating_sum: 0,
                rated_count: 0,
            });
            entry.ac_count += 1;
            if let Some(r) = prob.rating {
                entry.rating_sum += r;
                entry.rated_count += 1;
            }
        }
    }
    let mut tag_distribution: Vec<TagStat> = tag_map
        .into_values()
        .map(|agg| TagStat {
            tag: agg.display,
            ac_count: agg.ac_count,
            avg_rating: (agg.rated_count > 0)
                .then(|| (agg.rating_sum as f64 / agg.rated_count as f64).round() as i64),
        })
        .collect();
    tag_distribution.sort_by(|a, b| {
        b.ac_count
            .cmp(&a.ac_count)
            .then_with(|| a.tag.to_ascii_lowercase().cmp(&b.tag.to_ascii_lowercase()))
    });

    // Unsolved attempted problems (aggregated by problem, max 50, newest first).
    let mut global_solved = all_ac_set;
    global_solved.extend(summary_solved_keys);

    let aggregated_problems = aggregate_submissions_by_problem(&submissions, &global_solved);
    let mut unsolved_all: Vec<SubmissionViewRow> = aggregated_problems
        .into_iter()
        .filter(|row| !row.solved)
        .collect();
    let unsolved_attempted_count = unsolved_all.len() as u64;
    unsolved_all.truncate(MAX_UNSOLVED_DASHBOARD_ITEMS);
    for row in &mut unsolved_all {
        row.library_problem_id =
            lookup_library_problem_id(&source_to_problem, &row.platform, &row.problem_key);
    }

    let kpi = TrainingKpi {
        today_ac: today_ac_set.len() as u64,
        today_submissions,
        week_ac: week_ac_set.len() as u64,
        week_submissions,
        month_ac: month_ac_set.len() as u64,
        month_submissions,
        window_ac: window_ac_set.len() as u64,
        window_submissions,
        total_ac,
        total_submissions,
        streak_days,
        longest_streak_days,
        unsolved_attempted_count,
        library_todo_count,
        library_attempted_count,
    };

    Ok(TrainingDashboard {
        window: query.window,
        last_synced_at: config.last_synced_at.clone(),
        has_local_records,
        local_solved_count,
        accounts,
        kpi,
        available_years,
        heatmap_year,
        heatmap,
        rating_series,
        difficulty_trend,
        difficulty_buckets,
        tag_distribution,
        unsolved_problems: unsolved_all,
        workspace_backlog,
    })
}

/// Aggregates synced contests across all platforms, computing `end_time` and `status`
/// (`"running" | "upcoming" | "finished"`), ordered by `start_time` ascending.
pub fn query_contests(db: &SyncDb, query: &ContestsQuery) -> Result<Vec<ContestCalendarItem>> {
    let now_unix = query.now_unix.unwrap_or_else(current_unix_secs);
    let platform_filter = query
        .platform
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_ascii_lowercase);

    let contests = db.list_contests()?;
    let mut out = Vec::with_capacity(contests.len());

    for c in contests {
        if let Some(ref pf) = platform_filter
            && !c.platform.eq_ignore_ascii_case(pf)
        {
            continue;
        }
        let end_time = c.start_time.saturating_add(c.duration_sec.max(0));
        let status = if now_unix < c.start_time {
            "upcoming"
        } else if now_unix < end_time {
            "running"
        } else {
            "finished"
        };
        if !query.include_finished && status == "finished" {
            continue;
        }
        out.push(ContestCalendarItem {
            platform: c.platform,
            contest_id: c.contest_id,
            name: c.name,
            start_time: c.start_time,
            duration_sec: c.duration_sec,
            end_time,
            status: status.to_string(),
            url: c.url,
            updated_at: c.updated_at,
        });
    }

    Ok(out)
}

/// Queries synced submissions with optional per-problem deduplication, multi-dimensional filters,
/// pagination, and workspace `library_problem_id` enrichment.
pub fn query_submissions(
    db: &SyncDb,
    workspace: Option<&Workspace>,
    query: &SubmissionsQuery,
) -> Result<SubmissionsPage> {
    let platform_filter = query
        .platform
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_ascii_lowercase);
    let verdict_filter = query
        .verdict
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let tag_filter = query
        .tag
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_ascii_lowercase);
    let search_filter = query
        .search
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_ascii_lowercase);

    let submissions = db.list_submissions(platform_filter.as_deref())?;
    let summaries = db.list_summaries()?;

    let mut solved_set = build_summary_solved_set(&summaries);
    let mut oj_known_keys: HashSet<(String, String)> = solved_set.clone();
    let mut oj_url_to_key: HashMap<String, (String, String)> = HashMap::new();
    for sub in &submissions {
        let pk = (
            sub.platform.trim().to_ascii_lowercase(),
            sub.problem_key.trim().to_ascii_lowercase(),
        );
        oj_known_keys.insert(pk.clone());
        if let Some(ref url) = sub.problem_url {
            let norm = normalize_url(url);
            if !norm.is_empty() {
                oj_url_to_key.entry(norm).or_insert_with(|| pk.clone());
            }
        }
        if is_ac_verdict(&sub.verdict) {
            solved_set.insert(pk);
        }
    }

    let (problems_by_id, source_map, manual_logs) = if let Some(ws) = workspace {
        let (p_map, s_map) = load_workspace_problem_maps(ws)?;
        let logs = ws.read_training_logs()?;
        (p_map, s_map, logs)
    } else {
        (HashMap::new(), HashMap::new(), Vec::new())
    };

    let mut rows: Vec<SubmissionViewRow> = if query.by_problem {
        let mut out = aggregate_submissions_by_problem(&submissions, &solved_set);
        if !manual_logs.is_empty() {
            let resolved_manual = resolve_manual_log_problems(
                &manual_logs,
                &problems_by_id,
                &source_map,
                &oj_known_keys,
                &oj_url_to_key,
            );
            let mut existing_idx: HashMap<(String, String), usize> = HashMap::new();
            for (i, row) in out.iter().enumerate() {
                existing_idx.insert(
                    (
                        row.platform.trim().to_ascii_lowercase(),
                        row.problem_key.trim().to_ascii_lowercase(),
                    ),
                    i,
                );
            }
            for rec in resolved_manual.into_values() {
                if rec.latest_status == TrainingLogStatus::Clear {
                    continue;
                }
                let manual_solved = matches!(
                    rec.latest_status,
                    TrainingLogStatus::Ac | TrainingLogStatus::Upsolved
                );
                if let Some(&idx) = existing_idx.get(&rec.canonical_key) {
                    let row = &mut out[idx];
                    if manual_solved && !row.solved {
                        row.solved = true;
                        row.verdict = "AC".to_string();
                    }
                    if row.library_problem_id.is_none() && rec.problem_id.is_some() {
                        row.library_problem_id = rec.problem_id;
                    }
                    if row.problem_title.is_none() && rec.title.is_some() {
                        row.problem_title = rec.title;
                    }
                    if row.problem_url.is_none() && rec.url.is_some() {
                        row.problem_url = rec.url;
                    }
                    if row.normalized_rating.is_none() && rec.rating.is_some() {
                        row.normalized_rating = rec.rating;
                    }
                    if row.tags.is_empty() && !rec.tags.is_empty() {
                        row.tags = rec.tags;
                    }
                } else {
                    if let Some(ref pf) = platform_filter
                        && pf != "local"
                        && !rec.display_platform.eq_ignore_ascii_case(pf)
                    {
                        continue;
                    }
                    let solved = manual_solved || solved_set.contains(&rec.canonical_key);
                    let verdict = if solved {
                        "AC".to_string()
                    } else {
                        manual_status_verdict(rec.latest_status).to_string()
                    };
                    let submission_id =
                        format!("local:{}:{}", rec.canonical_key.0, rec.canonical_key.1);
                    out.push(SubmissionViewRow {
                        platform: rec.display_platform,
                        submission_id,
                        handle: "local".to_string(),
                        contest_id: None,
                        problem_index: None,
                        problem_key: rec.display_key,
                        problem_title: rec.title,
                        problem_url: rec.url,
                        raw_rating: rec.rating,
                        normalized_rating: rec.rating,
                        tags: rec.tags,
                        verdict,
                        language: None,
                        submitted_at: rec.latest_at,
                        attempt_count: rec.attempt_count.max(1),
                        solved,
                        library_problem_id: rec.problem_id,
                    });
                }
            }
            out.sort_by(|a, b| {
                b.submitted_at
                    .cmp(&a.submitted_at)
                    .then_with(|| a.platform.cmp(&b.platform))
                    .then_with(|| a.problem_key.cmp(&b.problem_key))
            });
        }
        out
    } else {
        let mut out: Vec<SubmissionViewRow> = submissions
            .into_iter()
            .map(|sub| {
                let solved = solved_set.contains(&(
                    sub.platform.trim().to_ascii_lowercase(),
                    sub.problem_key.trim().to_ascii_lowercase(),
                ));
                SubmissionViewRow {
                    platform: sub.platform,
                    submission_id: sub.submission_id,
                    handle: sub.handle,
                    contest_id: sub.contest_id,
                    problem_index: sub.problem_index,
                    problem_key: sub.problem_key,
                    problem_title: sub.problem_title,
                    problem_url: sub.problem_url,
                    raw_rating: sub.raw_rating,
                    normalized_rating: sub.normalized_rating,
                    tags: sub.tags,
                    verdict: sub.verdict,
                    language: sub.language,
                    submitted_at: sub.submitted_at,
                    attempt_count: 1,
                    solved,
                    library_problem_id: None,
                }
            })
            .collect();

        if !manual_logs.is_empty() {
            let resolved_manual = resolve_manual_log_problems(
                &manual_logs,
                &problems_by_id,
                &source_map,
                &oj_known_keys,
                &oj_url_to_key,
            );
            for (idx, entry) in manual_logs.iter().enumerate() {
                if entry.status == TrainingLogStatus::Clear {
                    continue;
                }
                let Some(ctx) = resolve_manual_entry_context(
                    entry,
                    &problems_by_id,
                    &source_map,
                    &oj_known_keys,
                    &oj_url_to_key,
                ) else {
                    continue;
                };
                if let Some(ref pf) = platform_filter
                    && pf != "local"
                    && !ctx.display_platform.eq_ignore_ascii_case(pf)
                {
                    continue;
                }
                let effective_manual_solved =
                    resolved_manual.get(&ctx.canonical_key).is_some_and(|rec| {
                        matches!(
                            rec.latest_status,
                            TrainingLogStatus::Ac | TrainingLogStatus::Upsolved
                        )
                    });
                let solved = matches!(
                    entry.status,
                    TrainingLogStatus::Ac | TrainingLogStatus::Upsolved
                ) || effective_manual_solved
                    || solved_set.contains(&ctx.canonical_key);
                let verdict = manual_status_verdict(entry.status).to_string();
                let submitted_at = parse_rfc3339_to_unix_secs(&entry.at).unwrap_or(0);
                out.push(SubmissionViewRow {
                    platform: ctx.display_platform,
                    submission_id: format!("local:{idx}"),
                    handle: "local".to_string(),
                    contest_id: None,
                    problem_index: None,
                    problem_key: ctx.display_key,
                    problem_title: ctx.title,
                    problem_url: ctx.url,
                    raw_rating: ctx.rating,
                    normalized_rating: ctx.rating,
                    tags: ctx.tags,
                    verdict,
                    language: None,
                    submitted_at,
                    attempt_count: 1,
                    solved,
                    library_problem_id: ctx.problem_id,
                });
            }
            out.sort_by(|a, b| {
                b.submitted_at
                    .cmp(&a.submitted_at)
                    .then_with(|| a.platform.cmp(&b.platform))
                    .then_with(|| a.problem_key.cmp(&b.problem_key))
                    .then_with(|| b.submission_id.cmp(&a.submission_id))
            });
        }
        out
    };

    if workspace.is_some() && !rows.is_empty() {
        for row in &mut rows {
            if row.library_problem_id.is_none() {
                row.library_problem_id =
                    lookup_library_problem_id(&source_map, &row.platform, &row.problem_key);
            }
        }
    }

    let filtered: Vec<SubmissionViewRow> = rows
        .into_iter()
        .filter(|row| {
            if query.unsolved_only && row.solved {
                return false;
            }
            if let Some(vf) = verdict_filter {
                if is_ac_verdict(vf) {
                    if !is_ac_verdict(&row.verdict) {
                        return false;
                    }
                } else if !row.verdict.trim().eq_ignore_ascii_case(vf) {
                    return false;
                }
            }
            if let Some(min_r) = query.min_rating
                && row.normalized_rating.is_none_or(|r| r < min_r)
            {
                return false;
            }
            if let Some(max_r) = query.max_rating
                && row.normalized_rating.is_none_or(|r| r > max_r)
            {
                return false;
            }
            if let Some(ref tf) = tag_filter
                && !row.tags.iter().any(|t| t.trim().eq_ignore_ascii_case(tf))
            {
                return false;
            }
            if let Some(ref needle) = search_filter {
                let key_match = row.problem_key.to_ascii_lowercase().contains(needle);
                let title_match = row
                    .problem_title
                    .as_deref()
                    .is_some_and(|t| t.to_ascii_lowercase().contains(needle));
                let tag_match = row
                    .tags
                    .iter()
                    .any(|t| t.to_ascii_lowercase().contains(needle));
                let contest_match = row
                    .contest_id
                    .as_deref()
                    .is_some_and(|c| c.to_ascii_lowercase().contains(needle));
                if !(key_match || title_match || tag_match || contest_match) {
                    return false;
                }
            }
            true
        })
        .collect();

    let total = filtered.len();
    let offset = query.offset.unwrap_or(0);
    let limit = query.limit.unwrap_or(DEFAULT_SUBMISSIONS_LIMIT);

    let page_items: Vec<SubmissionViewRow> =
        filtered.into_iter().skip(offset).take(limit).collect();

    Ok(SubmissionsPage {
        total,
        offset,
        limit,
        items: page_items,
    })
}

/// Queries workspace problems that have a non-empty `editorial.md`, filtering by search
/// (problem_id, title, excerpt, tags, sources), tag, and rating bounds.
pub fn query_training_editorials(
    workspace: &Workspace,
    query: &EditorialQuery,
) -> Result<EditorialQueryResult> {
    let all = workspace.list_editorials()?;

    let mut tag_display_map: BTreeMap<String, String> = BTreeMap::new();
    for item in &all {
        for raw in &item.tags {
            let trimmed = raw.trim();
            if !trimmed.is_empty() {
                tag_display_map
                    .entry(trimmed.to_ascii_lowercase())
                    .or_insert_with(|| trimmed.to_string());
            }
        }
    }
    let available_tags: Vec<String> = tag_display_map.into_values().collect();

    let search_filter = query
        .search
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_ascii_lowercase);
    let tag_filter = query
        .tag
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_ascii_lowercase);

    let items: Vec<EditorialSummary> = all
        .into_iter()
        .filter(|item| {
            if let Some(min_r) = query.min_rating
                && item.rating.is_none_or(|r| r < min_r)
            {
                return false;
            }
            if let Some(max_r) = query.max_rating
                && item.rating.is_none_or(|r| r > max_r)
            {
                return false;
            }
            if let Some(ref tf) = tag_filter
                && !item.tags.iter().any(|t| t.trim().eq_ignore_ascii_case(tf))
            {
                return false;
            }
            if let Some(ref needle) = search_filter {
                let id_match = item.problem_id.to_ascii_lowercase().contains(needle);
                let title_match = item.title.to_ascii_lowercase().contains(needle);
                let excerpt_match = item.excerpt.to_ascii_lowercase().contains(needle);
                let tag_match = item
                    .tags
                    .iter()
                    .any(|t| t.to_ascii_lowercase().contains(needle));
                let source_match = item.sources.iter().any(|s| {
                    s.platform.to_ascii_lowercase().contains(needle)
                        || s.id.to_ascii_lowercase().contains(needle)
                });
                if !(id_match || title_match || excerpt_match || tag_match || source_match) {
                    return false;
                }
            }
            true
        })
        .collect();

    let total = items.len();
    Ok(EditorialQueryResult {
        items,
        total,
        available_tags,
    })
}

pub(crate) fn is_ac_verdict(verdict: &str) -> bool {
    let v = verdict.trim();
    v.eq_ignore_ascii_case("AC") || v.eq_ignore_ascii_case("OK")
}

fn current_unix_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn build_summary_solved_set(summaries: &[crate::db::CachedSummary]) -> HashSet<(String, String)> {
    let mut set = HashSet::new();
    for s in summaries {
        let p = s.platform.trim().to_ascii_lowercase();
        for k in &s.solved_keys {
            let norm = k.trim().to_ascii_lowercase();
            if !p.is_empty() && !norm.is_empty() {
                set.insert((p.clone(), norm));
            }
        }
    }
    set
}

/// Aggregates raw submissions (assumed sorted newest-first) into one row per `(platform, problem_key)`.
fn aggregate_submissions_by_problem(
    submissions: &[CachedSubmission],
    solved_set: &HashSet<(String, String)>,
) -> Vec<SubmissionViewRow> {
    let mut map: HashMap<(String, String), SubmissionViewRow> = HashMap::new();

    for sub in submissions {
        let key = (
            sub.platform.trim().to_ascii_lowercase(),
            sub.problem_key.trim().to_ascii_lowercase(),
        );
        let sub_ac = is_ac_verdict(&sub.verdict);
        let is_solved = sub_ac || solved_set.contains(&key);

        if let Some(existing) = map.get_mut(&key) {
            existing.attempt_count += 1;
            if is_solved && !existing.solved {
                existing.solved = true;
                existing.verdict = "AC".to_string();
            }
            if sub.submitted_at > existing.submitted_at {
                existing.submitted_at = sub.submitted_at;
                existing.submission_id = sub.submission_id.clone();
                if !existing.solved {
                    existing.verdict = sub.verdict.clone();
                }
                if sub.language.is_some() {
                    existing.language = sub.language.clone();
                }
            }
            if existing.problem_title.is_none() && sub.problem_title.is_some() {
                existing.problem_title = sub.problem_title.clone();
            }
            if existing.problem_url.is_none() && sub.problem_url.is_some() {
                existing.problem_url = sub.problem_url.clone();
            }
            if existing.raw_rating.is_none() && sub.raw_rating.is_some() {
                existing.raw_rating = sub.raw_rating;
            }
            if existing.normalized_rating.is_none() && sub.normalized_rating.is_some() {
                existing.normalized_rating = sub.normalized_rating;
            }
            if existing.tags.is_empty() && !sub.tags.is_empty() {
                existing.tags = sub.tags.clone();
            }
            if existing.contest_id.is_none() && sub.contest_id.is_some() {
                existing.contest_id = sub.contest_id.clone();
            }
            if existing.problem_index.is_none() && sub.problem_index.is_some() {
                existing.problem_index = sub.problem_index.clone();
            }
        } else {
            let verdict = if is_solved {
                "AC".to_string()
            } else {
                sub.verdict.clone()
            };
            map.insert(
                key,
                SubmissionViewRow {
                    platform: sub.platform.clone(),
                    submission_id: sub.submission_id.clone(),
                    handle: sub.handle.clone(),
                    contest_id: sub.contest_id.clone(),
                    problem_index: sub.problem_index.clone(),
                    problem_key: sub.problem_key.clone(),
                    problem_title: sub.problem_title.clone(),
                    problem_url: sub.problem_url.clone(),
                    raw_rating: sub.raw_rating,
                    normalized_rating: sub.normalized_rating,
                    tags: sub.tags.clone(),
                    verdict,
                    language: sub.language.clone(),
                    submitted_at: sub.submitted_at,
                    attempt_count: 1,
                    solved: is_solved,
                    library_problem_id: None,
                },
            );
        }
    }

    let mut out: Vec<SubmissionViewRow> = map.into_values().collect();
    out.sort_by(|a, b| {
        b.submitted_at
            .cmp(&a.submitted_at)
            .then_with(|| a.platform.cmp(&b.platform))
            .then_with(|| a.problem_key.cmp(&b.problem_key))
    });
    out
}

#[allow(clippy::type_complexity)]
fn load_workspace_problem_maps(
    workspace: &Workspace,
) -> Result<(
    HashMap<String, ProblemMeta>,
    HashMap<(String, String), String>,
)> {
    let mut problems_by_id = HashMap::new();
    let mut source_map = HashMap::new();
    for id in workspace.list_problems()? {
        let Ok(meta) = workspace.load_problem_meta(&id) else {
            continue;
        };
        for src in &meta.sources {
            let p = src.platform.trim().to_ascii_lowercase();
            let k = src.id.trim().to_ascii_lowercase();
            if !p.is_empty() && !k.is_empty() {
                source_map.entry((p, k)).or_insert_with(|| id.clone());
            }
        }
        problems_by_id.insert(id, meta);
    }
    Ok((problems_by_id, source_map))
}

fn manual_status_verdict(status: TrainingLogStatus) -> &'static str {
    match status {
        TrainingLogStatus::Ac | TrainingLogStatus::Upsolved => "AC",
        TrainingLogStatus::Attempted => "ATTEMPTED",
        TrainingLogStatus::Todo => "TODO",
        TrainingLogStatus::Clear => "CLEAR",
    }
}

fn normalize_url(url: &str) -> String {
    url.trim().trim_end_matches('/').to_ascii_lowercase()
}

struct ManualEntryContext {
    canonical_key: (String, String),
    problem_id: Option<String>,
    display_platform: String,
    display_key: String,
    title: Option<String>,
    url: Option<String>,
    rating: Option<i64>,
    tags: Vec<String>,
}

struct ResolvedManualProblem {
    canonical_key: (String, String),
    problem_id: Option<String>,
    display_platform: String,
    display_key: String,
    title: Option<String>,
    url: Option<String>,
    rating: Option<i64>,
    tags: Vec<String>,
    latest_status: TrainingLogStatus,
    latest_at: i64,
    effective_ac_at: Option<i64>,
    attempt_count: u64,
}

fn resolve_manual_entry_context(
    entry: &TrainingLogEntry,
    problems_by_id: &HashMap<String, ProblemMeta>,
    source_map: &HashMap<(String, String), String>,
    oj_known_keys: &HashSet<(String, String)>,
    oj_url_to_key: &HashMap<String, (String, String)>,
) -> Option<ManualEntryContext> {
    let entry_pid = entry
        .problem_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let entry_src = entry
        .source
        .as_ref()
        .filter(|s| !s.platform.trim().is_empty() && !s.id.trim().is_empty());

    let ws_prob: Option<(&String, &ProblemMeta)> = entry_pid
        .and_then(|pid| problems_by_id.get_key_value(pid))
        .or_else(|| {
            entry_src.and_then(|src| {
                let key = (
                    src.platform.trim().to_ascii_lowercase(),
                    src.id.trim().to_ascii_lowercase(),
                );
                source_map
                    .get(&key)
                    .and_then(|pid| problems_by_id.get_key_value(pid))
            })
        });

    let linked_pid = ws_prob
        .map(|(id, _)| id.clone())
        .or_else(|| entry_pid.map(str::to_owned));

    // Determine external (platform, id) if linked to an external OJ problem.
    let external_pair: Option<(String, String, Option<String>)> = if let Some(src) = entry_src {
        Some((
            src.platform.trim().to_ascii_lowercase(),
            src.id.trim().to_string(),
            src.url.clone(),
        ))
    } else if let Some((_, meta)) = ws_prob {
        let best_src = meta
            .sources
            .iter()
            .find(|s| {
                !s.platform.trim().is_empty()
                    && !s.id.trim().is_empty()
                    && oj_known_keys.contains(&(
                        s.platform.trim().to_ascii_lowercase(),
                        s.id.trim().to_ascii_lowercase(),
                    ))
            })
            .or_else(|| {
                meta.sources
                    .iter()
                    .find(|s| !s.platform.trim().is_empty() && !s.id.trim().is_empty())
            });
        best_src.map(|s| {
            (
                s.platform.trim().to_ascii_lowercase(),
                s.id.trim().to_string(),
                s.url.clone(),
            )
        })
    } else if let Some(url) = entry.url.as_deref() {
        let norm = normalize_url(url);
        oj_url_to_key
            .get(&norm)
            .map(|(p, k)| (p.clone(), k.clone(), Some(url.trim().to_string())))
    } else {
        None
    };

    let title = entry
        .title
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .or_else(|| ws_prob.map(|(_, m)| m.title.clone()));

    let url = entry
        .url
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .or_else(|| external_pair.as_ref().and_then(|(_, _, u)| u.clone()));

    let rating = entry
        .rating
        .map(i64::from)
        .or_else(|| ws_prob.and_then(|(_, m)| m.rating).map(i64::from));

    let tags = if !entry.tags.is_empty() {
        entry.tags.clone()
    } else if let Some((_, meta)) = ws_prob {
        meta.tags.clone()
    } else {
        Vec::new()
    };

    let canonical_key = if let Some((ref p, ref k, _)) = external_pair {
        (p.clone(), k.to_ascii_lowercase())
    } else if let Some(ref pid) = linked_pid {
        (
            "local".to_string(),
            format!("id:{}", pid.to_ascii_lowercase()),
        )
    } else if let Some(ref u) = url {
        ("local".to_string(), format!("url:{}", normalize_url(u)))
    } else if let Some(ref t) = title {
        (
            "local".to_string(),
            format!("title:{}", t.trim().to_ascii_lowercase()),
        )
    } else {
        return None;
    };

    let display_platform = entry
        .platform
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_ascii_lowercase)
        .or_else(|| external_pair.as_ref().map(|(p, _, _)| p.clone()))
        .unwrap_or_else(|| "local".to_string());

    let display_key = external_pair
        .as_ref()
        .map(|(_, k, _)| k.clone())
        .or_else(|| linked_pid.clone())
        .or_else(|| title.clone())
        .or_else(|| url.clone())
        .unwrap_or_else(|| canonical_key.1.clone());

    Some(ManualEntryContext {
        canonical_key,
        problem_id: linked_pid,
        display_platform,
        display_key,
        title,
        url,
        rating,
        tags,
    })
}

fn resolve_manual_log_problems(
    logs: &[TrainingLogEntry],
    problems_by_id: &HashMap<String, ProblemMeta>,
    source_map: &HashMap<(String, String), String>,
    oj_known_keys: &HashSet<(String, String)>,
    oj_url_to_key: &HashMap<String, (String, String)>,
) -> HashMap<(String, String), ResolvedManualProblem> {
    let mut map: HashMap<(String, String), ResolvedManualProblem> = HashMap::new();

    for entry in logs {
        let Some(ctx) = resolve_manual_entry_context(
            entry,
            problems_by_id,
            source_map,
            oj_known_keys,
            oj_url_to_key,
        ) else {
            continue;
        };
        let ts = parse_rfc3339_to_unix_secs(&entry.at).unwrap_or(0);
        let is_ac = matches!(
            entry.status,
            TrainingLogStatus::Ac | TrainingLogStatus::Upsolved
        );

        let rec = map
            .entry(ctx.canonical_key.clone())
            .or_insert_with(|| ResolvedManualProblem {
                canonical_key: ctx.canonical_key,
                problem_id: ctx.problem_id.clone(),
                display_platform: ctx.display_platform.clone(),
                display_key: ctx.display_key.clone(),
                title: ctx.title.clone(),
                url: ctx.url.clone(),
                rating: ctx.rating,
                tags: ctx.tags.clone(),
                latest_status: entry.status,
                latest_at: ts,
                effective_ac_at: None,
                attempt_count: 0,
            });

        if rec.problem_id.is_none() && ctx.problem_id.is_some() {
            rec.problem_id = ctx.problem_id;
        }
        if ctx.title.is_some() {
            rec.title = ctx.title;
        }
        if ctx.url.is_some() {
            rec.url = ctx.url;
        }
        if ctx.rating.is_some() {
            rec.rating = ctx.rating;
        }
        if !ctx.tags.is_empty() {
            rec.tags = ctx.tags;
        }
        if rec.display_platform == "local" && ctx.display_platform != "local" {
            rec.display_platform = ctx.display_platform;
        }

        rec.latest_status = entry.status;
        rec.latest_at = ts;

        if entry.status == TrainingLogStatus::Clear {
            rec.effective_ac_at = None;
        } else {
            rec.attempt_count += 1;
            if is_ac {
                rec.effective_ac_at = Some(ts);
            } else {
                rec.effective_ac_at = None;
            }
        }
    }

    map
}

fn lookup_library_problem_id(
    source_map: &HashMap<(String, String), String>,
    platform: &str,
    problem_key: &str,
) -> Option<String> {
    let p = platform.trim().to_ascii_lowercase();
    let k = problem_key.trim().to_ascii_lowercase();
    source_map.get(&(p, k)).cloned()
}

#[derive(Default)]
struct WorkspaceTrainingInfo {
    source_map: HashMap<(String, String), String>,
    problems_by_id: HashMap<String, ProblemMeta>,
    logs: Vec<TrainingLogEntry>,
    backlog: Vec<WorkspaceBacklogItem>,
    todo_count: u64,
    attempted_count: u64,
}

fn collect_workspace_training_info(
    db: &SyncDb,
    workspace: &Workspace,
) -> Result<WorkspaceTrainingInfo> {
    let ids = workspace.list_problems()?;
    let mut problems: Vec<(String, ProblemMeta)> = Vec::with_capacity(ids.len());
    let mut source_map: HashMap<(String, String), String> = HashMap::new();

    for id in ids {
        let Ok(meta) = workspace.load_problem_meta(&id) else {
            continue;
        };
        for src in &meta.sources {
            let p = src.platform.trim().to_ascii_lowercase();
            let k = src.id.trim().to_ascii_lowercase();
            if !p.is_empty() && !k.is_empty() {
                source_map.entry((p, k)).or_insert_with(|| id.clone());
            }
        }
        problems.push((id, meta));
    }

    let lookup_items: Vec<(&str, &[ProblemSource])> = problems
        .iter()
        .map(|(id, meta)| (id.as_str(), meta.sources.as_slice()))
        .collect();
    let synced_map = db.get_problems_sync_statuses(&lookup_items)?;
    let logs = workspace.read_training_logs()?;
    let derived = Workspace::derive_problem_training_statuses(
        &logs,
        problems.iter().map(|(id, meta)| {
            (
                id.as_str(),
                meta.sources.as_slice(),
                synced_map.get(id).copied().unwrap_or_default(),
            )
        }),
    );

    let mut backlog = Vec::new();
    let mut todo_count = 0u64;
    let mut attempted_count = 0u64;

    let mut problems_by_id: HashMap<String, ProblemMeta> = HashMap::with_capacity(problems.len());
    for (id, meta) in problems {
        let status = derived.get(&id).copied().unwrap_or_default();
        match status {
            TrainingStatus::Todo => {
                todo_count += 1;
                backlog.push(WorkspaceBacklogItem {
                    problem_id: id.clone(),
                    title: meta.title.clone(),
                    status,
                    rating: meta.rating,
                    tags: meta.tags.clone(),
                    sources: meta.sources.clone(),
                });
            }
            TrainingStatus::Attempted => {
                attempted_count += 1;
                backlog.push(WorkspaceBacklogItem {
                    problem_id: id.clone(),
                    title: meta.title.clone(),
                    status,
                    rating: meta.rating,
                    tags: meta.tags.clone(),
                    sources: meta.sources.clone(),
                });
            }
            TrainingStatus::None | TrainingStatus::Ac | TrainingStatus::Upsolved => {}
        }
        problems_by_id.insert(id, meta);
    }

    backlog.sort_by(|a, b| {
        let rank = |s: TrainingStatus| match s {
            TrainingStatus::Attempted => 0u8,
            TrainingStatus::Todo => 1u8,
            _ => 2u8,
        };
        rank(a.status)
            .cmp(&rank(b.status))
            .then_with(|| a.problem_id.cmp(&b.problem_id))
    });

    Ok(WorkspaceTrainingInfo {
        source_map,
        problems_by_id,
        logs,
        backlog,
        todo_count,
        attempted_count,
    })
}

fn compute_streaks(ac_days: &BTreeSet<i64>, today_day: i64) -> (u64, u64) {
    if ac_days.is_empty() {
        return (0, 0);
    }

    // Current streak: consecutive days ending at today_day or today_day - 1.
    let mut current = 0u64;
    let start_day = if ac_days.contains(&today_day) {
        Some(today_day)
    } else if ac_days.contains(&(today_day - 1)) {
        Some(today_day - 1)
    } else {
        None
    };
    if let Some(mut cursor) = start_day {
        while ac_days.contains(&cursor) {
            current += 1;
            cursor -= 1;
        }
    }

    // Longest streak across all history.
    let mut longest = 0u64;
    let mut run = 0u64;
    let mut prev: Option<i64> = None;
    for &day in ac_days {
        if prev == Some(day - 1) {
            run += 1;
        } else {
            run = 1;
        }
        if run > longest {
            longest = run;
        }
        prev = Some(day);
    }

    (current, longest)
}

fn percentile_i64(sorted: &[i64], p: f64) -> Option<i64> {
    if sorted.is_empty() {
        return None;
    }
    if sorted.len() == 1 {
        return Some(sorted[0]);
    }
    let pos = p.clamp(0.0, 1.0) * (sorted.len() - 1) as f64;
    let lo = pos.floor() as usize;
    let hi = pos.ceil() as usize;
    if lo == hi {
        return Some(sorted[lo]);
    }
    let frac = pos - lo as f64;
    let val = sorted[lo] as f64 + frac * (sorted[hi] - sorted[lo]) as f64;
    Some(val.round() as i64)
}

fn default_difficulty_buckets() -> Vec<DifficultyBucket> {
    vec![
        DifficultyBucket {
            label: "<1200".into(),
            min_rating: None,
            max_rating: Some(1199),
            ac_count: 0,
        },
        DifficultyBucket {
            label: "1200-1399".into(),
            min_rating: Some(1200),
            max_rating: Some(1399),
            ac_count: 0,
        },
        DifficultyBucket {
            label: "1400-1599".into(),
            min_rating: Some(1400),
            max_rating: Some(1599),
            ac_count: 0,
        },
        DifficultyBucket {
            label: "1600-1799".into(),
            min_rating: Some(1600),
            max_rating: Some(1799),
            ac_count: 0,
        },
        DifficultyBucket {
            label: "1800-1999".into(),
            min_rating: Some(1800),
            max_rating: Some(1999),
            ac_count: 0,
        },
        DifficultyBucket {
            label: "2000-2199".into(),
            min_rating: Some(2000),
            max_rating: Some(2199),
            ac_count: 0,
        },
        DifficultyBucket {
            label: "2200-2399".into(),
            min_rating: Some(2200),
            max_rating: Some(2399),
            ac_count: 0,
        },
        DifficultyBucket {
            label: "2400-2599".into(),
            min_rating: Some(2400),
            max_rating: Some(2599),
            ac_count: 0,
        },
        DifficultyBucket {
            label: "2600+".into(),
            min_rating: Some(2600),
            max_rating: None,
            ac_count: 0,
        },
    ]
}

fn difficulty_bucket_index(rating: i64) -> usize {
    if rating < 1200 {
        0
    } else if rating < 1400 {
        1
    } else if rating < 1600 {
        2
    } else if rating < 1800 {
        3
    } else if rating < 2000 {
        4
    } else if rating < 2200 {
        5
    } else if rating < 2400 {
        6
    } else if rating < 2600 {
        7
    } else {
        8
    }
}

/// Converts days since 1970-01-01 to proleptic Gregorian `(year, month, day)`.
pub(crate) fn days_to_ymd(days: i64) -> (i32, u32, u32) {
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
    (year as i32, m as u32, d as u32)
}

/// Converts proleptic Gregorian `(year, month, day)` to days since 1970-01-01.
pub(crate) fn ymd_to_days(year: i32, month: u32, day: u32) -> i64 {
    let y = i64::from(year) - i64::from(month <= 2);
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u64;
    let m = u64::from(month);
    let d = u64::from(day);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe as i64 - 719_468
}

pub(crate) fn unix_secs_to_ymd(secs: i64) -> (i32, u32, u32) {
    days_to_ymd(secs.div_euclid(SECS_PER_DAY))
}

pub(crate) fn ymd_to_unix_secs(year: i32, month: u32, day: u32) -> i64 {
    ymd_to_days(year, month, day) * SECS_PER_DAY
}

/// Computes ISO-8601 `(iso_year, iso_week)` for a Unix day number.
pub(crate) fn iso_week_from_days(days: i64) -> (i32, u32) {
    let iso_weekday = (days + 3).rem_euclid(7) + 1; // 1 = Mon .. 7 = Sun
    let thu_days = days + (4 - iso_weekday);
    let (iso_year, _, _) = days_to_ymd(thu_days);
    let jan1_days = ymd_to_days(iso_year, 1, 1);
    let iso_week = ((thu_days - jan1_days) / 7 + 1) as u32;
    (iso_year, iso_week)
}

/// Parses an RFC 3339 timestamp (e.g. `2026-11-24T12:34:56Z`, `2026-11-24T20:34:56+08:00`,
/// with optional fractional seconds) or `YYYY-MM-DD` date into Unix seconds (UTC).
pub(crate) fn parse_rfc3339_to_unix_secs(raw: &str) -> Option<i64> {
    let s = raw.trim();
    if s.len() < 10 {
        return None;
    }
    let bytes = s.as_bytes();
    if bytes[4] != b'-' || bytes[7] != b'-' {
        return None;
    }
    let year: i32 = s[0..4].parse().ok()?;
    let month: u32 = s[5..7].parse().ok()?;
    let day: u32 = s[8..10].parse().ok()?;
    if !(1..=12).contains(&month) || day < 1 || day > days_in_month(year, month) {
        return None;
    }

    let base_day_secs = ymd_to_unix_secs(year, month, day);
    if s.len() == 10 {
        return Some(base_day_secs);
    }

    if !matches!(bytes[10], b'T' | b't' | b' ') || s.len() < 19 {
        return None;
    }
    if bytes[13] != b':' || bytes[16] != b':' {
        return None;
    }
    let hour: i64 = s[11..13].parse().ok()?;
    let minute: i64 = s[14..16].parse().ok()?;
    let second: i64 = s[17..19].parse().ok()?;
    if !(0..=23).contains(&hour) || !(0..=59).contains(&minute) || !(0..=60).contains(&second) {
        return None;
    }

    let mut rest = &s[19..];
    if let Some(after_dot) = rest.strip_prefix('.') {
        let digits = after_dot.bytes().take_while(|b| b.is_ascii_digit()).count();
        if digits == 0 {
            return None;
        }
        rest = &after_dot[digits..];
    }

    let offset_secs: i64 = if rest.eq_ignore_ascii_case("Z") {
        0
    } else if rest.len() == 6 && (rest.starts_with('+') || rest.starts_with('-')) {
        let rb = rest.as_bytes();
        if rb[3] != b':' {
            return None;
        }
        let sign = if rb[0] == b'-' { -1 } else { 1 };
        let oh: i64 = rest[1..3].parse().ok()?;
        let om: i64 = rest[4..6].parse().ok()?;
        if !(0..=23).contains(&oh) || !(0..=59).contains(&om) {
            return None;
        }
        sign * (oh * 3600 + om * 60)
    } else {
        return None;
    };

    Some(base_day_secs + hour * 3600 + minute * 60 + second - offset_secs)
}

fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            let leap = (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0);
            if leap { 29 } else { 28 }
        }
        _ => 0,
    }
}

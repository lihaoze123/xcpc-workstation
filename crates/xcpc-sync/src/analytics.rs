use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt;
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use xcpc_domain::{ProblemSource, TrainingStatus};
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

    // Pre-scan workspace once if provided (for library_problem_id mapping + workspace_backlog).
    let ws_info = if let Some(ws) = workspace {
        collect_workspace_training_info(db, ws)?
    } else {
        WorkspaceTrainingInfo::default()
    };
    let source_to_problem = ws_info.source_map;
    let workspace_backlog = ws_info.backlog;
    let library_todo_count = ws_info.todo_count;
    let library_attempted_count = ws_info.attempted_count;

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
    for sub in &submissions {
        if is_ac_verdict(&sub.verdict) {
            solved_set.insert((
                sub.platform.trim().to_ascii_lowercase(),
                sub.problem_key.trim().to_ascii_lowercase(),
            ));
        }
    }

    let rows: Vec<SubmissionViewRow> = if query.by_problem {
        aggregate_submissions_by_problem(&submissions, &solved_set)
    } else {
        submissions
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
            .collect()
    };

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

    let mut page_items: Vec<SubmissionViewRow> =
        filtered.into_iter().skip(offset).take(limit).collect();

    if let Some(ws) = workspace
        && !page_items.is_empty()
    {
        let source_map = build_workspace_source_map(ws)?;
        for row in &mut page_items {
            row.library_problem_id =
                lookup_library_problem_id(&source_map, &row.platform, &row.problem_key);
        }
    }

    Ok(SubmissionsPage {
        total,
        offset,
        limit,
        items: page_items,
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

fn build_workspace_source_map(workspace: &Workspace) -> Result<HashMap<(String, String), String>> {
    let mut map = HashMap::new();
    for id in workspace.list_problems()? {
        let Ok(problem) = workspace.load_problem(&id) else {
            continue;
        };
        for src in &problem.meta.sources {
            let p = src.platform.trim().to_ascii_lowercase();
            let k = src.id.trim().to_ascii_lowercase();
            if !p.is_empty() && !k.is_empty() {
                map.entry((p, k)).or_insert_with(|| id.clone());
            }
        }
    }
    Ok(map)
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
    backlog: Vec<WorkspaceBacklogItem>,
    todo_count: u64,
    attempted_count: u64,
}

fn collect_workspace_training_info(
    db: &SyncDb,
    workspace: &Workspace,
) -> Result<WorkspaceTrainingInfo> {
    let ids = workspace.list_problems()?;
    let mut problems = Vec::with_capacity(ids.len());
    let mut source_map: HashMap<(String, String), String> = HashMap::new();

    for id in ids {
        let Ok(problem) = workspace.load_problem(&id) else {
            continue;
        };
        for src in &problem.meta.sources {
            let p = src.platform.trim().to_ascii_lowercase();
            let k = src.id.trim().to_ascii_lowercase();
            if !p.is_empty() && !k.is_empty() {
                source_map
                    .entry((p, k))
                    .or_insert_with(|| problem.id.clone());
            }
        }
        problems.push(problem);
    }

    let lookup_items: Vec<(&str, &[ProblemSource])> = problems
        .iter()
        .map(|p| (p.id.as_str(), p.meta.sources.as_slice()))
        .collect();
    let synced_map = db.get_problems_sync_statuses(&lookup_items)?;
    let logs = workspace.read_training_logs()?;
    let derived = Workspace::derive_problem_training_statuses(
        &logs,
        problems.iter().map(|p| {
            (
                p.id.as_str(),
                p.meta.sources.as_slice(),
                synced_map.get(&p.id).copied().unwrap_or_default(),
            )
        }),
    );

    let mut backlog = Vec::new();
    let mut todo_count = 0u64;
    let mut attempted_count = 0u64;

    for problem in problems {
        let status = derived.get(&problem.id).copied().unwrap_or_default();
        match status {
            TrainingStatus::Todo => {
                todo_count += 1;
                backlog.push(WorkspaceBacklogItem {
                    problem_id: problem.id,
                    title: problem.meta.title,
                    status,
                    rating: problem.meta.rating,
                    tags: problem.meta.tags,
                    sources: problem.meta.sources,
                });
            }
            TrainingStatus::Attempted => {
                attempted_count += 1;
                backlog.push(WorkspaceBacklogItem {
                    problem_id: problem.id,
                    title: problem.meta.title,
                    status,
                    rating: problem.meta.rating,
                    tags: problem.meta.tags,
                    sources: problem.meta.sources,
                });
            }
            TrainingStatus::None | TrainingStatus::Ac | TrainingStatus::Upsolved => {}
        }
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

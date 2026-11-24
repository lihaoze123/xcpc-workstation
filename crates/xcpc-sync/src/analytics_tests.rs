use std::time::Instant;

use tempfile::TempDir;
use xcpc_domain::{ProblemSource, TrainingLogEntry, TrainingLogStatus, TrainingStatus};
use xcpc_plugin::{
    OjAccountSync, OjContest, OjRatingChange, OjSubmission, OjSummarySync, PluginManager,
};
use xcpc_storage::{CreateProblemOptions, Workspace, init_workspace};

use crate::analytics::ymd_to_unix_secs;
use crate::{
    ContestsQuery, IngestProblemRequest, SubmissionsQuery, SyncConfig, SyncDb, SyncError,
    TimeWindow, TrainingDashboardQuery, compute_training_dashboard, ingest_problem, query_contests,
    query_submissions,
};

#[allow(clippy::too_many_arguments)]
fn make_sub(
    platform: &str,
    id: &str,
    key: &str,
    title: &str,
    rating: Option<i64>,
    tags: &[&str],
    verdict: &str,
    submitted_at: i64,
) -> OjSubmission {
    OjSubmission {
        submission_id: id.to_string(),
        handle: "alice".to_string(),
        contest_id: Some("1000".to_string()),
        problem_index: Some("A".to_string()),
        problem_key: key.to_string(),
        problem_title: title.to_string(),
        problem_url: Some(format!("https://{platform}.example/p/{key}")),
        raw_rating: rating.map(|r| r as f64),
        normalized_rating: rating,
        tags: tags.iter().map(|s| (*s).to_string()).collect(),
        verdict: verdict.to_string(),
        language: Some("C++20".to_string()),
        submitted_at,
    }
}

#[test]
fn time_window_serde_and_from_str() {
    for (raw, expected) in [
        ("30d", TimeWindow::Days30),
        ("90d", TimeWindow::Days90),
        ("year", TimeWindow::Year),
        ("all", TimeWindow::All),
    ] {
        let parsed: TimeWindow = raw.parse().unwrap();
        assert_eq!(parsed, expected);
        assert_eq!(parsed.to_string(), raw);
        let json = serde_json::to_string(&parsed).unwrap();
        assert_eq!(json, format!("\"{raw}\""));
        let back: TimeWindow = serde_json::from_str(&json).unwrap();
        assert_eq!(back, expected);
    }
    assert!("7d".parse::<TimeWindow>().is_err());
}

#[test]
fn dashboard_analytics_kpis_windows_heatmap_streaks_trend_buckets_tags_and_backlog() {
    let ws_dir = TempDir::new().unwrap();
    init_workspace(ws_dir.path()).unwrap();
    let ws = Workspace::open(ws_dir.path(), false).unwrap();

    // Create 3 workspace problems:
    // - p-ac: linked to CF 1999A (AC in DB -> derived Ac)
    // - p-attempted: linked to CF 1999C (WA only in DB -> derived Attempted)
    // - p-todo: no submissions, manually logged Todo
    ws.create_problem_with_options(
        "p-ac",
        "Solved Problem",
        &CreateProblemOptions {
            sources: vec![ProblemSource {
                platform: "codeforces".into(),
                id: "1999A".into(),
                url: None,
            }],
            tags: vec!["math".into()],
            difficulty: None,
            rating: Some(1200),
        },
    )
    .unwrap();
    ws.create_problem_with_options(
        "p-attempted",
        "Unsolved Attempted Problem",
        &CreateProblemOptions {
            sources: vec![ProblemSource {
                platform: "codeforces".into(),
                id: "1999C".into(),
                url: None,
            }],
            tags: vec!["dp".into()],
            difficulty: None,
            rating: Some(1900),
        },
    )
    .unwrap();
    ws.create_problem_with_options(
        "p-todo",
        "Todo Problem",
        &CreateProblemOptions {
            sources: vec![],
            tags: vec!["graphs".into()],
            difficulty: None,
            rating: Some(2100),
        },
    )
    .unwrap();
    ws.append_training_log(&TrainingLogEntry {
        at: "2026-11-24T00:00:00Z".into(),
        problem_id: Some("p-todo".into()),
        source: None,
        status: TrainingLogStatus::Todo,
    })
    .unwrap();

    let mut db = SyncDb::open_in_memory().unwrap();
    let now = ymd_to_unix_secs(2026, 11, 24) + 12 * 3600; // 2026-11-24T12:00:00Z

    // Accounts + rating history + summary-only platform
    db.upsert_account(
        "codeforces",
        &OjAccountSync {
            handle: "alice".into(),
            rating: Some(2050),
            max_rating: Some(2120),
            rank: Some("candidate master".into()),
            rating_history: vec![
                OjRatingChange {
                    contest_id: "1900".into(),
                    contest_name: "CF Round 900".into(),
                    rating_before: 1950,
                    rating_after: 2000,
                    rank: Some(150),
                    at: ymd_to_unix_secs(2026, 10, 1),
                },
                OjRatingChange {
                    contest_id: "1999".into(),
                    contest_name: "CF Round 964".into(),
                    rating_before: 2000,
                    rating_after: 2050,
                    rank: Some(90),
                    at: ymd_to_unix_secs(2026, 11, 10),
                },
            ],
        },
        "2026-11-24T10:00:00Z",
    )
    .unwrap();

    db.upsert_summary(
        "luogu",
        &OjSummarySync {
            handle: "alice_lg".into(),
            solved_count: 10,
            submission_count: Some(25),
            solved_problem_keys: vec!["P1001".into()],
            rating: Some(1800),
            max_rating: None,
            rank: None,
        },
        "2026-11-24T10:00:00Z",
    )
    .unwrap();

    // Submissions:
    // Current streak: Nov 22, Nov 23, Nov 24 (3 days)
    // Earlier longest streak: Jan 1, Jan 2, Jan 3, Jan 4, Jan 5 in 2025 (5 days)
    let cf_subs = vec![
        // Today (Nov 24): 1999A AC twice (deduplicated -> 1 AC, 2 subs), 1999B AC (rating 2000)
        make_sub(
            "codeforces",
            "s1",
            "1999A",
            "A+B",
            Some(1100),
            &["math", "dp"],
            "AC",
            ymd_to_unix_secs(2026, 11, 24) + 3600,
        ),
        make_sub(
            "codeforces",
            "s2",
            "1999A",
            "A+B",
            Some(1100),
            &["math", "dp"],
            "OK",
            ymd_to_unix_secs(2026, 11, 24) + 7200,
        ),
        make_sub(
            "codeforces",
            "s3",
            "1999B",
            "Hard DP",
            Some(2000),
            &["dp", "trees"],
            "AC",
            ymd_to_unix_secs(2026, 11, 24) + 9000,
        ),
        // Yesterday (Nov 23): 1999C WA (unsolved), 1999D AC (rating 1600)
        make_sub(
            "codeforces",
            "s4",
            "1999C",
            "Unsolved",
            Some(1900),
            &["dp"],
            "WA",
            ymd_to_unix_secs(2026, 11, 23) + 3600,
        ),
        make_sub(
            "codeforces",
            "s5",
            "1999D",
            "Greedy",
            Some(1600),
            &["greedy"],
            "AC",
            ymd_to_unix_secs(2026, 11, 23) + 7200,
        ),
        // 2 days ago (Nov 22): 1999E AC (rating 2400)
        make_sub(
            "codeforces",
            "s6",
            "1999E",
            "Graphs",
            Some(2400),
            &["graphs", "dp"],
            "AC",
            ymd_to_unix_secs(2026, 11, 22) + 3600,
        ),
        // 20 days ago (Nov 4): 1999F AC (rating 2700)
        make_sub(
            "codeforces",
            "s7",
            "1999F",
            "Super Hard",
            Some(2700),
            &["dp"],
            "AC",
            ymd_to_unix_secs(2026, 11, 4) + 3600,
        ),
        // 60 days ago (Sep 25): 1999G AC (rating 1400) -> inside 90d & year, outside 30d
        make_sub(
            "codeforces",
            "s8",
            "1999G",
            "Mid",
            Some(1400),
            &["math"],
            "AC",
            ymd_to_unix_secs(2026, 9, 25) + 3600,
        ),
        // Previous year 2025 (5-day streak Jan 1..=5)
        make_sub(
            "codeforces",
            "s9",
            "1800A",
            "Old1",
            Some(1200),
            &["math"],
            "AC",
            ymd_to_unix_secs(2025, 1, 1) + 100,
        ),
        make_sub(
            "codeforces",
            "s10",
            "1800B",
            "Old2",
            Some(1300),
            &["math"],
            "AC",
            ymd_to_unix_secs(2025, 1, 2) + 100,
        ),
        make_sub(
            "codeforces",
            "s11",
            "1800C",
            "Old3",
            Some(1500),
            &["math"],
            "AC",
            ymd_to_unix_secs(2025, 1, 3) + 100,
        ),
        make_sub(
            "codeforces",
            "s12",
            "1800D",
            "Old4",
            Some(1800),
            &["math"],
            "AC",
            ymd_to_unix_secs(2025, 1, 4) + 100,
        ),
        make_sub(
            "codeforces",
            "s13",
            "1800E",
            "Old5",
            Some(2200),
            &["math"],
            "AC",
            ymd_to_unix_secs(2025, 1, 5) + 100,
        ),
    ];
    db.upsert_submissions("codeforces", &cf_subs, "2026-11-24T10:00:00Z")
        .unwrap();

    let mut cfg = SyncConfig::default();
    cfg.set_handle("codeforces", "alice");
    cfg.last_synced_at = Some("2026-11-24T10:00:00Z".into());

    // Query with 30d window and tag_min_rating = 1900
    let dash30 = compute_training_dashboard(
        &db,
        &cfg,
        Some(&ws),
        &TrainingDashboardQuery {
            window: TimeWindow::Days30,
            heatmap_year: None,
            tag_min_rating: Some(1900),
            now_unix: Some(now),
        },
    )
    .unwrap();

    assert_eq!(dash30.window, TimeWindow::Days30);
    assert_eq!(
        dash30.last_synced_at.as_deref(),
        Some("2026-11-24T10:00:00Z")
    );
    assert_eq!(dash30.accounts.len(), 2);
    let cf_acc = dash30
        .accounts
        .iter()
        .find(|a| a.platform == "codeforces")
        .unwrap();
    assert_eq!(cf_acc.rating, Some(2050));
    assert_eq!(cf_acc.solved_count, 11); // 11 distinct CF AC problems across all time
    assert_eq!(cf_acc.submission_count, 13);
    let lg_acc = dash30
        .accounts
        .iter()
        .find(|a| a.platform == "luogu")
        .unwrap();
    assert_eq!(lg_acc.solved_count, 10);
    assert_eq!(lg_acc.submission_count, 25);

    // KPI checks
    assert_eq!(dash30.kpi.today_ac, 2); // 1999A, 1999B
    assert_eq!(dash30.kpi.today_submissions, 3);
    assert_eq!(dash30.kpi.week_ac, 4); // 1999A, 1999B, 1999D, 1999E
    assert_eq!(dash30.kpi.week_submissions, 6);
    assert_eq!(dash30.kpi.month_ac, 5); // + 1999F
    assert_eq!(dash30.kpi.month_submissions, 7);
    assert_eq!(dash30.kpi.window_ac, 5);
    assert_eq!(dash30.kpi.window_submissions, 7);
    assert_eq!(dash30.kpi.total_ac, 21); // 11 CF + 10 Luogu
    assert_eq!(dash30.kpi.total_submissions, 38); // 13 CF + 25 Luogu
    assert_eq!(dash30.kpi.streak_days, 3);
    assert_eq!(dash30.kpi.longest_streak_days, 5);
    assert_eq!(dash30.kpi.unsolved_attempted_count, 1); // 1999C
    assert_eq!(dash30.kpi.library_todo_count, 1); // p-todo
    assert_eq!(dash30.kpi.library_attempted_count, 1); // p-attempted

    // Available years descending
    assert_eq!(dash30.available_years, vec![2026, 2025]);
    assert_eq!(dash30.heatmap_year, 2026);
    let nov24 = dash30
        .heatmap
        .iter()
        .find(|c| c.date == "2026-11-24")
        .expect("Nov 24 cell");
    assert_eq!(nov24.ac_count, 2);
    assert_eq!(nov24.submission_count, 3);
    assert_eq!(nov24.max_rating, Some(2000));

    // Rating series
    assert_eq!(dash30.rating_series.len(), 1);
    assert_eq!(dash30.rating_series[0].points.len(), 2);
    assert_eq!(dash30.rating_series[0].points[0].delta, Some(50));
    assert_eq!(dash30.rating_series[0].points[1].delta, Some(50));

    // Difficulty trend in 30d window uses ISO weeks
    assert!(
        dash30
            .difficulty_trend
            .iter()
            .all(|p| p.period.contains("-W"))
    );
    // Week containing Nov 22..24: Nov 22 (Sun 2026-W47: 2400), Nov 23-24 (Mon-Tue 2026-W48: 1100, 1600, 2000)
    let w48 = dash30
        .difficulty_trend
        .iter()
        .find(|p| p.period == "2026-W48")
        .expect("2026-W48 present");
    assert_eq!(w48.ac_count, 3);
    assert_eq!(w48.rated_ac_count, 3);
    assert_eq!(w48.median, Some(1600));
    assert_eq!(w48.p75, Some(1800));
    assert_eq!(w48.p90, Some(1920));

    // Difficulty buckets (30d window: 1100, 1600, 2000, 2400, 2700)
    assert_eq!(dash30.difficulty_buckets.len(), 9);
    assert_eq!(dash30.difficulty_buckets[0].ac_count, 1); // <1200: 1100
    assert_eq!(dash30.difficulty_buckets[3].ac_count, 1); // 1600-1799: 1600
    assert_eq!(dash30.difficulty_buckets[5].ac_count, 1); // 2000-2199: 2000
    assert_eq!(dash30.difficulty_buckets[7].ac_count, 1); // 2400-2599: 2400
    assert_eq!(dash30.difficulty_buckets[8].ac_count, 1); // 2600+: 2700

    // Tag distribution with tag_min_rating = 1900 in 30d window:
    // Qualifying AC problems: 1999B (2000: dp, trees), 1999E (2400: graphs, dp), 1999F (2700: dp)
    assert_eq!(dash30.tag_distribution[0].tag, "dp");
    assert_eq!(dash30.tag_distribution[0].ac_count, 3);
    assert_eq!(dash30.tag_distribution[0].avg_rating, Some(2367)); // (2000+2400+2700)/3 = 2366.67
    assert!(
        dash30
            .tag_distribution
            .iter()
            .all(|t| t.tag != "math" && t.tag != "greedy")
    );

    // Unsolved problems + library_problem_id
    assert_eq!(dash30.unsolved_problems.len(), 1);
    assert_eq!(dash30.unsolved_problems[0].problem_key, "1999C");
    assert_eq!(
        dash30.unsolved_problems[0].library_problem_id.as_deref(),
        Some("p-attempted")
    );

    // Workspace backlog
    assert_eq!(dash30.workspace_backlog.len(), 2);
    assert_eq!(dash30.workspace_backlog[0].problem_id, "p-attempted");
    assert_eq!(
        dash30.workspace_backlog[0].status,
        TrainingStatus::Attempted
    );
    assert_eq!(dash30.workspace_backlog[1].problem_id, "p-todo");
    assert_eq!(dash30.workspace_backlog[1].status, TrainingStatus::Todo);

    // Query with Year window + heatmap_year = 2025
    let dash_year = compute_training_dashboard(
        &db,
        &cfg,
        None,
        &TrainingDashboardQuery {
            window: TimeWindow::Year,
            heatmap_year: Some(2025),
            tag_min_rating: None,
            now_unix: Some(now),
        },
    )
    .unwrap();
    assert_eq!(dash_year.heatmap_year, 2025);
    assert_eq!(dash_year.heatmap.len(), 5);
    assert_eq!(dash_year.kpi.window_ac, 6); // 5 from Nov + 1 from Sep 2026
    assert!(
        dash_year
            .difficulty_trend
            .iter()
            .all(|p| !p.period.contains("-W"))
    );
    assert_eq!(dash_year.difficulty_trend.len(), 2); // 2026-09 and 2026-11
}

#[test]
fn contests_calendar_status_and_filtering() {
    let mut db = SyncDb::open_in_memory().unwrap();
    let now = 1_800_000_000i64;

    db.upsert_contests(
        "codeforces",
        &[
            OjContest {
                contest_id: "1".into(),
                name: "Finished CF".into(),
                start_time: now - 10_000,
                duration_sec: 7_200,
                format: None,
                url: Some("https://codeforces.com/contest/1".into()),
            },
            OjContest {
                contest_id: "2".into(),
                name: "Running CF".into(),
                start_time: now - 1_800,
                duration_sec: 7_200,
                format: None,
                url: Some("https://codeforces.com/contest/2".into()),
            },
            OjContest {
                contest_id: "3".into(),
                name: "Upcoming CF".into(),
                start_time: now + 3_600,
                duration_sec: 7_200,
                format: None,
                url: Some("https://codeforces.com/contest/3".into()),
            },
        ],
        "2026-11-24T00:00:00Z",
    )
    .unwrap();
    db.upsert_contests(
        "atcoder",
        &[OjContest {
            contest_id: "abc400".into(),
            name: "ABC 400".into(),
            start_time: now + 1_800,
            duration_sec: 6_000,
            format: None,
            url: Some("https://atcoder.jp/contests/abc400".into()),
        }],
        "2026-11-24T00:00:00Z",
    )
    .unwrap();

    // Default excludes finished
    let active = query_contests(
        &db,
        &ContestsQuery {
            platform: None,
            include_finished: false,
            now_unix: Some(now),
        },
    )
    .unwrap();
    assert_eq!(active.len(), 3);
    assert_eq!(active[0].contest_id, "2");
    assert_eq!(active[0].status, "running");
    assert_eq!(active[0].end_time, now - 1_800 + 7_200);
    assert_eq!(active[1].contest_id, "abc400");
    assert_eq!(active[1].status, "upcoming");
    assert_eq!(active[2].contest_id, "3");
    assert_eq!(active[2].status, "upcoming");

    // Include finished + platform filter
    let cf_all = query_contests(
        &db,
        &ContestsQuery {
            platform: Some("CodeForces".into()),
            include_finished: true,
            now_unix: Some(now),
        },
    )
    .unwrap();
    assert_eq!(cf_all.len(), 3);
    assert_eq!(cf_all[0].status, "finished");
    assert_eq!(cf_all[1].status, "running");
    assert_eq!(cf_all[2].status, "upcoming");
}

#[test]
fn submissions_query_by_problem_dedup_unsolved_filters_and_library_link() {
    let ws_dir = TempDir::new().unwrap();
    init_workspace(ws_dir.path()).unwrap();
    let ws = Workspace::open(ws_dir.path(), false).unwrap();
    ws.create_problem_with_options(
        "cf-1999a",
        "A+B Again?",
        &CreateProblemOptions {
            sources: vec![ProblemSource {
                platform: "codeforces".into(),
                id: "1999A".into(),
                url: None,
            }],
            tags: vec![],
            difficulty: None,
            rating: Some(800),
        },
    )
    .unwrap();

    let mut db = SyncDb::open_in_memory().unwrap();
    db.upsert_submissions(
        "codeforces",
        &[
            make_sub(
                "codeforces",
                "1",
                "1999A",
                "A+B Again?",
                Some(800),
                &["math"],
                "WA",
                1_000,
            ),
            make_sub(
                "codeforces",
                "2",
                "1999A",
                "A+B Again?",
                Some(800),
                &["math"],
                "AC",
                2_000,
            ),
            make_sub(
                "codeforces",
                "3",
                "1999B",
                "Tree DP",
                Some(1900),
                &["dp", "trees"],
                "WA",
                3_000,
            ),
            make_sub(
                "codeforces",
                "4",
                "1999B",
                "Tree DP",
                Some(1900),
                &["dp", "trees"],
                "TLE",
                4_000,
            ),
        ],
        "2026-11-24T00:00:00Z",
    )
    .unwrap();
    db.upsert_submissions(
        "atcoder",
        &[make_sub(
            "atcoder",
            "5",
            "abc350_c",
            "Sort",
            Some(1200),
            &["greedy"],
            "AC",
            5_000,
        )],
        "2026-11-24T00:00:00Z",
    )
    .unwrap();

    // By problem deduplication
    let by_prob = query_submissions(
        &db,
        Some(&ws),
        &SubmissionsQuery {
            by_problem: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(by_prob.total, 3);
    assert_eq!(by_prob.items[0].problem_key, "abc350_c");
    assert!(by_prob.items[0].solved);
    assert_eq!(by_prob.items[0].attempt_count, 1);

    assert_eq!(by_prob.items[1].problem_key, "1999B");
    assert!(!by_prob.items[1].solved);
    assert_eq!(by_prob.items[1].attempt_count, 2);
    assert_eq!(by_prob.items[1].verdict, "TLE"); // latest non-AC

    assert_eq!(by_prob.items[2].problem_key, "1999A");
    assert!(by_prob.items[2].solved);
    assert_eq!(by_prob.items[2].attempt_count, 2);
    assert_eq!(by_prob.items[2].verdict, "AC");
    assert_eq!(
        by_prob.items[2].library_problem_id.as_deref(),
        Some("cf-1999a")
    );

    // Unsolved only + search + rating range + tag
    let unsolved = query_submissions(
        &db,
        Some(&ws),
        &SubmissionsQuery {
            by_problem: true,
            unsolved_only: true,
            min_rating: Some(1800),
            max_rating: Some(2000),
            tag: Some("TREES".into()),
            search: Some("tree".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(unsolved.total, 1);
    assert_eq!(unsolved.items[0].problem_key, "1999B");

    // Raw mode pagination
    let paged = query_submissions(
        &db,
        None,
        &SubmissionsQuery {
            by_problem: false,
            offset: Some(1),
            limit: Some(2),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(paged.total, 5);
    assert_eq!(paged.offset, 1);
    assert_eq!(paged.limit, 2);
    assert_eq!(paged.items.len(), 2);
}

#[test]
fn ingest_problem_url_cache_dedup_slug_collision_initial_status_and_readonly() {
    let data_dir = TempDir::new().unwrap();
    let ws_dir = TempDir::new().unwrap();
    init_workspace(ws_dir.path()).unwrap();
    let ws = Workspace::open(ws_dir.path(), false).unwrap();

    // Seed oj_sync.db with CF 1999A and 1999B
    {
        let mut db = SyncDb::open(data_dir.path()).unwrap();
        db.upsert_submissions(
            "codeforces",
            &[
                make_sub(
                    "codeforces",
                    "10",
                    "1999A",
                    "A+B Again?",
                    Some(800),
                    &["math", "implementation"],
                    "AC",
                    1_722_961_000,
                ),
                make_sub(
                    "codeforces",
                    "11",
                    "1999B",
                    "Card Game",
                    Some(1000),
                    &["greedy"],
                    "WA",
                    1_722_962_000,
                ),
            ],
            "2026-11-24T00:00:00Z",
        )
        .unwrap();
    }

    let pm = PluginManager::new(data_dir.path());

    // 1. Ingest CF 1999A by URL with initial_status = Todo
    let res1 = ingest_problem(
        &ws,
        data_dir.path(),
        Some(&pm),
        &IngestProblemRequest {
            url: Some("https://codeforces.com/problemset/problem/1999/A".into()),
            initial_status: Some(TrainingLogStatus::Todo),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(res1.created);
    assert_eq!(res1.problem_id, "cf-1999a");
    assert_eq!(res1.title, "A+B Again?");
    assert_eq!(res1.rating, Some(800));
    assert_eq!(res1.tags, vec!["math", "implementation"]);
    assert_eq!(
        res1.source
            .as_ref()
            .map(|s| (s.platform.as_str(), s.id.as_str())),
        Some(("codeforces", "1999A"))
    );

    // Verify training status is NOT in problem.toml, only in training/logs.jsonl
    let problem_toml = ws.read_text("problems/cf-1999a/problem.toml").unwrap();
    assert!(!problem_toml.contains("todo"));
    assert!(!problem_toml.contains("status"));
    let logs = ws.read_training_logs().unwrap();
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].status, TrainingLogStatus::Todo);

    // 2. Deduplicate when ingesting same problem by (platform, problem_key)
    let res2 = ingest_problem(
        &ws,
        data_dir.path(),
        Some(&pm),
        &IngestProblemRequest {
            platform: Some("CodeForces".into()),
            problem_key: Some("1999a".into()),
            initial_status: Some(TrainingLogStatus::Upsolved),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(!res2.created);
    assert_eq!(res2.problem_id, "cf-1999a");
    let logs2 = ws.read_training_logs().unwrap();
    assert_eq!(logs2.len(), 2);
    assert_eq!(logs2[1].status, TrainingLogStatus::Upsolved);

    // 3. Slug collision: pre-create an unrelated problem at `cf-1999b`, then ingest CF 1999B -> `cf-1999b-2`
    ws.create_problem_with_options("cf-1999b", "Unrelated", &CreateProblemOptions::default())
        .unwrap();
    let res3 = ingest_problem(
        &ws,
        data_dir.path(),
        Some(&pm),
        &IngestProblemRequest {
            platform: Some("codeforces".into()),
            problem_key: Some("1999B".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(res3.created);
    assert_eq!(res3.problem_id, "cf-1999b-2");
    assert_eq!(res3.title, "Card Game");
    assert_eq!(res3.rating, Some(1000));

    // 4. Read-only workspace must reject ingest_problem
    let ro_ws = Workspace::open(ws_dir.path(), true).unwrap();
    let err = ingest_problem(
        &ro_ws,
        data_dir.path(),
        Some(&pm),
        &IngestProblemRequest {
            platform: Some("codeforces".into()),
            problem_key: Some("1999C".into()),
            title: Some("Should Fail".into()),
            ..Default::default()
        },
    )
    .unwrap_err();
    assert!(matches!(
        err,
        SyncError::Storage(xcpc_storage::Error::ReadOnly)
    ));
}

#[test]
fn dashboard_and_submissions_performance_with_3000_submissions() {
    let mut db = SyncDb::open_in_memory().unwrap();
    let now = ymd_to_unix_secs(2026, 11, 24) + 12 * 3600;

    let mut subs = Vec::with_capacity(3000);
    let tag_pool = [
        "dp",
        "greedy",
        "math",
        "graphs",
        "data structures",
        "strings",
    ];
    for i in 0..3000i64 {
        let problem_num = i / 2; // 1500 distinct problems, 2 submissions each
        let is_ac = i % 2 == 1 || (problem_num % 5 != 0); // 10% unsolved (when problem_num % 5 == 0 and i % 2 == 0? let's make every 10th problem unsolved)
        let verdict = if problem_num % 10 == 0 {
            "WA"
        } else if is_ac {
            "AC"
        } else {
            "WA"
        };
        let rating = 800 + (problem_num % 22) * 100; // 800..=2900
        let submitted_at = now - (i * 10_000); // spans ~347 days
        subs.push(make_sub(
            "codeforces",
            &format!("sub-{i}"),
            &format!("P{problem_num}"),
            &format!("Problem {problem_num}"),
            Some(rating),
            &[
                tag_pool[(problem_num as usize) % tag_pool.len()],
                tag_pool[((problem_num as usize) + 1) % tag_pool.len()],
            ],
            verdict,
            submitted_at,
        ));
    }
    db.upsert_submissions("codeforces", &subs, "2026-11-24T00:00:00Z")
        .unwrap();

    let start = Instant::now();
    let dash = compute_training_dashboard(
        &db,
        &SyncConfig::default(),
        None,
        &TrainingDashboardQuery {
            window: TimeWindow::All,
            heatmap_year: Some(2026),
            tag_min_rating: Some(1600),
            now_unix: Some(now),
        },
    )
    .unwrap();
    let page = query_submissions(
        &db,
        None,
        &SubmissionsQuery {
            by_problem: true,
            unsolved_only: true,
            limit: Some(50),
            ..Default::default()
        },
    )
    .unwrap();
    let elapsed = start.elapsed();

    assert_eq!(dash.kpi.total_submissions, 3000);
    assert_eq!(dash.kpi.total_ac, 1350); // 1500 problems - 150 unsolved (every 10th)
    assert_eq!(dash.kpi.unsolved_attempted_count, 150);
    assert_eq!(dash.unsolved_problems.len(), 50); // capped at 50
    assert_eq!(page.total, 150);
    assert_eq!(page.items.len(), 50);
    assert!(
        elapsed.as_secs_f64() < 2.0,
        "3000-submission analytics took too long: {elapsed:?}"
    );
}

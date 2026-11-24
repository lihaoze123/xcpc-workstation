use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use tempfile::TempDir;
use xcpc_domain::{
    ProblemSource, SyncedProblemStatus, TrainingStatus, check_id, derive_training_status,
};
use xcpc_plugin::{
    HttpRequest, HttpResponse, MemoryCredentialStore, MockHttpClient, PluginError, PluginManager,
};

use crate::{
    SyncConfig, SyncDb, SyncEngine, SyncOptions, default_data_dir_with, suggest_problem_slug,
    unix_secs_to_rfc3339_utc,
};

fn ok_json(body: &str) -> HttpResponse {
    HttpResponse {
        status: 200,
        headers: BTreeMap::new(),
        body: body.to_string(),
    }
}

#[test]
fn default_data_dir_matches_nsh_and_env_override() {
    let env = |pairs: &'static [(&'static str, &'static str)]| {
        move |key: &str| {
            pairs
                .iter()
                .find(|(k, _)| *k == key)
                .map(|(_, v)| PathBuf::from(v))
        }
    };

    assert_eq!(
        default_data_dir_with("linux", env(&[("XCPC_DATA_DIR", "/tmp/custom-xcpc")])),
        Some(PathBuf::from("/tmp/custom-xcpc"))
    );
    assert_eq!(
        default_data_dir_with(
            "linux",
            env(&[
                ("XCPC_DATA_DIR", ""),
                ("XDG_DATA_HOME", "/data"),
                ("HOME", "/home/u")
            ])
        ),
        Some(PathBuf::from("/data/dev.xcpc.workstation"))
    );
    assert_eq!(
        default_data_dir_with("linux", env(&[("XDG_DATA_HOME", ""), ("HOME", "/home/u")])),
        Some(PathBuf::from("/home/u/.local/share/dev.xcpc.workstation"))
    );
    assert_eq!(
        default_data_dir_with("macos", env(&[("HOME", "/Users/u")])),
        Some(PathBuf::from(
            "/Users/u/Library/Application Support/dev.xcpc.workstation"
        ))
    );
    assert_eq!(
        default_data_dir_with(
            "windows",
            env(&[("APPDATA", r"C:\Users\u\AppData\Roaming")])
        ),
        Some(PathBuf::from(r"C:\Users\u\AppData\Roaming").join("dev.xcpc.workstation"))
    );
    assert_eq!(unix_secs_to_rfc3339_utc(0), "1970-01-01T00:00:00Z");
    assert_eq!(
        unix_secs_to_rfc3339_utc(1_732_450_800),
        "2024-11-24T12:20:00Z"
    );
}

#[test]
fn sync_config_roundtrip_and_handle_management() {
    let dir = TempDir::new().unwrap();
    let mut cfg = SyncConfig::load(dir.path()).unwrap();
    assert!(cfg.handles.is_empty());
    assert!(cfg.auto_sync_on_startup);
    assert!(cfg.last_synced_at.is_none());

    cfg.set_handle("CodeForces", "  tourist  ");
    cfg.set_handle("atcoder", "chokudai");
    assert_eq!(cfg.handle_for("codeforces"), Some("tourist"));
    assert_eq!(cfg.handle_for("CODEFORCES"), Some("tourist"));
    assert_eq!(cfg.handle_for("atcoder"), Some("chokudai"));

    cfg.auto_sync_on_startup = false;
    cfg.last_synced_at = Some("2026-11-24T12:00:00Z".into());
    cfg.save(dir.path()).unwrap();

    let reloaded = SyncConfig::load(dir.path()).unwrap();
    assert_eq!(reloaded, cfg);

    let mut cleared = reloaded;
    cleared.set_handle("codeforces", "   ");
    assert_eq!(cleared.handle_for("codeforces"), None);
    assert_eq!(cleared.handle_for("atcoder"), Some("chokudai"));
}

#[test]
fn full_and_incremental_sync_codeforces_and_atcoder() {
    let data_dir = TempDir::new().unwrap();
    let phase = Arc::new(AtomicUsize::new(1));
    let phase_http = Arc::clone(&phase);

    let http = Arc::new(MockHttpClient::new(move |req: &HttpRequest| {
        let p = phase_http.load(Ordering::SeqCst);
        let url = req.url.as_str();

        // Codeforces endpoints
        if url.contains("codeforces.com/api/user.info") {
            return Ok(ok_json(
                r#"{"status":"OK","result":[{"handle":"tourist","rating":3800,"maxRating":3979,"rank":"legendary grandmaster"}]}"#,
            ));
        }
        if url.contains("codeforces.com/api/user.rating") {
            return Ok(ok_json(
                r#"{"status":"OK","result":[{"contestId":1999,"contestName":"Codeforces Round 964","oldRating":3750,"newRating":3800,"rank":1,"ratingUpdateTimeSeconds":1722960000}]}"#,
            ));
        }
        if url.contains("codeforces.com/api/user.status") {
            if p == 1 {
                return Ok(ok_json(
                    r#"{"status":"OK","result":[
                        {"id":1002,"creationTimeSeconds":1722961000,"programmingLanguage":"C++20","verdict":"OK","problem":{"contestId":1999,"index":"A","name":"A+B Again?","rating":800,"tags":["implementation","math"]}},
                        {"id":1001,"creationTimeSeconds":1722960500,"programmingLanguage":"C++20","verdict":"WRONG_ANSWER","problem":{"contestId":1999,"index":"B","name":"Card Game","rating":1000,"tags":["greedy"]}}
                    ]}"#,
                ));
            } else {
                return Ok(ok_json(
                    r#"{"status":"OK","result":[
                        {"id":1003,"creationTimeSeconds":1722962000,"programmingLanguage":"C++20","verdict":"OK","problem":{"contestId":1999,"index":"B","name":"Card Game","rating":1000,"tags":["greedy"]}},
                        {"id":1002,"creationTimeSeconds":1722961000,"programmingLanguage":"C++20","verdict":"OK","problem":{"contestId":1999,"index":"A","name":"A+B Again?","rating":800,"tags":["implementation","math"]}}
                    ]}"#,
                ));
            }
        }
        if url.contains("codeforces.com/api/contest.list") {
            return Ok(ok_json(
                r#"{"status":"OK","result":[{"id":2000,"name":"CF Round 966","phase":"BEFORE","startTimeSeconds":1800000000,"durationSeconds":7200,"type":"CF"}]}"#,
            ));
        }

        // AtCoder endpoints
        if url.contains("atcoder.jp/users/chokudai/history/json") {
            return Ok(ok_json(
                r#"[
                    {"ContestScreenName":"abc350.contest.atcoder.jp","ContestName":"ABC 350","IsRated":true,"ContestType":0,"OldRating":2800,"NewRating":2850,"Place":5,"EndTime":"2024-04-20T13:40:00+09:00"},
                    {"ContestScreenName":"ahc030.contest.atcoder.jp","ContestName":"AHC 030","IsRated":true,"ContestType":1,"OldRating":2000,"NewRating":2100,"Place":1,"EndTime":"2024-04-21T13:40:00+09:00"}
                ]"#,
            ));
        }
        if url.contains("kenkoooo.com/atcoder/atcoder-api/v3/user/submissions") {
            if url.contains("from_second=0") {
                return Ok(ok_json(
                    r#"[{"id":5001,"epoch_second":1713600000,"problem_id":"abc350_a","contest_id":"abc350","result":"AC","language":"C++ 23"}]"#,
                ));
            } else {
                return Ok(ok_json(
                    r#"[{"id":5002,"epoch_second":1713601000,"problem_id":"abc350_b","contest_id":"abc350","result":"WA","language":"C++ 23"}]"#,
                ));
            }
        }
        if url.contains("kenkoooo.com/atcoder/resources/problems.json") {
            return Ok(ok_json(
                r#"[
                    {"id":"abc350_a","contest_id":"abc350","problem_index":"A","title":"A. Past ABCs"},
                    {"id":"abc350_b","contest_id":"abc350","problem_index":"B","title":"B. Dentist Aoki"}
                ]"#,
            ));
        }
        if url.contains("kenkoooo.com/atcoder/resources/problem-models.json") {
            return Ok(ok_json(
                r#"{"abc350_a":{"difficulty":50},"abc350_b":{"difficulty":250}}"#,
            ));
        }
        if url == "https://atcoder.jp/contests/" {
            return Ok(ok_json(
                r#"<div id="contest-table-upcoming"><table><tbody>
                    <tr><td>2026-12-01 21:00:00+0900</td><td><a href="/contests/abc400">AtCoder Beginner Contest 400</a></td><td>01:40</td></tr>
                </tbody></table></div>"#,
            ));
        }

        Err(PluginError::Http(format!("unexpected URL: {url}")))
    }));

    let creds = Arc::new(MemoryCredentialStore::new());
    let engine = SyncEngine::new(data_dir.path(), http.clone(), creds);

    let mut cfg = SyncConfig::default();
    cfg.set_handle("codeforces", "tourist");
    cfg.set_handle("atcoder", "chokudai");
    engine.save_config(&cfg).unwrap();

    // First sync (initial).
    let report1 = engine.sync(&SyncOptions::default()).unwrap();
    assert_eq!(report1.platforms.len(), 2);
    assert!(report1.platforms.iter().all(|p| p.ok));

    let cf_rep1 = report1
        .platforms
        .iter()
        .find(|p| p.platform == "codeforces")
        .unwrap();
    assert!(cf_rep1.account_synced);
    assert_eq!(cf_rep1.submissions_synced, 2);
    assert_eq!(cf_rep1.rating_history_synced, 1);
    assert_eq!(cf_rep1.contests_synced, 1);

    let ac_rep1 = report1
        .platforms
        .iter()
        .find(|p| p.platform == "atcoder")
        .unwrap();
    assert!(ac_rep1.account_synced);
    assert_eq!(ac_rep1.submissions_synced, 1);
    assert_eq!(ac_rep1.rating_history_synced, 1); // AHC skipped
    assert_eq!(ac_rep1.contests_synced, 1);

    // Check DB contents after phase 1.
    let db = engine.open_db().unwrap();
    let accounts = db.list_accounts().unwrap();
    assert_eq!(accounts.len(), 2);
    assert_eq!(
        accounts
            .iter()
            .find(|a| a.platform == "codeforces")
            .unwrap()
            .rating,
        Some(3800)
    );
    assert_eq!(
        accounts
            .iter()
            .find(|a| a.platform == "atcoder")
            .unwrap()
            .rating,
        Some(2850)
    );

    let contests = db.list_contests().unwrap();
    assert_eq!(contests.len(), 2);

    // Phase 1 status for CF 1999B is attempted (not AC).
    let st_1999b = db
        .get_problem_sync_status(&[ProblemSource {
            platform: "CodeForces".into(),
            id: "1999b".into(),
            url: None,
        }])
        .unwrap();
    assert_eq!(
        st_1999b,
        SyncedProblemStatus {
            ac: false,
            attempted: true,
        }
    );

    // Verify cursor stored in plugin_kv.
    let pm = engine.plugin_manager();
    let cf_cursor = pm
        .kv_store("codeforces")
        .unwrap()
        .get("sync_cursor")
        .unwrap();
    assert_eq!(cf_cursor, Some(serde_json::json!("1002")));

    // Phase 2: incremental sync fetches only the new CF submission (1003) and AtCoder from_second > 0.
    phase.store(2, Ordering::SeqCst);
    let report2 = engine.sync(&SyncOptions::default()).unwrap();
    let cf_rep2 = report2
        .platforms
        .iter()
        .find(|p| p.platform == "codeforces")
        .unwrap();
    assert_eq!(cf_rep2.submissions_synced, 1);
    let ac_rep2 = report2
        .platforms
        .iter()
        .find(|p| p.platform == "atcoder")
        .unwrap();
    assert_eq!(ac_rep2.submissions_synced, 1);

    // Now CF 1999B is AC!
    let st_1999b_after = db
        .get_problem_sync_status(&[ProblemSource {
            platform: "codeforces".into(),
            id: "1999B".into(),
            url: None,
        }])
        .unwrap();
    assert_eq!(
        st_1999b_after,
        SyncedProblemStatus {
            ac: true,
            attempted: true,
        }
    );

    // Full sync ignores stored cursor (CF returns both 1003 & 1002 -> 2 submissions).
    let report3 = engine
        .sync(&SyncOptions {
            platform: Some("codeforces".into()),
            full: true,
        })
        .unwrap();
    assert_eq!(report3.platforms.len(), 1);
    assert_eq!(report3.platforms[0].submissions_synced, 2);

    // Config last_synced_at updated.
    let updated_cfg = engine.load_config().unwrap();
    assert!(updated_cfg.last_synced_at.is_some());
}

#[test]
fn lightweight_sync_summary_plugin_and_batch_training_status_merge() {
    let data_dir = TempDir::new().unwrap();
    let plugin_src = TempDir::new().unwrap();

    std::fs::write(
        plugin_src.path().join("manifest.toml"),
        r#"id = "luogu-lite"
name = "Luogu Lite"
version = "0.1.0"
description = "Lightweight summary OJ plugin"
entry = "index.js"
capabilities = ["oj_source"]

[permissions]
domains = ["www.luogu.com.cn"]
workspace_read = false
"#,
    )
    .unwrap();

    std::fs::write(
        plugin_src.path().join("index.js"),
        r#"
function syncSummary(ctx) {
  return {
    handle: ctx.handle,
    solved_count: 2,
    submission_count: 5,
    rating: 1950,
    solved_problem_keys: ["P1001", "P3372"],
  };
}
module.exports = { syncSummary };
"#,
    )
    .unwrap();

    let pm = PluginManager::new(data_dir.path());
    pm.install_from_dir(plugin_src.path()).unwrap();

    let http = Arc::new(MockHttpClient::new(|req| {
        Err(PluginError::Http(format!("unexpected HTTP: {}", req.url)))
    }));
    let creds = Arc::new(MemoryCredentialStore::new());
    let engine = SyncEngine::new(data_dir.path(), http, creds);

    let mut cfg = SyncConfig::default();
    cfg.set_handle("luogu-lite", "kkksc03");
    engine.save_config(&cfg).unwrap();

    let report = engine
        .sync(&SyncOptions {
            platform: Some("luogu-lite".into()),
            full: false,
        })
        .unwrap();
    assert_eq!(report.platforms.len(), 1);
    assert!(report.platforms[0].ok);
    assert!(report.platforms[0].summary_synced);
    assert!(!report.platforms[0].account_synced);

    let db = engine.open_db().unwrap();
    let summaries = db.list_summaries().unwrap();
    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].solved_count, 2);
    assert_eq!(summaries[0].submission_count, Some(5));
    assert_eq!(summaries[0].rating, Some(1950));

    // Single lookup (case-insensitive).
    let st = db
        .get_problem_sync_status(&[ProblemSource {
            platform: "LUOGU-LITE".into(),
            id: "p1001".into(),
            url: None,
        }])
        .unwrap();
    assert_eq!(
        st,
        SyncedProblemStatus {
            ac: true,
            attempted: true,
        }
    );

    // Batch lookup + merge with xcpc_domain::derive_training_status.
    let s1 = [ProblemSource {
        platform: "luogu-lite".into(),
        id: "P1001".into(),
        url: None,
    }];
    let s2 = [ProblemSource {
        platform: "luogu-lite".into(),
        id: "p3372".into(),
        url: None,
    }];
    let s3 = [ProblemSource {
        platform: "luogu-lite".into(),
        id: "P9999".into(),
        url: None,
    }];

    let batch = db
        .get_problems_sync_statuses(&[("p1", &s1), ("p2", &s2), ("p3", &s3)])
        .unwrap();
    assert_eq!(
        derive_training_status(batch["p1"], None),
        TrainingStatus::Ac
    );
    assert_eq!(
        derive_training_status(batch["p2"], Some(TrainingStatus::Upsolved)),
        TrainingStatus::Upsolved
    );
    assert_eq!(
        derive_training_status(batch["p3"], Some(TrainingStatus::Todo)),
        TrainingStatus::Todo
    );
}

#[test]
fn multi_workspace_shares_global_oj_sync_db() {
    let data_dir = TempDir::new().unwrap();
    let ws_a = TempDir::new().unwrap();
    let ws_b = TempDir::new().unwrap();

    // Seed global oj_sync.db from workspace A context.
    {
        let mut db_a = SyncDb::open(data_dir.path()).unwrap();
        db_a.upsert_submissions(
            "codeforces",
            &[xcpc_plugin::OjSubmission {
                submission_id: "777".into(),
                handle: "tourist".into(),
                contest_id: Some("1999".into()),
                problem_index: Some("A".into()),
                problem_key: "1999A".into(),
                problem_title: "A+B Again?".into(),
                problem_url: Some("https://codeforces.com/contest/1999/problem/A".into()),
                raw_rating: Some(800.0),
                normalized_rating: Some(800),
                tags: vec!["math".into()],
                verdict: "AC".into(),
                language: Some("C++20".into()),
                submitted_at: 1_722_961_000,
            }],
            "2026-11-24T12:00:00Z",
        )
        .unwrap();
    }

    // Both workspaces open the shared global <dataDir>/oj_sync.db independently.
    assert!(ws_a.path() != ws_b.path());
    let src = [ProblemSource {
        platform: "codeforces".into(),
        id: "1999a".into(),
        url: None,
    }];
    let db_from_a = SyncDb::open(data_dir.path()).unwrap();
    let db_from_b = SyncDb::open(data_dir.path()).unwrap();
    assert_eq!(
        db_from_a.get_problem_sync_status(&src).unwrap(),
        SyncedProblemStatus {
            ac: true,
            attempted: true,
        }
    );
    assert_eq!(
        db_from_b.get_problem_sync_status(&src).unwrap(),
        SyncedProblemStatus {
            ac: true,
            attempted: true,
        }
    );
}

#[test]
fn resolve_problem_url_enriches_from_cache_and_fetch_problem() {
    let data_dir = TempDir::new().unwrap();

    // Pre-populate cache with CF 1999A and AtCoder abc350_a.
    {
        let mut db = SyncDb::open(data_dir.path()).unwrap();
        db.upsert_submissions(
            "codeforces",
            &[xcpc_plugin::OjSubmission {
                submission_id: "1".into(),
                handle: "tourist".into(),
                contest_id: Some("1999".into()),
                problem_index: Some("A".into()),
                problem_key: "1999A".into(),
                problem_title: "A+B Again?".into(),
                problem_url: Some("https://codeforces.com/contest/1999/problem/A".into()),
                raw_rating: Some(800.0),
                normalized_rating: Some(800),
                tags: vec!["implementation".into(), "math".into()],
                verdict: "AC".into(),
                language: Some("C++20".into()),
                submitted_at: 1_722_961_000,
            }],
            "2026-11-24T12:00:00Z",
        )
        .unwrap();
        db.upsert_submissions(
            "atcoder",
            &[xcpc_plugin::OjSubmission {
                submission_id: "2".into(),
                handle: "chokudai".into(),
                contest_id: Some("abc350".into()),
                problem_index: Some("A".into()),
                problem_key: "abc350_a".into(),
                problem_title: "A. Past ABCs".into(),
                problem_url: Some("https://atcoder.jp/contests/abc350/tasks/abc350_a".into()),
                raw_rating: Some(50.0),
                normalized_rating: Some(933),
                tags: vec![],
                verdict: "AC".into(),
                language: Some("C++23".into()),
                submitted_at: 1_713_600_000,
            }],
            "2026-11-24T12:00:00Z",
        )
        .unwrap();
    }

    // Also install a custom plugin that supports fetchProblem for uncached URLs.
    let custom_dir = TempDir::new().unwrap();
    std::fs::write(
        custom_dir.path().join("manifest.toml"),
        r#"id = "customoj"
name = "Custom OJ"
version = "0.1.0"
description = "Custom OJ with fetchProblem"
entry = "index.js"
capabilities = ["oj_source"]

[permissions]
domains = ["custom.oj"]
workspace_read = false
"#,
    )
    .unwrap();
    std::fs::write(
        custom_dir.path().join("index.js"),
        r#"
function parseProblemUrl(url) {
  const m = String(url).match(/^https:\/\/custom\.oj\/p\/([A-Za-z0-9_]+)$/);
  if (!m) return null;
  return { platform: "customoj", problem_id: m[1], url };
}
function fetchProblem(_ctx, id) {
  return {
    problem_id: id,
    title: "Fetched Problem " + id,
    raw_rating: 1500,
    tags: ["dp", "graphs"],
    samples: [{ input: "1 2\n", output: "3\n" }],
  };
}
function normalizeRating(raw) {
  return Math.round(raw + 100);
}
module.exports = { parseProblemUrl, fetchProblem, normalizeRating };
"#,
    )
    .unwrap();
    PluginManager::new(data_dir.path())
        .install_from_dir(custom_dir.path())
        .unwrap();

    let http = Arc::new(MockHttpClient::new(|req| {
        Err(PluginError::Http(format!("unexpected HTTP: {}", req.url)))
    }));
    let creds = Arc::new(MemoryCredentialStore::new());
    let engine = SyncEngine::new(data_dir.path(), http, creds);

    let cf = engine
        .resolve_problem_url("https://codeforces.com/problemset/problem/1999/A")
        .unwrap()
        .expect("should parse CF URL");
    assert_eq!(cf.platform, "codeforces");
    assert_eq!(cf.problem_key, "1999A");
    assert_eq!(cf.suggested_slug, "cf-1999a");
    assert!(check_id(&cf.suggested_slug).is_ok());
    assert_eq!(cf.title.as_deref(), Some("A+B Again?"));
    assert_eq!(cf.raw_rating, Some(800));
    assert_eq!(cf.normalized_rating, Some(800));
    assert_eq!(cf.tags, vec!["implementation", "math"]);

    let ac = engine
        .resolve_problem_url("https://atcoder.jp/contests/abc350/tasks/abc350_a")
        .unwrap()
        .expect("should parse AtCoder URL");
    assert_eq!(ac.platform, "atcoder");
    assert_eq!(ac.problem_key, "abc350_a");
    assert_eq!(ac.suggested_slug, "atcoder-abc350-a");
    assert!(check_id(&ac.suggested_slug).is_ok());
    assert_eq!(ac.title.as_deref(), Some("A. Past ABCs"));
    assert_eq!(ac.normalized_rating, Some(933));

    let custom = engine
        .resolve_problem_url("https://custom.oj/p/PROB_42")
        .unwrap()
        .expect("should parse custom OJ URL");
    assert_eq!(custom.suggested_slug, "customoj-prob-42");
    assert!(check_id(&custom.suggested_slug).is_ok());
    assert_eq!(custom.title.as_deref(), Some("Fetched Problem PROB_42"));
    assert_eq!(custom.raw_rating, Some(1500));
    assert_eq!(custom.normalized_rating, Some(1600));
    assert_eq!(custom.tags, vec!["dp", "graphs"]);
    assert_eq!(custom.samples.len(), 1);

    // Unrecognized URL returns None.
    assert!(
        engine
            .resolve_problem_url("https://unknown.example.com/problem/1")
            .unwrap()
            .is_none()
    );

    // Slug edge cases always pass xcpc_domain::check_id.
    for (p, k) in [
        ("codeforces", "1999A"),
        ("atcoder", "abc350_a"),
        ("!!!", "???"),
        ("very-long-platform-name", &"a".repeat(100)),
    ] {
        let slug = suggest_problem_slug(p, k);
        assert!(
            check_id(&slug).is_ok(),
            "invalid slug {slug:?} for ({p}, {k})"
        );
    }
}

#[test]
fn error_isolation_records_and_clears_last_error() {
    let data_dir = TempDir::new().unwrap();
    let fail_cf = Arc::new(AtomicUsize::new(1));
    let fail_cf_http = Arc::clone(&fail_cf);

    let http = Arc::new(MockHttpClient::new(move |req: &HttpRequest| {
        let url = req.url.as_str();
        if url.contains("codeforces.com") {
            if fail_cf_http.load(Ordering::SeqCst) == 1 {
                return Err(PluginError::Http("CF simulated outage".into()));
            }
            if url.contains("user.info") {
                return Ok(ok_json(
                    r#"{"status":"OK","result":[{"handle":"tourist","rating":3800}]}"#,
                ));
            }
            return Ok(ok_json(r#"{"status":"OK","result":[]}"#));
        }
        if url.contains("atcoder.jp/users/chokudai/history/json") {
            return Ok(ok_json(r#"[]"#));
        }
        if url.contains("kenkoooo.com/atcoder/atcoder-api/v3/user/submissions") {
            return Ok(ok_json(r#"[]"#));
        }
        if url.contains("kenkoooo.com/atcoder/resources/problems.json") {
            return Ok(ok_json(r#"[]"#));
        }
        if url.contains("kenkoooo.com/atcoder/resources/problem-models.json") {
            return Ok(ok_json(r#"{}"#));
        }
        if url == "https://atcoder.jp/contests/" {
            return Ok(ok_json(r#"<html></html>"#));
        }
        if url.contains("kenkoooo.com/atcoder/resources/contests.json") {
            return Ok(ok_json(r#"[]"#));
        }
        Err(PluginError::Http(format!("unexpected URL: {url}")))
    }));

    let creds = Arc::new(MemoryCredentialStore::new());
    let engine = SyncEngine::new(data_dir.path(), http, creds);

    let mut cfg = SyncConfig::default();
    cfg.set_handle("codeforces", "tourist");
    cfg.set_handle("atcoder", "chokudai");
    engine.save_config(&cfg).unwrap();

    // Run 1: CF fails, AtCoder still succeeds.
    let rep1 = engine.sync(&SyncOptions::default()).unwrap();
    assert_eq!(rep1.platforms.len(), 2);
    let cf1 = rep1
        .platforms
        .iter()
        .find(|p| p.platform == "codeforces")
        .unwrap();
    let ac1 = rep1
        .platforms
        .iter()
        .find(|p| p.platform == "atcoder")
        .unwrap();
    assert!(!cf1.ok);
    assert!(
        cf1.error
            .as_deref()
            .unwrap_or("")
            .contains("CF simulated outage")
    );
    assert!(ac1.ok);
    assert!(ac1.error.is_none());

    let plugins = engine.plugin_manager().list_plugins().unwrap();
    let cf_info = plugins
        .iter()
        .find(|p| p.manifest.id == "codeforces")
        .unwrap();
    assert!(
        cf_info
            .last_error
            .as_deref()
            .unwrap_or("")
            .contains("CF simulated outage")
    );

    // Run 2: CF recovers -> last_error is cleared.
    fail_cf.store(0, Ordering::SeqCst);
    let rep2 = engine.sync(&SyncOptions::default()).unwrap();
    assert!(rep2.platforms.iter().all(|p| p.ok));
    let plugins2 = engine.plugin_manager().list_plugins().unwrap();
    let cf_info2 = plugins2
        .iter()
        .find(|p| p.manifest.id == "codeforces")
        .unwrap();
    assert!(cf_info2.last_error.is_none());
}

#[test]
fn scheduler_normalizes_rating_when_plugin_omits_normalized_rating() {
    let data_dir = TempDir::new().unwrap();
    let plugin_src = TempDir::new().unwrap();

    std::fs::write(
        plugin_src.path().join("manifest.toml"),
        r#"id = "rawrating-oj"
name = "Raw Rating OJ"
version = "0.1.0"
description = "Returns raw_rating without normalized_rating"
entry = "index.js"
capabilities = ["oj_source"]

[permissions]
domains = []
workspace_read = false
"#,
    )
    .unwrap();
    std::fs::write(
        plugin_src.path().join("index.js"),
        r#"
function syncSubmissions(ctx, _cursor) {
  return {
    submissions: [{
      submission_id: "s1",
      handle: ctx.handle,
      problem_key: "P1",
      problem_title: "Problem 1",
      raw_rating: 1200,
      tags: ["math"],
      verdict: "AC",
      submitted_at: 1700000000
    }],
    next_cursor: "s1"
  };
}
function normalizeRating(raw) {
  return Math.round(raw * 1.1);
}
module.exports = { syncSubmissions, normalizeRating };
"#,
    )
    .unwrap();

    PluginManager::new(data_dir.path())
        .install_from_dir(plugin_src.path())
        .unwrap();

    let http = Arc::new(MockHttpClient::new(|req| {
        Err(PluginError::Http(format!("unexpected HTTP: {}", req.url)))
    }));
    let creds = Arc::new(MemoryCredentialStore::new());
    let engine = SyncEngine::new(data_dir.path(), http, creds);

    let mut cfg = SyncConfig::default();
    cfg.set_handle("rawrating-oj", "alice");
    engine.save_config(&cfg).unwrap();

    let rep = engine
        .sync(&SyncOptions {
            platform: Some("rawrating-oj".into()),
            full: false,
        })
        .unwrap();
    assert!(rep.platforms[0].ok);

    let db = engine.open_db().unwrap();
    let meta = db
        .lookup_problem_meta("rawrating-oj", "P1")
        .unwrap()
        .expect("problem meta populated");
    assert_eq!(meta.raw_rating, Some(1200));
    assert_eq!(meta.normalized_rating, Some(1320));
}

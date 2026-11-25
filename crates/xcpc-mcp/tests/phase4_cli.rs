//! End-to-end CLI integration tests for Phase 4A subcommands:
//! `xcpc plugin`, `xcpc sync`, `xcpc training`, and `xcpc problem from-url`.

use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use serde_json::Value;
use zip::write::SimpleFileOptions;

fn run_xcpc(ws: Option<&Path>, data_dir: &Path, args: &[&str]) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_xcpc"));
    cmd.env("XCPC_DATA_DIR", data_dir);
    if let Some(w) = ws {
        cmd.arg("-w").arg(w);
    }
    cmd.args(args);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    cmd.output().expect("spawn xcpc")
}

fn parse_json_stdout(out: &Output) -> Value {
    let s = String::from_utf8(out.stdout.clone()).expect("utf8 stdout");
    serde_json::from_str(&s).unwrap_or_else(|e| {
        panic!(
            "stdout is not valid JSON ({e}):\nstdout: {s}\nstderr: {}",
            String::from_utf8_lossy(&out.stderr)
        )
    })
}

fn setup_workspace() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    xcpc_storage::init_workspace(dir.path()).unwrap();

    let p1 = dir.path().join("problems/p1");
    fs::create_dir_all(p1.join("samples")).unwrap();
    fs::write(
        p1.join("problem.toml"),
        r#"title = "Sample CF Problem"
rating = 1600
tags = ["dp"]

[limits]
time = "1s"
memory = "256MB"

[[source]]
platform = "codeforces"
id = "1999A"
"#,
    )
    .unwrap();
    fs::write(
        p1.join("statement.md"),
        "Statement.\n\n## 输入格式\n\nInput.\n\n## 输出格式\n\nOutput.\n",
    )
    .unwrap();
    dir
}

#[test]
fn plugin_list_shows_builtin_codeforces_and_atcoder() {
    let data_dir = tempfile::tempdir().unwrap();
    let out = run_xcpc(None, data_dir.path(), &["plugin", "list", "--json"]);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json = parse_json_stdout(&out);
    assert_eq!(json["ok"], true);

    let plugins = json["plugins"].as_array().expect("plugins array");
    let ids: Vec<&str> = plugins.iter().filter_map(|p| p["id"].as_str()).collect();
    assert!(ids.contains(&"codeforces"), "missing codeforces: {ids:?}");
    assert!(ids.contains(&"atcoder"), "missing atcoder: {ids:?}");

    let cf = plugins.iter().find(|p| p["id"] == "codeforces").unwrap();
    assert_eq!(cf["origin"], "builtin");
    assert_eq!(cf["enabled"], true);
    assert_eq!(cf["has_unsandboxed_ui"], false);
    assert!(
        cf["capabilities"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c == "oj_source")
    );
    assert!(!cf["domains"].as_array().unwrap().is_empty());
    assert!(cf["last_error"].is_null());
}

#[test]
fn plugin_init_and_test_oj_and_tool_scaffolds() {
    let data_dir = tempfile::tempdir().unwrap();
    let tmp = tempfile::tempdir().unwrap();

    // 1. OJ scaffold
    let oj_dir = tmp.path().join("my-oj");
    let init_oj = run_xcpc(
        None,
        data_dir.path(),
        &[
            "plugin",
            "init",
            oj_dir.to_str().unwrap(),
            "--kind",
            "oj",
            "--json",
        ],
    );
    assert!(init_oj.status.success());
    let init_oj_json = parse_json_stdout(&init_oj);
    assert_eq!(init_oj_json["ok"], true);
    assert_eq!(init_oj_json["kind"], "oj");
    assert!(oj_dir.join("manifest.toml").is_file());
    assert!(oj_dir.join("index.js").is_file());
    assert!(oj_dir.join("xcpc-plugin.d.ts").is_file());

    // Run `plugin test` with explicit action and --args
    let test_oj = run_xcpc(
        None,
        data_dir.path(),
        &[
            "plugin",
            "test",
            oj_dir.to_str().unwrap(),
            "verify",
            "--args",
            r#"{"handle":"tourist"}"#,
            "--json",
        ],
    );
    assert!(
        test_oj.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&test_oj.stderr)
    );
    let test_oj_json = parse_json_stdout(&test_oj);
    assert_eq!(test_oj_json["ok"], true);
    assert_eq!(test_oj_json["plugin_id"], "my-oj");
    assert_eq!(test_oj_json["action"], "verify");
    assert!(test_oj_json["duration_ms"].is_u64());
    assert_eq!(test_oj_json["result"]["ok"], true);
    assert_eq!(test_oj_json["result"]["handle"], "tourist");

    // 2. Tool scaffold
    let tool_dir = tmp.path().join("my-tool");
    let init_tool = run_xcpc(
        None,
        data_dir.path(),
        &[
            "plugin",
            "init",
            tool_dir.to_str().unwrap(),
            "--kind",
            "tool",
            "--json",
        ],
    );
    assert!(init_tool.status.success());

    let test_tool = run_xcpc(
        None,
        data_dir.path(),
        &[
            "plugin",
            "test",
            tool_dir.to_str().unwrap(),
            "runTool",
            "--args",
            r#"["hello", {"text":"XCPC"}]"#,
            "--json",
        ],
    );
    assert!(
        test_tool.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&test_tool.stderr)
    );
    let test_tool_json = parse_json_stdout(&test_tool);
    assert_eq!(test_tool_json["ok"], true);
    assert_eq!(test_tool_json["plugin_id"], "my-tool");
    assert_eq!(
        test_tool_json["result"]["blocks"][0]["content"],
        "Hello, XCPC!"
    );
}

#[test]
fn plugin_disable_enable_export_install_and_repo() {
    let data_dir = tempfile::tempdir().unwrap();
    let tmp = tempfile::tempdir().unwrap();

    // Disable codeforces
    let dis = run_xcpc(
        None,
        data_dir.path(),
        &["plugin", "disable", "codeforces", "--json"],
    );
    assert!(dis.status.success());
    assert_eq!(parse_json_stdout(&dis)["enabled"], false);

    let list1 = parse_json_stdout(&run_xcpc(
        None,
        data_dir.path(),
        &["plugin", "list", "--json"],
    ));
    let cf1 = list1["plugins"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == "codeforces")
        .unwrap();
    assert_eq!(cf1["enabled"], false);

    // Re-enable codeforces
    let en = run_xcpc(
        None,
        data_dir.path(),
        &["plugin", "enable", "codeforces", "--json"],
    );
    assert!(en.status.success());
    assert_eq!(parse_json_stdout(&en)["enabled"], true);

    // Export built-in codeforces -> becomes local_override
    let exp = run_xcpc(
        None,
        data_dir.path(),
        &["plugin", "export", "codeforces", "--json"],
    );
    assert!(exp.status.success());
    let exp_json = parse_json_stdout(&exp);
    assert_eq!(exp_json["ok"], true);
    let exported_path = Path::new(exp_json["path"].as_str().unwrap());
    assert!(exported_path.join("manifest.toml").is_file());
    assert!(exported_path.join("index.js").is_file());

    let list2 = parse_json_stdout(&run_xcpc(
        None,
        data_dir.path(),
        &["plugin", "list", "--json"],
    ));
    let cf2 = list2["plugins"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == "codeforces")
        .unwrap();
    assert_eq!(cf2["origin"], "local_override");

    // Install from local directory
    let custom_dir = tmp.path().join("custom-demo");
    assert!(
        run_xcpc(
            None,
            data_dir.path(),
            &[
                "plugin",
                "init",
                custom_dir.to_str().unwrap(),
                "--kind",
                "tool"
            ],
        )
        .status
        .success()
    );
    let inst_dir = run_xcpc(
        None,
        data_dir.path(),
        &["plugin", "install", custom_dir.to_str().unwrap(), "--json"],
    );
    assert!(
        inst_dir.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&inst_dir.stderr)
    );
    let inst_dir_json = parse_json_stdout(&inst_dir);
    assert_eq!(inst_dir_json["ok"], true);
    assert_eq!(inst_dir_json["plugin"]["id"], "custom-demo");
    assert_eq!(inst_dir_json["plugin"]["origin"], "local");

    // Install from valid .zip archive
    let zip_path = tmp.path().join("zipped-plugin.zip");
    {
        let file = fs::File::create(&zip_path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let opts = SimpleFileOptions::default();
        zip.start_file("manifest.toml", opts).unwrap();
        zip.write_all(
            br#"id = "zip-demo"
name = "Zip Demo"
version = "0.2.0"
entry = "index.js"
capabilities = ["tool"]
"#,
        )
        .unwrap();
        zip.start_file("index.js", opts).unwrap();
        zip.write_all(b"module.exports = { runTool: () => ({ blocks: [] }) };\n")
            .unwrap();
        zip.finish().unwrap();
    }
    let inst_zip = run_xcpc(
        None,
        data_dir.path(),
        &["plugin", "install", zip_path.to_str().unwrap(), "--json"],
    );
    assert!(inst_zip.status.success());
    let inst_zip_json = parse_json_stdout(&inst_zip);
    assert_eq!(inst_zip_json["ok"], true);
    assert_eq!(inst_zip_json["plugin"]["id"], "zip-demo");

    // Reject malicious Zip Slip archive
    let evil_zip = tmp.path().join("evil.zip");
    {
        let file = fs::File::create(&evil_zip).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let opts = SimpleFileOptions::default();
        zip.start_file("../escape.txt", opts).unwrap();
        zip.write_all(b"nope").unwrap();
        zip.finish().unwrap();
    }
    let inst_evil = run_xcpc(
        None,
        data_dir.path(),
        &["plugin", "install", evil_zip.to_str().unwrap(), "--json"],
    );
    assert!(!inst_evil.status.success());
    let inst_evil_json = parse_json_stdout(&inst_evil);
    assert_eq!(inst_evil_json["ok"], false);
    assert_eq!(inst_evil_json["error"]["code"], "ZIP_SLIP");

    // Repo list / add / remove
    let repo_url = "https://example.org/xcpc-plugins/index.json";
    let r_add = run_xcpc(
        None,
        data_dir.path(),
        &[
            "plugin",
            "repo",
            "add",
            repo_url,
            "--name",
            "Community",
            "--json",
        ],
    );
    assert!(r_add.status.success());
    assert_eq!(parse_json_stdout(&r_add)["ok"], true);

    let r_list = parse_json_stdout(&run_xcpc(
        None,
        data_dir.path(),
        &["plugin", "repo", "list", "--json"],
    ));
    let repos = r_list["repos"].as_array().unwrap();
    // Default official repo + newly added community repo.
    assert_eq!(repos.len(), 2);
    assert!(
        repos
            .iter()
            .any(|r| r["url"] == repo_url && r["name"] == "Community")
    );

    let r_rm = parse_json_stdout(&run_xcpc(
        None,
        data_dir.path(),
        &["plugin", "repo", "remove", repo_url, "--json"],
    ));
    assert_eq!(r_rm["ok"], true);
    assert_eq!(r_rm["removed"], true);

    let r_list2 = parse_json_stdout(&run_xcpc(
        None,
        data_dir.path(),
        &["plugin", "repo", "list", "--json"],
    ));
    assert_eq!(r_list2["repos"].as_array().unwrap().len(), 1);
}

#[test]
fn training_status_appends_logs_derives_status_and_handles_clear_and_readonly() {
    let ws = setup_workspace();
    let data_dir = tempfile::tempdir().unwrap();

    // Mark p1 as todo
    let out1 = run_xcpc(
        Some(ws.path()),
        data_dir.path(),
        &["training", "status", "p1", "todo", "--json"],
    );
    assert!(
        out1.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out1.stderr)
    );
    let j1 = parse_json_stdout(&out1);
    assert_eq!(j1["ok"], true);
    assert_eq!(j1["problem_id"], "p1");
    assert_eq!(j1["recorded"], "todo");
    assert_eq!(j1["status"], "todo");
    assert!(ws.path().join("training/logs.jsonl").is_file());

    // Mark p1 as upsolved
    let j2 = parse_json_stdout(&run_xcpc(
        Some(ws.path()),
        data_dir.path(),
        &["training", "status", "p1", "upsolved", "--json"],
    ));
    assert_eq!(j2["status"], "upsolved");

    // Clear manual override -> status returns to none (no synced DB yet)
    let j3 = parse_json_stdout(&run_xcpc(
        Some(ws.path()),
        data_dir.path(),
        &["training", "status", "p1", "clear", "--json"],
    ));
    assert_eq!(j3["recorded"], "clear");
    assert_eq!(j3["status"], "none");

    // Seed an AC submission in oj_sync.db for codeforces 1999A
    {
        let mut db = xcpc_sync::SyncDb::open(data_dir.path()).unwrap();
        db.upsert_submissions(
            "codeforces",
            &[xcpc_plugin::OjSubmission {
                submission_id: "42".into(),
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
                language: Some("GNU C++20".into()),
                submitted_at: 1_732_406_400,
            }],
            "2026-11-24T00:00:00Z",
        )
        .unwrap();
    }

    // With external AC, recording `todo` still derives `ac`
    let j4 = parse_json_stdout(&run_xcpc(
        Some(ws.path()),
        data_dir.path(),
        &["training", "status", "p1", "todo", "--json"],
    ));
    assert_eq!(j4["synced"]["ac"], true);
    assert_eq!(j4["status"], "ac");

    // With external AC, recording `upsolved` derives `upsolved`
    let j5 = parse_json_stdout(&run_xcpc(
        Some(ws.path()),
        data_dir.path(),
        &["training", "status", "p1", "upsolved", "--json"],
    ));
    assert_eq!(j5["status"], "upsolved");

    // Read-only workspace refuses write
    let ro = run_xcpc(
        Some(ws.path()),
        data_dir.path(),
        &["--read-only", "training", "status", "p1", "ac", "--json"],
    );
    assert_eq!(ro.status.code(), Some(2));
    assert_eq!(parse_json_stdout(&ro)["error"]["code"], "read_only");

    // Non-existent problem returns not_found
    let missing = run_xcpc(
        Some(ws.path()),
        data_dir.path(),
        &["training", "status", "no-such-prob", "todo", "--json"],
    );
    assert_eq!(missing.status.code(), Some(2));
    assert_eq!(parse_json_stdout(&missing)["error"]["code"], "not_found");
}

#[test]
fn problem_from_url_parses_cf_and_atcoder_enriches_and_deduplicates() {
    let ws = tempfile::tempdir().unwrap();
    xcpc_storage::init_workspace(ws.path()).unwrap();
    let data_dir = tempfile::tempdir().unwrap();

    // Seed oj_problem_meta in oj_sync.db to verify enrichment
    {
        let mut db = xcpc_sync::SyncDb::open(data_dir.path()).unwrap();
        db.upsert_problem_meta(&xcpc_sync::CachedProblemMeta {
            platform: "codeforces".into(),
            problem_key: "1999A".into(),
            title: Some("A+B Again?".into()),
            url: Some("https://codeforces.com/contest/1999/problem/A".into()),
            raw_rating: Some(800),
            normalized_rating: Some(800),
            tags: vec!["implementation".into(), "math".into()],
            updated_at: "2026-11-24T00:00:00Z".into(),
        })
        .unwrap();
    }

    // 1. Create Codeforces problem from URL
    let cf_url = "https://codeforces.com/contest/1999/problem/A";
    let out1 = run_xcpc(
        Some(ws.path()),
        data_dir.path(),
        &["problem", "from-url", cf_url, "--json"],
    );
    assert!(
        out1.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out1.stderr)
    );
    let j1 = parse_json_stdout(&out1);
    assert_eq!(j1["ok"], true);
    assert_eq!(j1["already_exists"], false);
    assert_eq!(j1["id"], "cf-1999a");
    assert_eq!(j1["title"], "A+B Again?");
    assert_eq!(j1["rating"], 800);
    assert_eq!(j1["platform"], "codeforces");
    assert_eq!(j1["problem_key"], "1999A");
    assert_eq!(j1["sources"][0]["platform"], "codeforces");
    assert_eq!(j1["sources"][0]["id"], "1999A");
    assert!(ws.path().join("problems/cf-1999a/problem.toml").is_file());

    // 2. Second call with equivalent problemset URL deduplicates cleanly
    let cf_alt_url = "https://codeforces.com/problemset/problem/1999/A";
    let out2 = run_xcpc(
        Some(ws.path()),
        data_dir.path(),
        &["problem", "from-url", cf_alt_url, "--json"],
    );
    assert!(out2.status.success());
    let j2 = parse_json_stdout(&out2);
    assert_eq!(j2["ok"], true);
    assert_eq!(j2["already_exists"], true);
    assert_eq!(j2["id"], "cf-1999a");

    // 3. Create AtCoder problem with custom --id and --title
    let ac_url = "https://atcoder.jp/contests/abc350/tasks/abc350_d";
    let out3 = run_xcpc(
        Some(ws.path()),
        data_dir.path(),
        &[
            "problem",
            "from-url",
            ac_url,
            "--id",
            "my-abc350d",
            "--title",
            "New Friends",
            "--json",
        ],
    );
    assert!(
        out3.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out3.stderr)
    );
    let j3 = parse_json_stdout(&out3);
    assert_eq!(j3["ok"], true);
    assert_eq!(j3["already_exists"], false);
    assert_eq!(j3["id"], "my-abc350d");
    assert_eq!(j3["title"], "New Friends");
    assert_eq!(j3["platform"], "atcoder");
    assert_eq!(j3["problem_key"], "abc350_d");

    // Deduplicate AtCoder on second call (even without --id)
    let j4 = parse_json_stdout(&run_xcpc(
        Some(ws.path()),
        data_dir.path(),
        &["problem", "from-url", ac_url, "--json"],
    ));
    assert_eq!(j4["already_exists"], true);
    assert_eq!(j4["id"], "my-abc350d");

    // Unsupported URL returns exit code 2
    let bad = run_xcpc(
        Some(ws.path()),
        data_dir.path(),
        &[
            "problem",
            "from-url",
            "https://example.invalid/p/1",
            "--json",
        ],
    );
    assert_eq!(bad.status.code(), Some(2));
    assert_eq!(parse_json_stdout(&bad)["error"]["code"], "unsupported_url");
}

#[test]
fn sync_handle_get_set_clear_and_sync_without_handle() {
    let data_dir = tempfile::tempdir().unwrap();

    // Initially unset
    let g0 = parse_json_stdout(&run_xcpc(
        None,
        data_dir.path(),
        &["sync", "handle", "codeforces", "--json"],
    ));
    assert_eq!(g0["ok"], true);
    assert_eq!(g0["platform"], "codeforces");
    assert!(g0["handle"].is_null());

    // Set handle
    let s1 = parse_json_stdout(&run_xcpc(
        None,
        data_dir.path(),
        &["sync", "handle", "codeforces", "tourist", "--json"],
    ));
    assert_eq!(s1["ok"], true);
    assert_eq!(s1["handle"], "tourist");

    // Get handle
    let g1 = parse_json_stdout(&run_xcpc(
        None,
        data_dir.path(),
        &["sync", "handle", "codeforces", "--json"],
    ));
    assert_eq!(g1["handle"], "tourist");

    // Clear handle
    let c1 = parse_json_stdout(&run_xcpc(
        None,
        data_dir.path(),
        &["sync", "handle", "codeforces", "--clear", "--json"],
    ));
    assert_eq!(c1["ok"], true);
    assert!(c1["handle"].is_null());

    // Sync for codeforces with no handle configured reports platform error cleanly
    let sync_out = run_xcpc(
        None,
        data_dir.path(),
        &["sync", "--platform", "codeforces", "--json"],
    );
    assert_eq!(sync_out.status.code(), Some(1));
    let sj = parse_json_stdout(&sync_out);
    assert_eq!(sj["ok"], false);
    assert_eq!(sj["platforms"][0]["platform"], "codeforces");
    assert_eq!(sj["platforms"][0]["ok"], false);
}

// ---------------------------------------------------------------------------
// Phase 4B: CLI (dashboard / contests / submissions) & MCP stdio E2E tests
// ---------------------------------------------------------------------------

fn seed_phase4b_sync_db(data_dir: &Path) {
    let mut cfg = xcpc_sync::SyncConfig::default();
    cfg.set_handle("codeforces", "tourist");
    cfg.set_handle("atcoder", "tourist_ac");
    cfg.last_synced_at = Some("2026-11-24T12:00:00Z".into());
    cfg.save(data_dir).unwrap();

    let mut db = xcpc_sync::SyncDb::open(data_dir).unwrap();
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let day = 86_400i64;

    db.upsert_account(
        "codeforces",
        &xcpc_plugin::OjAccountSync {
            handle: "tourist".into(),
            rating: Some(3800),
            max_rating: Some(3979),
            rank: Some("legendary grandmaster".into()),
            rating_history: vec![xcpc_plugin::OjRatingChange {
                contest_id: "1990".into(),
                contest_name: "Codeforces Round 960".into(),
                rating_before: 3750,
                rating_after: 3800,
                rank: Some(1),
                at: now_secs - 10 * day,
            }],
        },
        "2026-11-24T12:00:00Z",
    )
    .unwrap();

    db.upsert_submissions(
        "codeforces",
        &[
            xcpc_plugin::OjSubmission {
                submission_id: "101".into(),
                handle: "tourist".into(),
                contest_id: Some("1999".into()),
                problem_index: Some("A".into()),
                problem_key: "1999A".into(),
                problem_title: "A+B Again?".into(),
                problem_url: Some("https://codeforces.com/contest/1999/problem/A".into()),
                raw_rating: Some(1600.0),
                normalized_rating: Some(1600),
                tags: vec!["dp".into(), "math".into()],
                verdict: "AC".into(),
                language: Some("GNU C++20".into()),
                submitted_at: now_secs - 2 * day,
            },
            xcpc_plugin::OjSubmission {
                submission_id: "102".into(),
                handle: "tourist".into(),
                contest_id: Some("2000".into()),
                problem_index: Some("D".into()),
                problem_key: "2000D".into(),
                problem_title: "Hard Graph".into(),
                problem_url: Some("https://codeforces.com/contest/2000/problem/D".into()),
                raw_rating: Some(2100.0),
                normalized_rating: Some(2100),
                tags: vec!["graphs".into(), "dp".into()],
                verdict: "WA".into(),
                language: Some("GNU C++20".into()),
                submitted_at: now_secs - day,
            },
            xcpc_plugin::OjSubmission {
                submission_id: "103".into(),
                handle: "tourist".into(),
                contest_id: Some("2000".into()),
                problem_index: Some("D".into()),
                problem_key: "2000D".into(),
                problem_title: "Hard Graph".into(),
                problem_url: Some("https://codeforces.com/contest/2000/problem/D".into()),
                raw_rating: Some(2100.0),
                normalized_rating: Some(2100),
                tags: vec!["graphs".into(), "dp".into()],
                verdict: "TLE".into(),
                language: Some("GNU C++20".into()),
                submitted_at: now_secs - 3_600,
            },
        ],
        "2026-11-24T12:00:00Z",
    )
    .unwrap();

    db.upsert_submissions(
        "atcoder",
        &[xcpc_plugin::OjSubmission {
            submission_id: "201".into(),
            handle: "tourist_ac".into(),
            contest_id: Some("abc350".into()),
            problem_index: Some("D".into()),
            problem_key: "abc350_d".into(),
            problem_title: "New Friends".into(),
            problem_url: Some("https://atcoder.jp/contests/abc350/tasks/abc350_d".into()),
            raw_rating: Some(1950.0),
            normalized_rating: Some(1950),
            tags: vec!["dsu".into()],
            verdict: "AC".into(),
            language: Some("C++20".into()),
            submitted_at: now_secs - 1_800,
        }],
        "2026-11-24T12:00:00Z",
    )
    .unwrap();

    db.upsert_contests(
        "codeforces",
        &[
            xcpc_plugin::OjContest {
                contest_id: "1900".into(),
                name: "Old CF Round".into(),
                start_time: now_secs - 10 * day,
                duration_sec: 7200,
                format: Some("CF".into()),
                url: Some("https://codeforces.com/contest/1900".into()),
            },
            xcpc_plugin::OjContest {
                contest_id: "2050".into(),
                name: "Upcoming CF Round".into(),
                start_time: now_secs + 3 * day,
                duration_sec: 7200,
                format: Some("CF".into()),
                url: Some("https://codeforces.com/contest/2050".into()),
            },
        ],
        "2026-11-24T12:00:00Z",
    )
    .unwrap();

    db.upsert_contests(
        "atcoder",
        &[xcpc_plugin::OjContest {
            contest_id: "abc400".into(),
            name: "AtCoder Beginner Contest 400".into(),
            start_time: now_secs + 5 * day,
            duration_sec: 6000,
            format: Some("ICPC".into()),
            url: Some("https://atcoder.jp/contests/abc400".into()),
        }],
        "2026-11-24T12:00:00Z",
    )
    .unwrap();
}

#[test]
fn training_dashboard_contests_and_submissions_cli() {
    let ws = setup_workspace();
    let data_dir = tempfile::tempdir().unwrap();
    seed_phase4b_sync_db(data_dir.path());

    // 1. `xcpc training dashboard --json`
    let dash_out = run_xcpc(
        Some(ws.path()),
        data_dir.path(),
        &[
            "training",
            "dashboard",
            "--window",
            "30d",
            "--min-rating",
            "1900",
            "--json",
        ],
    );
    assert!(
        dash_out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&dash_out.stderr)
    );
    let dj = parse_json_stdout(&dash_out);
    assert_eq!(dj["ok"], true);
    assert_eq!(dj["window"], "30d");
    assert_eq!(dj["kpi"]["total_ac"], 2);
    assert_eq!(dj["kpi"]["unsolved_attempted_count"], 1);
    assert_eq!(dj["unsolved_problems"].as_array().unwrap().len(), 1);
    assert_eq!(dj["unsolved_problems"][0]["problem_key"], "2000D");
    assert_eq!(dj["unsolved_problems"][0]["attempt_count"], 2);
    // Tag filter `--min-rating 1900` only keeps abc350_d (1950 -> "dsu")
    let tags = dj["tag_distribution"].as_array().unwrap();
    assert_eq!(tags.len(), 1);
    assert_eq!(tags[0]["tag"], "dsu");

    // Human-readable smoke check
    let dash_human = run_xcpc(Some(ws.path()), data_dir.path(), &["training", "dashboard"]);
    assert!(dash_human.status.success());
    let human_txt = String::from_utf8(dash_human.stdout).unwrap();
    assert!(human_txt.contains("Training Dashboard"));
    assert!(human_txt.contains("tourist"));

    // 2. `xcpc training contests --json` (excludes finished by default)
    let c_out = run_xcpc(
        Some(ws.path()),
        data_dir.path(),
        &["training", "contests", "--json"],
    );
    assert!(c_out.status.success());
    let cj = parse_json_stdout(&c_out);
    assert_eq!(cj["ok"], true);
    assert_eq!(cj["count"], 2);
    let c_ids: Vec<&str> = cj["contests"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["contest_id"].as_str().unwrap())
        .collect();
    assert_eq!(c_ids, vec!["2050", "abc400"]);

    // `--all` includes finished, `--platform codeforces` filters
    let c_all = parse_json_stdout(&run_xcpc(
        Some(ws.path()),
        data_dir.path(),
        &[
            "training",
            "contests",
            "--platform",
            "codeforces",
            "--all",
            "--json",
        ],
    ));
    assert_eq!(c_all["count"], 2);
    assert_eq!(c_all["contests"][0]["contest_id"], "1900");
    assert_eq!(c_all["contests"][0]["status"], "finished");

    // 3. `xcpc training submissions --by-problem --unsolved --json`
    let s_unsolved = run_xcpc(
        Some(ws.path()),
        data_dir.path(),
        &[
            "training",
            "submissions",
            "--by-problem",
            "--unsolved",
            "--json",
        ],
    );
    assert!(
        s_unsolved.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&s_unsolved.stderr)
    );
    let sj = parse_json_stdout(&s_unsolved);
    assert_eq!(sj["ok"], true);
    assert_eq!(sj["total"], 1);
    assert_eq!(sj["items"][0]["problem_key"], "2000D");
    assert_eq!(sj["items"][0]["attempt_count"], 2);
    assert_eq!(sj["items"][0]["solved"], false);

    // Check `library_problem_id` annotation for problem `p1` (codeforces 1999A)
    let s_cf = parse_json_stdout(&run_xcpc(
        Some(ws.path()),
        data_dir.path(),
        &[
            "training",
            "submissions",
            "--platform",
            "codeforces",
            "--verdict",
            "AC",
            "--json",
        ],
    ));
    assert_eq!(s_cf["total"], 1);
    assert_eq!(s_cf["items"][0]["problem_key"], "1999A");
    assert_eq!(s_cf["items"][0]["library_problem_id"], "p1");
}

struct Phase4McpClient {
    child: std::process::Child,
    stdin: std::process::ChildStdin,
    stdout: std::io::BufReader<std::process::ChildStdout>,
    next_id: u64,
}

impl Phase4McpClient {
    fn spawn(workspace: &Path, data_dir: &Path, read_only: bool) -> Self {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_xcpc"));
        cmd.env("XCPC_DATA_DIR", data_dir);
        cmd.arg("-w").arg(workspace);
        if read_only {
            cmd.arg("--read-only");
        }
        cmd.arg("mcp")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        let mut child = cmd.spawn().expect("spawn xcpc mcp");
        let stdin = child.stdin.take().unwrap();
        let stdout = std::io::BufReader::new(child.stdout.take().unwrap());
        Self {
            child,
            stdin,
            stdout,
            next_id: 1,
        }
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        use std::io::BufRead as _;
        let id = self.next_id;
        self.next_id += 1;
        let msg = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        writeln!(self.stdin, "{msg}").unwrap();
        self.stdin.flush().unwrap();
        let mut line = String::new();
        let n = self.stdout.read_line(&mut line).unwrap();
        assert!(n > 0, "xcpc mcp closed stdout unexpectedly");
        let reply: Value = serde_json::from_str(&line).expect("valid JSON-RPC reply");
        assert_eq!(reply["id"], id);
        reply
    }

    fn call_tool(&mut self, name: &str, arguments: Value) -> Value {
        let reply = self.request(
            "tools/call",
            serde_json::json!({ "name": name, "arguments": arguments }),
        );
        assert!(
            reply.get("error").is_none(),
            "unexpected JSON-RPC error for {name}: {reply}"
        );
        reply["result"].clone()
    }
}

impl Drop for Phase4McpClient {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn mcp_err_code(result: &Value) -> String {
    assert_eq!(result["isError"], true, "expected isError=true: {result}");
    let text = result["content"][0]["text"].as_str().unwrap();
    let parsed: Value = serde_json::from_str(text).expect("error content JSON");
    parsed["code"].as_str().unwrap().to_owned()
}

#[test]
fn mcp_training_tools_end_to_end_and_read_only_guard() {
    let ws = setup_workspace();
    let data_dir = tempfile::tempdir().unwrap();
    seed_phase4b_sync_db(data_dir.path());

    // 1. Read-write MCP client
    let mut client = Phase4McpClient::spawn(ws.path(), data_dir.path(), false);

    let list = client.request("tools/list", serde_json::json!({}));
    let tools = list["result"]["tools"].as_array().unwrap();
    let find_tool = |name: &str| {
        tools
            .iter()
            .find(|t| t["name"] == name)
            .unwrap_or_else(|| panic!("missing tool {name}"))
    };
    assert_eq!(
        find_tool("get_training_dashboard")["annotations"]["readOnlyHint"],
        true
    );
    assert_eq!(
        find_tool("set_training_status")["annotations"]["readOnlyHint"],
        false
    );
    assert_eq!(
        find_tool("ingest_problem")["annotations"]["readOnlyHint"],
        false
    );

    // 2. `get_training_dashboard` returns full analytics
    let dash = client.call_tool(
        "get_training_dashboard",
        serde_json::json!({ "window": "all" }),
    );
    assert_ne!(dash["isError"], true);
    let sc = &dash["structuredContent"];
    assert_eq!(sc["ok"], true);
    assert_eq!(sc["kpi"]["total_ac"], 2);
    assert_eq!(sc["kpi"]["unsolved_attempted_count"], 1);
    assert_eq!(sc["unsolved_problems"][0]["problem_key"], "2000D");
    assert!(!sc["difficulty_trend"].as_array().unwrap().is_empty());

    // 3. `set_training_status` appends logs.jsonl and returns derived status
    let st = client.call_tool(
        "set_training_status",
        serde_json::json!({
            "problem_id": "p1",
            "status": "upsolved",
        }),
    );
    assert_ne!(st["isError"], true);
    assert_eq!(st["structuredContent"]["ok"], true);
    assert_eq!(st["structuredContent"]["recorded"], "upsolved");
    assert_eq!(st["structuredContent"]["status"], "upsolved");
    assert_eq!(st["structuredContent"]["synced"]["ac"], true);

    // 4. `ingest_problem` from synced `(platform, problem_key)` enriches & creates, then deduplicates
    let ing1 = client.call_tool(
        "ingest_problem",
        serde_json::json!({
            "platform": "codeforces",
            "problem_key": "2000D",
            "initial_status": "todo",
        }),
    );
    assert_ne!(ing1["isError"], true);
    let i1 = &ing1["structuredContent"];
    assert_eq!(i1["ok"], true);
    assert_eq!(i1["created"], true);
    assert_eq!(i1["problem_id"], "cf-2000d");
    assert_eq!(i1["title"], "Hard Graph");
    assert_eq!(i1["rating"], 2100);
    assert!(ws.path().join("problems/cf-2000d/problem.toml").is_file());

    // Second call with URL deduplicates cleanly (`created: false`)
    let ing2 = client.call_tool(
        "ingest_problem",
        serde_json::json!({
            "url": "https://codeforces.com/contest/2000/problem/D",
        }),
    );
    assert_ne!(ing2["isError"], true);
    assert_eq!(ing2["structuredContent"]["created"], false);
    assert_eq!(ing2["structuredContent"]["problem_id"], "cf-2000d");

    // Dashboard now reflects `cf-2000d` in `workspace_backlog` and `library_problem_id`
    let dash2 = client.call_tool("get_training_dashboard", serde_json::json!({}));
    let sc2 = &dash2["structuredContent"];
    assert_eq!(
        sc2["unsolved_problems"][0]["library_problem_id"],
        "cf-2000d"
    );
    assert!(
        sc2["workspace_backlog"]
            .as_array()
            .unwrap()
            .iter()
            .any(|b| b["problem_id"] == "cf-2000d")
    );

    drop(client);

    // 5. Read-only MCP client blocks writes but allows `get_training_dashboard`
    let mut ro = Phase4McpClient::spawn(ws.path(), data_dir.path(), true);

    let ro_dash = ro.call_tool("get_training_dashboard", serde_json::json!({}));
    assert_ne!(ro_dash["isError"], true);
    assert_eq!(ro_dash["structuredContent"]["ok"], true);

    let ro_set = ro.call_tool(
        "set_training_status",
        serde_json::json!({ "problem_id": "p1", "status": "todo" }),
    );
    assert_eq!(mcp_err_code(&ro_set), "read_only");

    let ro_ing = ro.call_tool(
        "ingest_problem",
        serde_json::json!({ "platform": "atcoder", "problem_key": "abc350_d" }),
    );
    assert_eq!(mcp_err_code(&ro_ing), "read_only");
}

// ---------------------------------------------------------------------------
// Phase 4C: CLI `xcpc tool`, `xcpc import <plugin>:<importer>`,
// and `xcpc export <plugin>:<exporter>` E2E tests
// ---------------------------------------------------------------------------

#[test]
fn tool_list_includes_four_official_builtin_plugins() {
    let data_dir = tempfile::tempdir().unwrap();
    let out = run_xcpc(None, data_dir.path(), &["tool", "list", "--json"]);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json = parse_json_stdout(&out);
    assert_eq!(json["ok"], true);

    // Phase 4D: main repo has zero built-in tool plugins (tools live in external official repo).
    let plugins = json["plugins"].as_array().expect("plugins array");
    assert!(
        plugins.is_empty(),
        "expected no built-in tools, got {plugins:?}"
    );

    // Installing a local tool plugin makes it immediately discoverable by `xcpc tool list`.
    let tmp = tempfile::tempdir().unwrap();
    let local_tool = tmp.path().join("demo-tool");
    let init = run_xcpc(
        None,
        data_dir.path(),
        &[
            "plugin",
            "init",
            local_tool.to_str().unwrap(),
            "--kind",
            "tool",
            "--json",
        ],
    );
    assert!(init.status.success());
    let inst = run_xcpc(
        None,
        data_dir.path(),
        &["plugin", "install", local_tool.to_str().unwrap(), "--json"],
    );
    assert!(inst.status.success());

    let listed = parse_json_stdout(&run_xcpc(
        None,
        data_dir.path(),
        &["tool", "list", "--json"],
    ));
    let installed_plugins = listed["plugins"].as_array().unwrap();
    assert_eq!(installed_plugins.len(), 1);
    assert_eq!(installed_plugins[0]["plugin_id"], "demo-tool");

    let human = run_xcpc(None, data_dir.path(), &["tool", "list"]);
    assert!(human.status.success());
    let stdout = String::from_utf8_lossy(&human.stdout);
    assert!(stdout.contains("demo-tool:hello"), "{stdout}");
}

#[test]
fn tool_run_executes_installed_tool_plugin() {
    let data_dir = tempfile::tempdir().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let local_tool = tmp.path().join("echo-tool");
    assert!(
        run_xcpc(
            None,
            data_dir.path(),
            &[
                "plugin",
                "init",
                local_tool.to_str().unwrap(),
                "--kind",
                "tool"
            ],
        )
        .status
        .success()
    );
    assert!(
        run_xcpc(
            None,
            data_dir.path(),
            &["plugin", "install", local_tool.to_str().unwrap()],
        )
        .status
        .success()
    );

    let out = run_xcpc(
        None,
        data_dir.path(),
        &[
            "tool",
            "run",
            "echo-tool:hello",
            "--input",
            "text=XCPC",
            "--json",
        ],
    );
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let j = parse_json_stdout(&out);
    assert_eq!(j["ok"], true);
    assert_eq!(j["plugin_id"], "echo-tool");
    assert_eq!(j["tool_id"], "hello");
    assert_eq!(j["blocks"][0]["content"], "Hello, XCPC!");
}

#[test]
fn tool_run_custom_headless_svg_and_workspace_read_plugin() {
    let ws = setup_workspace();
    let data_dir = tempfile::tempdir().unwrap();
    let tmp = tempfile::tempdir().unwrap();

    let plugin_dir = tmp.path().join("svg-badge");
    fs::create_dir_all(&plugin_dir).unwrap();
    fs::write(
        plugin_dir.join("manifest.toml"),
        r#"id = "svg-badge"
name = "SVG Badge Tool"
version = "0.1.0"
description = "Generates an SVG badge and reads workspace problems."
entry = "index.js"
capabilities = ["tool"]

[permissions]
domains = []
workspace_read = true

[[tools]]
id = "render"
name = "Render Badge"

[[tools.inputs]]
name = "label"
label = "Label"
kind = "string"
required = true
"#,
    )
    .unwrap();
    fs::write(
        plugin_dir.join("index.js"),
        r#"module.exports = {
  runTool(toolId, inputs) {
    const label = String((inputs && inputs.label) || "XCPC");
    const count = inputs && inputs.include_count ? host.workspace.listProblems().length : 0;
    const text = count > 0 ? label + " (" + count + ")" : label;
    const svg = '<svg xmlns="http://www.w3.org/2000/svg" width="120" height="32"><text x="8" y="20">' + text + '</text></svg>';
    return {
      summary: "rendered " + text,
      blocks: [
        { type: "svg", title: "Badge", svg, width: 120, height: 32 },
        { type: "text", title: "Meta", content: "tool=" + toolId + ", count=" + count }
      ]
    };
  }
};
"#,
    )
    .unwrap();

    let inst = run_xcpc(
        None,
        data_dir.path(),
        &["plugin", "install", plugin_dir.to_str().unwrap(), "--json"],
    );
    assert!(
        inst.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&inst.stderr)
    );

    let out = run_xcpc(
        Some(ws.path()),
        data_dir.path(),
        &[
            "tool",
            "run",
            "svg-badge:render",
            "--input",
            "label=Hello",
            "--input",
            "include_count=true",
            "--json",
        ],
    );
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let j = parse_json_stdout(&out);
    assert_eq!(j["ok"], true);
    assert_eq!(j["summary"], "rendered Hello (1)");
    assert_eq!(j["blocks"][0]["type"], "svg");
    assert!(
        j["blocks"][0]["svg"]
            .as_str()
            .unwrap()
            .contains("Hello (1)")
    );
    assert_eq!(j["blocks"][1]["content"], "tool=render, count=1");
}

#[test]
fn plugin_importer_and_exporter_e2e_and_path_traversal_rejection() {
    let ws = setup_workspace();
    let data_dir = tempfile::tempdir().unwrap();
    let tmp = tempfile::tempdir().unwrap();

    // Install a custom plugin declaring both `importer` and `exporter`.
    let plugin_dir = tmp.path().join("custom-io");
    fs::create_dir_all(&plugin_dir).unwrap();
    fs::write(
        plugin_dir.join("manifest.toml"),
        r#"id = "custom-io"
name = "Custom IO"
version = "0.1.0"
description = "Custom importer and exporter for CLI E2E testing."
entry = "index.js"
capabilities = ["importer", "exporter"]

[permissions]
domains = []
workspace_read = false

[[importers]]
id = "simple"
name = "Simple Text Importer"
extensions = [".txt"]

[[importers]]
id = "evil-asset"
name = "Evil Traversal Importer"
extensions = [".txt"]

[[exporters]]
id = "bundle"
name = "Simple Bundle Exporter"
target = "problem"
extension = ".txt"

[[exporters]]
id = "evil-out"
name = "Evil Traversal Exporter"
target = "problem"
extension = ".txt"
"#,
    )
    .unwrap();
    fs::write(
        plugin_dir.join("index.js"),
        r#"module.exports = {
  importPackage(files, importerId) {
    if (importerId === "evil-asset") {
      return {
        kind: "problem",
        id: "evil-prob",
        title: "Evil Problem",
        statement_md: "Evil.\n",
        assets: { "../escape.txt": "aGk=" }
      };
    }
    const f = files[0];
    const lines = String(f.text || "").split(/\r?\n/);
    return {
      kind: "problem",
      id: "imported-demo",
      title: lines[0] || "Imported Demo",
      statement_md: (lines.slice(1).join("\n") || "Body") + "\n\n## 输入格式\n\nIn.\n\n## 输出格式\n\nOut.\n",
      tags: ["imported", "dp"],
      rating: 1700,
      time_limit: "2s",
      memory_limit: "512MB",
      samples: [{ input: "1 2\n", output: "3\n" }]
    };
  },
  exportPackage(ctx, exporterId) {
    if (exporterId === "evil-out") {
      return {
        files: [{ path: "../escape.txt", text: "pwned" }],
        summary: "attempted escape"
      };
    }
    const files = [
      {
        path: ctx.id + "/meta.json",
        text: JSON.stringify({
          kind: ctx.kind,
          id: ctx.id,
          title: ctx.title,
          has_pdf: Boolean(ctx.pdf_base64 && ctx.pdf_base64.length > 0),
          samples: (ctx.samples || []).length
        })
      },
      {
        path: ctx.id + "/statement.md",
        text: ctx.statement_md || ""
      }
    ];
    if (ctx.pdf_base64) {
      files.push({
        path: ctx.id + "/statement.pdf",
        content_base64: ctx.pdf_base64
      });
    }
    return {
      files,
      summary: "exported " + ctx.id
    };
  }
};
"#,
    )
    .unwrap();

    let inst = run_xcpc(
        None,
        data_dir.path(),
        &["plugin", "install", plugin_dir.to_str().unwrap(), "--json"],
    );
    assert!(
        inst.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&inst.stderr)
    );

    // 1. E2E `xcpc import custom-io:simple <file> --json`
    let src_file = tmp.path().join("problem.txt");
    fs::write(&src_file, "Imported Sum\nGiven a and b, print a + b.").unwrap();

    let imp = run_xcpc(
        Some(ws.path()),
        data_dir.path(),
        &[
            "import",
            "custom-io:simple",
            src_file.to_str().unwrap(),
            "--json",
        ],
    );
    assert!(
        imp.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&imp.stderr)
    );
    let imp_json = parse_json_stdout(&imp);
    assert_eq!(imp_json["ok"], true);
    assert_eq!(imp_json["kind"], "problem");
    assert_eq!(
        imp_json["created_ids"],
        serde_json::json!(["imported-demo"])
    );
    assert!(
        ws.path()
            .join("problems/imported-demo/problem.toml")
            .is_file()
    );
    assert!(
        ws.path()
            .join("problems/imported-demo/samples/01.in")
            .is_file()
    );
    assert_eq!(
        fs::read_to_string(ws.path().join("problems/imported-demo/samples/01.ans")).unwrap(),
        "3\n"
    );

    // 2. Importer path traversal is rejected at Core layer (`ZIP_SLIP`)
    let evil_imp = run_xcpc(
        Some(ws.path()),
        data_dir.path(),
        &[
            "import",
            "custom-io:evil-asset",
            src_file.to_str().unwrap(),
            "--json",
        ],
    );
    assert_eq!(evil_imp.status.code(), Some(2));
    let evil_imp_json = parse_json_stdout(&evil_imp);
    assert_eq!(evil_imp_json["ok"], false);
    assert_eq!(evil_imp_json["error"]["code"], "ZIP_SLIP");
    assert!(!ws.path().join("problems/evil-prob").exists());

    // 3. E2E `xcpc export custom-io:bundle problem imported-demo --out <dir> --with-pdf --json`
    let out_dir = tmp.path().join("exported");
    let exp = run_xcpc(
        Some(ws.path()),
        data_dir.path(),
        &[
            "export",
            "custom-io:bundle",
            "problem",
            "imported-demo",
            "--out",
            out_dir.to_str().unwrap(),
            "--with-pdf",
            "--json",
        ],
    );
    assert!(
        exp.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&exp.stderr)
    );
    let exp_json = parse_json_stdout(&exp);
    assert_eq!(exp_json["ok"], true);
    assert_eq!(exp_json["summary"], "exported imported-demo");
    assert_eq!(exp_json["files"].as_array().unwrap().len(), 3);

    let meta_str = fs::read_to_string(out_dir.join("imported-demo/meta.json")).unwrap();
    let meta: Value = serde_json::from_str(&meta_str).unwrap();
    assert_eq!(meta["id"], "imported-demo");
    assert_eq!(meta["title"], "Imported Sum");
    assert_eq!(meta["has_pdf"], true);
    assert_eq!(meta["samples"], 1);
    let pdf_bytes = fs::read(out_dir.join("imported-demo/statement.pdf")).unwrap();
    assert!(pdf_bytes.starts_with(b"%PDF"), "expected valid PDF header");

    // 4. Default `--out` writes under `<workspace>/build/exports`
    let exp_default = run_xcpc(
        Some(ws.path()),
        data_dir.path(),
        &[
            "export",
            "custom-io:bundle",
            "problem",
            "imported-demo",
            "--json",
        ],
    );
    assert!(
        exp_default.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&exp_default.stderr)
    );
    assert!(
        ws.path()
            .join("build/exports/imported-demo/meta.json")
            .is_file()
    );

    // 5. Exporter path traversal is rejected at Core layer (`ZIP_SLIP`) and writes nothing outside
    let evil_exp_dir = tmp.path().join("evil-export-out");
    let evil_exp = run_xcpc(
        Some(ws.path()),
        data_dir.path(),
        &[
            "export",
            "custom-io:evil-out",
            "problem",
            "imported-demo",
            "--out",
            evil_exp_dir.to_str().unwrap(),
            "--json",
        ],
    );
    assert_eq!(evil_exp.status.code(), Some(2));
    let evil_exp_json = parse_json_stdout(&evil_exp);
    assert_eq!(evil_exp_json["ok"], false);
    assert_eq!(evil_exp_json["error"]["code"], "ZIP_SLIP");
    assert!(!tmp.path().join("escape.txt").exists());
}

#[test]
fn training_log_cli_standalone_library_and_read_only() {
    let ws = setup_workspace();
    let data_dir = tempfile::tempdir().unwrap();

    // 1. Standalone manual log (no --problem, no --create-in-library).
    let out = run_xcpc(
        Some(ws.path()),
        data_dir.path(),
        &[
            "training",
            "log",
            "--title",
            "Luogu P1001",
            "--status",
            "ac",
            "--rating",
            "1200",
            "--tags",
            "math,implementation",
            "--platform",
            "luogu",
            "--url",
            "https://www.luogu.com.cn/problem/P1001",
            "--at",
            "2026-11-24T08:30:00Z",
            "--note",
            "fast ac",
            "--json",
        ],
    );
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json = parse_json_stdout(&out);
    assert_eq!(json["ok"], true);
    assert_eq!(json["created_in_library"], false);
    assert!(json["library_problem_id"].is_null());
    assert_eq!(json["entry"]["title"], "Luogu P1001");
    assert_eq!(json["entry"]["platform"], "luogu");
    assert_eq!(json["entry"]["rating"], 1200);
    assert_eq!(json["entry"]["status"], "ac");
    assert_eq!(json["entry"]["at"], "2026-11-24T08:30:00Z");
    assert_eq!(json["entry"]["note"], "fast ac");
    assert_eq!(
        json["entry"]["tags"].as_array().unwrap(),
        &vec![Value::from("math"), Value::from("implementation")]
    );

    // 2. Manual log with `--create-in-library` creates a problem under `problems/`.
    let out_lib = run_xcpc(
        Some(ws.path()),
        data_dir.path(),
        &[
            "training",
            "log",
            "--title",
            "Tree Centroid",
            "--status",
            "upsolved",
            "--rating",
            "2200",
            "--tags",
            "trees,divide-and-conquer",
            "--platform",
            "local",
            "--create-in-library",
            "--json",
        ],
    );
    assert!(
        out_lib.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out_lib.stderr)
    );
    let json_lib = parse_json_stdout(&out_lib);
    assert_eq!(json_lib["ok"], true);
    assert_eq!(json_lib["created_in_library"], true);
    let lib_id = json_lib["library_problem_id"].as_str().unwrap();
    assert!(
        ws.path()
            .join("problems")
            .join(lib_id)
            .join("problem.toml")
            .is_file()
    );

    // 3. Human-readable output for workspace check-in (default status = ac).
    let out_human = run_xcpc(
        Some(ws.path()),
        data_dir.path(),
        &["training", "log", "--problem", "p1", "--note", "re-solved"],
    );
    assert!(
        out_human.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out_human.stderr)
    );
    let stdout = String::from_utf8(out_human.stdout).unwrap();
    assert!(stdout.contains("status=ac"), "unexpected output: {stdout}");

    // 4. `--read-only` refuses `xcpc training log`.
    let out_ro = run_xcpc(
        Some(ws.path()),
        data_dir.path(),
        &[
            "--read-only",
            "training",
            "log",
            "--title",
            "Blocked",
            "--json",
        ],
    );
    assert!(!out_ro.status.success());
    let json_ro = parse_json_stdout(&out_ro);
    assert_eq!(json_ro["ok"], false);
    assert_eq!(json_ro["error"]["code"], "read_only");
}

#[test]
fn training_editorials_cli_filters_and_output() {
    let ws = setup_workspace();
    let data_dir = tempfile::tempdir().unwrap();

    // `p1` (rating=1600, tags=["dp"]) gets an editorial.
    fs::write(
        ws.path().join("problems/p1/editorial.md"),
        "# Solution\n\nPrefix sums and dynamic programming.\n",
    )
    .unwrap();

    // Add `p2` (rating=2100, tags=["graphs"]) with an editorial.
    let p2 = ws.path().join("problems/p2");
    fs::create_dir_all(p2.join("samples")).unwrap();
    fs::write(
        p2.join("problem.toml"),
        "title = \"Shortest Path\"\nrating = 2100\ntags = [\"graphs\"]\n",
    )
    .unwrap();
    fs::write(p2.join("statement.md"), "Find shortest path.\n").unwrap();
    fs::write(
        p2.join("editorial.md"),
        "Run Dijkstra with a binary heap.\n",
    )
    .unwrap();

    // Add `p3` with NO editorial (must be excluded).
    let p3 = ws.path().join("problems/p3");
    fs::create_dir_all(p3.join("samples")).unwrap();
    fs::write(p3.join("problem.toml"), "title = \"No Editorial\"\n").unwrap();
    fs::write(p3.join("statement.md"), "No editorial yet.\n").unwrap();

    // 1. All editorials JSON.
    let all = run_xcpc(
        Some(ws.path()),
        data_dir.path(),
        &["training", "editorials", "--json"],
    );
    assert!(
        all.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&all.stderr)
    );
    let all_json = parse_json_stdout(&all);
    assert_eq!(all_json["ok"], true);
    assert_eq!(all_json["total"], 2);
    assert_eq!(all_json["items"].as_array().unwrap().len(), 2);

    // 2. Filter by `--tag graphs`.
    let by_tag = run_xcpc(
        Some(ws.path()),
        data_dir.path(),
        &["training", "editorials", "--tag", "graphs", "--json"],
    );
    assert!(by_tag.status.success());
    let by_tag_json = parse_json_stdout(&by_tag);
    assert_eq!(by_tag_json["total"], 1);
    assert_eq!(by_tag_json["items"][0]["problem_id"], "p2");

    // 3. Filter by `--search Dijkstra`.
    let by_search = run_xcpc(
        Some(ws.path()),
        data_dir.path(),
        &["training", "editorials", "--search", "Dijkstra", "--json"],
    );
    assert!(by_search.status.success());
    let by_search_json = parse_json_stdout(&by_search);
    assert_eq!(by_search_json["total"], 1);
    assert_eq!(by_search_json["items"][0]["problem_id"], "p2");

    // 4. Filter by `--min-rating 1500 --max-rating 1800`.
    let by_rating = run_xcpc(
        Some(ws.path()),
        data_dir.path(),
        &[
            "training",
            "editorials",
            "--min-rating",
            "1500",
            "--max-rating",
            "1800",
            "--json",
        ],
    );
    assert!(by_rating.status.success());
    let by_rating_json = parse_json_stdout(&by_rating);
    assert_eq!(by_rating_json["total"], 1);
    assert_eq!(by_rating_json["items"][0]["problem_id"], "p1");

    // 5. Human-readable table output.
    let human = run_xcpc(
        Some(ws.path()),
        data_dir.path(),
        &["training", "editorials"],
    );
    assert!(human.status.success());
    let stdout = String::from_utf8(human.stdout).unwrap();
    assert!(stdout.contains("editorials: 2"), "unexpected: {stdout}");
    assert!(stdout.contains("p1"), "unexpected: {stdout}");
    assert!(stdout.contains("p2"), "unexpected: {stdout}");
}

#[test]
fn phase4d_plugin_cli_init_vite_check_pack_link_unlink() {
    let tmp = tempfile::tempdir().unwrap();
    let data_dir = tempfile::tempdir().unwrap();

    // 1. `xcpc plugin init --kind tool --vite`
    let vite_dir = tmp.path().join("my-vite-tool");
    let init_vite = run_xcpc(
        None,
        data_dir.path(),
        &[
            "plugin",
            "init",
            vite_dir.to_str().unwrap(),
            "--kind",
            "tool",
            "--vite",
            "--json",
        ],
    );
    assert!(
        init_vite.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&init_vite.stderr)
    );
    let init_vite_json = parse_json_stdout(&init_vite);
    assert_eq!(init_vite_json["ok"], true);
    assert_eq!(init_vite_json["vite"], true);
    assert!(vite_dir.join("package.json").is_file());
    assert!(vite_dir.join("vite.config.ts").is_file());
    assert!(vite_dir.join("src/index.ts").is_file());
    assert!(vite_dir.join("src/ui.tsx").is_file());

    // 2. Plain JS tool -> `check` -> `pack` -> `link` -> `list` -> `unlink`
    let plain_dir = tmp.path().join("plain-tool");
    let init_plain = run_xcpc(
        None,
        data_dir.path(),
        &[
            "plugin",
            "init",
            plain_dir.to_str().unwrap(),
            "--kind",
            "tool",
            "--json",
        ],
    );
    assert!(init_plain.status.success());

    let check = run_xcpc(
        None,
        data_dir.path(),
        &["plugin", "check", plain_dir.to_str().unwrap(), "--json"],
    );
    assert!(
        check.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&check.stderr)
    );
    let check_json = parse_json_stdout(&check);
    assert_eq!(check_json["ok"], true);
    assert_eq!(check_json["plugin_id"], "plain-tool");

    let out_zip = tmp.path().join("plain-tool.zip");
    let pack = run_xcpc(
        None,
        data_dir.path(),
        &[
            "plugin",
            "pack",
            plain_dir.to_str().unwrap(),
            "-o",
            out_zip.to_str().unwrap(),
            "--json",
        ],
    );
    assert!(
        pack.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&pack.stderr)
    );
    let pack_json = parse_json_stdout(&pack);
    assert_eq!(pack_json["ok"], true);
    assert_eq!(pack_json["sha256"].as_str().unwrap().len(), 64);
    assert!(out_zip.is_file());

    #[cfg(unix)]
    {
        let link = run_xcpc(
            None,
            data_dir.path(),
            &["plugin", "link", plain_dir.to_str().unwrap(), "--json"],
        );
        assert!(
            link.status.success(),
            "stderr: {}",
            String::from_utf8_lossy(&link.stderr)
        );
        let link_json = parse_json_stdout(&link);
        assert_eq!(link_json["plugin"]["is_linked"], true);

        let list = run_xcpc(None, data_dir.path(), &["plugin", "list", "--json"]);
        assert!(list.status.success());
        let list_json = parse_json_stdout(&list);
        let found = list_json["plugins"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["id"] == "plain-tool")
            .expect("linked plugin should appear in list");
        assert_eq!(found["is_linked"], true);

        let unlink = run_xcpc(
            None,
            data_dir.path(),
            &["plugin", "unlink", "plain-tool", "--json"],
        );
        assert!(unlink.status.success());
        let unlink_json = parse_json_stdout(&unlink);
        assert_eq!(unlink_json["removed"], true);
        assert!(plain_dir.exists(), "unlink must never delete source dir");
    }
}

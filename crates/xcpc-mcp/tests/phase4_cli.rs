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
    assert_eq!(repos.len(), 1);
    assert_eq!(repos[0]["url"], repo_url);
    assert_eq!(repos[0]["name"], "Community");

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
    assert!(r_list2["repos"].as_array().unwrap().is_empty());
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

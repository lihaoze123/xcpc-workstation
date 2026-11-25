//! End-to-end CLI integration tests for `xcpc` (all Phase 1–2 subcommands, `--json`,
//! deterministic exit codes 0 / 1 / 2, and interactive `unfreeze` confirmation).

use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use serde_json::Value;
use xcpc_storage::content_hash;

/// Minimal valid 1x1 transparent PNG bytes.
const TINY_PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE,
    0x42, 0x60, 0x82,
];

fn run_xcpc(ws: Option<&Path>, args: &[&str], stdin_data: Option<&str>) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_xcpc"));
    if let Some(w) = ws {
        cmd.arg("-w").arg(w);
    }
    cmd.args(args);
    if stdin_data.is_some() {
        cmd.stdin(Stdio::piped());
    } else {
        cmd.stdin(Stdio::null());
    }
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().expect("spawn xcpc");
    if let Some(data) = stdin_data {
        child
            .stdin
            .as_mut()
            .unwrap()
            .write_all(data.as_bytes())
            .unwrap();
    }
    child.wait_with_output().expect("wait xcpc")
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
    fs::create_dir_all(p1.join("solutions")).unwrap();
    fs::write(
        p1.join("problem.toml"),
        r#"title = "First Problem"
rating = 1000
tags = ["greedy", "math"]

[limits]
time = "1s"
memory = "256MB"

[[source]]
platform = "codeforces"
id = "1000A"
"#,
    )
    .unwrap();
    fs::write(
        p1.join("statement.md"),
        "PRIVATE_CLI_BODY_TOKEN 给定 $n$。\n\n## 输入格式\n\n一个整数 $n$。\n\n## 输出格式\n\n输出 $n$。\n",
    )
    .unwrap();
    fs::write(p1.join("editorial.md"), "## 题解\n\n输出 $n$ 即可。\n").unwrap();
    fs::write(p1.join("samples/01.in"), "5\n").unwrap();
    fs::write(p1.join("samples/01.ans"), "5\n").unwrap();
    fs::write(p1.join("solutions/std.cpp"), "int main() {}\n").unwrap();

    let p2 = dir.path().join("problems/p2");
    fs::create_dir_all(p2.join("samples")).unwrap();
    fs::write(
        p2.join("problem.toml"),
        r#"title = "Second Problem"
rating = 1900
tags = ["dp", "math"]

[limits]
time = "2s"
memory = "512MB"

[[source]]
platform = "atcoder"
id = "abc123_d"
"#,
    )
    .unwrap();
    fs::write(
        p2.join("statement.md"),
        "动态规划问题。\n\n## 输入格式\n\n数列。\n\n## 输出格式\n\n最大值。\n",
    )
    .unwrap();
    fs::write(p2.join("samples/01.in"), "3\n1 2 3\n").unwrap();
    fs::write(p2.join("samples/01.ans"), "6\n").unwrap();

    dir
}

#[test]
fn init_creates_workspace_and_never_overwrites_agents_md() {
    let parent = tempfile::tempdir().unwrap();
    let target = parent.path().join("fresh-ws");

    // 1. --read-only refuses init with exit code 2.
    let ro = run_xcpc(
        None,
        &["--read-only", "--json", "init", target.to_str().unwrap()],
        None,
    );
    assert_eq!(ro.status.code(), Some(2));
    assert_eq!(parse_json_stdout(&ro)["error"]["code"], "read_only");
    assert!(!target.exists());

    // 2. Normal init with explicit dir + --json (no existing workspace needed).
    let init = run_xcpc(None, &["--json", "init", target.to_str().unwrap()], None);
    assert_eq!(init.status.code(), Some(0));
    let j = parse_json_stdout(&init);
    assert_eq!(j["ok"], true);
    assert!(target.join("workspace.toml").is_file());
    assert!(target.join("problems").is_dir());
    assert!(target.join("contests").is_dir());
    assert!(target.join(".gitignore").is_file());
    assert!(target.join("AGENTS.md").is_file());

    // 3. Modify AGENTS.md, then re-run `xcpc init` in current_dir (default `.`) -> AGENTS.md untouched!
    let custom_agents = "# My Custom Workspace Agents Guide\nDo not overwrite me!\n";
    fs::write(target.join("AGENTS.md"), custom_agents).unwrap();

    let reinit = Command::new(env!("CARGO_BIN_EXE_xcpc"))
        .current_dir(&target)
        .arg("init")
        .output()
        .expect("run xcpc init in cwd");
    assert_eq!(reinit.status.code(), Some(0));
    assert_eq!(
        fs::read_to_string(target.join("AGENTS.md")).unwrap(),
        custom_agents
    );
}

#[test]
fn check_build_render_integrate_lint_errors_and_warnings() {
    let dir = setup_workspace();

    // 1. Add a warning-only issue (unpaired sample + missing output section on a new problem).
    let warn_dir = dir.path().join("problems/warn-only");
    fs::create_dir_all(warn_dir.join("samples")).unwrap();
    fs::write(warn_dir.join("problem.toml"), "title = \"Warn Only\"\n").unwrap();
    fs::write(
        warn_dir.join("statement.md"),
        "仅输入。\n\n## 输入格式\n\n一个数。\n",
    )
    .unwrap();
    fs::write(warn_dir.join("samples/01.in"), "1\n").unwrap();
    fs::write(warn_dir.join("samples/01.ans"), "1\n").unwrap();
    fs::write(warn_dir.join("samples/02.in"), "2\n").unwrap();

    // Warning does NOT fail check or block PDF build (exit code 0).
    let chk = run_xcpc(
        Some(dir.path()),
        &["--json", "check", "problem", "warn-only"],
        None,
    );
    assert_eq!(chk.status.code(), Some(0));
    let chk_j = parse_json_stdout(&chk);
    assert_eq!(chk_j["ok"], true);
    let diags = chk_j["diagnostics"].as_array().unwrap();
    assert!(diags.iter().any(|d| d["code"] == "sample_unpaired"));
    assert!(diags.iter().any(|d| d["code"] == "missing_io_section"));

    let bld = run_xcpc(
        Some(dir.path()),
        &["--json", "build", "problem", "warn-only"],
        None,
    );
    assert_eq!(bld.status.code(), Some(0));
    assert!(dir.path().join("build/Warn Only.pdf").is_file());

    let rnd = run_xcpc(
        Some(dir.path()),
        &["--json", "render", "problem", "warn-only", "--page", "1"],
        None,
    );
    assert_eq!(rnd.status.code(), Some(0));
    assert!(dir.path().join("build/warn-only/page-1.png").is_file());

    // 2. Lint error (missing_image) -> exit code 1 AND build writes NO PDF!
    let err_dir = dir.path().join("problems/broken-img");
    fs::create_dir_all(&err_dir).unwrap();
    fs::write(err_dir.join("problem.toml"), "title = \"Broken\"\n").unwrap();
    fs::write(
        err_dir.join("statement.md"),
        "![miss](assets/nope.png)\n\n## 输入格式\n\nx\n\n## 输出格式\n\ny\n",
    )
    .unwrap();

    let err_chk = run_xcpc(
        Some(dir.path()),
        &["--json", "check", "problem", "broken-img"],
        None,
    );
    assert_eq!(err_chk.status.code(), Some(1));
    let err_j = parse_json_stdout(&err_chk);
    assert_eq!(err_j["ok"], false);
    assert!(
        err_j["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["code"] == "missing_image" && d["severity"] == "error")
    );

    let err_bld = run_xcpc(
        Some(dir.path()),
        &["--json", "build", "problem", "broken-img"],
        None,
    );
    assert_eq!(err_bld.status.code(), Some(1));
    assert!(!dir.path().join("build/broken-img.pdf").exists());

    // 3. Missing target -> exit code 2 (`not_found`).
    let missing = run_xcpc(
        Some(dir.path()),
        &["--json", "check", "problem", "does-not-exist"],
        None,
    );
    assert_eq!(missing.status.code(), Some(2));
    assert_eq!(parse_json_stdout(&missing)["error"]["code"], "not_found");

    // 4. Contest referencing a missing problem -> exit code 1 (`missing_problem`) and no PDF!
    let ghost_contest = dir.path().join("contests/ghost");
    fs::create_dir_all(&ghost_contest).unwrap();
    fs::write(
        ghost_contest.join("contest.toml"),
        "name = \"Ghost\"\n[[problem]]\nid = \"p1\"\nlabel = \"A\"\n[[problem]]\nid = \"vanished\"\nlabel = \"B\"\n",
    )
    .unwrap();
    let ghost_chk = run_xcpc(
        Some(dir.path()),
        &["--json", "check", "contest", "ghost"],
        None,
    );
    assert_eq!(ghost_chk.status.code(), Some(1));
    let gj = parse_json_stdout(&ghost_chk);
    assert_eq!(gj["ok"], false);
    assert!(
        gj["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["code"] == "missing_problem" && d["severity"] == "error")
    );
    let ghost_bld = run_xcpc(
        Some(dir.path()),
        &["--json", "build", "contest", "ghost"],
        None,
    );
    assert_eq!(ghost_bld.status.code(), Some(1));
    assert!(!dir.path().join("build/ghost.pdf").exists());
}

#[test]
fn search_and_show_problem_and_contest() {
    let dir = setup_workspace();

    // 1. xcpc search with query + filters (JSON & human).
    let s1 = run_xcpc(
        Some(dir.path()),
        &[
            "--json",
            "search",
            "PRIVATE_CLI_BODY_TOKEN",
            "--tag",
            "math",
            "--rating-min",
            "900",
            "--rating-max",
            "1500",
            "--platform",
            "codeforces",
        ],
        None,
    );
    assert_eq!(s1.status.code(), Some(0));
    let raw_stdout = String::from_utf8(s1.stdout.clone()).unwrap();
    assert!(
        !raw_stdout.contains("PRIVATE_CLI_BODY_TOKEN"),
        "xcpc search must not output statement body"
    );
    let s1_j = parse_json_stdout(&s1);
    let probs = s1_j["problems"].as_array().unwrap();
    assert_eq!(probs.len(), 1);
    assert_eq!(probs[0]["id"], "p1");
    assert_eq!(probs[0]["has_editorial"], true);
    assert_eq!(probs[0]["has_solutions"], true);

    // Human-readable search output.
    let s_human = run_xcpc(Some(dir.path()), &["search", "--tag", "dp"], None);
    assert_eq!(s_human.status.code(), Some(0));
    let sh_text = String::from_utf8(s_human.stdout).unwrap();
    assert!(sh_text.contains("p2\tSecond Problem\t1900"));

    // 2. xcpc show problem <id>.
    let show_p = run_xcpc(Some(dir.path()), &["--json", "show", "problem", "p1"], None);
    assert_eq!(show_p.status.code(), Some(0));
    let sp = parse_json_stdout(&show_p);
    assert_eq!(sp["meta"]["title"], "First Problem");
    assert_eq!(sp["solutions"], serde_json::json!(["std.cpp"]));
    assert_eq!(sp["samples"].as_array().unwrap().len(), 1);
    let stmt_text = sp["statements"][0]["text"].as_str().unwrap();
    assert_eq!(
        sp["statements"][0]["hash"],
        content_hash(stmt_text.as_bytes())
    );
    let ed_text = sp["editorial"]["text"].as_str().unwrap();
    assert_eq!(sp["editorial"]["hash"], content_hash(ed_text.as_bytes()));

    // 3. Compose a contest, override B, and verify `xcpc show contest <id>`.
    let comp = run_xcpc(
        Some(dir.path()),
        &[
            "--json",
            "contest",
            "compose",
            "final-2026",
            "p1",
            "p2",
            "--name",
            "Final 2026",
            "--location",
            "Beijing",
            "--date",
            "2026-11-20",
            "--author",
            "Committee",
            "--language",
            "chinese",
            "--cover",
            "true",
            "--blank-pages",
            "false",
            "--show-metadata",
            "true",
        ],
        None,
    );
    assert_eq!(comp.status.code(), Some(0));

    let ov = run_xcpc(
        Some(dir.path()),
        &[
            "--json",
            "contest",
            "override",
            "final-2026",
            "B",
            "--title",
            "Overridden B",
            "--time",
            "3s",
            "--memory",
            "1024MB",
            "--input",
            "b.in",
            "--output",
            "b.out",
            "--show-metadata",
            "false",
        ],
        None,
    );
    assert_eq!(ov.status.code(), Some(0));
    let ov_j = parse_json_stdout(&ov);
    assert_eq!(ov_j["item"]["title"], "Overridden B");
    assert_eq!(ov_j["item"]["limits"]["time"], "3s");
    assert_eq!(ov_j["item"]["limits"]["input"], "b.in");
    assert_eq!(ov_j["item"]["show-metadata"], false);

    let show_c = run_xcpc(
        Some(dir.path()),
        &["--json", "show", "contest", "final-2026"],
        None,
    );
    assert_eq!(show_c.status.code(), Some(0));
    let sc = parse_json_stdout(&show_c);
    assert_eq!(sc["name"], "Final 2026");
    assert_eq!(sc["frozen"], false);
    assert!(sc["pages"].as_u64().unwrap() >= 3);
    assert_eq!(sc["problems"][0]["label"], "A");
    assert_eq!(sc["problems"][0]["first"], 2);
    assert_eq!(sc["problems"][1]["label"], "B");
    assert_eq!(sc["problems"][1]["title"], "Overridden B");
    assert_eq!(sc["problems"][1]["overrides"]["title"], "Overridden B");
    assert!(sc["problems"][1]["first"].as_u64().unwrap() > 2);
    assert_eq!(sc["stats"]["problem_count"], 2);
    assert_eq!(sc["stats"]["rating_distribution"]["newbie"], 1);
    assert_eq!(sc["stats"]["rating_distribution"]["candidate_master"], 1);

    // Human-readable `xcpc show problem` and `xcpc show contest`.
    let show_p_human = run_xcpc(Some(dir.path()), &["show", "problem", "p1"], None);
    assert_eq!(show_p_human.status.code(), Some(0));
    let sp_text = String::from_utf8(show_p_human.stdout).unwrap();
    assert!(sp_text.contains("problem p1: First Problem"));
    assert!(sp_text.contains("solutions: std.cpp"));
    assert!(sp_text.contains("sha256="));

    let show_c_human = run_xcpc(Some(dir.path()), &["show", "contest", "final-2026"], None);
    assert_eq!(show_c_human.status.code(), Some(0));
    let sc_text = String::from_utf8(show_c_human.stdout).unwrap();
    assert!(sc_text.contains("contest final-2026: Final 2026 [unfrozen]"));
    assert!(sc_text.contains("Overridden B"));

    // Clear overrides via --clear-* flags.
    let cleared = run_xcpc(
        Some(dir.path()),
        &[
            "--json",
            "contest",
            "override",
            "final-2026",
            "B",
            "--clear-title",
            "--clear-time",
            "--clear-memory",
            "--clear-input",
            "--clear-output",
            "--clear-show-metadata",
        ],
        None,
    );
    assert_eq!(cleared.status.code(), Some(0));
    let cl_j = parse_json_stdout(&cleared);
    assert!(cl_j["item"].get("title").is_none());
    assert!(cl_j["item"].get("limits").is_none());
    assert!(cl_j["item"].get("show-metadata").is_none());
}

#[test]
fn contest_freeze_and_unfreeze_confirmation_and_diffs() {
    let dir = setup_workspace();

    // Compose contest with p1 and p2.
    assert_eq!(
        run_xcpc(
            Some(dir.path()),
            &["contest", "compose", "cup", "p1", "p2", "--name", "Cup"],
            None,
        )
        .status
        .code(),
        Some(0)
    );

    // Break an image in p1 -> freeze must be rejected with exit code 1.
    fs::write(
        dir.path().join("problems/p1/statement.md"),
        "图：![bad](assets/missing.png)\n\n## 输入格式\n\nx\n\n## 输出格式\n\ny\n",
    )
    .unwrap();
    let bad_freeze = run_xcpc(
        Some(dir.path()),
        &["--json", "contest", "freeze", "cup"],
        None,
    );
    assert_eq!(bad_freeze.status.code(), Some(1));
    let bf_j = parse_json_stdout(&bad_freeze);
    assert_eq!(bf_j["frozen"], false);
    assert!(!dir.path().join("contests/cup/snapshot").exists());

    // Fix the image -> p1 still has an unverified `solutions/std.cpp`, so freeze must be rejected!
    fs::create_dir_all(dir.path().join("problems/p1/assets")).unwrap();
    fs::write(dir.path().join("problems/p1/assets/missing.png"), TINY_PNG).unwrap();

    let unverified_freeze = run_xcpc(
        Some(dir.path()),
        &["--json", "contest", "freeze", "cup"],
        None,
    );
    assert_eq!(unverified_freeze.status.code(), Some(1));
    let uf_j = parse_json_stdout(&unverified_freeze);
    assert_eq!(uf_j["frozen"], false);
    assert!(
        uf_j["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["code"] == "package_unverified"),
        "expected package_unverified diagnostic: {uf_j}"
    );

    // Seed a fresh strict-verified report for p1 -> freeze succeeds with exit code 0.
    seed_verified_report(dir.path(), "p1");

    let ok_freeze = run_xcpc(
        Some(dir.path()),
        &["--json", "contest", "freeze", "cup"],
        None,
    );
    assert_eq!(ok_freeze.status.code(), Some(0));
    let ok_j = parse_json_stdout(&ok_freeze);
    assert_eq!(ok_j["ok"], true);
    assert_eq!(ok_j["frozen"], true);
    assert_eq!(
        ok_j["outputs"].as_array().unwrap().len(),
        2,
        "expected statements and editorial snapshot PDFs"
    );
    assert!(dir.path().join("contests/cup/snapshot/cup.pdf").is_file());
    assert!(
        dir.path()
            .join("contests/cup/snapshot/cup-editorial.pdf")
            .is_file()
    );

    // Mutate p1 statement and p2 sample in the library while frozen.
    fs::write(
        dir.path().join("problems/p1/statement.md"),
        "已修改题面 ![ok](assets/missing.png)\n\n## 输入格式\n\nx\n\n## 输出格式\n\ny\n",
    )
    .unwrap();
    fs::write(dir.path().join("problems/p2/samples/01.ans"), "999\n").unwrap();

    // 0. Frozen contest refuses compose & override via CLI with exit code 2 (`contest_frozen`).
    let frozen_comp = run_xcpc(
        Some(dir.path()),
        &["--json", "contest", "compose", "cup", "p1"],
        None,
    );
    assert_eq!(frozen_comp.status.code(), Some(2));
    assert_eq!(
        parse_json_stdout(&frozen_comp)["error"]["code"],
        "contest_frozen"
    );

    let frozen_ov = run_xcpc(
        Some(dir.path()),
        &[
            "--json", "contest", "override", "cup", "A", "--title", "Nope",
        ],
        None,
    );
    assert_eq!(frozen_ov.status.code(), Some(2));
    assert_eq!(
        parse_json_stdout(&frozen_ov)["error"]["code"],
        "contest_frozen"
    );

    // 1. --read-only refuses unfreeze with exit code 2 (`read_only`), contest stays frozen.
    let ro_unfreeze = run_xcpc(
        Some(dir.path()),
        &[
            "--read-only",
            "--json",
            "contest",
            "unfreeze",
            "cup",
            "--yes",
        ],
        None,
    );
    assert_eq!(ro_unfreeze.status.code(), Some(2));
    assert_eq!(
        parse_json_stdout(&ro_unfreeze)["error"]["code"],
        "read_only"
    );
    assert!(dir.path().join("contests/cup/snapshot").is_dir());

    // 2. Interactive unfreeze with wrong confirmation -> prints diffs, exits 2, stays frozen!
    let wrong = run_xcpc(
        Some(dir.path()),
        &["contest", "unfreeze", "cup"],
        Some("wrong-id\n"),
    );
    assert_eq!(wrong.status.code(), Some(2));
    let wrong_stdout = String::from_utf8(wrong.stdout).unwrap();
    assert!(
        wrong_stdout.contains("A (p1)") && wrong_stdout.contains("statement changed"),
        "expected p1 statement diff before prompt, got: {wrong_stdout}"
    );
    assert!(
        wrong_stdout.contains("B (p2)") && wrong_stdout.contains("samples changed"),
        "expected p2 samples diff before prompt, got: {wrong_stdout}"
    );
    assert!(dir.path().join("contests/cup/snapshot").is_dir());

    // 3. Interactive unfreeze with EOF -> exits 2 (`unconfirmed`), stays frozen!
    let eof = run_xcpc(
        Some(dir.path()),
        &["--json", "contest", "unfreeze", "cup"],
        Some(""),
    );
    assert_eq!(eof.status.code(), Some(2));
    assert_eq!(parse_json_stdout(&eof)["error"]["code"], "unconfirmed");
    assert!(dir.path().join("contests/cup/snapshot").is_dir());

    // 4. Interactive unfreeze with matching contest ID on stdin -> succeeds (exit 0)!
    let confirmed = run_xcpc(
        Some(dir.path()),
        &["contest", "unfreeze", "cup"],
        Some("cup\n"),
    );
    assert_eq!(confirmed.status.code(), Some(0));
    let conf_stdout = String::from_utf8(confirmed.stdout).unwrap();
    assert!(conf_stdout.contains("unfrozen contest cup"));
    assert!(!dir.path().join("contests/cup/snapshot").exists());

    // 5. Unfreezing an already unfrozen contest -> exit 2 (`not_frozen`).
    let not_frozen = run_xcpc(
        Some(dir.path()),
        &["--json", "contest", "unfreeze", "cup", "--yes"],
        None,
    );
    assert_eq!(not_frozen.status.code(), Some(2));
    assert_eq!(
        parse_json_stdout(&not_frozen)["error"]["code"],
        "not_frozen"
    );

    // 6. Re-freeze, delete p2 from library, and test `--yes --json` unfreeze showing problem_deleted.
    assert_eq!(
        run_xcpc(Some(dir.path()), &["contest", "freeze", "cup"], None)
            .status
            .code(),
        Some(0)
    );
    fs::remove_dir_all(dir.path().join("problems/p2")).unwrap();

    let yes_json = run_xcpc(
        Some(dir.path()),
        &["--json", "contest", "unfreeze", "cup", "--yes"],
        None,
    );
    assert_eq!(yes_json.status.code(), Some(0));
    let yj = parse_json_stdout(&yes_json);
    assert_eq!(yj["ok"], true);
    assert_eq!(yj["frozen"], false);
    let diffs = yj["diffs"].as_array().unwrap();
    assert_eq!(diffs.len(), 1);
    assert_eq!(diffs[0]["label"], "B");
    assert_eq!(diffs[0]["problem_id"], "p2");
    assert_eq!(diffs[0]["kinds"], serde_json::json!(["problem_deleted"]));
}

#[test]
fn init_example_and_output_override_and_human_error_messages() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("demo-ws");

    // 1. `xcpc init --example <dir>`
    let out = run_xcpc(
        None,
        &["--json", "init", "--example", target.to_str().unwrap()],
        None,
    );
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let j = parse_json_stdout(&out);
    assert_eq!(j["ok"], true);
    assert_eq!(j["example"], true);
    assert!(target.join("problems/crc-check/statement.md").is_file());
    assert!(target.join("contests/welcome-round/contest.toml").is_file());

    // 1b. `xcpc init --example <non-empty-dir>` refuses to mix files into an existing directory.
    let non_empty = dir.path().join("has-files");
    std::fs::create_dir_all(&non_empty).unwrap();
    std::fs::write(non_empty.join("notes.txt"), b"existing").unwrap();
    let refused = run_xcpc(
        None,
        &["--json", "init", "--example", non_empty.to_str().unwrap()],
        None,
    );
    assert_eq!(refused.status.code(), Some(2));
    let rj = parse_json_stdout(&refused);
    assert_eq!(rj["ok"], false);
    assert_eq!(rj["error"]["code"], "dir_not_empty");
    assert!(!non_empty.join("workspace.toml").exists());

    // 2. Default contest build outputs `<比赛名>-试题册.pdf`, and `--output` overrides it.
    let bld = run_xcpc(
        Some(&target),
        &["--json", "build", "contest", "welcome-round"],
        None,
    );
    assert_eq!(bld.status.code(), Some(0));
    assert!(
        target
            .join("build/XCPC Workstation 示例赛-试题册.pdf")
            .is_file()
    );

    let custom_out = target.join("build/custom-name.pdf");
    let bld2 = run_xcpc(
        Some(&target),
        &[
            "--json",
            "build",
            "contest",
            "welcome-round",
            "--output",
            custom_out.to_str().unwrap(),
        ],
        None,
    );
    assert_eq!(bld2.status.code(), Some(0));
    assert!(custom_out.is_file());

    // 3. Human stderr prints centralized Chinese error message; `--json` keeps structured code.
    let err_plain = run_xcpc(Some(&target), &["show", "problem", "no-such-prob"], None);
    assert_eq!(err_plain.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&err_plain.stderr);
    assert!(
        stderr.contains("找不到题目「no-such-prob」"),
        "expected typed Chinese error, got: {stderr}"
    );

    let err_json = run_xcpc(
        Some(&target),
        &["--json", "show", "problem", "no-such-prob"],
        None,
    );
    assert_eq!(err_json.status.code(), Some(2));
    let ej = parse_json_stdout(&err_json);
    assert_eq!(ej["ok"], false);
    assert_eq!(ej["error"]["code"], "not_found");

    // 4. `xcpc template list`, `xcpc template add`, `xcpc template schema`
    let tpl_list = run_xcpc(Some(&target), &["--json", "template", "list"], None);
    assert_eq!(tpl_list.status.code(), Some(0));
    let tlj = parse_json_stdout(&tpl_list);
    assert_eq!(tlj["presets"].as_array().unwrap().len(), 5);
    assert_eq!(tlj["templates"].as_array().unwrap().len(), 4);

    let tpl_add = run_xcpc(
        Some(&target),
        &["--json", "template", "add", "minimal", "my-min"],
        None,
    );
    assert_eq!(tpl_add.status.code(), Some(0));
    let taj = parse_json_stdout(&tpl_add);
    assert_eq!(taj["ok"], true);
    assert_eq!(taj["path"], "templates/my-min.typ");
    assert!(target.join("templates/my-min.typ").is_file());

    let tpl_schema = run_xcpc(
        Some(&target),
        &["--json", "template", "schema", "my-min"],
        None,
    );
    assert_eq!(tpl_schema.status.code(), Some(0));
    let tsj = parse_json_stdout(&tpl_schema);
    assert_eq!(tsj["template"]["id"], "my-min");
    assert_eq!(tsj["template"]["path"], "templates/my-min.typ");
}

#[test]
fn problem_duplicate_and_contest_archive_cli_and_read_only() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("ws");
    assert_eq!(
        run_xcpc(
            None,
            &["--json", "init", "--example", target.to_str().unwrap()],
            None,
        )
        .status
        .code(),
        Some(0)
    );

    // 1. `xcpc problem duplicate crc-check crc-copy --title "新标题" --json`
    let dup = run_xcpc(
        Some(&target),
        &[
            "--json",
            "problem",
            "duplicate",
            "crc-check",
            "crc-copy",
            "--title",
            "新标题",
        ],
        None,
    );
    assert_eq!(dup.status.code(), Some(0));
    let dj = parse_json_stdout(&dup);
    assert_eq!(dj["ok"], true);
    assert_eq!(dj["source_id"], "crc-check");
    assert_eq!(dj["id"], "crc-copy");
    assert_eq!(dj["title"], "新标题");
    assert_eq!(dj["path"], "problems/crc-copy");
    assert!(target.join("problems/crc-copy/statement.md").is_file());
    assert!(target.join("problems/crc-copy/samples/01.in").is_file());

    // Duplicate into same ID -> conflict (exit 2).
    let dup_conflict = run_xcpc(
        Some(&target),
        &["--json", "problem", "duplicate", "crc-check", "crc-copy"],
        None,
    );
    assert_eq!(dup_conflict.status.code(), Some(2));
    assert_eq!(
        parse_json_stdout(&dup_conflict)["error"]["code"],
        "conflict"
    );

    // Read-only refuses duplicate.
    let dup_ro = run_xcpc(
        Some(&target),
        &[
            "--read-only",
            "--json",
            "problem",
            "duplicate",
            "crc-check",
            "crc-ro",
        ],
        None,
    );
    assert_eq!(dup_ro.status.code(), Some(2));
    assert_eq!(parse_json_stdout(&dup_ro)["error"]["code"], "read_only");

    // 2. `xcpc contest archive welcome-round --json`
    let arch = run_xcpc(
        Some(&target),
        &["--json", "contest", "archive", "welcome-round"],
        None,
    );
    assert_eq!(arch.status.code(), Some(0));
    let aj = parse_json_stdout(&arch);
    assert_eq!(aj["ok"], true);
    assert_eq!(aj["id"], "welcome-round");
    assert_eq!(aj["archived"], true);
    assert_eq!(aj["frozen"], false);

    // `xcpc list --json` and `xcpc show contest welcome-round --json` expose `archived: true`.
    let listed = run_xcpc(Some(&target), &["--json", "list"], None);
    assert_eq!(listed.status.code(), Some(0));
    let lj = parse_json_stdout(&listed);
    let wr = lj["contests"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == "welcome-round")
        .unwrap();
    assert_eq!(wr["archived"], true);

    let shown = run_xcpc(
        Some(&target),
        &["--json", "show", "contest", "welcome-round"],
        None,
    );
    assert_eq!(shown.status.code(), Some(0));
    assert_eq!(parse_json_stdout(&shown)["archived"], true);

    // Archived contest refuses compose (`contest_archived`).
    let blocked = run_xcpc(
        Some(&target),
        &["--json", "contest", "compose", "welcome-round", "crc-check"],
        None,
    );
    assert_eq!(blocked.status.code(), Some(2));
    assert_eq!(
        parse_json_stdout(&blocked)["error"]["code"],
        "contest_archived"
    );

    // Read-only refuses unarchive.
    let ro_unarch = run_xcpc(
        Some(&target),
        &[
            "--read-only",
            "--json",
            "contest",
            "unarchive",
            "welcome-round",
        ],
        None,
    );
    assert_eq!(ro_unarch.status.code(), Some(2));
    assert_eq!(parse_json_stdout(&ro_unarch)["error"]["code"], "read_only");

    // 3. `xcpc contest unarchive welcome-round --json`
    let unarch = run_xcpc(
        Some(&target),
        &["--json", "contest", "unarchive", "welcome-round"],
        None,
    );
    assert_eq!(unarch.status.code(), Some(0));
    let uj = parse_json_stdout(&unarch);
    assert_eq!(uj["ok"], true);
    assert_eq!(uj["archived"], false);
}

fn seed_verified_report(ws_root: &std::path::Path, slug: &str) {
    let ws = xcpc_storage::Workspace::open(ws_root, false).unwrap();
    let hash = xcpc_judge::compute_package_hash(&ws, slug).unwrap();
    let path = xcpc_judge::verified_report_path_for(&ws, slug);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let report = xcpc_judge::BuildReport {
        problem_id: slug.to_owned(),
        package_hash: hash,
        mode: xcpc_judge::BuildMode::Verify,
        verified: true,
        ok: true,
        subset: false,
        built_at_epoch_secs: 1_700_000_000,
        compiler: None,
        checker: "wcmp".to_owned(),
        interactive: false,
        runs: 1,
        subtasks: Vec::new(),
        total_score: None,
        compile_failures: Vec::new(),
        dirty_inputs: Vec::new(),
        self_tests: Vec::new(),
        tests: Vec::new(),
        solutions: Vec::new(),
        matrix: Vec::new(),
        gates: Vec::new(),
        safety_margin: xcpc_judge::SafetyMargin::default(),
        diagnostics: Vec::new(),
        repro_commands: Vec::new(),
        next_steps: Vec::new(),
    };
    fs::write(&path, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
}

#[test]
fn init_with_skills_installs_problem_creator_and_upgraded_agents_md() {
    let parent = tempfile::tempdir().unwrap();
    let target = parent.path().join("skills-ws");

    let out = run_xcpc(
        None,
        &["--json", "init", "--with-skills", target.to_str().unwrap()],
        None,
    );
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let j = parse_json_stdout(&out);
    assert_eq!(j["ok"], true);
    assert_eq!(j["with_skills"], true);
    assert_eq!(j["skills"].as_array().unwrap().len(), 5);

    for rel in [
        ".agents/skills/xcpc-problem-creator/SKILL.md",
        ".agents/skills/xcpc-problem-creator/mistake-taxonomy.md",
        ".agents/skills/xcpc-problem-creator/judge-validator/SKILL.md",
        ".agents/skills/xcpc-problem-creator/solutions-tutorial/SKILL.md",
        ".agents/skills/xcpc-problem-creator/wrong-gen/SKILL.md",
    ] {
        assert!(target.join(rel).is_file(), "missing {rel}");
    }

    let agents = fs::read_to_string(target.join("AGENTS.md")).unwrap();
    assert!(agents.contains("xcpc-problem-creator"));
    assert!(agents.contains("inspect_package"));
    assert!(agents.contains("stress_package"));
}

#[test]
fn package_cli_init_build_strict_and_stress_end_to_end() {
    let dir = tempfile::tempdir().unwrap();
    let ws = xcpc_storage::init_workspace(dir.path()).unwrap();
    ws.create_problem("aplusb", "A + B").unwrap();
    fs::write(
        dir.path().join("problems/aplusb/statement.md"),
        "Compute $a + b$.\n\n## 输入格式\n\n两个整数。\n\n## 输出格式\n\n输出和。\n",
    )
    .unwrap();
    fs::write(dir.path().join("problems/aplusb/samples/01.in"), "1 2\n").unwrap();
    fs::write(dir.path().join("problems/aplusb/samples/01.ans"), "3\n").unwrap();

    // 1. `xcpc package init aplusb --json`
    let init_out = run_xcpc(
        Some(dir.path()),
        &["--json", "package", "init", "aplusb"],
        None,
    );
    assert_eq!(
        init_out.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&init_out.stderr)
    );
    let init_j = parse_json_stdout(&init_out);
    assert_eq!(init_j["ok"], true);
    assert_eq!(init_j["updated_problem_toml"], true);
    assert!(dir.path().join("problems/aplusb/validator.cpp").is_file());
    assert!(
        dir.path()
            .join("problems/aplusb/solutions/std.cpp")
            .is_file()
    );

    // Read-only blocks package init / build / stress.
    let ro_build = run_xcpc(
        Some(dir.path()),
        &["--read-only", "--json", "package", "build", "aplusb"],
        None,
    );
    assert_eq!(ro_build.status.code(), Some(2));
    assert_eq!(parse_json_stdout(&ro_build)["error"]["code"], "read_only");

    // 2. Before strict verify, `xcpc check problem aplusb` fails statically with `package_unverified`.
    let pre_check = run_xcpc(
        Some(dir.path()),
        &["--json", "check", "problem", "aplusb"],
        None,
    );
    assert_eq!(pre_check.status.code(), Some(1));
    let pre_j = parse_json_stdout(&pre_check);
    assert!(
        pre_j["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["code"] == "package_unverified"),
        "{pre_j}"
    );

    // 3. `xcpc package build aplusb --strict --json` compiles & verifies the scaffolded A+B package!
    let verify_out = run_xcpc(
        Some(dir.path()),
        &["--json", "package", "build", "aplusb", "--strict"],
        None,
    );
    assert_eq!(
        verify_out.status.code(),
        Some(0),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&verify_out.stdout),
        String::from_utf8_lossy(&verify_out.stderr)
    );
    let vj = parse_json_stdout(&verify_out);
    assert_eq!(vj["ok"], true);
    assert_eq!(vj["verified"], true);
    assert_eq!(vj["mode"], "verify");

    // Human-readable output also works cleanly (consistently Chinese).
    let human_out = run_xcpc(
        Some(dir.path()),
        &[
            "package",
            "build",
            "aplusb",
            "--tests",
            "sample-01",
            "--solutions",
            "std",
        ],
        None,
    );
    assert_eq!(human_out.status.code(), Some(0));
    let human_stdout = String::from_utf8_lossy(&human_out.stdout);
    assert!(
        human_stdout.contains("题目「aplusb」：通过"),
        "{human_stdout}"
    );
    assert!(human_stdout.contains("安全边际："), "{human_stdout}");

    // Validator failure prints concrete test name and validator error message in CLI output.
    fs::write(dir.path().join("problems/aplusb/samples/02.in"), "-5 3\n").unwrap();
    fs::write(dir.path().join("problems/aplusb/samples/02.ans"), "-2\n").unwrap();
    let val_fail_out = run_xcpc(
        Some(dir.path()),
        &["package", "build", "aplusb", "--tests", "sample-02"],
        None,
    );
    assert_eq!(val_fail_out.status.code(), Some(1));
    let val_fail_stdout = String::from_utf8_lossy(&val_fail_out.stdout);
    assert!(
        val_fail_stdout.contains("校验器未通过："),
        "{val_fail_stdout}"
    );
    assert!(val_fail_stdout.contains("sample-02"), "{val_fail_stdout}");
    fs::remove_file(dir.path().join("problems/aplusb/samples/02.in")).unwrap();
    fs::remove_file(dir.path().join("problems/aplusb/samples/02.ans")).unwrap();

    // Human-readable `contest freeze` error for unverified/stale package is natural Chinese + CLI hint.
    ws.compose_contest("c-check", &["aplusb".into()], None)
        .unwrap();
    let orig_std =
        fs::read_to_string(dir.path().join("problems/aplusb/solutions/std.cpp")).unwrap();
    fs::write(
        dir.path().join("problems/aplusb/solutions/std.cpp"),
        format!("{orig_std}// touch\n"),
    )
    .unwrap();
    let stale_freeze = run_xcpc(Some(dir.path()), &["contest", "freeze", "c-check"], None);
    assert_eq!(stale_freeze.status.code(), Some(1));
    let sf_stderr = String::from_utf8_lossy(&stale_freeze.stderr);
    assert!(sf_stderr.contains("题目 A（aplusb）"), "{sf_stderr}");
    assert!(sf_stderr.contains("重新检查"), "{sf_stderr}");
    assert!(
        sf_stderr.contains("xcpc package build aplusb --strict"),
        "{sf_stderr}"
    );
    assert!(sf_stderr.contains("未能定稿"), "{sf_stderr}");
    fs::write(
        dir.path().join("problems/aplusb/solutions/std.cpp"),
        orig_std,
    )
    .unwrap();

    // 4. After strict verify, `xcpc check problem aplusb` passes statically!
    let post_check = run_xcpc(
        Some(dir.path()),
        &["--json", "check", "problem", "aplusb"],
        None,
    );
    assert_eq!(post_check.status.code(), Some(0));

    // 5. Add a wrong solution and run `xcpc package stress` -> finds counterexample and saves `tests/stress-1.in`.
    fs::write(
        dir.path().join("problems/aplusb/solutions/wa.cpp"),
        "#include <iostream>\nint main() { long long a, b; std::cin >> a >> b; std::cout << (a - b) << '\\n'; }\n",
    )
    .unwrap();
    let stress_out = run_xcpc(
        Some(dir.path()),
        &[
            "--json",
            "package",
            "stress",
            "aplusb",
            "--gen",
            "gen_random",
            "--args",
            "-n 10 {i}",
            "--solutions",
            "wa",
            "--max-iter",
            "10",
            "--timeout",
            "15",
        ],
        None,
    );
    assert_eq!(
        stress_out.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&stress_out.stderr)
    );
    let sj = parse_json_stdout(&stress_out);
    assert_eq!(sj["found_counterexample"], true);
    assert_eq!(sj["failing_solution"], "wa");
    assert_eq!(sj["saved_test_path"], "tests/stress-1.in");
    assert!(
        dir.path()
            .join("problems/aplusb/tests/stress-1.in")
            .is_file()
    );
}

fn build_sample_polygon_zip(zip_path: &Path, short_name: &str) {
    let file = fs::File::create(zip_path).unwrap();
    let mut zw = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    let problem_xml = format!(
        r#"<?xml version="1.0" encoding="utf-8" standalone="no"?>
<problem short-name="{short_name}" revision="1">
  <names>
    <name language="english" value="Polygon Sum"/>
  </names>
  <judging cpu-name="Intel" cpu-speed="3600" input-file="" output-file="">
    <testset name="tests">
      <time-limit>1000</time-limit>
      <memory-limit>268435456</memory-limit>
      <test-count>2</test-count>
      <input-path-pattern>tests/%02d</input-path-pattern>
      <answer-path-pattern>tests/%02d.a</answer-path-pattern>
      <tests>
        <test method="manual" sample="true"/>
        <test method="manual"/>
      </tests>
    </testset>
  </judging>
  <files>
    <executables/>
  </files>
  <assets>
    <checker name="std::ncmp.cpp" type="testlib"/>
    <validators>
      <validator>
        <source path="files/validator.cpp" type="cpp.g++17"/>
        <testset>
          <tests>
            <test verdict="valid"/>
            <test verdict="invalid"/>
          </tests>
        </testset>
      </validator>
    </validators>
    <solutions>
      <solution tag="main">
        <source path="solutions/std.cpp" type="cpp.g++17"/>
      </solution>
    </solutions>
  </assets>
</problem>
"#
    );

    let entries: &[(&str, &str)] = &[
        ("problem.xml", &problem_xml),
        ("statement-sections/english/name.tex", "Polygon Sum\n"),
        (
            "statement-sections/english/legend.tex",
            "Compute $a + b$.\n",
        ),
        (
            "statement-sections/english/input.tex",
            "Two integers $a$ and $b$ ($0 \\le a, b \\le 10^9$).\n",
        ),
        ("statement-sections/english/output.tex", "Print $a + b$.\n"),
        ("tests/01", "1 2\n"),
        ("tests/01.a", "3\n"),
        ("tests/02", "10 20\n"),
        ("tests/02.a", "30\n"),
        ("files/tests/validator-tests/01", "1 2\n"),
        ("files/tests/validator-tests/02", "-1 2\n"),
        (
            "files/validator.cpp",
            r#"#include "testlib.h"
int main(int argc, char* argv[]) {
    registerValidation(argc, argv);
    inf.readInt(0, 1000000000, "a");
    inf.readSpace();
    inf.readInt(0, 1000000000, "b");
    inf.readEoln();
    inf.readEof();
    return 0;
}
"#,
        ),
        (
            "solutions/std.cpp",
            r#"#include <iostream>
int main() {
    long long a, b;
    if (std::cin >> a >> b) std::cout << (a + b) << '\n';
    return 0;
}
"#,
        ),
    ];

    for (name, content) in entries {
        zw.start_file(*name, opts).unwrap();
        zw.write_all(content.as_bytes()).unwrap();
    }
    zw.finish().unwrap();
}

#[test]
fn cli_import_polygon_and_export_oj_packages() {
    let dir = tempfile::tempdir().unwrap();
    xcpc_storage::init_workspace(dir.path()).unwrap();

    let external_dir = tempfile::tempdir().unwrap();
    let poly_zip = external_dir.path().join("poly-sum.zip");
    build_sample_polygon_zip(&poly_zip, "poly-sum");

    // 1. Read-only workspace rejects `import polygon` with exit code 2.
    let ro_import = run_xcpc(
        Some(dir.path()),
        &[
            "--read-only",
            "--json",
            "import",
            "polygon",
            poly_zip.to_str().unwrap(),
        ],
        None,
    );
    assert_eq!(ro_import.status.code(), Some(2));
    assert_eq!(parse_json_stdout(&ro_import)["error"]["code"], "read_only");

    // 2. Valid `import polygon --json` succeeds (exit 0) and returns PolygonImportResult.
    let imp_json = run_xcpc(
        Some(dir.path()),
        &["--json", "import", "polygon", poly_zip.to_str().unwrap()],
        None,
    );
    assert_eq!(
        imp_json.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&imp_json.stderr)
    );
    let ij = parse_json_stdout(&imp_json);
    assert_eq!(ij["ok"], true);
    assert_eq!(ij["kind"], "problem");
    assert_eq!(ij["problems"][0]["id"], "poly-sum");
    assert_eq!(ij["problems"][0]["samples_count"], 1);
    assert!(dir.path().join("problems/poly-sum/problem.toml").is_file());

    // 3. Slug conflict returns exit code 1 (`target_exists`).
    let dup = run_xcpc(
        Some(dir.path()),
        &["--json", "import", "polygon", poly_zip.to_str().unwrap()],
        None,
    );
    assert_eq!(dup.status.code(), Some(1));
    assert_eq!(parse_json_stdout(&dup)["error"]["code"], "target_exists");

    // 4. Human output with `--id` override succeeds (exit 0) with concise Chinese summary.
    let imp_human = run_xcpc(
        Some(dir.path()),
        &[
            "import",
            "polygon",
            poly_zip.to_str().unwrap(),
            "--id",
            "poly-sum-2",
        ],
        None,
    );
    assert_eq!(imp_human.status.code(), Some(0));
    let human_out = String::from_utf8_lossy(&imp_human.stdout);
    assert!(
        human_out.contains("已导入题目「poly-sum-2」"),
        "{human_out}"
    );

    // 5. `export domjudge` before strict verify fails with exit code 1 (`not_verified`).
    let unverified = run_xcpc(
        Some(dir.path()),
        &["--json", "export", "domjudge", "problem", "poly-sum"],
        None,
    );
    assert_eq!(unverified.status.code(), Some(1));
    assert_eq!(
        parse_json_stdout(&unverified)["error"]["code"],
        "not_verified"
    );

    // 6. Strict verify the imported problem, then export both DOMjudge and Hydro packages.
    let verify = run_xcpc(
        Some(dir.path()),
        &["--json", "package", "build", "poly-sum", "--strict"],
        None,
    );
    assert_eq!(
        verify.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&verify.stderr)
    );
    assert_eq!(parse_json_stdout(&verify)["verified"], true);

    let dj_out = run_xcpc(
        Some(dir.path()),
        &["--json", "export", "domjudge", "problem", "poly-sum"],
        None,
    );
    assert_eq!(
        dj_out.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&dj_out.stderr)
    );
    let dj_json = parse_json_stdout(&dj_out);
    assert_eq!(dj_json["ok"], true);
    assert_eq!(dj_json["format"], "domjudge");
    let dj_zip_path = dj_json["artifacts"][0]["zip_path"].as_str().unwrap();
    assert!(Path::new(dj_zip_path).is_file());
    {
        let mut archive = zip::ZipArchive::new(fs::File::open(dj_zip_path).unwrap()).unwrap();
        assert!(archive.by_name("domjudge-problem.ini").is_ok());
        assert!(archive.by_name("problem.yaml").is_ok());
    }

    let hydro_custom = external_dir.path().join("poly-sum-hydro.zip");
    let hydro_out = run_xcpc(
        Some(dir.path()),
        &[
            "export",
            "hydro",
            "problem",
            "poly-sum",
            "-o",
            hydro_custom.to_str().unwrap(),
        ],
        None,
    );
    assert_eq!(
        hydro_out.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&hydro_out.stderr)
    );
    let hydro_stdout = String::from_utf8_lossy(&hydro_out.stdout);
    assert!(
        hydro_stdout.contains("已导出 Hydro 题目包"),
        "{hydro_stdout}"
    );
    assert!(hydro_custom.is_file());
    {
        let mut archive = zip::ZipArchive::new(fs::File::open(&hydro_custom).unwrap()).unwrap();
        assert!(archive.by_name("problem.yaml").is_ok());
        assert!(archive.by_name("testdata/config.yaml").is_ok());
    }

    // 7. Read-only workspace rejects `export` with exit code 2.
    let ro_export = run_xcpc(
        Some(dir.path()),
        &[
            "--read-only",
            "--json",
            "export",
            "domjudge",
            "problem",
            "poly-sum",
        ],
        None,
    );
    assert_eq!(ro_export.status.code(), Some(2));
    assert_eq!(parse_json_stdout(&ro_export)["error"]["code"], "read_only");
}

#[test]
fn cli_groups_search_and_batch_metadata_and_delete() {
    let dir = setup_workspace();

    // 1. Group CRUD: create, list, rename, reorder, delete.
    let c1 = run_xcpc(
        Some(dir.path()),
        &["--json", "group", "create", "省赛候选"],
        None,
    );
    assert_eq!(c1.status.code(), Some(0));
    assert_eq!(parse_json_stdout(&c1)["name"], "省赛候选");

    let c2 = run_xcpc(
        Some(dir.path()),
        &["--json", "group", "create", "校赛备选"],
        None,
    );
    assert_eq!(c2.status.code(), Some(0));

    // Read-only allows `group list`, refuses mutations.
    let ro_list = run_xcpc(
        Some(dir.path()),
        &["--read-only", "--json", "group", "list"],
        None,
    );
    assert_eq!(ro_list.status.code(), Some(0));
    let ro_list_j = parse_json_stdout(&ro_list);
    assert_eq!(ro_list_j["groups"].as_array().unwrap().len(), 2);
    assert_eq!(ro_list_j["ungrouped_count"], 2);

    let ro_create = run_xcpc(
        Some(dir.path()),
        &["--read-only", "--json", "group", "create", "禁止"],
        None,
    );
    assert_eq!(ro_create.status.code(), Some(2));
    assert_eq!(parse_json_stdout(&ro_create)["error"]["code"], "read_only");

    // Reorder groups.
    let reo = run_xcpc(
        Some(dir.path()),
        &["--json", "group", "reorder", "校赛备选", "省赛候选"],
        None,
    );
    assert_eq!(reo.status.code(), Some(0));
    assert_eq!(
        parse_json_stdout(&reo)["groups"],
        serde_json::json!(["校赛备选", "省赛候选"])
    );

    // 2. Batch assign group + search --group / --ungrouped + conflict.
    let pg = run_xcpc(
        Some(dir.path()),
        &["--json", "problem", "group", "省赛候选", "p1"],
        None,
    );
    assert_eq!(pg.status.code(), Some(0));
    assert_eq!(parse_json_stdout(&pg)["succeeded"], 1);

    let sg = run_xcpc(
        Some(dir.path()),
        &["--json", "search", "--group", "省赛候选"],
        None,
    );
    assert_eq!(sg.status.code(), Some(0));
    let sg_probs = parse_json_stdout(&sg)["problems"]
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(sg_probs.len(), 1);
    assert_eq!(sg_probs[0]["id"], "p1");

    let su = run_xcpc(Some(dir.path()), &["--json", "search", "--ungrouped"], None);
    assert_eq!(su.status.code(), Some(0));
    let su_probs = parse_json_stdout(&su)["problems"]
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(su_probs.len(), 1);
    assert_eq!(su_probs[0]["id"], "p2");

    // `--group` conflicts with `--ungrouped` (clap usage error -> exit 2).
    let conflict = run_xcpc(
        Some(dir.path()),
        &["search", "--group", "省赛候选", "--ungrouped"],
        None,
    );
    assert_eq!(conflict.status.code(), Some(2));

    // Rename group updates member problem.
    let ren = run_xcpc(
        Some(dir.path()),
        &["--json", "group", "rename", "省赛候选", "省赛正选"],
        None,
    );
    assert_eq!(ren.status.code(), Some(0));
    assert_eq!(parse_json_stdout(&ren)["updated_problems"], 1);

    // Clear group from p1.
    let clr_g = run_xcpc(
        Some(dir.path()),
        &["--json", "problem", "group", "--clear", "p1"],
        None,
    );
    assert_eq!(clr_g.status.code(), Some(0));
    assert_eq!(parse_json_stdout(&clr_g)["succeeded"], 1);

    // Delete group.
    let del_g = run_xcpc(
        Some(dir.path()),
        &["--json", "group", "delete", "省赛正选"],
        None,
    );
    assert_eq!(del_g.status.code(), Some(0));

    // 3. Batch tags and ratings.
    let tag_res = run_xcpc(
        Some(dir.path()),
        &[
            "--json", "problem", "tag", "--add", "graphs", "--remove", "math", "p1", "p2",
        ],
        None,
    );
    assert_eq!(tag_res.status.code(), Some(0));
    assert_eq!(parse_json_stdout(&tag_res)["succeeded"], 2);

    let rat_set = run_xcpc(
        Some(dir.path()),
        &["--json", "problem", "rating", "2100", "p1", "p2"],
        None,
    );
    assert_eq!(rat_set.status.code(), Some(0));
    assert_eq!(parse_json_stdout(&rat_set)["succeeded"], 2);

    let rat_clr = run_xcpc(
        Some(dir.path()),
        &["--json", "problem", "rating", "--clear", "p2"],
        None,
    );
    assert_eq!(rat_clr.status.code(), Some(0));
    assert_eq!(parse_json_stdout(&rat_clr)["succeeded"], 1);

    // Read-only refuses problem tag/rating/group/delete.
    for args in [
        &[
            "--read-only",
            "--json",
            "problem",
            "group",
            "校赛备选",
            "p1",
        ][..],
        &[
            "--read-only",
            "--json",
            "problem",
            "tag",
            "--add",
            "x",
            "p1",
        ][..],
        &["--read-only", "--json", "problem", "rating", "1500", "p1"][..],
        &["--read-only", "--json", "problem", "delete", "--yes", "p1"][..],
    ] {
        let ro = run_xcpc(Some(dir.path()), args, None);
        assert_eq!(ro.status.code(), Some(2), "args={args:?}");
        assert_eq!(parse_json_stdout(&ro)["error"]["code"], "read_only");
    }

    // 4. Problem delete: reference guard + confirmation.
    let compose = run_xcpc(
        Some(dir.path()),
        &["--json", "contest", "compose", "c1", "p1"],
        None,
    );
    assert_eq!(compose.status.code(), Some(0));

    // Without `--yes` and empty stdin -> aborted (`unconfirmed`, exit 2).
    let unconf = run_xcpc(
        Some(dir.path()),
        &["--json", "problem", "delete", "p2"],
        None,
    );
    assert_eq!(unconf.status.code(), Some(2));
    assert_eq!(parse_json_stdout(&unconf)["error"]["code"], "unconfirmed");
    assert!(dir.path().join("problems/p2").is_dir());

    // With `--yes`: p1 is skipped (`problem_referenced`), p2 is deleted.
    let del = run_xcpc(
        Some(dir.path()),
        &["--json", "problem", "delete", "--yes", "p1", "p2"],
        None,
    );
    assert_eq!(del.status.code(), Some(0));
    let dj = parse_json_stdout(&del);
    assert_eq!(dj["succeeded"], 1);
    assert_eq!(dj["skipped"], 1);
    assert_eq!(dj["items"][0]["code"], "problem_referenced");
    assert_eq!(
        dj["items"][0]["referencing_contests"],
        serde_json::json!(["c1"])
    );
    assert!(dir.path().join("problems/p1").is_dir());
    assert!(!dir.path().join("problems/p2").exists());
}

#[test]
fn cli_batch_build_packages_export_problems_import_group_and_merged_pdf() {
    let dir = setup_workspace();
    let external_dir = tempfile::tempdir().unwrap();
    let poly_zip = external_dir.path().join("poly-batch.zip");
    build_sample_polygon_zip(&poly_zip, "poly-batch");

    // 1. `import polygon --group <name> --no-auto-group` assigns explicit group.
    let imp = run_xcpc(
        Some(dir.path()),
        &[
            "--json",
            "import",
            "polygon",
            poly_zip.to_str().unwrap(),
            "--id",
            "poly-1",
            "--group",
            "导入集",
            "--no-auto-group",
        ],
        None,
    );
    assert_eq!(
        imp.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&imp.stderr)
    );
    assert_eq!(parse_json_stdout(&imp)["group"], "导入集");

    // 2. Multi-ID `package build <ID1> <ID2> --strict --json` runs batch build:
    //    `poly-1` passes & is verified, pure-publishing `p2` is skipped (`not_a_package`).
    let bb = run_xcpc(
        Some(dir.path()),
        &["--json", "package", "build", "poly-1", "p2", "--strict"],
        None,
    );
    assert_eq!(
        bb.status.code(),
        Some(0),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&bb.stdout),
        String::from_utf8_lossy(&bb.stderr)
    );
    let bbj = parse_json_stdout(&bb);
    assert_eq!(bbj["ok"], true);
    assert_eq!(bbj["passed"], 1);
    assert_eq!(bbj["skipped"], 1);
    assert_eq!(bbj["items"][0]["problem_id"], "poly-1");
    assert_eq!(bbj["items"][0]["verified"], true);
    assert_eq!(bbj["items"][1]["problem_id"], "p2");
    assert_eq!(bbj["items"][1]["status"], "skipped");
    assert_eq!(bbj["items"][1]["code"], "not_a_package");

    // 3. `export domjudge problems <ID>... -o <DIR> --json` exports verified & skips pure-publishing.
    let export_dir = external_dir.path().join("batch-dj");
    let be = run_xcpc(
        Some(dir.path()),
        &[
            "--json",
            "export",
            "domjudge",
            "problems",
            "poly-1",
            "p2",
            "-o",
            export_dir.to_str().unwrap(),
        ],
        None,
    );
    assert_eq!(
        be.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&be.stderr)
    );
    let bej = parse_json_stdout(&be);
    assert_eq!(bej["ok"], true);
    assert_eq!(bej["exported"], 1);
    assert_eq!(bej["skipped"], 1);
    assert!(export_dir.join("poly-1.zip").is_file());

    // Read-only rejects batch export.
    let ro_be = run_xcpc(
        Some(dir.path()),
        &[
            "--read-only",
            "--json",
            "export",
            "hydro",
            "problems",
            "poly-1",
        ],
        None,
    );
    assert_eq!(ro_be.status.code(), Some(2));
    assert_eq!(parse_json_stdout(&ro_be)["error"]["code"], "read_only");

    // 4. `build problems <ID>...` produces merged PDF without creating any contest directory.
    let merged = run_xcpc(
        Some(dir.path()),
        &[
            "--json",
            "build",
            "problems",
            "p1",
            "p2",
            "--title",
            "精选训练",
            "--lang",
            "zh",
            "--cover",
            "true",
        ],
        None,
    );
    assert_eq!(
        merged.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&merged.stderr)
    );
    let mj = parse_json_stdout(&merged);
    assert_eq!(mj["ok"], true);
    assert!(mj["pages"].as_u64().unwrap() >= 2);
    let out_pdf = mj["outputs"][0].as_str().unwrap();
    assert!(Path::new(out_pdf).is_file());
    // Must not create any contest directory.
    let contests_entries: Vec<_> = fs::read_dir(dir.path().join("contests")).unwrap().collect();
    assert!(contests_entries.is_empty());

    // Editorial merged PDF with explicit `-o`.
    let ed_pdf = external_dir.path().join("merged-editorial.pdf");
    let merged_ed = run_xcpc(
        Some(dir.path()),
        &[
            "--json",
            "build",
            "problems",
            "p1",
            "p2",
            "--editorial",
            "-o",
            ed_pdf.to_str().unwrap(),
        ],
        None,
    );
    assert_eq!(
        merged_ed.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&merged_ed.stderr)
    );
    assert!(ed_pdf.is_file());

    // Read-only rejects `build problems` writing into default workspace `build/`.
    let ro_merged = run_xcpc(
        Some(dir.path()),
        &["--read-only", "--json", "build", "problems", "p1", "p2"],
        None,
    );
    assert_eq!(ro_merged.status.code(), Some(2));
    assert_eq!(parse_json_stdout(&ro_merged)["error"]["code"], "read_only");
}

#[test]
fn cli_package_phase_3c_init_inspect_build_and_stress() {
    let dir = tempfile::tempdir().unwrap();
    let ws = xcpc_storage::init_workspace(dir.path()).unwrap();
    ws.create_problem("guess", "Guess").unwrap();

    // 1. `xcpc package init guess --interactive --runs 2 --subtasks --json`
    let init = run_xcpc(
        Some(dir.path()),
        &[
            "--json",
            "package",
            "init",
            "guess",
            "--interactive",
            "--runs",
            "2",
            "--subtasks",
        ],
        None,
    );
    assert_eq!(
        init.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&init.stderr)
    );
    let init_json = parse_json_stdout(&init);
    assert_eq!(init_json["ok"], true);
    assert!(dir.path().join("problems/guess/interactor.cpp").is_file());

    // 2. `xcpc package inspect guess --json` (also works under --read-only)
    let insp = run_xcpc(
        Some(dir.path()),
        &["--read-only", "--json", "package", "inspect", "guess"],
        None,
    );
    assert_eq!(
        insp.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&insp.stderr)
    );
    let insp_json = parse_json_stdout(&insp);
    assert_eq!(insp_json["interactive"], true);
    assert_eq!(insp_json["runs"], 2);
    assert!(insp_json["subtasks"].as_array().unwrap().len() >= 2);
    assert_eq!(insp_json["total_score"], 100);
    assert_eq!(insp_json["files"]["interactor"], "interactor.cpp");

    // Human-readable inspect output surfaces interactive/runs/subtasks.
    let insp_human = run_xcpc(Some(dir.path()), &["package", "inspect", "guess"], None);
    assert_eq!(insp_human.status.code(), Some(0));
    let human_stdout = String::from_utf8_lossy(&insp_human.stdout);
    assert!(human_stdout.contains("交互题"), "{human_stdout}");
    assert!(human_stdout.contains("多轮 ×2"), "{human_stdout}");
    assert!(human_stdout.contains("子任务"), "{human_stdout}");

    // 3. Build & stress an interactive single-run problem with a buggy solution to verify transcript & scores.
    ws.create_problem("echo-int", "Echo Interactive").unwrap();
    let init2 = run_xcpc(
        Some(dir.path()),
        &[
            "--json",
            "package",
            "init",
            "echo-int",
            "--interactive",
            "--subtasks",
        ],
        None,
    );
    assert_eq!(init2.status.code(), Some(0));

    // Add a wrong solution that always prints -1.
    fs::write(
        dir.path().join("problems/echo-int/solutions/wa.cpp"),
        "#include <iostream>\nint main(){ int x; if (std::cin >> x) std::cout << -1 << std::endl; return 0; }\n",
    )
    .unwrap();
    let mut meta = ws.load_problem("echo-int").unwrap().meta;
    meta.solutions.insert(
        "wa".to_owned(),
        xcpc_domain::SolutionSpec {
            main: None,
            expect: Some(xcpc_domain::ExpectVerdict::Single("WA".to_owned())),
            tags: None,
            score: Some(xcpc_domain::ScoreExpect::Exact(0)),
            subtasks: std::collections::BTreeMap::new(),
        },
    );
    ws.save_problem_meta("echo-int", &meta).unwrap();

    let build = run_xcpc(
        Some(dir.path()),
        &["--json", "package", "build", "echo-int"],
        None,
    );
    assert_eq!(
        build.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&build.stderr)
    );
    let build_json = parse_json_stdout(&build);
    assert_eq!(build_json["interactive"], true);
    assert_eq!(build_json["runs"], 1);
    assert_eq!(build_json["total_score"], 100);
    assert!(!build_json["subtasks"].as_array().unwrap().is_empty());
    let sols = build_json["solutions"].as_array().unwrap();
    let std_sol = sols.iter().find(|s| s["name"] == "std").unwrap();
    assert_eq!(std_sol["total_score"], 100.0);
    let wa_sol = sols.iter().find(|s| s["name"] == "wa").unwrap();
    assert_eq!(wa_sol["total_score"], 0.0);
    let wa_cell = build_json["matrix"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["solution"] == "wa")
        .unwrap();
    assert!(wa_cell["transcript"].is_object(), "{wa_cell}");

    // Stress interactive problem -> finds counterexample with transcript.
    let stress = run_xcpc(
        Some(dir.path()),
        &[
            "--json",
            "package",
            "stress",
            "echo-int",
            "--gen",
            "gen_random",
            "--args",
            "{i}",
            "--solutions",
            "wa",
            "--max-iter",
            "5",
        ],
        None,
    );
    assert_eq!(
        stress.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&stress.stderr)
    );
    let stress_json = parse_json_stdout(&stress);
    assert_eq!(stress_json["found_counterexample"], true);
    assert!(stress_json["transcript"].is_object(), "{stress_json}");
}

#[test]
fn cli_phase_3d_sanitize_audit_external_and_adopt() {
    let dir = tempfile::tempdir().unwrap();
    let ws = xcpc_storage::init_workspace(dir.path()).unwrap();
    ws.create_problem("sum", "Sum").unwrap();
    ws.create_problem("pub-only", "Pub Only").unwrap();

    // Initialize package for `sum` and compose contest `round-3d`.
    let init = run_xcpc(
        Some(dir.path()),
        &["--json", "package", "init", "sum"],
        None,
    );
    assert_eq!(init.status.code(), Some(0));

    let compose = run_xcpc(
        Some(dir.path()),
        &[
            "--json", "contest", "compose", "round-3d", "sum", "pub-only",
        ],
        None,
    );
    assert_eq!(compose.status.code(), Some(0));

    // Strict verify `sum`.
    let ver = run_xcpc(
        Some(dir.path()),
        &["--json", "package", "build", "sum", "--strict"],
        None,
    );
    assert_eq!(
        ver.status.code(),
        Some(0),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&ver.stdout),
        String::from_utf8_lossy(&ver.stderr)
    );
    assert_eq!(parse_json_stdout(&ver)["verified"], true);
    let report_before = fs::read(dir.path().join("build/problems/sum/report.json")).unwrap();

    // 1. `package build sum --sanitize --json` (and conflict / read-only checks).
    let conflict = run_xcpc(
        Some(dir.path()),
        &["package", "build", "sum", "--strict", "--sanitize"],
        None,
    );
    assert_eq!(conflict.status.code(), Some(2));

    let ro_san = run_xcpc(
        Some(dir.path()),
        &[
            "--read-only",
            "--json",
            "package",
            "build",
            "sum",
            "--sanitize",
        ],
        None,
    );
    assert_eq!(ro_san.status.code(), Some(2));
    assert_eq!(parse_json_stdout(&ro_san)["error"]["code"], "read_only");

    let san = run_xcpc(
        Some(dir.path()),
        &["--json", "package", "build", "sum", "--sanitize"],
        None,
    );
    let san_json = parse_json_stdout(&san);
    let san_status = san_json["status"].as_str().unwrap();
    assert!(
        matches!(san_status, "passed" | "unsupported"),
        "unexpected sanitizer status: {san_json}"
    );
    if san_status == "passed" {
        assert_eq!(san.status.code(), Some(0));
        assert_eq!(san_json["ok"], true);
    }
    assert!(
        dir.path()
            .join("build/problems/sum/sanitizer-report.json")
            .is_file()
    );
    // Must not overwrite normal report.json.
    let report_after = fs::read(dir.path().join("build/problems/sum/report.json")).unwrap();
    assert_eq!(report_before, report_after);

    let san_human = run_xcpc(
        Some(dir.path()),
        &["package", "build", "sum", "--sanitize"],
        None,
    );
    let san_stdout = String::from_utf8_lossy(&san_human.stdout);
    assert!(san_stdout.contains("内存检查"), "{san_stdout}");

    // 2. `contest audit round-3d` (read-only, JSON + Markdown + human).
    let ext_dir = tempfile::tempdir().unwrap();
    let audit_md = ext_dir.path().join("audit.md");
    let audit = run_xcpc(
        Some(dir.path()),
        &[
            "--read-only",
            "--json",
            "contest",
            "audit",
            "round-3d",
            "--markdown",
            audit_md.to_str().unwrap(),
        ],
        None,
    );
    assert_eq!(
        audit.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&audit.stderr)
    );
    let audit_json = parse_json_stdout(&audit);
    assert_eq!(audit_json["ok"], true);
    assert_eq!(audit_json["contest_id"], "round-3d");
    let rows = audit_json["problems"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["verification_status"], "fresh");
    if san_status == "passed" {
        assert_eq!(rows[0]["sanitizer_status"], "passed_fresh");
    }
    assert_eq!(rows[1]["verification_status"], "statement_only");
    assert!(
        rows[1]["lint_warning_count"].as_u64().unwrap() >= 1,
        "expected xcpc-lint warning for pub-only: {rows:?}"
    );
    assert!(audit_md.is_file());
    let audit_md_text = fs::read_to_string(&audit_md).unwrap();
    assert!(audit_md_text.contains("sum"), "{audit_md_text}");

    let audit_human = run_xcpc(
        Some(dir.path()),
        &["--read-only", "contest", "audit", "round-3d"],
        None,
    );
    assert_eq!(audit_human.status.code(), Some(0));
    let audit_human_stdout = String::from_utf8_lossy(&audit_human.stdout);
    assert!(
        audit_human_stdout.contains("检查结果：可以定稿"),
        "{audit_human_stdout}"
    );

    // 3. External trial run (multi-tester contest + single problem) & adopt.
    let testers_dir = ext_dir.path().join("testers");
    fs::create_dir_all(testers_dir.join("alice")).unwrap();
    fs::create_dir_all(testers_dir.join("bob")).unwrap();
    let ac_cpp = testers_dir.join("alice/A.cpp");
    let wa_cpp = testers_dir.join("bob/sum.cpp");
    let unmatched_cpp = testers_dir.join("bob/unknown.cpp");
    fs::write(
        &ac_cpp,
        "#include <iostream>\nint main(){ long long a, b; if (std::cin >> a >> b) std::cout << (a + b) << '\\n'; return 0; }\n",
    )
    .unwrap();
    fs::write(
        &wa_cpp,
        "#include <iostream>\nint main(){ long long a, b; if (std::cin >> a >> b) std::cout << (a + b + 1) << '\\n'; return 0; }\n",
    )
    .unwrap();
    fs::write(&unmatched_cpp, "int main(){ return 0; }\n").unwrap();

    let ext_md = ext_dir.path().join("external.md");
    let cv = run_xcpc(
        Some(dir.path()),
        &[
            "--read-only",
            "--json",
            "contest",
            "verify-external",
            "round-3d",
            testers_dir.to_str().unwrap(),
            "--markdown",
            ext_md.to_str().unwrap(),
        ],
        None,
    );
    assert_eq!(
        cv.status.code(),
        Some(0),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&cv.stdout),
        String::from_utf8_lossy(&cv.stderr)
    );
    let cv_json = parse_json_stdout(&cv);
    assert_eq!(cv_json["ok"], true);
    assert_eq!(cv_json["mode"], "multi_tester");
    let subs = cv_json["submissions"].as_array().unwrap();
    assert_eq!(subs.len(), 2);
    assert_eq!(cv_json["unmatched"].as_array().unwrap().len(), 1);
    let alice_sub = subs.iter().find(|s| s["tester"] == "alice").unwrap();
    assert_eq!(alice_sub["verdict"], "AC");
    let bob_sub = subs.iter().find(|s| s["tester"] == "bob").unwrap();
    assert_eq!(bob_sub["verdict"], "WA");
    assert!(ext_md.is_file());
    // Zero workspace pollution: solutions/ still only has std.cpp.
    assert!(!dir.path().join("problems/sum/solutions/A.cpp").exists());

    // Read-only rejects `--auto-build`.
    let ro_ab = run_xcpc(
        Some(dir.path()),
        &[
            "--read-only",
            "--json",
            "contest",
            "verify-external",
            "round-3d",
            testers_dir.to_str().unwrap(),
            "--auto-build",
        ],
        None,
    );
    assert_eq!(ro_ab.status.code(), Some(2));
    assert_eq!(parse_json_stdout(&ro_ab)["error"]["code"], "read_only");

    // Single-problem external trial run.
    let pv = run_xcpc(
        Some(dir.path()),
        &[
            "--read-only",
            "--json",
            "package",
            "verify-external",
            "sum",
            ac_cpp.to_str().unwrap(),
        ],
        None,
    );
    assert_eq!(pv.status.code(), Some(0));
    let pv_json = parse_json_stdout(&pv);
    assert_eq!(pv_json["ok"], true);
    assert_eq!(pv_json["submissions"][0]["verdict"], "AC");

    // Adopt external solution into solutions/ and problem.toml.
    let ro_adopt = run_xcpc(
        Some(dir.path()),
        &[
            "--read-only",
            "--json",
            "package",
            "adopt",
            "sum",
            ac_cpp.to_str().unwrap(),
        ],
        None,
    );
    assert_eq!(ro_adopt.status.code(), Some(2));
    assert_eq!(parse_json_stdout(&ro_adopt)["error"]["code"], "read_only");

    let adopt = run_xcpc(
        Some(dir.path()),
        &[
            "--json",
            "package",
            "adopt",
            "sum",
            ac_cpp.to_str().unwrap(),
            "--name",
            "alice_ac",
            "--expect",
            "AC",
        ],
        None,
    );
    assert_eq!(
        adopt.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&adopt.stderr)
    );
    let adopt_json = parse_json_stdout(&adopt);
    assert_eq!(adopt_json["ok"], true);
    assert_eq!(adopt_json["solution_name"], "alice_ac");
    assert_eq!(
        adopt_json["dest_rel"],
        "problems/sum/solutions/alice_ac.cpp"
    );
    assert_eq!(adopt_json["updated_problem_toml"], true);
    assert!(
        dir.path()
            .join("problems/sum/solutions/alice_ac.cpp")
            .is_file()
    );
    let toml_text = fs::read_to_string(dir.path().join("problems/sum/problem.toml")).unwrap();
    assert!(toml_text.contains("[solutions.alice_ac]"), "{toml_text}");
}

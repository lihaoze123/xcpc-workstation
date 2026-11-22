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

    // Fix the image -> freeze succeeds with exit code 0 (even with p2 missing editorial warning).
    fs::create_dir_all(dir.path().join("problems/p1/assets")).unwrap();
    fs::write(dir.path().join("problems/p1/assets/missing.png"), TINY_PNG).unwrap();

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
}

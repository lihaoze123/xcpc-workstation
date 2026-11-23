//! Phase 3C integration tests:
//! - Interactive (`interactor.cpp`): AC, WA, ILE, TLE, quitp, bounded transcript, 2-stage checker,
//!   samples/ opt-in, strict gate without tests/interactor/, interactive stress_package.
//! - Multi-run (`runs = N`): with & without interactor, cumulative CPU limit.
//! - OI Subtasks (`[[subtask]]`): min/sum/depends/quitp, --group validator & grouped self-tests,
//!   solution score/subtasks assertions, strict gate errors (cycle/unknown/empty/unassigned) & sum!=100 warning.
//! - Ops CRUD & `init_package_with_options` preserving comments and unknown fields.

use std::collections::BTreeMap;
use std::fs;
use tempfile::TempDir;
use xcpc_domain::{ExpectVerdict, ScoreExpect, SubtaskScoring, SubtaskSpec};
use xcpc_judge::{
    BuildMode, BuildOptions, InitPackageOptions, PackageSourceKind, StressOptions,
    TranscriptDirection, Verdict, build_package, create_package_source, delete_interactor,
    delete_subtask, expand_tests, init_package_with_options, inspect_package,
    parse_testlib_verdict, set_package_runs, set_solution_scoring, set_test_subtask,
    stress_package, upsert_subtask, validate_subtask_config,
};
use xcpc_storage::Workspace;

fn setup_problem(slug: &str, problem_toml: &str) -> (TempDir, Workspace) {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("workspace.toml"), "").unwrap();
    let pdir = dir.path().join("problems").join(slug);
    fs::create_dir_all(pdir.join("samples")).unwrap();
    fs::write(pdir.join("problem.toml"), problem_toml).unwrap();
    fs::write(
        pdir.join("statement.md"),
        "描述。\n\n## 交互方式\n\n按行交互。\n\n## 子任务\n\n见下。\n",
    )
    .unwrap();
    let ws = Workspace::open(dir.path(), false).unwrap();
    (dir, ws)
}

#[test]
fn parse_testlib_quitp_and_verdicts() {
    let ok = parse_testlib_verdict(Some(0), "ok 1 number");
    assert_eq!(ok.verdict, Verdict::AC);
    assert!((ok.score_ratio - 1.0).abs() < 1e-9);

    let wa = parse_testlib_verdict(Some(1), "wrong answer");
    assert_eq!(wa.verdict, Verdict::WA);
    assert_eq!(wa.score_ratio, 0.0);

    let pe = parse_testlib_verdict(Some(2), "presentation error");
    assert_eq!(pe.verdict, Verdict::PE);

    let partial = parse_testlib_verdict(Some(7), "\x1b[1;33mpoints 0.5\x1b[0m half credit");
    assert_eq!(partial.verdict, Verdict::AC);
    assert!((partial.score_ratio - 0.5).abs() < 1e-9);
    assert!(partial.message.contains("half credit"));

    let zero_partial = parse_testlib_verdict(Some(7), "points 0.0 none");
    assert_eq!(zero_partial.verdict, Verdict::WA);
    assert_eq!(zero_partial.score_ratio, 0.0);

    let eof = parse_testlib_verdict(Some(8), "unexpected eof");
    assert_eq!(eof.verdict, Verdict::WA);
}

#[test]
fn interactive_ac_wa_ile_tle_quitp_transcript_samples_and_stress() {
    let toml = r#"
title = "Guess Number"

[limits]
time = "400ms"
memory = "256MB"

[[test]]
file = "tests/t1.in"
name = "case-1"

[[test]]
file = "tests/t2.in"
name = "case-2"

[solutions.std]
main = true

[solutions.wrong_wa]
expect = "WA"

[solutions.deadlock_ile]
expect = "ILE"

[solutions.cpu_tle]
expect = "TLE"

[solutions.partial_sol]
expect = "AC"
"#;
    let (_dir, ws) = setup_problem("guess", toml);
    let cache_dir = TempDir::new().unwrap();

    // Put a sample in samples/ that should NOT be auto-included for interactive problems.
    ws.write("problems/guess/samples/01.in", b"not-a-valid-input\n")
        .unwrap();
    ws.write("problems/guess/samples/01.ans", b"dialogue\n")
        .unwrap();

    ws.write("problems/guess/tests/t1.in", b"42\n").unwrap();
    ws.write("problems/guess/tests/t2.in", b"7\n").unwrap();

    // Validator + self-tests.
    ws.write(
        "problems/guess/validator.cpp",
        br#"#include "testlib.h"
int main(int argc, char* argv[]) {
    registerValidation(argc, argv);
    inf.readInt(1, 100, "x");
    inf.readEoln();
    inf.readEof();
    return 0;
}
"#,
    )
    .unwrap();
    ws.write("problems/guess/tests/validator/valid/01.in", b"50\n")
        .unwrap();
    ws.write("problems/guess/tests/validator/invalid/01.in", b"0\n")
        .unwrap();

    // Interactor: sends `1 100`, accepts `? g` queries (`<`, `>`, `=`) or `! ans`.
    // Returns quitp(0.5) if solution outputs `! -1` (special partial marker), _ok if ans == x, _wa otherwise.
    ws.write(
        "problems/guess/interactor.cpp",
        br#"#include "testlib.h"
#include <iostream>
#include <string>

int main(int argc, char* argv[]) {
    registerInteraction(argc, argv);
    int secret = inf.readInt();
    std::cout << "1 100" << std::endl;
    for (int q = 0; q < 20; ++q) {
        std::string cmd = ouf.readWord();
        int v = ouf.readInt();
        if (cmd == "?") {
            if (v < secret) std::cout << "<" << std::endl;
            else if (v > secret) std::cout << ">" << std::endl;
            else std::cout << "=" << std::endl;
        } else if (cmd == "!") {
            tout << v << '\n';
            if (v == -1) {
                quitp(0.5, "partial credit");
            }
            if (v == secret) {
                quitf(_ok, "guessed %d in %d queries", secret, q);
            } else {
                quitf(_wa, "expected %d, got %d", secret, v);
            }
        } else {
            quitf(_wa, "unknown command %s", cmd.c_str());
        }
    }
    quitf(_wa, "too many queries");
}
"#,
    )
    .unwrap();

    // std: binary search with flush.
    ws.write(
        "problems/guess/solutions/std.cpp",
        br#"#include <iostream>
#include <string>
int main() {
    int l, r;
    if (!(std::cin >> l >> r)) return 0;
    while (l < r) {
        int m = (l + r) / 2;
        std::cout << "? " << m << std::endl;
        std::string resp;
        if (!(std::cin >> resp)) return 0;
        if (resp == "<") l = m + 1;
        else if (resp == ">") r = m - 1;
        else { l = r = m; break; }
    }
    std::cout << "! " << l << std::endl;
    return 0;
}
"#,
    )
    .unwrap();

    // wrong_wa: always guesses 1.
    ws.write(
        "problems/guess/solutions/wrong_wa.cpp",
        br#"#include <iostream>
int main() {
    int l, r;
    if (std::cin >> l >> r) {
        std::cout << "! 1" << std::endl;
    }
    return 0;
}
"#,
    )
    .unwrap();

    // deadlock_ile: prints without flushing and waits for response => ILE!
    ws.write(
        "problems/guess/solutions/deadlock_ile.cpp",
        br#"#include <iostream>
#include <string>
int main() {
    std::ios::sync_with_stdio(false);
    std::cin.tie(nullptr);
    int l, r;
    if (std::cin >> l >> r) {
        std::cout << "? 50\n"; // intentionally NOT flushed!
        std::string resp;
        std::cin >> resp;
    }
    return 0;
}
"#,
    )
    .unwrap();

    // cpu_tle: burns CPU > 400ms => TLE (NOT ILE)!
    ws.write(
        "problems/guess/solutions/cpu_tle.cpp",
        br#"#include <iostream>
int main() {
    int l, r;
    std::cin >> l >> r;
    volatile long long s = 0;
    while (true) { s += 1; }
    return 0;
}
"#,
    )
    .unwrap();

    // partial_sol: triggers quitp(0.5).
    ws.write(
        "problems/guess/solutions/partial_sol.cpp",
        br#"#include <iostream>
int main() {
    int l, r;
    if (std::cin >> l >> r) {
        std::cout << "! -1" << std::endl;
    }
    return 0;
}
"#,
    )
    .unwrap();

    // Verify samples/01.in is NOT expanded for interactive problems.
    let prob = ws.load_problem("guess").unwrap();
    let expanded = expand_tests(&ws.resolve(&prob.dir).unwrap(), &prob.meta).unwrap();
    assert_eq!(expanded.len(), 2);
    assert!(expanded.iter().all(|t| !t.is_sample));

    // Strict verify passes without tests/interactor/.
    let report = build_package(
        &ws,
        "guess",
        &BuildOptions {
            mode: BuildMode::Verify,
            cache_dir: Some(cache_dir.path().to_path_buf()),
            ..Default::default()
        },
        None,
        None,
    )
    .unwrap();

    assert!(report.interactive);
    assert_eq!(report.runs, 1);
    assert_eq!(report.checker, "interactor");
    assert!(
        report.verified,
        "expected interactive strict verify to pass; gates={:#?}, diag={:#?}",
        report.gates, report.diagnostics
    );

    let verdict_of = |name: &str| {
        report
            .solutions
            .iter()
            .find(|s| s.name == name)
            .unwrap()
            .actual_verdict
    };
    assert_eq!(verdict_of("std"), Verdict::AC);
    assert_eq!(verdict_of("wrong_wa"), Verdict::WA);
    assert_eq!(verdict_of("deadlock_ile"), Verdict::ILE);
    assert_eq!(verdict_of("cpu_tle"), Verdict::TLE);
    assert_eq!(verdict_of("partial_sol"), Verdict::AC);

    // Check quitp ratio on partial_sol cell.
    let partial_cell = report
        .matrix
        .iter()
        .find(|c| c.solution == "partial_sol" && c.test_index == 1)
        .unwrap();
    assert!((partial_cell.score_ratio.unwrap() - 0.5).abs() < 1e-6);

    // Check transcript on std cell (has both I2S and S2I entries).
    let std_cell = report
        .matrix
        .iter()
        .find(|c| c.solution == "std" && c.test_index == 1)
        .unwrap();
    let tr = std_cell.transcript.as_ref().expect("transcript missing");
    assert!(!tr.head.is_empty());
    assert!(
        tr.head
            .iter()
            .any(|e| e.direction == TranscriptDirection::InteractorToSolution && e.text == "1 100")
    );
    assert!(tr.head.iter().any(
        |e| e.direction == TranscriptDirection::SolutionToInteractor && e.text.starts_with('!')
    ));

    // Interactive stress test: generator produces random x in [2, 100]; wrong_wa fails and returns transcript.
    ws.write(
        "problems/guess/generators/gen.cpp",
        br#"#include "testlib.h"
#include <iostream>
int main(int argc, char* argv[]) {
    registerGen(argc, argv, 1);
    std::cout << rnd.next(2, 100) << '\n';
    return 0;
}
"#,
    )
    .unwrap();
    let stress = stress_package(
        &ws,
        "guess",
        &StressOptions {
            generator: "gen".to_owned(),
            args: "{seed}".to_owned(),
            solutions: vec!["wrong_wa".to_owned()],
            max_iter: 10,
            timeout_secs: 15,
            cxx_override: None,
            cache_dir: Some(cache_dir.path().to_path_buf()),
        },
        None,
    )
    .unwrap();
    assert!(stress.found_counterexample);
    assert_eq!(stress.failing_solution.as_deref(), Some("wrong_wa"));
    assert_eq!(stress.verdict, Some(Verdict::WA));
    assert_eq!(stress.saved_test_path.as_deref(), Some("tests/stress-1.in"));
    assert!(stress.transcript.is_some());
    assert!(!stress.transcript.unwrap().head.is_empty());
}

#[test]
fn interactive_two_stage_checker() {
    let toml = r#"
title = "Two Stage Interactive"

[limits]
time = "1s"
memory = "256MB"

[package]
checker = "checker.cpp"

[[test]]
file = "tests/t1.in"

[solutions.std]
main = true

[solutions.bad]
expect = "WA"
"#;
    let (_dir, ws) = setup_problem("two-stage", toml);
    let cache_dir = TempDir::new().unwrap();

    ws.write("problems/two-stage/tests/t1.in", b"10\n").unwrap();
    ws.write(
        "problems/two-stage/validator.cpp",
        br#"#include "testlib.h"
int main(int argc, char* argv[]) {
    registerValidation(argc, argv);
    inf.readInt(1, 100);
    inf.readEoln();
    inf.readEof();
}
"#,
    )
    .unwrap();
    ws.write("problems/two-stage/tests/validator/valid/01.in", b"10\n")
        .unwrap();
    ws.write("problems/two-stage/tests/validator/invalid/01.in", b"-1\n")
        .unwrap();

    // Interactor always passes `_ok` after writing solution's answer to `tout`.
    ws.write(
        "problems/two-stage/interactor.cpp",
        br#"#include "testlib.h"
#include <iostream>
int main(int argc, char* argv[]) {
    registerInteraction(argc, argv);
    int x = inf.readInt();
    std::cout << x << std::endl;
    int y = ouf.readInt();
    tout << y << '\n';
    quitf(_ok, "interaction finished");
}
"#,
    )
    .unwrap();

    // Checker verifies `tout` equals `2 * x`.
    ws.write(
        "problems/two-stage/checker.cpp",
        br#"#include "testlib.h"
int main(int argc, char* argv[]) {
    registerTestlibCmd(argc, argv);
    int x = inf.readInt();
    int y = ouf.readInt();
    if (y != x * 2) quitf(_wa, "expected %d, got %d", x * 2, y);
    quitf(_ok, "double ok");
}
"#,
    )
    .unwrap();
    ws.write("problems/two-stage/tests/checker/ok-1/in", b"5\n")
        .unwrap();
    ws.write("problems/two-stage/tests/checker/ok-1/out", b"10\n")
        .unwrap();
    ws.write("problems/two-stage/tests/checker/ok-1/ans", b"10\n")
        .unwrap();
    ws.write("problems/two-stage/tests/checker/wa-1/in", b"5\n")
        .unwrap();
    ws.write("problems/two-stage/tests/checker/wa-1/out", b"9\n")
        .unwrap();
    ws.write("problems/two-stage/tests/checker/wa-1/ans", b"10\n")
        .unwrap();

    ws.write(
        "problems/two-stage/solutions/std.cpp",
        br#"#include <iostream>
int main() { int x; if (std::cin >> x) std::cout << (x * 2) << std::endl; }
"#,
    )
    .unwrap();
    ws.write(
        "problems/two-stage/solutions/bad.cpp",
        br#"#include <iostream>
int main() { int x; if (std::cin >> x) std::cout << (x + 1) << std::endl; }
"#,
    )
    .unwrap();

    let report = build_package(
        &ws,
        "two-stage",
        &BuildOptions {
            mode: BuildMode::Verify,
            cache_dir: Some(cache_dir.path().to_path_buf()),
            ..Default::default()
        },
        None,
        None,
    )
    .unwrap();

    assert!(report.interactive);
    assert_eq!(report.checker, "checker.cpp");
    assert!(report.verified, "gates={:#?}", report.gates);
    let bad = report.solutions.iter().find(|s| s.name == "bad").unwrap();
    assert_eq!(bad.actual_verdict, Verdict::WA);
}

#[test]
fn multi_run_without_interactor_and_cumulative_cpu() {
    let toml = r#"
title = "Run Twice Stdio"

[limits]
time = "300ms"
memory = "256MB"

[package]
runs = 2

[[test]]
file = "tests/t1.in"

[solutions.std]
main = true

[solutions.wrong]
expect = "WA"

[solutions.cum_tle]
expect = "TLE"
"#;
    let (_dir, ws) = setup_problem("run2-stdio", toml);
    let cache_dir = TempDir::new().unwrap();

    ws.write("problems/run2-stdio/tests/t1.in", b"1 5\n")
        .unwrap();

    // Pass 1 receives "1 5" -> outputs "2 15". Pass 2 receives "2 15" -> outputs "30".
    ws.write(
        "problems/run2-stdio/solutions/std.cpp",
        br#"#include <iostream>
int main() {
    int phase, x;
    std::cin >> phase >> x;
    if (phase == 1) std::cout << "2 " << (x + 10) << '\n';
    else std::cout << (x * 2) << '\n';
}
"#,
    )
    .unwrap();
    ws.write(
        "problems/run2-stdio/solutions/wrong.cpp",
        br#"#include <iostream>
int main() {
    int phase, x;
    std::cin >> phase >> x;
    if (phase == 1) std::cout << "2 " << (x + 9) << '\n';
    else std::cout << (x * 2) << '\n';
}
"#,
    )
    .unwrap();
    // Each pass burns ~200ms CPU: individually < 300ms, cumulatively ~400ms > 300ms => TLE!
    ws.write(
        "problems/run2-stdio/solutions/cum_tle.cpp",
        br#"#include <ctime>
#include <iostream>
int main() {
    int phase, x;
    std::cin >> phase >> x;
    clock_t start = std::clock();
    volatile long long s = 0;
    while ((std::clock() - start) * 1000 / CLOCKS_PER_SEC < 210) { s += 1; }
    if (phase == 1) std::cout << "2 " << (x + 10) << '\n';
    else std::cout << (x * 2) << '\n';
}
"#,
    )
    .unwrap();

    let report = build_package(
        &ws,
        "run2-stdio",
        &BuildOptions {
            mode: BuildMode::Debug,
            cache_dir: Some(cache_dir.path().to_path_buf()),
            ..Default::default()
        },
        None,
        None,
    )
    .unwrap();

    assert!(!report.interactive);
    assert_eq!(report.runs, 2);
    assert!(
        report.ok,
        "gates={:#?}, diag={:#?}",
        report.gates, report.diagnostics
    );
    assert_eq!(
        report
            .solutions
            .iter()
            .find(|s| s.name == "std")
            .unwrap()
            .actual_verdict,
        Verdict::AC
    );
    assert_eq!(
        report
            .solutions
            .iter()
            .find(|s| s.name == "wrong")
            .unwrap()
            .actual_verdict,
        Verdict::WA
    );
    assert_eq!(
        report
            .solutions
            .iter()
            .find(|s| s.name == "cum_tle")
            .unwrap()
            .actual_verdict,
        Verdict::TLE
    );
}

#[test]
fn multi_run_with_interactor() {
    let toml = r#"
title = "Run Twice Interactive"

[limits]
time = "1s"
memory = "256MB"

[package]
runs = 2

[[test]]
file = "tests/t1.in"

[solutions.std]
main = true

[solutions.bad]
expect = "WA"
"#;
    let (_dir, ws) = setup_problem("run2-inter", toml);
    let cache_dir = TempDir::new().unwrap();

    ws.write("problems/run2-inter/tests/t1.in", b"1 7\n")
        .unwrap();

    // Pass 1: reads `1 7`, sends `1 7` to sol, reads encoded `e`, writes `2 7 <e>` to `tout`.
    // Pass 2: reads `2 7 <e>`, sends `2 <e>` to sol, reads decoded `d`, verifies `d == 7`.
    ws.write(
        "problems/run2-inter/interactor.cpp",
        br#"#include "testlib.h"
#include <iostream>
int main(int argc, char* argv[]) {
    registerInteraction(argc, argv);
    int phase = inf.readInt();
    int secret = inf.readInt();
    if (phase == 1) {
        std::cout << "1 " << secret << std::endl;
        int enc = ouf.readInt();
        tout << "2 " << secret << " " << enc << '\n';
        quitf(_ok, "pass 1 ok");
    } else {
        int enc = inf.readInt();
        std::cout << "2 " << enc << std::endl;
        int dec = ouf.readInt();
        if (dec != secret) quitf(_wa, "expected %d, got %d", secret, dec);
        quitf(_ok, "pass 2 ok");
    }
}
"#,
    )
    .unwrap();

    ws.write(
        "problems/run2-inter/solutions/std.cpp",
        br#"#include <iostream>
int main() {
    int phase, x;
    std::cin >> phase >> x;
    if (phase == 1) std::cout << (x ^ 123) << std::endl;
    else std::cout << (x ^ 123) << std::endl;
}
"#,
    )
    .unwrap();
    ws.write(
        "problems/run2-inter/solutions/bad.cpp",
        br#"#include <iostream>
int main() {
    int phase, x;
    std::cin >> phase >> x;
    if (phase == 1) std::cout << (x ^ 123) << std::endl;
    else std::cout << (x ^ 124) << std::endl;
}
"#,
    )
    .unwrap();

    let report = build_package(
        &ws,
        "run2-inter",
        &BuildOptions {
            mode: BuildMode::Debug,
            cache_dir: Some(cache_dir.path().to_path_buf()),
            ..Default::default()
        },
        None,
        None,
    )
    .unwrap();

    assert!(report.interactive);
    assert_eq!(report.runs, 2);
    assert!(report.ok, "diag={:#?}", report.diagnostics);
    let std_cell = report.matrix.iter().find(|c| c.solution == "std").unwrap();
    let tr = std_cell.transcript.as_ref().unwrap();
    assert!(tr.head.iter().any(|e| e.run == Some(1)));
    assert!(tr.head.iter().any(|e| e.run == Some(2)));
}

#[test]
fn oi_subtasks_min_sum_depends_quitp_validator_group_and_assertions() {
    let toml = r#"
title = "OI Subtasks"

[limits]
time = "1s"
memory = "256MB"

[package]
checker = "checker.cpp"

[[subtask]]
id = "1"
score = 30
scoring = "min"

[[subtask]]
id = "2"
score = 40
depends = ["1"]
scoring = "sum"

[[subtask]]
id = "3"
score = 30
depends = ["2"]
scoring = "min"

[[test]]
file = "tests/s1_a.in"
subtask = "1"

[[test]]
file = "tests/s2_a.in"
subtask = "2"

[[test]]
file = "tests/s2_b.in"
subtask = "2"

[[test]]
file = "tests/s3_a.in"
subtask = "3"

[solutions.std]
main = true

[solutions.brute]
score = 50
subtasks = { "1" = "AC", "2" = "WA", "3" = "WA" }

[solutions.half_credit]
score = [60, 70]
"#;
    let (_dir, ws) = setup_problem("oi-sub", toml);
    let cache_dir = TempDir::new().unwrap();

    // Test inputs: subtask 1 requires n <= 10, subtask 2 requires n <= 100, subtask 3 requires n <= 1000.
    ws.write("problems/oi-sub/tests/s1_a.in", b"5\n").unwrap();
    ws.write("problems/oi-sub/tests/s2_a.in", b"50\n").unwrap();
    ws.write("problems/oi-sub/tests/s2_b.in", b"80\n").unwrap();
    ws.write("problems/oi-sub/tests/s3_a.in", b"500\n").unwrap();

    // Validator using `validator.group()`!
    ws.write(
        "problems/oi-sub/validator.cpp",
        br#"#include "testlib.h"
int main(int argc, char* argv[]) {
    registerValidation(argc, argv);
    int max_n = 1000;
    if (validator.group() == "1") max_n = 10;
    else if (validator.group() == "2") max_n = 100;
    inf.readInt(1, max_n, "n");
    inf.readEoln();
    inf.readEof();
}
"#,
    )
    .unwrap();
    // Root + grouped validator self-tests (`tests/validator/{valid,invalid}/<subtask_id>/*.in`).
    ws.write("problems/oi-sub/tests/validator/valid/01.in", b"500\n")
        .unwrap();
    ws.write("problems/oi-sub/tests/validator/invalid/01.in", b"2000\n")
        .unwrap();
    ws.write("problems/oi-sub/tests/validator/valid/1/01.in", b"10\n")
        .unwrap();
    ws.write("problems/oi-sub/tests/validator/invalid/1/01.in", b"50\n")
        .unwrap();

    // Custom checker supporting quitp(0.5) when output is `-1`.
    ws.write(
        "problems/oi-sub/checker.cpp",
        br#"#include "testlib.h"
int main(int argc, char* argv[]) {
    registerTestlibCmd(argc, argv);
    int ja = ans.readInt();
    int pa = ouf.readInt();
    if (pa == ja) quitf(_ok, "exact");
    if (pa == -1) quitp(0.5, "half");
    quitf(_wa, "expected %d, got %d", ja, pa);
}
"#,
    )
    .unwrap();
    ws.write("problems/oi-sub/tests/checker/ok-1/in", b"5\n")
        .unwrap();
    ws.write("problems/oi-sub/tests/checker/ok-1/out", b"10\n")
        .unwrap();
    ws.write("problems/oi-sub/tests/checker/ok-1/ans", b"10\n")
        .unwrap();
    ws.write("problems/oi-sub/tests/checker/wa-1/in", b"5\n")
        .unwrap();
    ws.write("problems/oi-sub/tests/checker/wa-1/out", b"9\n")
        .unwrap();
    ws.write("problems/oi-sub/tests/checker/wa-1/ans", b"10\n")
        .unwrap();

    // std: outputs 2*n -> 100 points.
    ws.write(
        "problems/oi-sub/solutions/std.cpp",
        br#"#include <iostream>
int main() { int n; if (std::cin >> n) std::cout << (2 * n) << '\n'; }
"#,
    )
    .unwrap();
    // brute: passes n <= 60 (subtask 1 = 30, subtask 2 gets 1/2 = 20, subtask 3 = 0 because dep 2 not full) => total 50!
    ws.write(
        "problems/oi-sub/solutions/brute.cpp",
        br#"#include <iostream>
int main() {
    int n;
    if (std::cin >> n) std::cout << (n <= 60 ? 2 * n : 0) << '\n';
}
"#,
    )
    .unwrap();
    // half_credit: outputs -1 everywhere (quitp 0.5):
    // subtask 1 (min): 30 * 0.5 = 15 (not full!)
    // subtask 2 (sum, depends 1): dep 1 not full => 0
    // wait: let's pass subtask 1 with full score (30), and get 0.5 on subtask 2 (20), 0 on subtask 3 (dep 2 not full) => 50!
    ws.write(
        "problems/oi-sub/solutions/half_credit.cpp",
        br#"#include <iostream>
int main() {
    int n;
    if (std::cin >> n) std::cout << (n <= 10 ? 2 * n : -1) << '\n';
}
"#,
    )
    .unwrap();

    let report = build_package(
        &ws,
        "oi-sub",
        &BuildOptions {
            mode: BuildMode::Verify,
            cache_dir: Some(cache_dir.path().to_path_buf()),
            ..Default::default()
        },
        None,
        None,
    )
    .unwrap();

    assert!(
        report.verified,
        "expected OI subtasks strict verify to pass; gates={:#?}, diag={:#?}",
        report.gates, report.diagnostics
    );
    assert_eq!(report.total_score, Some(100));
    assert_eq!(report.subtasks.len(), 3);

    // Check grouped self-tests ran and passed.
    assert!(
        report
            .self_tests
            .iter()
            .any(|st| st.name == "1/01.in" && st.kind == "validator_invalid" && st.passed)
    );

    let std_sum = report.solutions.iter().find(|s| s.name == "std").unwrap();
    assert_eq!(std_sum.total_score, Some(100.0));
    assert_eq!(std_sum.max_score, Some(100));

    let brute_sum = report.solutions.iter().find(|s| s.name == "brute").unwrap();
    assert_eq!(brute_sum.total_score, Some(50.0));
    assert!(brute_sum.matched);

    let half_sum = report
        .solutions
        .iter()
        .find(|s| s.name == "half_credit")
        .unwrap();
    assert_eq!(half_sum.total_score, Some(65.0));
    assert!(half_sum.matched);
}

#[test]
fn subtask_validation_catches_cycle_unknown_empty_unassigned_and_warns_sum() {
    let subtasks = vec![
        SubtaskSpec {
            id: "1".to_owned(),
            score: 30,
            depends: vec!["2".to_owned()],
            scoring: SubtaskScoring::Min,
        },
        SubtaskSpec {
            id: "2".to_owned(),
            score: 40,
            depends: vec!["1".to_owned(), "ghost".to_owned()],
            scoring: SubtaskScoring::Sum,
        },
        SubtaskSpec {
            id: "3".to_owned(),
            score: 10,
            depends: vec![],
            scoring: SubtaskScoring::Min,
        },
    ];
    let tests = vec![
        xcpc_judge::ExpandedTest {
            index: 1,
            name: "t1".to_owned(),
            tag: None,
            subtask: Some("1".to_owned()),
            is_sample: false,
            write_back_sample: false,
            source: xcpc_judge::TestSource::File {
                file_rel: "tests/t1.in".to_owned(),
            },
            gen_cmd: None,
            rule_index: Some(0),
        },
        xcpc_judge::ExpandedTest {
            index: 2,
            name: "t2".to_owned(),
            tag: None,
            subtask: Some("unknown".to_owned()),
            is_sample: false,
            write_back_sample: false,
            source: xcpc_judge::TestSource::File {
                file_rel: "tests/t2.in".to_owned(),
            },
            gen_cmd: None,
            rule_index: Some(1),
        },
        xcpc_judge::ExpandedTest {
            index: 3,
            name: "t3".to_owned(),
            tag: None,
            subtask: None,
            is_sample: false,
            write_back_sample: false,
            source: xcpc_judge::TestSource::File {
                file_rel: "tests/t3.in".to_owned(),
            },
            gen_cmd: None,
            rule_index: Some(2),
        },
    ];

    let val = validate_subtask_config(&subtasks, &tests);
    assert!(val.errors.iter().any(|e| e.contains("循环依赖")));
    assert!(val.errors.iter().any(|e| e.contains("ghost")));
    assert!(val.errors.iter().any(|e| e.contains("unknown")));
    assert!(val.errors.iter().any(|e| e.contains("未归属任何子任务")));
    assert!(val.errors.iter().any(|e| e.contains("为空")));
    assert_eq!(val.total_score, 80);
    assert!(val.warnings.iter().any(|w| w.contains("80")));
}

#[test]
fn ops_crud_and_init_package_with_options_preserve_comments() {
    let toml = "# keep this comment\ntitle = \"CRUD Test\"\ncustom_unknown = 42\n";
    let (_dir, ws) = setup_problem("crud", toml);

    let init = init_package_with_options(
        &ws,
        "crud",
        &InitPackageOptions {
            interactive: true,
            runs: Some(2),
            subtasks: true,
        },
    )
    .unwrap();
    assert!(init.created_files.iter().any(|f| f == "interactor.cpp"));
    assert!(init.updated_problem_toml);

    let insp = inspect_package(&ws, "crud").unwrap();
    assert!(insp.interactive);
    assert_eq!(insp.runs, 2);
    assert_eq!(insp.subtasks.len(), 2);
    assert_eq!(insp.total_score, Some(100));
    assert_eq!(insp.files.interactor.as_deref(), Some("interactor.cpp"));

    // Delete & recreate interactor.
    assert!(delete_interactor(&ws, "crud").unwrap());
    assert!(!inspect_package(&ws, "crud").unwrap().interactive);
    let rel = create_package_source(&ws, "crud", PackageSourceKind::Interactor, "").unwrap();
    assert_eq!(rel, "interactor.cpp");
    assert!(inspect_package(&ws, "crud").unwrap().interactive);

    // Update runs, upsert subtask, assign test subtask, set solution scoring.
    set_package_runs(&ws, "crud", Some(3)).unwrap();
    upsert_subtask(
        &ws,
        "crud",
        &SubtaskSpec {
            id: "2".to_owned(),
            score: 70,
            depends: vec!["1".to_owned()],
            scoring: SubtaskScoring::Sum,
        },
    )
    .unwrap();
    set_test_subtask(&ws, "crud", 0, Some("2")).unwrap();
    let mut sub_map = BTreeMap::new();
    sub_map.insert("1".to_owned(), ExpectVerdict::Single("AC".to_owned()));
    sub_map.insert(
        "2".to_owned(),
        ExpectVerdict::Multiple(vec!["WA".to_owned(), "ILE".to_owned()]),
    );
    set_solution_scoring(
        &ws,
        "crud",
        "brute",
        Some(Some(ScoreExpect::Range([20, 40]))),
        Some(sub_map),
    )
    .unwrap();

    // Verify comment and unknown field survived all toml_edit mutations.
    let raw = ws.read_text("problems/crud/problem.toml").unwrap();
    assert!(raw.contains("# keep this comment"), "{raw}");
    assert!(raw.contains("custom_unknown = 42"), "{raw}");

    let prob = ws.load_problem("crud").unwrap();
    assert_eq!(prob.meta.run_count(), 3);
    assert_eq!(prob.meta.subtask[1].scoring, SubtaskScoring::Sum);
    assert_eq!(prob.meta.test[0].subtask.as_deref(), Some("2"));
    assert_eq!(
        prob.meta.solutions["brute"].score,
        Some(ScoreExpect::Range([20, 40]))
    );
    assert!(prob.meta.solutions["brute"].subtasks["2"].matches("ILE"));

    // Delete subtask with clear_references = true.
    assert!(delete_subtask(&ws, "crud", "2", true).unwrap());
    let prob2 = ws.load_problem("crud").unwrap();
    assert_eq!(prob2.meta.subtask.len(), 1);
    assert_eq!(prob2.meta.test[0].subtask, None);
    assert!(!prob2.meta.solutions["brute"].subtasks.contains_key("2"));
}

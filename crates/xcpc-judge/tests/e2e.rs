use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use tempfile::TempDir;
use xcpc_judge::{
    BuildMode, BuildOptions, CppStd, GateStatus, PackageFreshness, ProgressEvent, SourceLanguage,
    StressOptions, Verdict, build_package, check_dirty_inputs, check_package_freshness,
    detect_toolchain, init_package, inspect_package, inspect_test_case, parse_compiler_diagnostics,
    parse_memory_limit_kib, parse_time_limit_ms, stress_package,
};
use xcpc_storage::{Workspace, init_workspace};

fn setup_workspace_with_problem(slug: &str, extra_toml: &str) -> (TempDir, Workspace) {
    let dir = tempfile::tempdir().unwrap();
    let ws = init_workspace(dir.path()).unwrap();
    let toml =
        format!("title = \"A+B\"\n\n[limits]\ntime = \"1s\"\nmemory = \"256MB\"\n{extra_toml}");
    ws.write(&format!("problems/{slug}/problem.toml"), toml.as_bytes())
        .unwrap();
    ws.write(
        &format!("problems/{slug}/statement.md"),
        b"Compute $a + b$.\n",
    )
    .unwrap();
    (dir, ws)
}

#[test]
fn unit_parsers_and_language_detection() {
    assert_eq!(parse_time_limit_ms("1s").unwrap(), 1000);
    assert_eq!(parse_time_limit_ms("2.5s").unwrap(), 2500);
    assert_eq!(parse_time_limit_ms("500ms").unwrap(), 500);
    assert!(parse_time_limit_ms("0s").is_err());

    assert_eq!(parse_memory_limit_kib("256MB").unwrap(), 256 * 1024);
    assert_eq!(parse_memory_limit_kib("1GB").unwrap(), 1024 * 1024);
    assert_eq!(parse_memory_limit_kib("65536KB").unwrap(), 65536);

    assert_eq!(
        SourceLanguage::from_path(Path::new("std.cpp")),
        Some(SourceLanguage::Cpp { std: CppStd::Cpp20 })
    );
    assert_eq!(
        SourceLanguage::from_path(Path::new("std.17.cpp")),
        Some(SourceLanguage::Cpp { std: CppStd::Cpp17 })
    );
    assert_eq!(
        SourceLanguage::from_path(Path::new("std.23.cpp")),
        Some(SourceLanguage::Cpp { std: CppStd::Cpp23 })
    );
    assert_eq!(
        SourceLanguage::logical_stem(Path::new("sol_a.17.cpp")).as_deref(),
        Some("sol_a")
    );
    assert_eq!(
        SourceLanguage::logical_stem(Path::new("std.cpp")).as_deref(),
        Some("std")
    );

    let diags =
        parse_compiler_diagnostics("solutions/std.cpp:12:5: error: expected ';' before 'return'\n");
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].file.as_deref(), Some("solutions/std.cpp"));
    assert_eq!(diags[0].line, Some(12));
    assert_eq!(diags[0].column, Some(5));
    assert_eq!(diags[0].severity, "error");

    let tc = detect_toolchain(None);
    assert!(tc.active.is_some(), "expected g++ or clang++ in PATH");
    assert!(!tc.install_guide.is_empty());
}

#[test]
fn pure_publishing_problem_is_not_a_package() {
    let (_dir, ws) = setup_workspace_with_problem("pub-only", "");
    assert_eq!(
        check_package_freshness(&ws, "pub-only").unwrap(),
        PackageFreshness::NotAPackage
    );
    let insp = inspect_package(&ws, "pub-only").unwrap();
    assert!(!insp.has_package);
    assert!(!insp.next_steps.is_empty());
}

#[test]
fn init_package_scaffolds_and_passes_strict_verify() {
    let (_dir, ws) = setup_workspace_with_problem("ab", "");
    let cache_dir = tempfile::tempdir().unwrap();

    // First init creates all scaffold files.
    let init1 = init_package(&ws, "ab").unwrap();
    assert!(init1.updated_problem_toml);
    assert_eq!(init1.created_files.len(), 5);
    assert!(init1.skipped_files.is_empty());

    // Second init never overwrites existing files.
    let init2 = init_package(&ws, "ab").unwrap();
    assert!(!init2.updated_problem_toml);
    assert!(init2.created_files.is_empty());
    assert_eq!(init2.skipped_files.len(), 5);

    // Before building, freshness is Unverified.
    assert!(matches!(
        check_package_freshness(&ws, "ab").unwrap(),
        PackageFreshness::Unverified { .. }
    ));

    let opts = BuildOptions {
        mode: BuildMode::Verify,
        cache_dir: Some(cache_dir.path().to_path_buf()),
        ..Default::default()
    };
    let report = build_package(&ws, "ab", &opts, None, None).unwrap();
    assert!(report.ok, "report failed: {:#?}", report.gates);
    assert!(report.verified);
    assert_eq!(report.tests.len(), 3);
    assert_eq!(report.self_tests.len(), 2);
    assert!(report.self_tests.iter().all(|s| s.passed));

    assert!(matches!(
        check_package_freshness(&ws, "ab").unwrap(),
        PackageFreshness::VerifiedFresh { .. }
    ));
}

#[test]
fn full_e2e_verify_subset_cache_staleness_and_stress() {
    let extra_toml = r#"
[package]
checker = "ncmp"

[[test]]
file = "tests/hand-01.in"
tag = "edge"
name = "zero-zero"

[[test]]
gen = "gen_ab"
args = "-max 100 {i}"
repeat = 3
tag = "small"

[solutions.std]
main = true

[solutions.alt_ac]
expect = "AC"

[solutions.wrong_wa]
expect = "WA"

[solutions.slow_tle]
expect = "TLE"
"#;
    let (_dir, ws) = setup_workspace_with_problem("full-ab", extra_toml);
    // Use tight 200ms time limit so slow_tle triggers TLE quickly.
    let mut toml_text = ws.read_text("problems/full-ab/problem.toml").unwrap();
    toml_text = toml_text.replace("time = \"1s\"", "time = \"300ms\"");
    ws.write("problems/full-ab/problem.toml", toml_text.as_bytes())
        .unwrap();

    // Manual sample (must NOT be overwritten by build).
    ws.write("problems/full-ab/samples/01.in", b"3 4\n")
        .unwrap();
    ws.write("problems/full-ab/samples/01.ans", b"7\n").unwrap();

    // Hand test.
    ws.write("problems/full-ab/tests/hand-01.in", b"0 0\n")
        .unwrap();

    // Validator + self-tests.
    ws.write(
        "problems/full-ab/validator.cpp",
        br#"#include "testlib.h"
int main(int argc, char* argv[]) {
    registerValidation(argc, argv);
    inf.readInt(0, 1000, "a");
    inf.readSpace();
    inf.readInt(0, 1000, "b");
    inf.readEoln();
    inf.readEof();
    return 0;
}
"#,
    )
    .unwrap();
    ws.write("problems/full-ab/tests/validator/valid/01.in", b"5 6\n")
        .unwrap();
    ws.write("problems/full-ab/tests/validator/invalid/01.in", b"-5 6\n")
        .unwrap();

    // Generator.
    ws.write(
        "problems/full-ab/generators/gen_ab.cpp",
        br#"#include "testlib.h"
#include <iostream>
int main(int argc, char* argv[]) {
    registerGen(argc, argv, 1);
    int mx = opt<int>("max", 10);
    int a = rnd.next(1, mx);
    int b = rnd.next(1, mx);
    std::cout << a << ' ' << b << '\n';
    return 0;
}
"#,
    )
    .unwrap();

    // Solutions: std, alt_ac, wrong_wa, slow_tle.
    ws.write(
        "problems/full-ab/solutions/std.cpp",
        br#"#include <iostream>
int main() {
    long long a, b;
    if (std::cin >> a >> b) std::cout << (a + b) << '\n';
    return 0;
}
"#,
    )
    .unwrap();
    ws.write(
        "problems/full-ab/solutions/alt_ac.cpp",
        br#"#include <cstdio>
int main() {
    long long a, b;
    if (scanf("%lld%lld", &a, &b) == 2) printf("%lld\n", b + a);
    return 0;
}
"#,
    )
    .unwrap();
    ws.write(
        "problems/full-ab/solutions/wrong_wa.cpp",
        br#"#include <iostream>
int main() {
    long long a, b;
    if (std::cin >> a >> b) std::cout << (a + b + 1) << '\n';
    return 0;
}
"#,
    )
    .unwrap();
    ws.write(
        "problems/full-ab/solutions/slow_tle.cpp",
        br#"#include <iostream>
int main() {
    long long a, b;
    if (!(std::cin >> a >> b)) return 0;
    volatile long long s = 0;
    for (long long i = 0; i < 2000000000LL; ++i) s += (i & 1);
    std::cout << (a + b + (s & 0)) << '\n';
    return 0;
}
"#,
    )
    .unwrap();

    let cache_dir = tempfile::tempdir().unwrap();
    let events_seen = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let events_clone = Arc::clone(&events_seen);
    let cb = move |_ev: ProgressEvent| {
        events_clone.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    };

    // 1. Strict verify build.
    let opts = BuildOptions {
        mode: BuildMode::Verify,
        cache_dir: Some(cache_dir.path().to_path_buf()),
        ..Default::default()
    };
    let report = build_package(
        &ws,
        "full-ab",
        &opts,
        Some(Arc::new(AtomicBool::new(false))),
        Some(&cb),
    )
    .unwrap();
    assert!(report.ok, "verify failed: {:#?}", report.gates);
    assert!(report.verified);
    assert!(!report.subset);
    assert_eq!(report.tests.len(), 5); // 1 sample + 1 hand + 3 generated
    assert_eq!(report.solutions.len(), 4);
    assert!(events_seen.load(std::sync::atomic::Ordering::Relaxed) > 0);

    // Verify solution verdicts.
    let verdict_of = |name: &str| {
        report
            .solutions
            .iter()
            .find(|s| s.name == name)
            .unwrap()
            .actual_verdict
    };
    assert_eq!(verdict_of("std"), Verdict::AC);
    assert_eq!(verdict_of("alt_ac"), Verdict::AC);
    assert_eq!(verdict_of("wrong_wa"), Verdict::WA);
    assert_eq!(verdict_of("slow_tle"), Verdict::TLE);

    // Diversity gate should pass (has independent AC + wrong solutions).
    let div_gate = report
        .gates
        .iter()
        .find(|g| g.id == "solution_diversity")
        .unwrap();
    assert_eq!(div_gate.status, GateStatus::Pass);

    // Manual sample .ans was preserved.
    assert_eq!(
        ws.read_text("problems/full-ab/samples/01.ans").unwrap(),
        "7\n"
    );

    // Freshness is VerifiedFresh.
    assert!(matches!(
        check_package_freshness(&ws, "full-ab").unwrap(),
        PackageFreshness::VerifiedFresh { .. }
    ));

    // 2. Debug subset run MUST NOT overwrite verified state, and should hit Level 3 cache.
    let debug_opts = BuildOptions {
        mode: BuildMode::Debug,
        tests: vec!["1".to_owned()],
        solutions: vec!["alt_ac".to_owned()],
        cache_dir: Some(cache_dir.path().to_path_buf()),
        ..Default::default()
    };
    let debug_report = build_package(&ws, "full-ab", &debug_opts, None, None).unwrap();
    assert!(debug_report.ok);
    assert!(!debug_report.verified);
    assert!(debug_report.subset);
    assert_eq!(debug_report.matrix.len(), 1);
    assert!(debug_report.matrix[0].cache_hit);

    // Verified state is STILL fresh!
    assert!(matches!(
        check_package_freshness(&ws, "full-ab").unwrap(),
        PackageFreshness::VerifiedFresh { .. }
    ));

    // 3. inspect_test_case on generated test #3.
    let preview = inspect_test_case(&ws, "full-ab", "3", 5, 5).unwrap();
    assert!(preview.available_on_disk);
    assert!(preview.size_bytes > 0);
    assert!(preview.gen_cmd.as_deref().unwrap().contains("gen_ab"));

    // 4. Stress test against `wrong_wa` finds counterexample on seed 1 and saves `tests/stress-1.in`
    // without modifying `problem.toml`.
    let toml_before = ws.read_text("problems/full-ab/problem.toml").unwrap();
    let stress = stress_package(
        &ws,
        "full-ab",
        &StressOptions {
            generator: "gen_ab".to_owned(),
            args: "-max 20 {i}".to_owned(),
            solutions: vec!["wrong_wa".to_owned()],
            max_iter: 10,
            timeout_secs: 10,
            cache_dir: Some(cache_dir.path().to_path_buf()),
            ..Default::default()
        },
        None,
    )
    .unwrap();
    assert!(stress.found_counterexample);
    assert_eq!(stress.seed, Some(1));
    assert_eq!(stress.failing_solution.as_deref(), Some("wrong_wa"));
    assert_eq!(stress.verdict, Some(Verdict::WA));
    assert_eq!(stress.saved_test_path.as_deref(), Some("tests/stress-1.in"));
    assert!(
        ws.resolve("problems/full-ab/tests/stress-1.in")
            .unwrap()
            .is_file()
    );
    assert_eq!(
        ws.read_text("problems/full-ab/problem.toml").unwrap(),
        toml_before
    );

    // 5. After adding stress-1.in (which changed tests/), freshness becomes Stale.
    assert!(matches!(
        check_package_freshness(&ws, "full-ab").unwrap(),
        PackageFreshness::Stale { .. }
    ));
}

#[test]
fn custom_checker_self_tests_and_dirty_inputs_and_compile_error() {
    let extra_toml = r#"
[package]
checker = "checker.cpp"

[solutions.std]
main = true
"#;
    let (_dir, ws) = setup_workspace_with_problem("custom-chk", extra_toml);
    let cache_dir = tempfile::tempdir().unwrap();

    // Dirty sample: CRLF + missing trailing newline.
    ws.write("problems/custom-chk/samples/01.in", b"1 2\r\n")
        .unwrap();
    ws.write("problems/custom-chk/samples/01.ans", b"3\n")
        .unwrap();
    let dirty = check_dirty_inputs(&ws.resolve("problems/custom-chk").unwrap());
    assert_eq!(dirty.len(), 1);
    assert!(dirty[0].has_crlf);

    // Validator + self tests.
    ws.write(
        "problems/custom-chk/validator.cpp",
        br#"#include "testlib.h"
int main(int argc, char* argv[]) {
    registerValidation(argc, argv);
    inf.readInt(0, 100);
    inf.readSpace();
    inf.readInt(0, 100);
    inf.readEoln();
    inf.readEof();
    return 0;
}
"#,
    )
    .unwrap();
    ws.write("problems/custom-chk/tests/validator/valid/01.in", b"1 2\n")
        .unwrap();
    ws.write(
        "problems/custom-chk/tests/validator/invalid/01.in",
        b"999 2\n",
    )
    .unwrap();

    // Custom checker.
    ws.write(
        "problems/custom-chk/checker.cpp",
        br#"#include "testlib.h"
int main(int argc, char* argv[]) {
    registerTestlibCmd(argc, argv);
    int ja = ans.readInt();
    int pa = ouf.readInt();
    if (ja != pa) quitf(_wa, "expected %d, found %d", ja, pa);
    quitf(_ok, "ok");
}
"#,
    )
    .unwrap();

    // Broken std first -> structured compile error.
    ws.write(
        "problems/custom-chk/solutions/std.cpp",
        b"int main() { return undeclared_var; }\n",
    )
    .unwrap();
    let ce_report = build_package(
        &ws,
        "custom-chk",
        &BuildOptions {
            mode: BuildMode::Debug,
            cache_dir: Some(cache_dir.path().to_path_buf()),
            ..Default::default()
        },
        None,
        None,
    )
    .unwrap();
    assert!(!ce_report.ok);
    assert_eq!(ce_report.compile_failures.len(), 1);
    assert!(!ce_report.compile_failures[0].diagnostics.is_empty());

    // Fix std & clean sample CRLF.
    ws.write("problems/custom-chk/samples/01.in", b"1 2\n")
        .unwrap();
    ws.write(
        "problems/custom-chk/solutions/std.cpp",
        br#"#include <iostream>
int main() { int a, b; if (std::cin >> a >> b) std::cout << a + b << '\n'; }
"#,
    )
    .unwrap();

    // Verify fails without custom checker self-tests.
    let fail_verify = build_package(
        &ws,
        "custom-chk",
        &BuildOptions {
            mode: BuildMode::Verify,
            cache_dir: Some(cache_dir.path().to_path_buf()),
            ..Default::default()
        },
        None,
        None,
    )
    .unwrap();
    assert!(!fail_verify.verified);
    let chk_gate = fail_verify
        .gates
        .iter()
        .find(|g| g.id == "checker_self_tests")
        .unwrap();
    assert_eq!(chk_gate.status, GateStatus::Fail);

    // Add ok-1 and wa-1 checker self-tests -> Verify passes!
    let prob_dir = ws.resolve("problems/custom-chk").unwrap();
    fs::create_dir_all(prob_dir.join("tests/checker/ok-1")).unwrap();
    fs::write(prob_dir.join("tests/checker/ok-1/in"), "1 2\n").unwrap();
    fs::write(prob_dir.join("tests/checker/ok-1/out"), "3\n").unwrap();
    fs::write(prob_dir.join("tests/checker/ok-1/ans"), "3\n").unwrap();

    fs::create_dir_all(prob_dir.join("tests/checker/wa-1")).unwrap();
    fs::write(prob_dir.join("tests/checker/wa-1/in"), "1 2\n").unwrap();
    fs::write(prob_dir.join("tests/checker/wa-1/out"), "4\n").unwrap();
    fs::write(prob_dir.join("tests/checker/wa-1/ans"), "3\n").unwrap();

    let ok_verify = build_package(
        &ws,
        "custom-chk",
        &BuildOptions {
            mode: BuildMode::Verify,
            cache_dir: Some(cache_dir.path().to_path_buf()),
            ..Default::default()
        },
        None,
        None,
    )
    .unwrap();
    assert!(ok_verify.ok, "gates: {:#?}", ok_verify.gates);
    assert!(ok_verify.verified);
}

#[test]
fn sample_write_back_is_idempotent_and_preserves_verified_freshness() {
    let extra_toml = r#"
[[test]]
gen = "gen_ab"
args = "-a 10 -b 20"
sample = true
tag = "small"

[solutions.std]
main = true
"#;
    let (_dir, ws) = setup_workspace_with_problem("wb-sample", extra_toml);
    let cache_dir = tempfile::tempdir().unwrap();

    // Existing manual sample 01 (must never be overwritten).
    ws.write("problems/wb-sample/samples/01.in", b"1 2\n")
        .unwrap();
    ws.write("problems/wb-sample/samples/01.ans", b"3\n")
        .unwrap();

    ws.write(
        "problems/wb-sample/validator.cpp",
        br#"#include "testlib.h"
int main(int argc, char* argv[]) {
    registerValidation(argc, argv);
    inf.readInt(0, 100);
    inf.readSpace();
    inf.readInt(0, 100);
    inf.readEoln();
    inf.readEof();
    return 0;
}
"#,
    )
    .unwrap();
    ws.write("problems/wb-sample/tests/validator/valid/01.in", b"1 2\n")
        .unwrap();
    ws.write(
        "problems/wb-sample/tests/validator/invalid/01.in",
        b"-1 2\n",
    )
    .unwrap();

    ws.write(
        "problems/wb-sample/generators/gen_ab.cpp",
        br#"#include "testlib.h"
#include <iostream>
int main(int argc, char* argv[]) {
    registerGen(argc, argv, 1);
    int a = opt<int>("a", 1);
    int b = opt<int>("b", 2);
    std::cout << a << ' ' << b << '\n';
    return 0;
}
"#,
    )
    .unwrap();

    ws.write(
        "problems/wb-sample/solutions/std.cpp",
        br#"#include <iostream>
int main() {
    int a, b;
    if (std::cin >> a >> b) std::cout << (a + b) << '\n';
    return 0;
}
"#,
    )
    .unwrap();

    let opts = BuildOptions {
        mode: BuildMode::Verify,
        cache_dir: Some(cache_dir.path().to_path_buf()),
        ..Default::default()
    };

    // First strict build writes samples/02.{in,ans} and recomputes package_hash so freshness is VerifiedFresh.
    let r1 = build_package(&ws, "wb-sample", &opts, None, None).unwrap();
    assert!(r1.ok && r1.verified, "first build failed: {:#?}", r1.gates);
    assert_eq!(
        ws.read_text("problems/wb-sample/samples/01.ans").unwrap(),
        "3\n"
    );
    assert_eq!(
        ws.read_text("problems/wb-sample/samples/02.in").unwrap(),
        "10 20\n"
    );
    assert_eq!(
        ws.read_text("problems/wb-sample/samples/02.ans").unwrap(),
        "30\n"
    );
    assert!(matches!(
        check_package_freshness(&ws, "wb-sample").unwrap(),
        PackageFreshness::VerifiedFresh { .. }
    ));

    // Second strict build is idempotent: does NOT create samples/03.{in,ans}, keeps VerifiedFresh.
    let r2 = build_package(&ws, "wb-sample", &opts, None, None).unwrap();
    assert!(r2.ok && r2.verified, "second build failed: {:#?}", r2.gates);
    assert!(
        !ws.resolve("problems/wb-sample/samples/03.in")
            .unwrap()
            .exists()
    );
    assert_eq!(r1.package_hash, r2.package_hash);
    assert!(matches!(
        check_package_freshness(&ws, "wb-sample").unwrap(),
        PackageFreshness::VerifiedFresh { .. }
    ));
}

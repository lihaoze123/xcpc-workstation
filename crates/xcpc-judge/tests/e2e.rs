use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use tempfile::TempDir;
use xcpc_judge::{
    BuildMode, BuildOptions, CppStd, GateStatus, PackageFreshness, PackageSourceKind,
    ProgressEvent, SourceLanguage, StressOptions, Verdict, add_generator_test, add_hand_test,
    build_package, check_dirty_inputs, check_package_freshness, create_package_source,
    detect_toolchain, init_package, inspect_package, inspect_test_case, normalize_dirty_inputs,
    parse_compiler_diagnostics, parse_memory_limit_kib, parse_time_limit_ms, remove_test_rule,
    set_package_checker, set_solution_config, stress_package,
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
    let mut seen_nv = std::collections::HashSet::new();
    for c in &tc.compilers {
        assert!(
            seen_nv.insert((c.name.clone(), c.version.clone())),
            "duplicate (name, version) in detected compilers: {c:?}"
        );
    }
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

#[test]
fn inspect_package_includes_solution_specs_before_first_build() {
    let extra_toml = r#"
[solutions.std]
main = true

[solutions.brute]
expect = "AC"
tags = ["sample", "small"]

[solutions.wrong_wa]
expect = ["WA", "TLE"]
"#;
    let (_dir, ws) = setup_workspace_with_problem("prebuild-insp", extra_toml);
    ws.write(
        "problems/prebuild-insp/solutions/std.cpp",
        b"int main() { return 0; }\n",
    )
    .unwrap();
    ws.write(
        "problems/prebuild-insp/solutions/brute.cpp",
        b"int main() { return 0; }\n",
    )
    .unwrap();
    ws.write(
        "problems/prebuild-insp/solutions/wrong_wa.cpp",
        b"int main() { return 0; }\n",
    )
    .unwrap();

    let insp = inspect_package(&ws, "prebuild-insp").unwrap();
    assert!(insp.last_report.is_none());
    assert_eq!(insp.solutions.len(), 3);
    assert_eq!(insp.solutions[0].name, "std");
    assert!(insp.solutions[0].is_main);
    assert_eq!(insp.solutions[0].expected, vec!["AC"]);

    let brute = insp.solutions.iter().find(|s| s.name == "brute").unwrap();
    assert!(!brute.is_main);
    assert_eq!(brute.expected, vec!["AC"]);
    assert_eq!(
        brute.tags.as_deref(),
        Some(&["sample".to_string(), "small".to_string()][..])
    );

    let wrong = insp
        .solutions
        .iter()
        .find(|s| s.name == "wrong_wa")
        .unwrap();
    assert!(!wrong.is_main);
    assert_eq!(wrong.expected, vec!["WA", "TLE"]);
}

#[cfg(target_os = "linux")]
#[test]
fn linux_runner_does_not_leak_parent_rss() {
    // Allocate and touch 64 MiB in the parent test process to inflate parent hiwater_rss.
    let mut parent_ballast = vec![0u8; 64 * 1024 * 1024];
    for chunk in parent_ballast.chunks_mut(4096) {
        chunk[0] = 42;
    }
    let _keep = std::hint::black_box(&parent_ballast);

    let tmp = tempfile::tempdir().unwrap();
    let cache = xcpc_judge::JudgeCache::open(tmp.path().join("cache")).unwrap();
    let compiler = xcpc_judge::resolve_active_compiler(None).unwrap();
    let src = br#"#include <iostream>
int main() {
    int a, b;
    if (std::cin >> a >> b) std::cout << (a + b) << '\n';
    return 0;
}
"#;
    let src_path = tmp.path().join("tiny.cpp");
    fs::write(&src_path, src).unwrap();
    let testlib = xcpc_judge::assets::resolve_testlib(tmp.path(), tmp.path());
    let compiled = xcpc_judge::toolchain::compile_cached(
        &cache,
        &compiler,
        SourceLanguage::Cpp { std: CppStd::Cpp20 },
        "tiny",
        "tiny.cpp",
        src,
        Some(&src_path),
        &testlib,
    )
    .unwrap();

    let in_path = tmp.path().join("in.txt");
    let out_path = tmp.path().join("out.txt");
    fs::write(&in_path, b"3 4\n").unwrap();

    let limits = xcpc_judge::RunLimits::from_problem_limits(Some("1s"), Some("64MB")).unwrap();
    let outcome = xcpc_judge::run_process(
        &compiled.binary_path,
        &[],
        Some(&in_path),
        Some(&out_path),
        limits,
        None,
    )
    .unwrap();
    assert_eq!(outcome.verdict, Verdict::AC);
    assert!(
        outcome.peak_memory_kib < 32 * 1024,
        "parent RSS leaked into child peak_memory_kib: {} KiB",
        outcome.peak_memory_kib
    );
    drop(parent_ballast);
}

#[test]
fn inspect_test_case_large_multiline_single_line_and_ans() {
    let extra_toml = r#"
[[test]]
file = "tests/big-multi.in"
name = "big-multi"
tag = "max"

[[test]]
file = "tests/big-single.in"
name = "big-single"
tag = "max"

[[test]]
file = "tests/wide-small.in"
name = "wide-small"
tag = "edge"
"#;
    let (_dir, ws) = setup_workspace_with_problem("big-data", extra_toml);
    let prob_dir = ws.resolve("problems/big-data").unwrap();
    fs::create_dir_all(prob_dir.join("tests")).unwrap();
    fs::create_dir_all(prob_dir.join("samples")).unwrap();

    // Sample 01 with manual .ans.
    ws.write("problems/big-data/samples/01.in", b"1 2\n")
        .unwrap();
    ws.write("problems/big-data/samples/01.ans", b"3\n")
        .unwrap();

    // 1. 5 MiB multi-line file.
    let mut big_multi = String::with_capacity(5 * 1024 * 1024 + 1024);
    for i in 1..=200_000 {
        use std::fmt::Write;
        let _ = writeln!(big_multi, "line-{i:06} 1234567890 9876543210");
    }
    assert!(big_multi.len() > 5 * 1024 * 1024);
    fs::write(prob_dir.join("tests/big-multi.in"), &big_multi).unwrap();

    // Create a matching built .ans for test #2 (1=sample-01, 2=big-multi).
    let build_tests_dir = xcpc_judge::build_dir_for(&ws, "big-data").join("tests");
    fs::create_dir_all(&build_tests_dir).unwrap();
    fs::write(build_tests_dir.join("002.ans"), "42\n99\n").unwrap();

    // 2. 500 KiB single-line file (e.g. 100k ints on line 1, plus line 2 "SECOND_LINE").
    let mut big_single = String::with_capacity(600 * 1024);
    for i in 0..100_000 {
        use std::fmt::Write;
        if i > 0 {
            big_single.push(' ');
        }
        let _ = write!(big_single, "{i}");
    }
    big_single.push_str("\nSECOND_LINE\n");
    assert!(big_single.len() > 500 * 1024);
    fs::write(prob_dir.join("tests/big-single.in"), &big_single).unwrap();

    // 3. Small file with one wide line (> 200 chars) + line 2 ("LINE2"), fits in head+tail.
    let wide_small = format!("{}\nLINE2\n", "x".repeat(400));
    fs::write(prob_dir.join("tests/wide-small.in"), &wide_small).unwrap();

    // Sample preview includes .ans from samples/01.ans, rule_index == None, file_rel == Some.
    let p_sample = inspect_test_case(&ws, "big-data", "1", 10, 5).unwrap();
    assert!(p_sample.available_on_disk);
    assert!(p_sample.ans_available);
    assert_eq!(p_sample.ans_head, "3");
    assert_eq!(p_sample.ans_total_lines, Some(1));
    assert_eq!(p_sample.rule_index, None);
    assert_eq!(p_sample.file_rel.as_deref(), Some("samples/01.in"));

    // 5 MiB multi-line preview: fast, bounded, head starts at line-000001, tail ends at line-200000.
    let t0 = std::time::Instant::now();
    let p_multi = inspect_test_case(&ws, "big-data", "big-multi", 5, 5).unwrap();
    assert!(
        t0.elapsed().as_millis() < 500,
        "inspect_test_case took too long: {:?}",
        t0.elapsed()
    );
    assert!(p_multi.available_on_disk);
    assert!(p_multi.truncated);
    assert!(p_multi.size_bytes > 5 * 1024 * 1024);
    assert!(p_multi.head.contains("line-000001"));
    assert!(p_multi.tail.contains("line-200000"));
    assert!(p_multi.head.len() <= 4096 + 64);
    assert!(p_multi.tail.len() <= 4096 + 64);
    assert!(p_multi.ans_available);
    assert_eq!(p_multi.ans_head, "42\n99");
    assert_eq!(p_multi.ans_total_lines, Some(2));
    assert_eq!(p_multi.rule_index, Some(0));
    assert_eq!(p_multi.file_rel.as_deref(), Some("tests/big-multi.in"));

    // 500 KiB single-line preview: per-line truncated, bounded, truncated == true.
    let p_single = inspect_test_case(&ws, "big-data", "big-single", 5, 5).unwrap();
    assert!(p_single.available_on_disk);
    assert!(p_single.truncated);
    assert!(p_single.head.contains("chars)"));
    assert!(p_single.head.len() <= 4096 + 64);
    assert!(p_single.tail.contains("SECOND_LINE"));

    // Small file with wide line: line 1 truncated, line 2 preserved, truncated == true even with empty tail.
    let p_wide = inspect_test_case(&ws, "big-data", "wide-small", 10, 5).unwrap();
    assert!(p_wide.truncated);
    assert!(p_wide.tail.is_empty());
    assert!(p_wide.head.contains("(+200 chars)"));
    assert!(p_wide.head.contains("LINE2"));
}

#[test]
fn structured_package_mutation_apis_preserve_comments_and_unknown_fields() {
    let extra_toml = r#"
# Keep this author comment!
custom_extra = "preserve-me"

[solutions.std]
main = true
"#;
    let (_dir, ws) = setup_workspace_with_problem("mut-pkg", extra_toml);
    ws.write(
        "problems/mut-pkg/solutions/std.cpp",
        b"int main() { return 0; }\n",
    )
    .unwrap();

    // 1. set_package_checker: built-in then custom checker.cpp.
    set_package_checker(&ws, "mut-pkg", "ncmp").unwrap();
    assert!(set_package_checker(&ws, "mut-pkg", "bogus_chk").is_err());
    let p = ws.load_problem("mut-pkg").unwrap();
    assert_eq!(
        p.meta.package.as_ref().and_then(|c| c.checker.as_deref()),
        Some("ncmp")
    );

    set_package_checker(&ws, "mut-pkg", "checker.cpp").unwrap();
    let chk_path = ws.resolve("problems/mut-pkg/checker.cpp").unwrap();
    assert!(chk_path.is_file());
    // Modify checker.cpp and call set_package_checker again -> must NOT overwrite.
    ws.write("problems/mut-pkg/checker.cpp", b"// custom\n")
        .unwrap();
    set_package_checker(&ws, "mut-pkg", "checker.cpp").unwrap();
    assert_eq!(
        ws.read_text("problems/mut-pkg/checker.cpp").unwrap(),
        "// custom\n"
    );

    // 2. create_package_source: rejects path traversal, creates files without overwriting.
    assert!(create_package_source(&ws, "mut-pkg", PackageSourceKind::Solution, "../evil").is_err());
    let sol_rel =
        create_package_source(&ws, "mut-pkg", PackageSourceKind::Solution, "brute").unwrap();
    assert_eq!(sol_rel, "solutions/brute.cpp");
    let gen_rel =
        create_package_source(&ws, "mut-pkg", PackageSourceKind::Generator, "gen_tree").unwrap();
    assert_eq!(gen_rel, "generators/gen_tree.cpp");
    let val_rel =
        create_package_source(&ws, "mut-pkg", PackageSourceKind::Validator, "validator").unwrap();
    assert_eq!(val_rel, "validator.cpp");
    let v_ok = create_package_source(
        &ws,
        "mut-pkg",
        PackageSourceKind::ValidatorSelfTestValid,
        "01",
    )
    .unwrap();
    assert_eq!(v_ok, "tests/validator/valid/01.in");
    let v_bad = create_package_source(
        &ws,
        "mut-pkg",
        PackageSourceKind::ValidatorSelfTestInvalid,
        "01",
    )
    .unwrap();
    assert_eq!(v_bad, "tests/validator/invalid/01.in");

    let insp = inspect_package(&ws, "mut-pkg").unwrap();
    assert!(
        insp.files
            .self_test_files
            .contains(&"tests/validator/valid/01.in".to_owned())
    );
    assert!(
        insp.files
            .self_test_files
            .contains(&"tests/validator/invalid/01.in".to_owned())
    );

    // 3. set_solution_config: set expect/tags on brute, then switch main to brute.
    set_solution_config(
        &ws,
        "mut-pkg",
        "brute",
        None,
        Some(vec!["WA".into(), "TLE".into()]),
        Some(vec!["small".into()]),
    )
    .unwrap();
    let p = ws.load_problem("mut-pkg").unwrap();
    assert_eq!(
        p.meta.solutions["brute"]
            .expect
            .as_ref()
            .unwrap()
            .normalized(),
        vec!["WA", "TLE"]
    );
    assert_eq!(
        p.meta.solutions["brute"].tags.as_deref(),
        Some(&["small".to_string()][..])
    );

    set_solution_config(&ws, "mut-pkg", "brute", Some(true), None, None).unwrap();
    let p = ws.load_problem("mut-pkg").unwrap();
    assert_eq!(p.meta.solutions["brute"].main, Some(true));
    assert!(p.meta.solutions["brute"].expect.is_none());
    assert_ne!(p.meta.solutions["std"].main, Some(true));

    // 4. add_hand_test, add_generator_test, remove_test_rule.
    let hand = add_hand_test(
        &ws,
        "mut-pkg",
        Some("edge-min"),
        Some("edge"),
        "\u{feff}1 2\r\n3 4",
    )
    .unwrap();
    assert_eq!(hand.rule_index, 0);
    assert_eq!(hand.file_rel.as_deref(), Some("tests/hand-01.in"));
    assert_eq!(
        ws.read_text("problems/mut-pkg/tests/hand-01.in").unwrap(),
        "1 2\n3 4\n"
    );

    let gen_added = add_generator_test(
        &ws,
        "mut-pkg",
        "gen_tree",
        "-n 100 {i}",
        5,
        Some("max"),
        Some("tree-max"),
    )
    .unwrap();
    assert_eq!(gen_added.rule_index, 1);
    assert!(add_generator_test(&ws, "mut-pkg", "gen_tree", "", 0, None, None).is_err());

    remove_test_rule(&ws, "mut-pkg", 0).unwrap();
    let p = ws.load_problem("mut-pkg").unwrap();
    assert_eq!(p.meta.test.len(), 1);
    assert_eq!(p.meta.test[0].generator.as_deref(), Some("gen_tree"));

    // 5. normalize_dirty_inputs fixes BOM + CRLF + missing trailing newline.
    ws.write("problems/mut-pkg/samples/01.in", b"\xEF\xBB\xBF1 2\r\n3 4")
        .unwrap();
    let fixed = normalize_dirty_inputs(&ws, "mut-pkg").unwrap();
    assert_eq!(fixed, vec!["samples/01.in"]);
    assert_eq!(
        ws.read_text("problems/mut-pkg/samples/01.in").unwrap(),
        "1 2\n3 4\n"
    );

    // 6. Verify comments and unknown fields were preserved across all toml_edit writes.
    let final_toml = ws.read_text("problems/mut-pkg/problem.toml").unwrap();
    assert!(final_toml.contains("# Keep this author comment!"));
    assert!(final_toml.contains("custom_extra = \"preserve-me\""));
}

//! Integration tests for Phase 3D: Contest Readiness
//! - Sanitizer (ASan / UBSan) UB check mode + strict isolation
//! - External tester submission trial runs (single / contest-dir / multi-tester) + Markdown + Adopt
//! - Contest Readiness Audit (`audit_contest`) + warnings + Markdown export

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use tempfile::TempDir;
use xcpc_judge::{
    AdoptSolutionOptions, AuditLintFinding, AuditOptions, AuditRiskLevel, BuildMode, BuildOptions,
    ExternalContestMode, ExternalRunOptions, ProblemLintSummary, SanitizeOptions,
    SanitizerAuditStatus, SanitizerStatus, SelfTestAuditStatus, Verdict, VerificationStatus,
    adopt_external_solution, audit_contest, build_package, detect_toolchain, load_latest_report,
    load_sanitizer_report, load_verified_report, render_contest_audit_markdown,
    render_external_contest_markdown, render_external_problem_markdown, report_path_for,
    run_external_solutions_for_contest, run_external_solutions_for_problem, sanitize_package,
    sanitizer_report_path_for,
};
use xcpc_storage::Workspace;

fn setup_workspace() -> (TempDir, Workspace, TempDir) {
    let dir = TempDir::new().unwrap();
    let cache = TempDir::new().unwrap();
    fs::write(dir.path().join("workspace.toml"), "name = \"test-3d\"\n").unwrap();
    let ws = Workspace::open(dir.path(), false).unwrap();
    (dir, ws, cache)
}

fn create_apb_package(ws: &Workspace, slug: &str, with_extra_solutions: bool) {
    let prob = ws.root().join("problems").join(slug);
    fs::create_dir_all(prob.join("samples")).unwrap();
    fs::create_dir_all(prob.join("solutions")).unwrap();
    fs::create_dir_all(prob.join("generators")).unwrap();
    fs::create_dir_all(prob.join("tests/validator/valid")).unwrap();
    fs::create_dir_all(prob.join("tests/validator/invalid")).unwrap();

    let mut toml = format!(
        r#"# User comment that must be preserved
title = "{slug}"
custom_unknown_field = "keep-me"

[limits]
time = "1s"
memory = "256MB"

[solutions.std]
main = true
"#
    );
    if with_extra_solutions {
        toml.push_str(
            r#"
[solutions.alt_ac]
expect = "AC"

[solutions.wa_off]
expect = "WA"
"#,
        );
    }
    toml.push_str(
        r#"
[[test]]
gen = "gen_pair"
args = "10 20"
name = "gen-1"
"#,
    );
    fs::write(prob.join("problem.toml"), toml).unwrap();
    fs::write(
        prob.join("statement.md"),
        "Compute $a+b$.\n\n## Input\n\nTwo integers.\n\n## Output\n\nTheir sum.\n",
    )
    .unwrap();
    fs::write(prob.join("editorial.md"), "## Solution\n\nPrint `a + b`.\n").unwrap();

    fs::write(prob.join("samples/01.in"), "1 2\n").unwrap();
    fs::write(prob.join("samples/01.ans"), "3\n").unwrap();

    fs::write(
        prob.join("solutions/std.cpp"),
        r#"#include <iostream>
int main() {
    long long a, b;
    if (std::cin >> a >> b) std::cout << (a + b) << "\n";
    return 0;
}
"#,
    )
    .unwrap();

    if with_extra_solutions {
        fs::write(
            prob.join("solutions/alt_ac.cpp"),
            r#"#include <cstdio>
int main() {
    long long a, b;
    if (std::scanf("%lld%lld", &a, &b) == 2) std::printf("%lld\n", b + a);
    return 0;
}
"#,
        )
        .unwrap();
        fs::write(
            prob.join("solutions/wa_off.cpp"),
            r#"#include <iostream>
int main() {
    long long a, b;
    if (std::cin >> a >> b) std::cout << (a + b + 1) << "\n";
    return 0;
}
"#,
        )
        .unwrap();
    }

    fs::write(
        prob.join("generators/gen_pair.cpp"),
        r#"#include <iostream>
#include <cstdlib>
int main(int argc, char** argv) {
    long long a = argc > 1 ? std::atoll(argv[1]) : 1;
    long long b = argc > 2 ? std::atoll(argv[2]) : 2;
    std::cout << a << " " << b << "\n";
    return 0;
}
"#,
    )
    .unwrap();

    fs::write(
        prob.join("validator.cpp"),
        r#"#include "testlib.h"
int main(int argc, char* argv[]) {
    registerValidation(argc, argv);
    inf.readLong(-1000000000LL, 1000000000LL, "a");
    inf.readSpace();
    inf.readLong(-1000000000LL, 1000000000LL, "b");
    inf.readEoln();
    inf.readEof();
    return 0;
}
"#,
    )
    .unwrap();
    fs::write(prob.join("tests/validator/valid/01.in"), "5 6\n").unwrap();
    fs::write(prob.join("tests/validator/invalid/01.in"), "only_one\n").unwrap();
}

fn create_statement_only_problem(ws: &Workspace, slug: &str) {
    let prob = ws.root().join("problems").join(slug);
    fs::create_dir_all(prob.join("samples")).unwrap();
    fs::write(prob.join("problem.toml"), format!("title = \"{slug}\"\n")).unwrap();
    fs::write(
        prob.join("statement.md"),
        "Statement only.\n\n## Input\n\nNone.\n\n## Output\n\nNone.\n",
    )
    .unwrap();
    fs::write(prob.join("editorial.md"), "## Editorial\n\nTrivial.\n").unwrap();
    fs::write(prob.join("samples/01.in"), "1\n").unwrap();
    fs::write(prob.join("samples/01.ans"), "1\n").unwrap();
}

fn create_contest(ws: &Workspace, cid: &str, problems: &[(&str, &str)]) {
    let cdir = ws.root().join("contests").join(cid);
    fs::create_dir_all(&cdir).unwrap();
    let mut toml = format!("name = \"Contest {cid}\"\n\n");
    for (label, slug) in problems {
        toml.push_str(&format!(
            "[[problem]]\nlabel = \"{label}\"\nid = \"{slug}\"\n\n"
        ));
    }
    fs::write(cdir.join("contest.toml"), toml).unwrap();
}

#[test]
fn sanitizer_detects_ub_and_preserves_normal_report_isolation() {
    if detect_toolchain(None).active.is_none() {
        return;
    }

    let (_dir, ws, cache) = setup_workspace();
    create_apb_package(&ws, "apb-clean", true);

    // 1. Run strict Verify build first to produce report.json & verified_report.json.
    let build_opts = BuildOptions {
        mode: BuildMode::Verify,
        cache_dir: Some(cache.path().to_path_buf()),
        ..Default::default()
    };
    let verify_report = build_package(&ws, "apb-clean", &build_opts, None, None).unwrap();
    assert!(verify_report.verified, "{verify_report:#?}");
    let report_before = fs::read(report_path_for(&ws, "apb-clean")).unwrap();
    let verified_before = load_verified_report(&ws, "apb-clean").unwrap();

    // 2. Run Sanitizer UB check on the clean problem.
    let san_opts = SanitizeOptions {
        cache_dir: Some(cache.path().to_path_buf()),
        ..Default::default()
    };
    let san_report = sanitize_package(&ws, "apb-clean", &san_opts, None, None).unwrap();
    assert!(
        san_report.relaxed_time_limit_ms >= 5_000,
        "relaxed TL must be at least 5000ms"
    );
    assert!(sanitizer_report_path_for(&ws, "apb-clean").is_file());
    assert!(load_sanitizer_report(&ws, "apb-clean").is_some());

    // Strict isolation: normal report.json and verified_report.json must be byte-for-byte untouched!
    let report_after = fs::read(report_path_for(&ws, "apb-clean")).unwrap();
    assert_eq!(report_before, report_after);
    assert_eq!(
        verified_before,
        load_verified_report(&ws, "apb-clean").unwrap()
    );
    assert_eq!(
        load_latest_report(&ws, "apb-clean").unwrap().package_hash,
        verify_report.package_hash
    );

    if san_report.status == SanitizerStatus::Unsupported {
        // Toolchain lacks ASan runtime on this host: graceful Unsupported verified.
        assert!(san_report.unsupported_reason.is_some());
        return;
    }

    assert_eq!(
        san_report.status,
        SanitizerStatus::Passed,
        "{san_report:#?}"
    );
    assert!(san_report.ok);
    assert!(san_report.findings.is_empty());

    // 3. Now create a problem with a real stack-buffer-overflow UB in an expected-AC solution.
    create_apb_package(&ws, "apb-ub", false);
    fs::write(
        ws.root().join("problems/apb-ub/solutions/std.cpp"),
        r#"#include <iostream>
int main() {
    volatile int arr[2] = {1, 2};
    long long a, b;
    if (std::cin >> a >> b) {
        int bad = arr[5]; // stack-buffer-overflow!
        std::cout << (a + b + (bad - bad)) << "\n";
    }
    return 0;
}
"#,
    )
    .unwrap();

    let ub_report = sanitize_package(&ws, "apb-ub", &san_opts, None, None).unwrap();
    assert_eq!(ub_report.status, SanitizerStatus::Failed, "{ub_report:#?}");
    assert!(!ub_report.ok);
    assert!(
        !ub_report.findings.is_empty(),
        "expected ASan/UBSan finding for out-of-bounds read"
    );
    let finding = &ub_report.findings[0];
    assert_eq!(finding.target, "sol:std");
    assert!(
        finding.summary.contains("AddressSanitizer")
            || finding.summary.contains("runtime error")
            || finding.summary.contains("buffer-overflow"),
        "unexpected summary: {}",
        finding.summary
    );
}

#[test]
fn external_trial_run_single_contest_multi_tester_and_adopt() {
    if detect_toolchain(None).active.is_none() {
        return;
    }

    let (_dir, ws, cache) = setup_workspace();
    create_apb_package(&ws, "apb-a", true);
    create_apb_package(&ws, "sum-b", true);
    create_contest(&ws, "round1", &[("A", "apb-a"), ("B", "sum-b")]);

    let build_opts = BuildOptions {
        mode: BuildMode::Verify,
        cache_dir: Some(cache.path().to_path_buf()),
        ..Default::default()
    };
    build_package(&ws, "apb-a", &build_opts, None, None).unwrap();
    build_package(&ws, "sum-b", &build_opts, None, None).unwrap();

    let ext_dir = TempDir::new().unwrap();
    let ac_file = ext_dir.path().join("tester_ac.cpp");
    let wa_file = ext_dir.path().join("tester_wa.cpp");
    let ce_file = ext_dir.path().join("tester_ce.cpp");

    fs::write(
        &ac_file,
        "#include <iostream>\nint main(){long long a,b;if(std::cin>>a>>b)std::cout<<a+b<<'\\n';}\n",
    )
    .unwrap();
    fs::write(
        &wa_file,
        "#include <iostream>\nint main(){long long a,b;if(std::cin>>a>>b)std::cout<<a+b+42<<'\\n';}\n",
    )
    .unwrap();
    fs::write(&ce_file, "int main( { syntax error }\n").unwrap();

    let toml_before = fs::read_to_string(ws.root().join("problems/apb-a/problem.toml")).unwrap();
    let sols_before: Vec<_> = fs::read_dir(ws.root().join("problems/apb-a/solutions"))
        .unwrap()
        .flatten()
        .map(|e| e.file_name())
        .collect();

    // 1. Single problem multi-file trial run.
    let ext_opts = ExternalRunOptions {
        auto_build: false,
        cache_dir: Some(cache.path().to_path_buf()),
        ..Default::default()
    };
    let prob_rep = run_external_solutions_for_problem(
        &ws,
        "apb-a",
        &[ac_file.clone(), wa_file.clone(), ce_file.clone()],
        &ext_opts,
        None,
    )
    .unwrap();
    assert_eq!(prob_rep.submissions.len(), 3);
    assert_eq!(prob_rep.submissions[0].verdict, Verdict::AC);
    assert_eq!(
        prob_rep.submissions[0].passed_tests,
        prob_rep.submissions[0].total_tests
    );
    assert_eq!(prob_rep.submissions[1].verdict, Verdict::WA);
    assert!(prob_rep.submissions[1].first_failure.is_some());
    assert_eq!(prob_rep.submissions[2].verdict, Verdict::CE);
    assert!(prob_rep.submissions[2].compile_error.is_some());

    let prob_md = render_external_problem_markdown(&prob_rep);
    assert!(prob_md.contains("tester_ac.cpp"));
    assert!(prob_md.contains("**AC**"));
    assert!(prob_md.contains("**WA**"));

    // Zero-pollution guarantee: solutions/ and problem.toml untouched.
    assert_eq!(
        toml_before,
        fs::read_to_string(ws.root().join("problems/apb-a/problem.toml")).unwrap()
    );
    let sols_after: Vec<_> = fs::read_dir(ws.root().join("problems/apb-a/solutions"))
        .unwrap()
        .flatten()
        .map(|e| e.file_name())
        .collect();
    assert_eq!(sols_before, sols_after);

    // 2. Whole contest single-directory auto-matching + unmatched file skipping.
    let single_dir = TempDir::new().unwrap();
    fs::copy(&ac_file, single_dir.path().join("a.cpp")).unwrap();
    fs::copy(&wa_file, single_dir.path().join("B-sum.cpp")).unwrap();
    fs::copy(&ac_file, single_dir.path().join("random_notes.cpp")).unwrap();

    let single_rep =
        run_external_solutions_for_contest(&ws, "round1", single_dir.path(), &ext_opts, None)
            .unwrap();
    assert_eq!(single_rep.mode, ExternalContestMode::SingleDirectory);
    assert_eq!(single_rep.submissions.len(), 2);
    assert_eq!(single_rep.unmatched.len(), 1);
    assert!(
        single_rep.unmatched[0]
            .file_path
            .contains("random_notes.cpp")
    );

    // 3. Multi-tester 2-level directory (`<dir>/<tester>/<prob>.cpp`).
    let multi_dir = TempDir::new().unwrap();
    fs::create_dir_all(multi_dir.path().join("alice")).unwrap();
    fs::create_dir_all(multi_dir.path().join("bob")).unwrap();
    fs::copy(&ac_file, multi_dir.path().join("alice/A.cpp")).unwrap();
    fs::copy(&ac_file, multi_dir.path().join("alice/sum-b.cpp")).unwrap();
    fs::copy(&wa_file, multi_dir.path().join("bob/A-greedy.cpp")).unwrap();

    let multi_rep =
        run_external_solutions_for_contest(&ws, "round1", multi_dir.path(), &ext_opts, None)
            .unwrap();
    assert_eq!(multi_rep.mode, ExternalContestMode::MultiTester);
    assert_eq!(
        multi_rep.testers,
        vec!["alice".to_owned(), "bob".to_owned()]
    );
    assert_eq!(multi_rep.submissions.len(), 3);

    let contest_md = render_external_contest_markdown(&multi_rep);
    assert!(contest_md.contains("验题人 × 题目矩阵"));
    assert!(contest_md.contains("| alice |"));
    assert!(contest_md.contains("| bob |"));

    // 4. Explicit atomic Adopt + comment/unknown-field preservation + overwrite/traversal guard.
    let adopt_res = adopt_external_solution(
        &ws,
        "apb-a",
        &ac_file,
        &AdoptSolutionOptions {
            name: Some("tester_alice".to_owned()),
            expect: vec!["AC".to_owned()],
            tags: vec![],
            is_main: false,
        },
    )
    .unwrap();
    assert_eq!(adopt_res.solution_name, "tester_alice");
    assert!(ws.root().join(&adopt_res.dest_rel).is_file());

    let updated_toml = fs::read_to_string(ws.root().join("problems/apb-a/problem.toml")).unwrap();
    assert!(updated_toml.contains("# User comment that must be preserved"));
    assert!(updated_toml.contains("custom_unknown_field = \"keep-me\""));
    assert!(updated_toml.contains("[solutions.tester_alice]"));

    // Reject duplicate overwrite.
    assert!(
        adopt_external_solution(
            &ws,
            "apb-a",
            &ac_file,
            &AdoptSolutionOptions {
                name: Some("tester_alice".to_owned()),
                ..Default::default()
            },
        )
        .is_err()
    );

    // Reject path traversal.
    assert!(
        adopt_external_solution(
            &ws,
            "apb-a",
            &ac_file,
            &AdoptSolutionOptions {
                name: Some("../escape".to_owned()),
                ..Default::default()
            },
        )
        .is_err()
    );

    // Reject read-only workspace.
    let ro_ws = Workspace::open(ws.root(), true).unwrap();
    assert!(
        adopt_external_solution(
            &ro_ws,
            "apb-a",
            &ac_file,
            &AdoptSolutionOptions {
                name: Some("tester_ro".to_owned()),
                ..Default::default()
            },
        )
        .is_err()
    );
}

#[test]
fn contest_audit_aggregates_statuses_warnings_lint_and_markdown() {
    if detect_toolchain(None).active.is_none() {
        return;
    }

    let (_dir, ws, cache) = setup_workspace();
    create_apb_package(&ws, "prob-fresh", true);
    create_apb_package(&ws, "prob-stale", false);
    create_apb_package(&ws, "prob-unbuilt", false);
    create_statement_only_problem(&ws, "prob-stmt");
    create_contest(
        &ws,
        "finals",
        &[
            ("A", "prob-fresh"),
            ("B", "prob-stale"),
            ("C", "prob-unbuilt"),
            ("D", "prob-stmt"),
        ],
    );

    let build_opts = BuildOptions {
        mode: BuildMode::Verify,
        cache_dir: Some(cache.path().to_path_buf()),
        ..Default::default()
    };
    build_package(&ws, "prob-fresh", &build_opts, None, None).unwrap();
    build_package(&ws, "prob-stale", &build_opts, None, None).unwrap();

    // Run sanitizer on prob-fresh.
    let san_opts = SanitizeOptions {
        cache_dir: Some(cache.path().to_path_buf()),
        ..Default::default()
    };
    let san = sanitize_package(&ws, "prob-fresh", &san_opts, None, None).unwrap();

    // Mutate prob-stale to make it Stale.
    fs::write(ws.root().join("problems/prob-stale/samples/01.in"), "2 3\n").unwrap();

    // Create a large test file (> 1 KiB threshold in custom AuditOptions) in prob-fresh build dir.
    let big_in = ws.root().join("build/problems/prob-fresh/tests/099.in");
    fs::write(&big_in, vec![b'a'; 2048]).unwrap();

    // Inject mock xcpc-lint findings for prob-stmt.
    let mut lint_map = BTreeMap::new();
    lint_map.insert(
        "prob-stmt".to_owned(),
        ProblemLintSummary {
            error_count: 1,
            warning_count: 2,
            findings: vec![AuditLintFinding {
                severity: "error".to_owned(),
                code: "missing_image".to_owned(),
                message: "picture missing".to_owned(),
                file: Some("problems/prob-stmt/statement.md".to_owned()),
            }],
        },
    );

    let audit_opts = AuditOptions {
        max_single_file_bytes: 1024, // trigger large_test_file warning on 2048-byte file
        max_cumulative_std_ms: 30_000,
        lint_by_problem: lint_map,
    };

    // Audit must work in read-only mode too!
    let ro_ws = Workspace::open(ws.root(), true).unwrap();
    let audit = audit_contest(&ro_ws, "finals", &audit_opts).unwrap();

    assert_eq!(audit.summary.total_problems, 4);
    assert_eq!(audit.summary.fresh_count, 1);
    assert_eq!(audit.summary.stale_count, 1);
    assert_eq!(audit.summary.unbuilt_count, 1);
    assert_eq!(audit.summary.statement_only_count, 1);
    assert!(!audit.summary.ready_for_freeze);

    let row_a = &audit.problems[0];
    assert_eq!(row_a.verification_status, VerificationStatus::Fresh);
    assert_eq!(row_a.independent_ac_count, 1);
    assert_eq!(row_a.wrong_solution_count, 1);
    assert_eq!(row_a.validator_self_test, SelfTestAuditStatus::Passed);
    assert!(row_a.max_test_file_bytes >= 2048);
    assert!(
        row_a
            .risk_flags
            .iter()
            .any(|f| f.code == "large_test_file" && f.level == AuditRiskLevel::Warning)
    );
    if san.status == SanitizerStatus::Passed {
        assert_eq!(row_a.sanitizer_status, SanitizerAuditStatus::PassedFresh);
    } else {
        assert_eq!(row_a.sanitizer_status, SanitizerAuditStatus::Unsupported);
    }

    let row_b = &audit.problems[1];
    assert_eq!(row_b.verification_status, VerificationStatus::Stale);
    assert!(row_b.risk_flags.iter().any(|f| f.code == "package_stale"));

    let row_c = &audit.problems[2];
    assert_eq!(row_c.verification_status, VerificationStatus::Unbuilt);
    assert_eq!(row_c.sanitizer_status, SanitizerAuditStatus::NotRun);

    let row_d = &audit.problems[3];
    assert_eq!(row_d.verification_status, VerificationStatus::StatementOnly);
    assert_eq!(row_d.lint_error_count, 1);
    assert_eq!(row_d.lint_warning_count, 2);

    assert!(!audit.next_steps.is_empty());

    let md = render_contest_audit_markdown(&audit);
    assert!(md.contains("整场验题体检报告"));
    assert!(md.contains("prob-fresh"));
    assert!(md.contains("prob-stale"));
    assert!(md.contains("large_test_file"));

    // Clean up temporary big file check.
    assert!(Path::new(&big_in).exists());
}

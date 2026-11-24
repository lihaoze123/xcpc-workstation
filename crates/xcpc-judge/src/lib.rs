//! `xcpc-judge`: Local Polygon native execution engine, strict verification gates,
//! 3-level content-hash cache, and stress testing for XCPC Workstation (Phase 3A).

pub mod assets;
pub mod audit;
pub mod build;
pub mod cache;
pub mod error;
pub mod external;
pub mod ops;
pub mod package;
pub mod report;
pub mod runner;
pub mod sanitize;
pub mod subtask;
pub mod toolchain;

pub use assets::{STANDARD_CHECKERS, TESTLIB_H, is_standard_checker, standard_checker_source};
pub use audit::{
    AuditLintFinding, AuditOptions, AuditRiskFlag, AuditRiskLevel, ContestAuditReport,
    ContestAuditSummary, DEFAULT_MAX_CUMULATIVE_STD_MS, DEFAULT_MAX_SINGLE_FILE_BYTES,
    ProblemAuditRow, ProblemLintSummary, SanitizerAuditStatus, SelfTestAuditStatus,
    VerificationStatus, audit_contest, render_contest_audit_markdown,
};
pub use build::{
    BatchBuildEvent, BatchBuildItemResult, BatchBuildItemStatus, BatchBuildReport, BuildOptions,
    ProgressCallback, build_package, build_packages_batch,
};
pub use cache::{JudgeCache, default_cache_root};
pub use error::{JudgeError, Result};
pub use external::{
    AdoptSolutionOptions, AdoptSolutionResult, ContestProblemRef, ExternalContestMode,
    ExternalContestReport, ExternalFirstFailure, ExternalProblemReport, ExternalRunOptions,
    ExternalSubmissionResult, UnmatchedExternalFile, adopt_external_solution,
    render_external_contest_markdown, render_external_problem_markdown,
    run_external_solutions_for_contest, run_external_solutions_for_problem,
};
pub use ops::{
    AddedTestInfo, InitPackageOptions, InitPackageResult, PackageFiles, PackageInspection,
    PackageSolutionInfo, PackageSourceKind, StressOptions, StressResult, TestCasePreview,
    add_generator_test, add_hand_test, create_package_source, delete_generator, delete_interactor,
    delete_self_test, delete_solution, delete_subtask, init_package, init_package_with_options,
    inspect_package, inspect_test_case, normalize_dirty_inputs, remove_test_rule,
    set_package_checker, set_package_runs, set_solution_config, set_solution_scoring,
    set_test_subtask, stress_package, update_generator_test, upsert_subtask,
};
pub use package::{
    CheckerSpec, DiscoveredSolution, ExpandedTest, PackageFreshness, TestSource, build_dir_for,
    check_dirty_inputs, check_package_freshness, check_package_freshness_in, compute_package_hash,
    compute_package_hash_in, contest_limits_key, discover_solutions, effective_run_limits,
    expand_tests, find_interactor_source, has_explicit_checker, load_latest_report,
    load_verified_report, load_verified_report_in, report_path_for, resolve_checker_spec,
    sanitizer_report_path_for, verified_report_path_for, verified_report_path_in,
};
pub use report::{
    BuildMode, BuildReport, DirtyInputIssue, GateResult, GateStatus, InteractionTranscript,
    MatrixCell, ProgressEvent, SafetyMargin, SelfTestResult, SolutionSubtaskScore, SolutionSummary,
    SubtaskSummary, TestSummary, TranscriptDirection, TranscriptEntry,
};
pub use runner::{
    CheckerOutcome, InteractiveOutcome, RunLimits, RunOutcome, Verdict, parse_memory_limit_kib,
    parse_testlib_verdict, parse_time_limit_ms, run_checker, run_checker_with_score,
    run_checker_with_score_and_limits, run_interactive, run_multi_run_interactive,
    run_multi_run_process, run_process, run_validator, run_validator_with_group,
    run_validator_with_group_and_limits,
};
pub use sanitize::{
    SanitizeOptions, SanitizerFinding, SanitizerReport, SanitizerStatus, SanitizerTargetKind,
    SanitizerTargetResult, extract_sanitizer_diagnostic, is_expected_ac_solution,
    load_sanitizer_report, sanitize_package, sanitize_packages_batch,
};
pub use subtask::{SubtaskValidation, validate_subtask_config};
pub use toolchain::{
    CompileDiagnostic, CompileFailure, CompilerKind, CppStd, DetectedCompiler, SourceLanguage,
    ToolchainStatus, detect_toolchain, parse_compiler_diagnostics, probe_sanitizer_support,
    resolve_active_compiler, validate_compiler_path,
};

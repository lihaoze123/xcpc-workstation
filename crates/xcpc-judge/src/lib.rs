//! `xcpc-judge`: Local Polygon native execution engine, strict verification gates,
//! 3-level content-hash cache, and stress testing for XCPC Workstation (Phase 3A).

pub mod assets;
pub mod build;
pub mod cache;
pub mod error;
pub mod ops;
pub mod package;
pub mod report;
pub mod runner;
pub mod toolchain;

pub use assets::{STANDARD_CHECKERS, TESTLIB_H, is_standard_checker, standard_checker_source};
pub use build::{BuildOptions, ProgressCallback, build_package};
pub use cache::{JudgeCache, default_cache_root};
pub use error::{JudgeError, Result};
pub use ops::{
    InitPackageResult, PackageFiles, PackageInspection, StressOptions, StressResult,
    TestCasePreview, init_package, inspect_package, inspect_test_case, stress_package,
};
pub use package::{
    CheckerSpec, DiscoveredSolution, ExpandedTest, PackageFreshness, TestSource, build_dir_for,
    check_dirty_inputs, check_package_freshness, compute_package_hash, discover_solutions,
    expand_tests, load_latest_report, load_verified_report, report_path_for, resolve_checker_spec,
    verified_report_path_for,
};
pub use report::{
    BuildMode, BuildReport, DirtyInputIssue, GateResult, GateStatus, MatrixCell, ProgressEvent,
    SafetyMargin, SelfTestResult, SolutionSummary, TestSummary,
};
pub use runner::{
    RunLimits, RunOutcome, Verdict, parse_memory_limit_kib, parse_time_limit_ms, run_checker,
    run_process, run_validator,
};
pub use toolchain::{
    CompileDiagnostic, CompileFailure, CompilerKind, CppStd, DetectedCompiler, SourceLanguage,
    ToolchainStatus, detect_toolchain, parse_compiler_diagnostics, resolve_active_compiler,
};

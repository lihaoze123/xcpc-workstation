//! Build report, gates, safety margin, and progress event data structures.

use serde::{Deserialize, Serialize};

use crate::runner::Verdict;
use crate::toolchain::{CompileFailure, DetectedCompiler};

/// Build mode: development (`Debug`) vs strict acceptance (`Verify`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BuildMode {
    #[default]
    Debug,
    Verify,
}

/// Outcome of an individual verification gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GateStatus {
    Pass,
    Fail,
    Warn,
    Skip,
}

/// One gate evaluation in a build report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GateResult {
    pub id: String,
    pub title: String,
    pub status: GateStatus,
    pub message: String,
}

/// Summary of safety margins for the problem package.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SafetyMargin {
    pub max_input_bytes: u64,
    pub time_limit_ms: u64,
    pub memory_limit_kib: u64,
    pub std_max_time_ms: u64,
    /// `std_max_time_ms / time_limit_ms` (hard gate requires `<= 0.50` in Verify mode).
    pub std_time_ratio: f64,
    pub std_max_memory_kib: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slowest_ac_solution: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slowest_ac_time_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fastest_tle_solution: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fastest_tle_time_ms: Option<u64>,
}

/// One cell in the `tests × solutions` execution matrix.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MatrixCell {
    pub test_index: usize,
    pub test_name: String,
    pub solution: String,
    pub verdict: Verdict,
    pub cpu_time_ms: u64,
    pub wall_time_ms: u64,
    pub peak_memory_kib: u64,
    pub cache_hit: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub checker_message: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub stderr: String,
    /// Bounded snippet (<= 1 KiB) of the solution's actual stdout for 3-column diagnostics.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_preview: Option<String>,
    /// `true` when preserved from a previous report during a subset run (not re-executed).
    #[serde(default)]
    pub stale: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repro_cmd: Option<String>,
}

/// Aggregated result for one solution across the tests it ran on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SolutionSummary {
    pub name: String,
    pub is_main: bool,
    pub expected: Vec<String>,
    pub actual_verdict: Verdict,
    pub matched: bool,
    pub max_time_ms: u64,
    pub max_memory_kib: u64,
    pub passed_tests: usize,
    pub total_tests: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
}

/// Summary of one expanded test case in a build report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TestSummary {
    pub index: usize,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    pub is_sample: bool,
    pub size_bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gen_cmd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub validator_ok: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub validator_message: Option<String>,
}

/// Outcome of one validator or checker self-test case.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelfTestResult {
    /// `"validator_valid"`, `"validator_invalid"`, `"checker_ok"`, `"checker_wa"`, `"checker_pe"`.
    pub kind: String,
    pub name: String,
    pub passed: bool,
    pub expected: String,
    pub actual: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub message: String,
}

/// Dirty formatting issue detected in a hand-written input file (`samples/` or `tests/`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirtyInputIssue {
    pub path: String,
    pub has_crlf: bool,
    pub has_bom: bool,
    pub missing_trailing_newline: bool,
}

/// Full structured report written atomically to `build/problems/<slug>/report.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BuildReport {
    pub problem_id: String,
    pub package_hash: String,
    pub mode: BuildMode,
    /// `true` only when `mode == Verify`, not a subset run, and all hard gates passed.
    pub verified: bool,
    /// `true` when there are no hard failures in the current run.
    pub ok: bool,
    /// `true` when `--tests` or `--solutions` subset filter was active.
    pub subset: bool,
    pub built_at_epoch_secs: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compiler: Option<DetectedCompiler>,
    pub checker: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub compile_failures: Vec<CompileFailure>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dirty_inputs: Vec<DirtyInputIssue>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub self_tests: Vec<SelfTestResult>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tests: Vec<TestSummary>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub solutions: Vec<SolutionSummary>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub matrix: Vec<MatrixCell>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub gates: Vec<GateResult>,
    pub safety_margin: SafetyMargin,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub repro_commands: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub next_steps: Vec<String>,
}

/// Live progress notification emitted during `build_package`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "phase", rename_all = "snake_case")]
pub enum ProgressEvent {
    Compiling {
        target: String,
        completed: usize,
        total: usize,
    },
    SelfTesting {
        completed: usize,
        total: usize,
    },
    Generating {
        test_name: String,
        completed: usize,
        total: usize,
    },
    Validating {
        test_name: String,
        completed: usize,
        total: usize,
    },
    Running {
        solution: String,
        test_name: String,
        verdict: Verdict,
        cpu_time_ms: u64,
        completed: usize,
        total: usize,
        /// Completed matrix cell for live per-cell UI updates.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cell: Option<Box<MatrixCell>>,
    },
    Finished {
        ok: bool,
        verified: bool,
    },
}

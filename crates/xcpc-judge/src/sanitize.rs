//! Sanitizer (ASan / UBSan) undefined-behavior check mode (Phase 3D).
//!
//! - Compiles `generators/*`, `validator.cpp`, custom `checker.cpp`, `interactor.cpp`, and all
//!   expected-AC solutions with `-fsanitize=address,undefined -fno-omit-frame-pointer -g`.
//! - Strictly isolated: uses `<cache>/sanitize/bin/` for binaries, temporary directories for
//!   generated inputs/outputs, and writes ONLY `build/problems/<slug>/sanitizer-report.json`.
//!   Never overwrites `report.json`, `verified_report.json`, normal `-O2` cache, or `samples/`.
//! - Relaxes time limits to `max(5 * TL, 5000ms)` and disables `RLIMIT_AS` so ASan shadow
//!   memory and instrumentation overhead do not cause false TLE/MLE.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use xcpc_storage::Workspace;

use crate::assets::{resolve_testlib, standard_checker_source};
use crate::build::{
    BatchBuildEvent, BatchBuildItemResult, BatchBuildItemStatus, BatchBuildReport, ProgressCallback,
};
use crate::cache::{JudgeCache, write_atomic};
use crate::error::{JudgeError, Result, io_err};
use crate::package::{
    CheckerSpec, DiscoveredSolution, ExpandedTest, TestSource, compute_package_hash,
    discover_solutions, expand_tests, find_generator_source, find_interactor_source,
    find_validator_source, has_explicit_checker, resolve_checker_spec, sanitizer_report_path_for,
};
use crate::report::ProgressEvent;
use crate::runner::{
    RunLimits, Verdict, run_checker_with_score_and_limits, run_multi_run_interactive,
    run_multi_run_process, run_process, run_validator_with_group_and_limits,
};
use crate::toolchain::{
    CompileFailure, CompileResult, CppStd, DetectedCompiler, SourceLanguage,
    compile_cached_sanitized, probe_sanitizer_support, resolve_active_compiler,
};

/// Options controlling `sanitize_package`.
#[derive(Debug, Clone, Default)]
pub struct SanitizeOptions {
    /// Optional subset of tests (1-based index, test name, or tag).
    pub tests: Vec<String>,
    /// Optional subset of solution names (defaults to all expected-AC solutions).
    pub solutions: Vec<String>,
    /// Parallelism for compilation and independent checks.
    pub jobs: Option<usize>,
    /// Optional explicit compiler path override.
    pub cxx_override: Option<PathBuf>,
    /// Optional explicit cache directory override.
    pub cache_dir: Option<PathBuf>,
}

/// Overall outcome of a sanitizer UB check run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SanitizerStatus {
    Passed,
    Failed,
    Unsupported,
}

/// Kind of package target checked by the sanitizer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SanitizerTargetKind {
    Generator,
    Validator,
    Checker,
    Interactor,
    Solution,
}

/// One ASan/UBSan or runtime failure captured during sanitizer checking.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SanitizerFinding {
    pub target: String,
    pub kind: SanitizerTargetKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub test_index: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub test_name: Option<String>,
    /// Concise one-line ASan/UBSan summary (e.g. `AddressSanitizer: heap-buffer-overflow`).
    pub summary: String,
    /// Concise stack frames / key diagnostic lines extracted from stderr.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stack_summary: Vec<String>,
    /// Bounded raw stderr excerpt (at most 4 KiB).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub raw_stderr: String,
}

/// Per-target summary inside [`SanitizerReport`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SanitizerTargetResult {
    pub target: String,
    pub kind: SanitizerTargetKind,
    pub source_path: String,
    pub passed: bool,
    pub checked_runs: usize,
    pub findings_count: usize,
}

/// Persisted sanitizer report written atomically to `build/problems/<slug>/sanitizer-report.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SanitizerReport {
    pub problem_id: String,
    pub package_hash: String,
    pub status: SanitizerStatus,
    pub ok: bool,
    pub subset: bool,
    pub checked_at_epoch_secs: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compiler: Option<DetectedCompiler>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unsupported_reason: Option<String>,
    pub relaxed_time_limit_ms: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub compile_failures: Vec<CompileFailure>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub targets: Vec<SanitizerTargetResult>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub findings: Vec<SanitizerFinding>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub next_steps: Vec<String>,
}

/// Loads the isolated sanitizer report if present and valid JSON.
pub fn load_sanitizer_report(ws: &Workspace, slug: &str) -> Option<SanitizerReport> {
    let bytes = fs::read(sanitizer_report_path_for(ws, slug)).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Returns `true` if a solution is expected to pass (AC on full suite or on at least one subtask).
pub fn is_expected_ac_solution(sol: &DiscoveredSolution) -> bool {
    if sol.is_main {
        return true;
    }
    if let Some(ref exp) = sol.explicit_expect
        && exp.matches("AC")
    {
        return true;
    }
    if sol.explicit_expect.is_none() && sol.expect.matches("AC") {
        return true;
    }
    sol.subtasks.values().any(|e| e.matches("AC"))
}

/// Extracts a concise ASan/UBSan summary line and stack frames from stderr, if sanitizer
/// diagnostics are present.
pub fn extract_sanitizer_diagnostic(stderr: &str) -> Option<(String, Vec<String>)> {
    let mut summary: Option<String> = None;
    let mut stack: Vec<String> = Vec::new();
    let mut saw_sanitizer = false;

    for raw_line in stderr.lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        if line.contains("SUMMARY: AddressSanitizer:")
            || line.contains("SUMMARY: UndefinedBehaviorSanitizer:")
            || line.contains("SUMMARY: LeakSanitizer:")
        {
            saw_sanitizer = true;
            summary = Some(
                line.strip_prefix("SUMMARY: ")
                    .unwrap_or(line)
                    .trim()
                    .to_owned(),
            );
        } else if line.contains("ERROR: AddressSanitizer:")
            || line.contains("ERROR: UndefinedBehaviorSanitizer:")
        {
            saw_sanitizer = true;
            if summary.is_none() {
                let cleaned = line
                    .split("ERROR: ")
                    .nth(1)
                    .unwrap_or(line)
                    .trim()
                    .to_owned();
                summary = Some(cleaned);
            }
        } else if line.contains("runtime error:") {
            saw_sanitizer = true;
            if summary.is_none() {
                summary = Some(line.to_owned());
            }
            if stack.len() < 8 {
                stack.push(line.to_owned());
            }
        } else if line.starts_with('#')
            && line.chars().nth(1).is_some_and(|c| c.is_ascii_digit())
            && stack.len() < 8
        {
            stack.push(line.to_owned());
        } else if (line.starts_with("READ of size")
            || line.starts_with("WRITE of size")
            || line.starts_with("Address ")
            || line.starts_with("is located"))
            && stack.len() < 8
        {
            saw_sanitizer = true;
            stack.push(line.to_owned());
        }
    }

    if saw_sanitizer {
        let sum = summary.unwrap_or_else(|| "ASan/UBSan detected undefined behavior".to_owned());
        Some((sum, stack))
    } else {
        None
    }
}

#[derive(Debug, Clone)]
struct SanCompileTarget {
    id: String,
    display_path: String,
    language: SourceLanguage,
    source_bytes: Vec<u8>,
    disk_path: Option<PathBuf>,
}

/// Runs ASan + UBSan UB check on a problem package.
///
/// Writes ONLY `build/problems/<slug>/sanitizer-report.json` (when workspace is writable).
/// Never touches `report.json`, `verified_report.json`, normal `-O2` cache, or `samples/`.
pub fn sanitize_package(
    ws: &Workspace,
    slug: &str,
    opts: &SanitizeOptions,
    cancel: Option<Arc<AtomicBool>>,
    progress: Option<ProgressCallback<'_>>,
) -> Result<SanitizerReport> {
    let check_cancel = || -> Result<()> {
        if let Some(ref c) = cancel
            && c.load(Ordering::Relaxed)
        {
            return Err(JudgeError::Cancelled);
        }
        Ok(())
    };
    let emit = |ev: ProgressEvent| {
        if let Some(cb) = progress {
            cb(ev);
        }
    };

    check_cancel()?;

    let problem = ws.load_problem(slug)?;
    let prob_dir = ws.resolve(&problem.dir)?;
    let package_hash = compute_package_hash(ws, slug)?;
    let compiler = resolve_active_compiler(opts.cxx_override.as_deref())?;
    let cache = match &opts.cache_dir {
        Some(dir) => JudgeCache::open(dir)?,
        None => JudgeCache::open_default()?,
    };
    let testlib = resolve_testlib(ws.root(), &prob_dir);

    let base_limits = RunLimits::from_problem_limits(
        problem.meta.limits.time.as_deref(),
        problem.meta.limits.memory.as_deref(),
    )?;
    let san_limits = RunLimits::for_sanitizer(base_limits);
    let san_tool_limits = RunLimits::sanitizer_tool_defaults();
    let is_subset = !opts.tests.is_empty() || !opts.solutions.is_empty();

    // Probe toolchain sanitizer support gracefully.
    if let Err(reason) = probe_sanitizer_support(&compiler) {
        let report = SanitizerReport {
            problem_id: slug.to_owned(),
            package_hash,
            status: SanitizerStatus::Unsupported,
            ok: false,
            subset: is_subset,
            checked_at_epoch_secs: now_epoch_secs(),
            compiler: Some(compiler),
            unsupported_reason: Some(reason.clone()),
            relaxed_time_limit_ms: san_limits.time_limit_ms,
            compile_failures: vec![],
            targets: vec![],
            findings: vec![],
            diagnostics: vec![reason],
            next_steps: vec![
                "当前工具链不支持 ASan/UBSan 运行库（不影响普通 --strict 验收与冻结）。可在支持 ASan 的 Linux/macOS 环境或安装 libasan 后重跑。"
                    .to_owned(),
            ],
        };
        if !ws.is_read_only() {
            persist_sanitizer_report(ws, slug, &report)?;
        }
        emit(ProgressEvent::Finished {
            ok: false,
            verified: false,
        });
        return Ok(report);
    }

    let interactor_src = find_interactor_source(&prob_dir);
    let is_interactive = interactor_src.is_some();
    let runs = problem.meta.run_count();
    let use_checker = !is_interactive || has_explicit_checker(&prob_dir, &problem.meta);
    let checker_spec = resolve_checker_spec(&prob_dir, &problem.meta)?;
    let validator_src = find_validator_source(&prob_dir);
    let all_tests = expand_tests(&prob_dir, &problem.meta)?;
    let all_solutions = discover_solutions(&prob_dir, &problem.meta)?;

    let main_solutions: Vec<&DiscoveredSolution> =
        all_solutions.iter().filter(|s| s.is_main).collect();
    if main_solutions.is_empty() {
        return Err(JudgeError::NoMainSolution(slug.to_owned()));
    }
    if main_solutions.len() > 1 {
        return Err(JudgeError::MultipleMainSolutions(
            slug.to_owned(),
            main_solutions.iter().map(|s| s.name.clone()).collect(),
        ));
    }
    let main_sol = main_solutions[0].clone();

    // Filter tests.
    let selected_tests: Vec<ExpandedTest> = if opts.tests.is_empty() {
        all_tests.clone()
    } else {
        let filtered: Vec<ExpandedTest> = all_tests
            .iter()
            .filter(|t| matches_test_filter(t, &opts.tests))
            .cloned()
            .collect();
        if filtered.is_empty() {
            return Err(JudgeError::TestNotFound(opts.tests.join(", ")));
        }
        filtered
    };

    // Filter solutions: default to all expected-AC solutions; if explicit `opts.solutions` is set,
    // validate names and check those.
    let selected_solutions: Vec<DiscoveredSolution> = if opts.solutions.is_empty() {
        all_solutions
            .iter()
            .filter(|s| is_expected_ac_solution(s))
            .cloned()
            .collect()
    } else {
        let wanted: HashSet<&str> = opts.solutions.iter().map(String::as_str).collect();
        for w in &wanted {
            if !all_solutions.iter().any(|s| s.name == *w) {
                return Err(JudgeError::SolutionNotFound((*w).to_owned()));
            }
        }
        all_solutions
            .iter()
            .filter(|s| wanted.contains(s.name.as_str()))
            .cloned()
            .collect()
    };

    let parallel_jobs = opts.jobs.filter(|&n| n > 0).unwrap_or_else(|| {
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4)
    });
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(parallel_jobs)
        .build()
        .map_err(|e| JudgeError::Internal(format!("create thread pool: {e}")))?;

    // Collect sanitizer compile targets.
    let mut compile_targets: Vec<SanCompileTarget> = Vec::new();
    let mut target_meta: HashMap<String, (SanitizerTargetKind, String)> = HashMap::new();

    if let Some((ref p, ref rel, lang)) = validator_src {
        let bytes = fs::read(p).map_err(|e| io_err(format!("read {}", p.display()), e))?;
        compile_targets.push(SanCompileTarget {
            id: "validator".to_owned(),
            display_path: rel.clone(),
            language: lang,
            source_bytes: bytes,
            disk_path: Some(p.clone()),
        });
        target_meta.insert(
            "validator".to_owned(),
            (SanitizerTargetKind::Validator, rel.clone()),
        );
    }

    if let Some((ref p, ref rel, lang)) = interactor_src {
        let bytes = fs::read(p).map_err(|e| io_err(format!("read {}", p.display()), e))?;
        compile_targets.push(SanCompileTarget {
            id: "interactor".to_owned(),
            display_path: rel.clone(),
            language: lang,
            source_bytes: bytes,
            disk_path: Some(p.clone()),
        });
        target_meta.insert(
            "interactor".to_owned(),
            (SanitizerTargetKind::Interactor, rel.clone()),
        );
    }

    let checker_is_custom = use_checker && checker_spec.is_custom();
    if use_checker {
        match &checker_spec {
            CheckerSpec::Builtin(name) => {
                let src = standard_checker_source(name)
                    .ok_or_else(|| JudgeError::UnknownChecker(name.clone()))?;
                compile_targets.push(SanCompileTarget {
                    id: "checker".to_owned(),
                    display_path: format!("<builtin:{name}>"),
                    language: SourceLanguage::Cpp { std: CppStd::Cpp20 },
                    source_bytes: src.as_bytes().to_vec(),
                    disk_path: None,
                });
            }
            CheckerSpec::Custom { path, display_name } => {
                let lang = SourceLanguage::from_path(path)
                    .unwrap_or(SourceLanguage::Cpp { std: CppStd::Cpp20 });
                let bytes =
                    fs::read(path).map_err(|e| io_err(format!("read {}", path.display()), e))?;
                compile_targets.push(SanCompileTarget {
                    id: "checker".to_owned(),
                    display_path: display_name.clone(),
                    language: lang,
                    source_bytes: bytes,
                    disk_path: Some(path.clone()),
                });
                target_meta.insert(
                    "checker".to_owned(),
                    (SanitizerTargetKind::Checker, display_name.clone()),
                );
            }
        }
    }

    // Generators used by selected tests.
    let mut seen_gens = HashSet::new();
    for t in &selected_tests {
        if let TestSource::Generated { ref gen_name, .. } = t.source
            && seen_gens.insert(gen_name.clone())
        {
            let (p, rel, lang) = find_generator_source(&prob_dir, gen_name)?;
            let bytes = fs::read(&p).map_err(|e| io_err(format!("read {}", p.display()), e))?;
            let id = format!("gen:{gen_name}");
            compile_targets.push(SanCompileTarget {
                id: id.clone(),
                display_path: rel.clone(),
                language: lang,
                source_bytes: bytes,
                disk_path: Some(p),
            });
            target_meta.insert(id, (SanitizerTargetKind::Generator, rel));
        }
    }

    // Always compile main_sol (needed to produce reference .ans) plus selected expected-AC solutions.
    let mut sol_seen = HashSet::new();
    for s in std::iter::once(&main_sol).chain(selected_solutions.iter()) {
        if sol_seen.insert(s.name.clone()) {
            let bytes = fs::read(&s.source_path)
                .map_err(|e| io_err(format!("read {}", s.source_path.display()), e))?;
            let id = format!("sol:{}", s.name);
            compile_targets.push(SanCompileTarget {
                id: id.clone(),
                display_path: s.source_rel.clone(),
                language: s.language,
                source_bytes: bytes,
                disk_path: Some(s.source_path.clone()),
            });
            target_meta.insert(id, (SanitizerTargetKind::Solution, s.source_rel.clone()));
        }
    }

    check_cancel()?;

    // Phase 1: Parallel sanitizer compilation into isolated `<cache>/sanitize/bin/`.
    let total_compile = compile_targets.len();
    let done_compile = AtomicUsize::new(0);
    let compile_results: Vec<(String, std::result::Result<CompileResult, CompileFailure>)> = pool
        .install(|| {
            compile_targets
                .par_iter()
                .map(|t| {
                    let res = compile_cached_sanitized(
                        &cache,
                        &compiler,
                        t.language,
                        &t.id,
                        &t.display_path,
                        &t.source_bytes,
                        t.disk_path.as_deref(),
                        &testlib,
                    );
                    let completed = done_compile.fetch_add(1, Ordering::Relaxed) + 1;
                    emit(ProgressEvent::Compiling {
                        target: t.id.clone(),
                        completed,
                        total: total_compile,
                    });
                    (t.id.clone(), res)
                })
                .collect()
        });

    let mut compiled: HashMap<String, CompileResult> = HashMap::new();
    let mut compile_failures: Vec<CompileFailure> = Vec::new();
    for (id, res) in compile_results {
        match res {
            Ok(cr) => {
                compiled.insert(id, cr);
            }
            Err(fail) => {
                compile_failures.push(fail);
            }
        }
    }

    if !compile_failures.is_empty() {
        let diagnostics: Vec<String> = compile_failures
            .iter()
            .map(|f| {
                format!(
                    "sanitizer compile error in {} ({})",
                    f.target, f.source_path
                )
            })
            .collect();
        let report = SanitizerReport {
            problem_id: slug.to_owned(),
            package_hash,
            status: SanitizerStatus::Failed,
            ok: false,
            subset: is_subset,
            checked_at_epoch_secs: now_epoch_secs(),
            compiler: Some(compiler),
            unsupported_reason: None,
            relaxed_time_limit_ms: san_limits.time_limit_ms,
            compile_failures,
            targets: vec![],
            findings: vec![],
            diagnostics,
            next_steps: vec!["修复编译错误后重新运行 Sanitizer UB 体检。".to_owned()],
        };
        if !ws.is_read_only() {
            persist_sanitizer_report(ws, slug, &report)?;
        }
        emit(ProgressEvent::Finished {
            ok: false,
            verified: false,
        });
        return Ok(report);
    }

    check_cancel()?;

    // Track per-target run counts and findings.
    let mut run_counts: HashMap<String, usize> = HashMap::new();
    let mut findings: Vec<SanitizerFinding> = Vec::new();
    let mut diagnostics: Vec<String> = Vec::new();

    // Isolated temporary directory for generated inputs & reference answers.
    let tmp_dir = tempfile::tempdir().map_err(|e| io_err("create sanitizer temp dir", e))?;
    let tmp_tests_dir = tmp_dir.path().join("tests");
    fs::create_dir_all(&tmp_tests_dir).map_err(|e| io_err("create sanitizer temp tests dir", e))?;

    // Phase 2: Materialize inputs (running sanitized generators directly without touching normal cache).
    let mut mat_inputs: Vec<(ExpandedTest, Option<PathBuf>)> =
        Vec::with_capacity(selected_tests.len());
    let total_gen = selected_tests.len();
    for (i, t) in selected_tests.iter().enumerate() {
        check_cancel()?;
        let dest = tmp_tests_dir.join(format!("{:03}.in", t.index));
        let materialized_path = match &t.source {
            TestSource::Sample { input_rel, .. } => {
                let src = prob_dir.join(input_rel);
                if src.is_file() {
                    Some(src)
                } else {
                    diagnostics.push(format!("missing sample input {input_rel}"));
                    None
                }
            }
            TestSource::File { file_rel } => {
                let src = prob_dir.join(file_rel);
                if src.is_file() {
                    Some(src)
                } else {
                    diagnostics.push(format!("missing hand test input {file_rel}"));
                    None
                }
            }
            TestSource::Generated {
                gen_name,
                args,
                args_raw,
            } => {
                let target_id = format!("gen:{gen_name}");
                *run_counts.entry(target_id.clone()).or_insert(0) += 1;
                let gen_cr = compiled
                    .get(&target_id)
                    .ok_or_else(|| JudgeError::GeneratorNotFound(gen_name.clone()))?;
                let outcome = run_process(
                    &gen_cr.binary_path,
                    args,
                    None,
                    Some(&dest),
                    san_tool_limits,
                    cancel.as_ref(),
                )?;
                if let Some((sum, stack)) = extract_sanitizer_diagnostic(&outcome.stderr) {
                    findings.push(SanitizerFinding {
                        target: target_id.clone(),
                        kind: SanitizerTargetKind::Generator,
                        test_index: Some(t.index),
                        test_name: Some(t.name.clone()),
                        summary: sum,
                        stack_summary: stack,
                        raw_stderr: truncate_stderr(&outcome.stderr),
                    });
                    None
                } else if outcome.verdict != Verdict::AC {
                    findings.push(SanitizerFinding {
                        target: target_id.clone(),
                        kind: SanitizerTargetKind::Generator,
                        test_index: Some(t.index),
                        test_name: Some(t.name.clone()),
                        summary: format!(
                            "generator `{gen_name}` failed on `{args_raw}` ({})",
                            outcome.verdict
                        ),
                        stack_summary: vec![],
                        raw_stderr: truncate_stderr(&outcome.stderr),
                    });
                    None
                } else {
                    Some(dest)
                }
            }
        };
        emit(ProgressEvent::Generating {
            test_name: t.name.clone(),
            completed: i + 1,
            total: total_gen,
        });
        mat_inputs.push((t.clone(), materialized_path));
    }

    check_cancel()?;

    // Phase 3: Run sanitized validator on all materialized inputs.
    if let Some(val_cr) = compiled.get("validator") {
        let total_val = mat_inputs.len();
        for (i, (spec, maybe_in)) in mat_inputs.iter().enumerate() {
            check_cancel()?;
            if let Some(in_path) = maybe_in {
                *run_counts.entry("validator".to_owned()).or_insert(0) += 1;
                let outcome = run_validator_with_group_and_limits(
                    &val_cr.binary_path,
                    in_path,
                    spec.subtask.as_deref(),
                    san_tool_limits,
                    cancel.as_ref(),
                )?;
                if let Err(msg) = outcome
                    && let Some((sum, stack)) = extract_sanitizer_diagnostic(&msg)
                {
                    findings.push(SanitizerFinding {
                        target: "validator".to_owned(),
                        kind: SanitizerTargetKind::Validator,
                        test_index: Some(spec.index),
                        test_name: Some(spec.name.clone()),
                        summary: sum,
                        stack_summary: stack,
                        raw_stderr: truncate_stderr(&msg),
                    });
                }
            }
            emit(ProgressEvent::Validating {
                test_name: spec.name.clone(),
                completed: i + 1,
                total: total_val,
            });
        }
    }

    check_cancel()?;

    // Phase 4: Run sanitized solutions (+ sanitized interactor / custom checker).
    let interactor_bin = compiled.get("interactor");
    let checker_bin = compiled.get("checker");
    let main_cr = compiled
        .get(&format!("sol:{}", main_sol.name))
        .ok_or_else(|| JudgeError::Internal("main solution sanitizer binary missing".to_owned()))?;

    let mut ans_paths: Vec<Option<PathBuf>> = vec![None; mat_inputs.len()];
    let mut planned_runs = mat_inputs.len();
    for sol in &selected_solutions {
        if sol.name != main_sol.name {
            planned_runs += mat_inputs.len();
        }
    }
    let done_runs = AtomicUsize::new(0);

    // Run main solution first to produce reference `.ans` in temp dir (and check main for UB).
    for (idx, (spec, maybe_in)) in mat_inputs.iter().enumerate() {
        check_cancel()?;
        let Some(in_path) = maybe_in else {
            done_runs.fetch_add(1, Ordering::Relaxed);
            continue;
        };
        let sol_target = format!("sol:{}", main_sol.name);
        *run_counts.entry(sol_target.clone()).or_insert(0) += 1;

        let out_path = tmp_tests_dir.join(format!("{:03}.ans", spec.index));
        let (verdict, cpu_ms) = if let Some(inter_cr) = interactor_bin {
            *run_counts.entry("interactor".to_owned()).or_insert(0) += 1;
            let inter_out = run_multi_run_interactive(
                &inter_cr.binary_path,
                &main_cr.binary_path,
                in_path,
                &out_path,
                runs,
                san_limits,
                cancel.as_ref(),
            )?;
            if let Some((sum, stack)) = extract_sanitizer_diagnostic(&inter_out.stderr) {
                findings.push(SanitizerFinding {
                    target: sol_target.clone(),
                    kind: SanitizerTargetKind::Solution,
                    test_index: Some(spec.index),
                    test_name: Some(spec.name.clone()),
                    summary: sum,
                    stack_summary: stack,
                    raw_stderr: truncate_stderr(&inter_out.stderr),
                });
            }
            if let Some((sum, stack)) =
                extract_sanitizer_diagnostic(&inter_out.transcript.interactor_message)
            {
                findings.push(SanitizerFinding {
                    target: "interactor".to_owned(),
                    kind: SanitizerTargetKind::Interactor,
                    test_index: Some(spec.index),
                    test_name: Some(spec.name.clone()),
                    summary: sum,
                    stack_summary: stack,
                    raw_stderr: truncate_stderr(&inter_out.transcript.interactor_message),
                });
            }
            if inter_out.verdict == Verdict::AC && out_path.is_file() {
                ans_paths[idx] = Some(out_path.clone());
                if checker_is_custom && let Some(chk_cr) = checker_bin {
                    *run_counts.entry("checker".to_owned()).or_insert(0) += 1;
                    let chk_res = run_checker_with_score_and_limits(
                        &chk_cr.binary_path,
                        in_path,
                        &out_path,
                        &out_path,
                        san_tool_limits,
                        cancel.as_ref(),
                    )?;
                    if let Some((sum, stack)) = extract_sanitizer_diagnostic(&chk_res.message) {
                        findings.push(SanitizerFinding {
                            target: "checker".to_owned(),
                            kind: SanitizerTargetKind::Checker,
                            test_index: Some(spec.index),
                            test_name: Some(spec.name.clone()),
                            summary: sum,
                            stack_summary: stack,
                            raw_stderr: truncate_stderr(&chk_res.message),
                        });
                    }
                }
            }
            (inter_out.verdict, inter_out.cpu_time_ms)
        } else {
            let outcome = run_multi_run_process(
                &main_cr.binary_path,
                in_path,
                &out_path,
                runs,
                san_limits,
                cancel.as_ref(),
            )?;
            if let Some((sum, stack)) = extract_sanitizer_diagnostic(&outcome.stderr) {
                findings.push(SanitizerFinding {
                    target: sol_target.clone(),
                    kind: SanitizerTargetKind::Solution,
                    test_index: Some(spec.index),
                    test_name: Some(spec.name.clone()),
                    summary: sum,
                    stack_summary: stack,
                    raw_stderr: truncate_stderr(&outcome.stderr),
                });
            } else if outcome.verdict == Verdict::RE {
                findings.push(SanitizerFinding {
                    target: sol_target.clone(),
                    kind: SanitizerTargetKind::Solution,
                    test_index: Some(spec.index),
                    test_name: Some(spec.name.clone()),
                    summary: format!(
                        "runtime error (exit={:?}, signal={:?})",
                        outcome.exit_code, outcome.signal
                    ),
                    stack_summary: vec![],
                    raw_stderr: truncate_stderr(&outcome.stderr),
                });
            }
            if outcome.verdict == Verdict::AC {
                ans_paths[idx] = Some(out_path.clone());
                if checker_is_custom && let Some(chk_cr) = checker_bin {
                    let ref_ans = if let TestSource::Sample {
                        manual_ans_rel: Some(ref rel),
                        ..
                    } = spec.source
                    {
                        let p = prob_dir.join(rel);
                        if p.is_file() { p } else { out_path.clone() }
                    } else {
                        out_path.clone()
                    };
                    *run_counts.entry("checker".to_owned()).or_insert(0) += 1;
                    let chk_res = run_checker_with_score_and_limits(
                        &chk_cr.binary_path,
                        in_path,
                        &out_path,
                        &ref_ans,
                        san_tool_limits,
                        cancel.as_ref(),
                    )?;
                    if let Some((sum, stack)) = extract_sanitizer_diagnostic(&chk_res.message) {
                        findings.push(SanitizerFinding {
                            target: "checker".to_owned(),
                            kind: SanitizerTargetKind::Checker,
                            test_index: Some(spec.index),
                            test_name: Some(spec.name.clone()),
                            summary: sum,
                            stack_summary: stack,
                            raw_stderr: truncate_stderr(&chk_res.message),
                        });
                    }
                }
            }
            (outcome.verdict, outcome.cpu_time_ms)
        };

        let completed = done_runs.fetch_add(1, Ordering::Relaxed) + 1;
        emit(ProgressEvent::Running {
            solution: main_sol.name.clone(),
            test_name: spec.name.clone(),
            verdict,
            cpu_time_ms: cpu_ms,
            completed,
            total: planned_runs,
            cell: None,
        });
    }

    // Run remaining selected expected-AC solutions.
    for sol in &selected_solutions {
        if sol.name == main_sol.name {
            continue;
        }
        let sol_target = format!("sol:{}", sol.name);
        let sol_cr = compiled.get(&sol_target).ok_or_else(|| {
            JudgeError::Internal(format!("missing sanitizer binary for {}", sol.name))
        })?;

        for (idx, (spec, maybe_in)) in mat_inputs.iter().enumerate() {
            check_cancel()?;
            let Some(in_path) = maybe_in else {
                done_runs.fetch_add(1, Ordering::Relaxed);
                continue;
            };
            // Skip tests outside subtasks where this solution is expected to be AC (for partial-AC solutions).
            if !solution_should_sanitize_test(sol, spec) {
                done_runs.fetch_add(1, Ordering::Relaxed);
                continue;
            }
            *run_counts.entry(sol_target.clone()).or_insert(0) += 1;
            let tmp_out = tempfile::NamedTempFile::new()
                .map_err(|e| io_err("create temp sanitizer solution output", e))?
                .into_temp_path();

            let (verdict, cpu_ms) = if let Some(inter_cr) = interactor_bin {
                *run_counts.entry("interactor".to_owned()).or_insert(0) += 1;
                let inter_out = run_multi_run_interactive(
                    &inter_cr.binary_path,
                    &sol_cr.binary_path,
                    in_path,
                    &tmp_out,
                    runs,
                    san_limits,
                    cancel.as_ref(),
                )?;
                if let Some((sum, stack)) = extract_sanitizer_diagnostic(&inter_out.stderr) {
                    findings.push(SanitizerFinding {
                        target: sol_target.clone(),
                        kind: SanitizerTargetKind::Solution,
                        test_index: Some(spec.index),
                        test_name: Some(spec.name.clone()),
                        summary: sum,
                        stack_summary: stack,
                        raw_stderr: truncate_stderr(&inter_out.stderr),
                    });
                }
                if let Some((sum, stack)) =
                    extract_sanitizer_diagnostic(&inter_out.transcript.interactor_message)
                {
                    findings.push(SanitizerFinding {
                        target: "interactor".to_owned(),
                        kind: SanitizerTargetKind::Interactor,
                        test_index: Some(spec.index),
                        test_name: Some(spec.name.clone()),
                        summary: sum,
                        stack_summary: stack,
                        raw_stderr: truncate_stderr(&inter_out.transcript.interactor_message),
                    });
                }
                (inter_out.verdict, inter_out.cpu_time_ms)
            } else {
                let outcome = run_multi_run_process(
                    &sol_cr.binary_path,
                    in_path,
                    &tmp_out,
                    runs,
                    san_limits,
                    cancel.as_ref(),
                )?;
                if let Some((sum, stack)) = extract_sanitizer_diagnostic(&outcome.stderr) {
                    findings.push(SanitizerFinding {
                        target: sol_target.clone(),
                        kind: SanitizerTargetKind::Solution,
                        test_index: Some(spec.index),
                        test_name: Some(spec.name.clone()),
                        summary: sum,
                        stack_summary: stack,
                        raw_stderr: truncate_stderr(&outcome.stderr),
                    });
                } else if outcome.verdict == Verdict::RE {
                    findings.push(SanitizerFinding {
                        target: sol_target.clone(),
                        kind: SanitizerTargetKind::Solution,
                        test_index: Some(spec.index),
                        test_name: Some(spec.name.clone()),
                        summary: format!(
                            "runtime error (exit={:?}, signal={:?})",
                            outcome.exit_code, outcome.signal
                        ),
                        stack_summary: vec![],
                        raw_stderr: truncate_stderr(&outcome.stderr),
                    });
                }
                if outcome.verdict == Verdict::AC
                    && checker_is_custom
                    && let (Some(chk_cr), Some(ans_path)) = (checker_bin, ans_paths[idx].as_ref())
                {
                    *run_counts.entry("checker".to_owned()).or_insert(0) += 1;
                    let chk_res = run_checker_with_score_and_limits(
                        &chk_cr.binary_path,
                        in_path,
                        &tmp_out,
                        ans_path,
                        san_tool_limits,
                        cancel.as_ref(),
                    )?;
                    if let Some((sum, stack)) = extract_sanitizer_diagnostic(&chk_res.message) {
                        findings.push(SanitizerFinding {
                            target: "checker".to_owned(),
                            kind: SanitizerTargetKind::Checker,
                            test_index: Some(spec.index),
                            test_name: Some(spec.name.clone()),
                            summary: sum,
                            stack_summary: stack,
                            raw_stderr: truncate_stderr(&chk_res.message),
                        });
                    }
                }
                (outcome.verdict, outcome.cpu_time_ms)
            };

            let completed = done_runs.fetch_add(1, Ordering::Relaxed) + 1;
            emit(ProgressEvent::Running {
                solution: sol.name.clone(),
                test_name: spec.name.clone(),
                verdict,
                cpu_time_ms: cpu_ms,
                completed,
                total: planned_runs,
                cell: None,
            });
        }
    }

    // Build per-target summaries.
    let mut finding_counts: HashMap<String, usize> = HashMap::new();
    for f in &findings {
        *finding_counts.entry(f.target.clone()).or_insert(0) += 1;
        diagnostics.push(format!(
            "[{}] {} ({}): {}",
            f.target,
            f.test_name.as_deref().unwrap_or("global"),
            f.test_index
                .map(|i| format!("#{i}"))
                .unwrap_or_else(|| "-".to_owned()),
            f.summary
        ));
    }

    let mut targets: Vec<SanitizerTargetResult> = target_meta
        .into_iter()
        .map(|(target, (kind, source_path))| {
            let fc = finding_counts.get(&target).copied().unwrap_or(0);
            let runs = run_counts.get(&target).copied().unwrap_or(0);
            SanitizerTargetResult {
                target,
                kind,
                source_path,
                passed: fc == 0,
                checked_runs: runs,
                findings_count: fc,
            }
        })
        .collect();
    targets.sort_by(|a, b| a.target.cmp(&b.target));

    let ok = findings.is_empty();
    let status = if ok {
        SanitizerStatus::Passed
    } else {
        SanitizerStatus::Failed
    };
    let next_steps = if ok {
        Vec::new()
    } else {
        vec![format!(
            "根据 ASan/UBSan 堆栈摘要修复 {} 处未定义行为后，重新运行 `xcpc package build {slug} --sanitize`。",
            findings.len()
        )]
    };

    let report = SanitizerReport {
        problem_id: slug.to_owned(),
        package_hash,
        status,
        ok,
        subset: is_subset,
        checked_at_epoch_secs: now_epoch_secs(),
        compiler: Some(compiler),
        unsupported_reason: None,
        relaxed_time_limit_ms: san_limits.time_limit_ms,
        compile_failures,
        targets,
        findings,
        diagnostics,
        next_steps,
    };

    if !ws.is_read_only() {
        persist_sanitizer_report(ws, slug, &report)?;
    }

    emit(ProgressEvent::Finished {
        ok,
        verified: false,
    });
    Ok(report)
}

/// Runs sanitizer UB checks sequentially across multiple problems (e.g. a full contest).
pub fn sanitize_packages_batch(
    ws: &Workspace,
    slugs: &[String],
    opts: &SanitizeOptions,
    cancel: Option<Arc<AtomicBool>>,
    progress: Option<&(dyn Fn(BatchBuildEvent) + Send + Sync)>,
) -> Result<BatchBuildReport> {
    if ws.is_read_only() {
        return Err(JudgeError::Storage(xcpc_storage::Error::ReadOnly));
    }
    resolve_active_compiler(opts.cxx_override.as_deref())?;

    let mut seen = HashSet::with_capacity(slugs.len());
    let unique_slugs: Vec<String> = slugs
        .iter()
        .filter(|s| seen.insert((*s).as_str()))
        .cloned()
        .collect();

    let total = unique_slugs.len();
    let batch_start = Instant::now();
    let is_cancelled = || cancel.as_ref().is_some_and(|c| c.load(Ordering::Relaxed));
    let emit = |ev: BatchBuildEvent| {
        if let Some(cb) = progress {
            cb(ev);
        }
    };

    let mut items = Vec::with_capacity(total);
    let mut passed = 0usize;
    let mut failed = 0usize;
    let mut skipped = 0usize;
    let mut cancelled_count = 0usize;
    let mut was_cancelled = false;

    for (index, slug) in unique_slugs.iter().enumerate() {
        if is_cancelled() {
            was_cancelled = true;
            for rem_slug in &unique_slugs[index..] {
                cancelled_count += 1;
                items.push(BatchBuildItemResult {
                    problem_id: rem_slug.clone(),
                    status: BatchBuildItemStatus::Cancelled,
                    verified: false,
                    elapsed_ms: 0,
                    code: Some(JudgeError::Cancelled.code().to_owned()),
                    message: Some(JudgeError::Cancelled.human_message()),
                });
            }
            emit(BatchBuildEvent::Cancelled {
                completed: index,
                total,
            });
            break;
        }

        let item_start = Instant::now();
        let problem = match ws.load_problem(slug) {
            Ok(p) => p,
            Err(e) => {
                let je = JudgeError::Storage(e);
                let item = BatchBuildItemResult {
                    problem_id: slug.clone(),
                    status: BatchBuildItemStatus::Error,
                    verified: false,
                    elapsed_ms: item_start.elapsed().as_millis() as u64,
                    code: Some(je.code().to_owned()),
                    message: Some(je.human_message()),
                };
                failed += 1;
                emit(BatchBuildEvent::ProblemFinished {
                    item: item.clone(),
                    index,
                    total,
                });
                items.push(item);
                continue;
            }
        };
        let prob_dir = match ws.resolve(&problem.dir) {
            Ok(d) => d,
            Err(e) => {
                let je = JudgeError::Storage(e);
                let item = BatchBuildItemResult {
                    problem_id: slug.clone(),
                    status: BatchBuildItemStatus::Error,
                    verified: false,
                    elapsed_ms: item_start.elapsed().as_millis() as u64,
                    code: Some(je.code().to_owned()),
                    message: Some(je.human_message()),
                };
                failed += 1;
                emit(BatchBuildEvent::ProblemFinished {
                    item: item.clone(),
                    index,
                    total,
                });
                items.push(item);
                continue;
            }
        };
        if !problem.meta.has_package_in_dir(&prob_dir) {
            let item = BatchBuildItemResult {
                problem_id: slug.clone(),
                status: BatchBuildItemStatus::Skipped,
                verified: false,
                elapsed_ms: item_start.elapsed().as_millis() as u64,
                code: Some("not_a_package".to_owned()),
                message: Some(format!("题目「{slug}」为纯出版题，已跳过 UB 体检。")),
            };
            skipped += 1;
            emit(BatchBuildEvent::ProblemFinished {
                item: item.clone(),
                index,
                total,
            });
            items.push(item);
            continue;
        }

        emit(BatchBuildEvent::ProblemStarted {
            problem_id: slug.clone(),
            index,
            total,
        });

        let wrapped_cb = |ev: ProgressEvent| {
            if let Some(cb) = progress {
                cb(BatchBuildEvent::ProblemProgress {
                    problem_id: slug.clone(),
                    index,
                    total,
                    event: ev,
                });
            }
        };
        let inner_progress: Option<ProgressCallback<'_>> =
            progress.map(|_| &wrapped_cb as ProgressCallback<'_>);

        match sanitize_package(ws, slug, opts, cancel.clone(), inner_progress) {
            Ok(report) => {
                let elapsed_ms = item_start.elapsed().as_millis() as u64;
                let item = match report.status {
                    SanitizerStatus::Passed => {
                        passed += 1;
                        BatchBuildItemResult {
                            problem_id: slug.clone(),
                            status: BatchBuildItemStatus::Passed,
                            verified: false,
                            elapsed_ms,
                            code: None,
                            message: None,
                        }
                    }
                    SanitizerStatus::Unsupported => {
                        skipped += 1;
                        BatchBuildItemResult {
                            problem_id: slug.clone(),
                            status: BatchBuildItemStatus::Skipped,
                            verified: false,
                            elapsed_ms,
                            code: Some("sanitizer_unsupported".to_owned()),
                            message: report.unsupported_reason,
                        }
                    }
                    SanitizerStatus::Failed => {
                        failed += 1;
                        let msg = report
                            .findings
                            .first()
                            .map(|f| format!("[{}] {}", f.target, f.summary))
                            .or_else(|| report.diagnostics.first().cloned())
                            .unwrap_or_else(|| "ASan/UBSan 检测到未定义行为。".to_owned());
                        BatchBuildItemResult {
                            problem_id: slug.clone(),
                            status: BatchBuildItemStatus::Failed,
                            verified: false,
                            elapsed_ms,
                            code: Some("sanitizer_failed".to_owned()),
                            message: Some(msg),
                        }
                    }
                };
                emit(BatchBuildEvent::ProblemFinished {
                    item: item.clone(),
                    index,
                    total,
                });
                items.push(item);
            }
            Err(JudgeError::Cancelled) => {
                was_cancelled = true;
                for rem_slug in &unique_slugs[index..] {
                    cancelled_count += 1;
                    items.push(BatchBuildItemResult {
                        problem_id: rem_slug.clone(),
                        status: BatchBuildItemStatus::Cancelled,
                        verified: false,
                        elapsed_ms: 0,
                        code: Some(JudgeError::Cancelled.code().to_owned()),
                        message: Some(JudgeError::Cancelled.human_message()),
                    });
                }
                emit(BatchBuildEvent::Cancelled {
                    completed: index,
                    total,
                });
                break;
            }
            Err(e) => {
                failed += 1;
                let item = BatchBuildItemResult {
                    problem_id: slug.clone(),
                    status: BatchBuildItemStatus::Error,
                    verified: false,
                    elapsed_ms: item_start.elapsed().as_millis() as u64,
                    code: Some(e.code().to_owned()),
                    message: Some(e.human_message()),
                };
                emit(BatchBuildEvent::ProblemFinished {
                    item: item.clone(),
                    index,
                    total,
                });
                items.push(item);
            }
        }
    }

    let elapsed_ms = batch_start.elapsed().as_millis() as u64;
    emit(BatchBuildEvent::AllFinished {
        passed,
        failed,
        skipped,
        cancelled: cancelled_count,
        total,
        elapsed_ms,
    });

    Ok(BatchBuildReport {
        ok: failed == 0 && !was_cancelled,
        cancelled: was_cancelled,
        total,
        passed,
        failed,
        skipped,
        cancelled_count,
        elapsed_ms,
        items,
    })
}

fn persist_sanitizer_report(ws: &Workspace, slug: &str, report: &SanitizerReport) -> Result<()> {
    let json = serde_json::to_vec_pretty(report)
        .map_err(|e| JudgeError::Internal(format!("serialize sanitizer report: {e}")))?;
    write_atomic(&sanitizer_report_path_for(ws, slug), &json)
}

fn solution_should_sanitize_test(sol: &DiscoveredSolution, test: &ExpandedTest) -> bool {
    if sol.is_main {
        return true;
    }
    if let Some(ref tags) = sol.tags
        && !tags.is_empty()
    {
        return test
            .tag
            .as_ref()
            .is_some_and(|tt| tags.iter().any(|allowed| allowed == tt));
    }
    // If solution is only expected AC on specific subtasks (not overall AC), restrict to those subtasks.
    let overall_ac = sol
        .explicit_expect
        .as_ref()
        .is_some_and(|e| e.matches("AC"))
        || (sol.explicit_expect.is_none() && sol.expect.matches("AC"));
    if !overall_ac && !sol.subtasks.is_empty() {
        return test
            .subtask
            .as_ref()
            .and_then(|st| sol.subtasks.get(st))
            .is_some_and(|e| e.matches("AC"));
    }
    true
}

fn matches_test_filter(t: &ExpandedTest, filters: &[String]) -> bool {
    for raw in filters {
        let f = raw.trim();
        if f.is_empty() {
            continue;
        }
        if f.parse::<usize>().ok() == Some(t.index) {
            return true;
        }
        if t.name == f {
            return true;
        }
        if t.tag.as_deref() == Some(f) {
            return true;
        }
        if t.subtask.as_deref() == Some(f) {
            return true;
        }
    }
    false
}

fn truncate_stderr(s: &str) -> String {
    const MAX_BYTES: usize = 4096;
    if s.len() <= MAX_BYTES {
        s.trim().to_owned()
    } else {
        let end = s.floor_char_boundary(MAX_BYTES);
        format!("{}...\n(truncated)", s[..end].trim())
    }
}

fn now_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

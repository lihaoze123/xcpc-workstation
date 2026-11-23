//! Full package build & strict verification pipeline (`build_package`).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use xcpc_storage::Workspace;

use crate::assets::{resolve_testlib, standard_checker_source};
use crate::cache::{JudgeCache, write_atomic};
use crate::error::{JudgeError, Result, io_err};
use crate::package::{
    CheckerSpec, DiscoveredSolution, ExpandedTest, TestSource, build_dir_for, check_dirty_inputs,
    compute_package_hash, discover_solutions, expand_tests, find_generator_source,
    find_validator_source, report_path_for, resolve_checker_spec, verified_report_path_for,
};
use crate::report::{
    BuildMode, BuildReport, GateResult, GateStatus, MatrixCell, ProgressEvent, SafetyMargin,
    SelfTestResult, SolutionSummary, TestSummary,
};
use crate::runner::{RunLimits, RunOutcome, Verdict, run_checker, run_process, run_validator};
use crate::toolchain::{
    CompileFailure, CompileResult, CppStd, SourceLanguage, compile_cached, resolve_active_compiler,
};

/// Options controlling `build_package`.
#[derive(Debug, Clone, Default)]
pub struct BuildOptions {
    pub mode: BuildMode,
    /// Optional subset of tests (1-based index, test name, or tag). Must be empty in `Verify` mode.
    pub tests: Vec<String>,
    /// Optional subset of solution names. Must be empty in `Verify` mode.
    pub solutions: Vec<String>,
    /// Parallelism for compile, generator, validator, checker (default: CPU count).
    pub jobs: Option<usize>,
    /// Parallelism for running solutions (default: 1 = serial to avoid cache/turbo contention).
    pub solution_jobs: Option<usize>,
    /// Optional explicit compiler path override.
    pub cxx_override: Option<PathBuf>,
    /// Optional explicit cache directory override (default: `~/.cache/xcpc`).
    pub cache_dir: Option<PathBuf>,
}

/// Optional progress callback invoked during `build_package`.
pub type ProgressCallback<'a> = &'a (dyn Fn(ProgressEvent) + Send + Sync);

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedSolutionRun {
    verdict: Verdict,
    cpu_time_ms: u64,
    wall_time_ms: u64,
    peak_memory_kib: u64,
    checker_message: String,
    stderr: String,
}

#[derive(Debug, Clone)]
struct CompileTarget {
    id: String,
    display_path: String,
    language: SourceLanguage,
    source_bytes: Vec<u8>,
    disk_path: Option<PathBuf>,
}

/// Builds and/or strictly verifies a problem package.
///
/// Writes `build/problems/<slug>/report.json` atomically. When `mode == Verify` and all hard
/// gates pass, also writes `build/problems/<slug>/verified_report.json` so subsequent `Debug`
/// or subset runs never overwrite the verified acceptance state.
pub fn build_package(
    ws: &Workspace,
    slug: &str,
    opts: &BuildOptions,
    cancel: Option<Arc<AtomicBool>>,
    progress: Option<ProgressCallback<'_>>,
) -> Result<BuildReport> {
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

    let is_subset = !opts.tests.is_empty() || !opts.solutions.is_empty();
    if opts.mode == BuildMode::Verify && is_subset {
        return Err(JudgeError::InvalidConfig(
            "verify mode requires full tests and full solutions (remove --tests / --solutions)"
                .to_owned(),
        ));
    }

    let problem = ws.load_problem(slug)?;
    let prob_dir = ws.resolve(&problem.dir)?;
    let mut package_hash = compute_package_hash(ws, slug)?;
    let compiler = resolve_active_compiler(opts.cxx_override.as_deref())?;
    let cache = match &opts.cache_dir {
        Some(dir) => JudgeCache::open(dir)?,
        None => JudgeCache::open_default()?,
    };
    let testlib = resolve_testlib(ws.root(), &prob_dir);

    let limits = RunLimits::from_problem_limits(
        problem.meta.limits.time.as_deref(),
        problem.meta.limits.memory.as_deref(),
    )?;

    let checker_spec = resolve_checker_spec(&prob_dir, &problem.meta)?;
    let validator_src = find_validator_source(&prob_dir);
    let all_tests = expand_tests(&prob_dir, &problem.meta)?;
    let all_solutions = discover_solutions(&prob_dir, &problem.meta)?;

    // Validate main solution: must have exactly one `main = true`.
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

    // Apply subset filters.
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

    let selected_solutions: Vec<DiscoveredSolution> = if opts.solutions.is_empty() {
        all_solutions.clone()
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

    // Dirty input check on hand-written files.
    let dirty_inputs = check_dirty_inputs(&prob_dir);

    // Parallel thread pool for compile / self-test / generate / validate / check.
    let parallel_jobs = opts.jobs.filter(|&n| n > 0).unwrap_or_else(|| {
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4)
    });
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(parallel_jobs)
        .build()
        .map_err(|e| JudgeError::Internal(format!("create thread pool: {e}")))?;

    // Collect compilation targets.
    let mut compile_targets: Vec<CompileTarget> = Vec::new();
    if let Some((ref p, ref rel, lang)) = validator_src {
        let bytes = fs::read(p).map_err(|e| io_err(format!("read {}", p.display()), e))?;
        compile_targets.push(CompileTarget {
            id: "validator".to_owned(),
            display_path: rel.clone(),
            language: lang,
            source_bytes: bytes,
            disk_path: Some(p.clone()),
        });
    }

    match &checker_spec {
        CheckerSpec::Builtin(name) => {
            let src = standard_checker_source(name)
                .ok_or_else(|| JudgeError::UnknownChecker(name.clone()))?;
            compile_targets.push(CompileTarget {
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
            compile_targets.push(CompileTarget {
                id: "checker".to_owned(),
                display_path: display_name.clone(),
                language: lang,
                source_bytes: bytes,
                disk_path: Some(path.clone()),
            });
        }
    }

    // Unique generators needed for selected tests.
    let mut seen_gens = HashSet::new();
    for t in &selected_tests {
        if let TestSource::Generated { ref gen_name, .. } = t.source
            && seen_gens.insert(gen_name.clone())
        {
            let (p, rel, lang) = find_generator_source(&prob_dir, gen_name)?;
            let bytes = fs::read(&p).map_err(|e| io_err(format!("read {}", p.display()), e))?;
            compile_targets.push(CompileTarget {
                id: format!("gen:{gen_name}"),
                display_path: rel,
                language: lang,
                source_bytes: bytes,
                disk_path: Some(p),
            });
        }
    }

    // Solutions to compile: always include main_sol (needed to produce .ans) plus selected solutions.
    let mut sol_ids_seen = HashSet::new();
    for s in std::iter::once(&main_sol).chain(selected_solutions.iter()) {
        if sol_ids_seen.insert(s.name.clone()) {
            let bytes = fs::read(&s.source_path)
                .map_err(|e| io_err(format!("read {}", s.source_path.display()), e))?;
            compile_targets.push(CompileTarget {
                id: format!("sol:{}", s.name),
                display_path: s.source_rel.clone(),
                language: s.language,
                source_bytes: bytes,
                disk_path: Some(s.source_path.clone()),
            });
        }
    }

    check_cancel()?;

    // Phase 2: Parallel compilation.
    let total_compile = compile_targets.len();
    let done_compile = AtomicUsize::new(0);
    let compile_results: Vec<(String, std::result::Result<CompileResult, CompileFailure>)> = pool
        .install(|| {
            compile_targets
                .par_iter()
                .map(|t| {
                    let res = compile_cached(
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
        let mut diagnostics = Vec::new();
        for f in &compile_failures {
            diagnostics.push(format!("compile error in {} ({})", f.target, f.source_path));
        }
        let gates = vec![GateResult {
            id: "compile".to_owned(),
            title: "编译通过".to_owned(),
            status: GateStatus::Fail,
            message: format!("{} 个目标编译失败", compile_failures.len()),
        }];
        let report = BuildReport {
            problem_id: slug.to_owned(),
            package_hash,
            mode: opts.mode,
            verified: false,
            ok: false,
            subset: is_subset,
            built_at_epoch_secs: now_epoch_secs(),
            compiler: Some(compiler),
            checker: checker_spec.display_name().to_owned(),
            compile_failures,
            dirty_inputs,
            self_tests: vec![],
            tests: vec![],
            solutions: vec![],
            matrix: vec![],
            gates,
            safety_margin: SafetyMargin {
                time_limit_ms: limits.time_limit_ms,
                memory_limit_kib: limits.memory_limit_kib,
                ..Default::default()
            },
            diagnostics,
            repro_commands: vec![],
            next_steps: vec!["修复编译错误后重新运行构建。".to_owned()],
        };
        persist_report(ws, slug, &report)?;
        emit(ProgressEvent::Finished {
            ok: false,
            verified: false,
        });
        return Ok(report);
    }

    check_cancel()?;

    // Phase 3: Self-tests (validator + custom checker) in parallel.
    let validator_bin = compiled.get("validator");
    let checker_bin = compiled
        .get("checker")
        .ok_or_else(|| JudgeError::Internal("checker binary missing after compile".to_owned()))?;

    let self_tests = run_self_tests(
        &pool,
        &prob_dir,
        validator_bin.map(|c| c.binary_path.as_path()),
        &checker_spec,
        checker_bin.binary_path.as_path(),
        cancel.as_ref(),
        &emit,
    )?;

    check_cancel()?;

    // Phase 4: Materialize test inputs into `build/problems/<slug>/tests/` in parallel.
    let build_tests_dir = build_dir_for(ws, slug).join("tests");
    fs::create_dir_all(&build_tests_dir).map_err(|e| {
        io_err(
            format!("create build tests dir {}", build_tests_dir.display()),
            e,
        )
    })?;

    let total_gen = selected_tests.len();
    let done_gen = AtomicUsize::new(0);
    let materialized: Vec<Result<MaterializedTest>> = pool.install(|| {
        selected_tests
            .par_iter()
            .map(|t| {
                let res = materialize_test(
                    &prob_dir,
                    &build_tests_dir,
                    t,
                    &compiled,
                    &cache,
                    cancel.as_ref(),
                );
                let completed = done_gen.fetch_add(1, Ordering::Relaxed) + 1;
                emit(ProgressEvent::Generating {
                    test_name: t.name.clone(),
                    completed,
                    total: total_gen,
                });
                res
            })
            .collect()
    });

    let mut mat_tests: Vec<MaterializedTest> = Vec::with_capacity(materialized.len());
    for item in materialized {
        mat_tests.push(item?);
    }

    check_cancel()?;

    // Phase 5: Validate all materialized test inputs in parallel (if validator exists).
    let val_results: Vec<Result<Option<std::result::Result<(), String>>>> = if let Some(val_cr) =
        validator_bin
    {
        let total_val = mat_tests.len();
        let done_val = AtomicUsize::new(0);
        pool.install(|| {
            mat_tests
                .par_iter()
                .map(|mt| {
                    let r = run_validator(&val_cr.binary_path, &mt.input_path, cancel.as_ref())?;
                    let completed = done_val.fetch_add(1, Ordering::Relaxed) + 1;
                    emit(ProgressEvent::Validating {
                        test_name: mt.spec.name.clone(),
                        completed,
                        total: total_val,
                    });
                    Ok(Some(r))
                })
                .collect()
        })
    } else {
        (0..mat_tests.len()).map(|_| Ok(None)).collect()
    };

    let mut test_summaries = Vec::with_capacity(mat_tests.len());
    let mut diagnostics = Vec::new();
    let mut repro_commands = Vec::new();
    let mut all_validator_ok = true;

    for (mt, vr) in mat_tests.iter().zip(val_results) {
        let vr = vr?;
        let (val_ok, val_msg) = match vr {
            None => (None, None),
            Some(Ok(())) => (Some(true), None),
            Some(Err(msg)) => {
                all_validator_ok = false;
                diagnostics.push(format!(
                    "validator failed on test #{} ({}): {msg}",
                    mt.spec.index, mt.spec.name
                ));
                repro_commands.push(format!(
                    "xcpc package build {slug} --tests {}",
                    mt.spec.index
                ));
                (Some(false), Some(msg))
            }
        };
        test_summaries.push(TestSummary {
            index: mt.spec.index,
            name: mt.spec.name.clone(),
            tag: mt.spec.tag.clone(),
            is_sample: mt.spec.is_sample,
            size_bytes: mt.size_bytes,
            gen_cmd: mt.spec.gen_cmd.clone(),
            validator_ok: val_ok,
            validator_message: val_msg,
        });
    }

    check_cancel()?;

    // Phase 6: Run main solution (`std`) to generate `.ans` and validate manual samples.
    let sol_jobs = opts.solution_jobs.unwrap_or(1).max(1);
    let main_cr = compiled
        .get(&format!("sol:{}", main_sol.name))
        .ok_or_else(|| JudgeError::Internal("main solution binary missing".to_owned()))?;

    // Total solution-test runs for progress reporting.
    let mut planned_runs = mat_tests.len(); // main_sol always runs on all selected tests
    for sol in &selected_solutions {
        if sol.name == main_sol.name {
            continue;
        }
        planned_runs += mat_tests
            .iter()
            .filter(|mt| solution_applies_to_test(sol, &mt.spec))
            .count();
    }
    let done_runs = AtomicUsize::new(0);

    let mut ans_paths: Vec<Option<(PathBuf, String)>> = vec![None; mat_tests.len()];
    let mut matrix_cells: Vec<MatrixCell> = Vec::new();
    let include_main_in_report = selected_solutions.iter().any(|s| s.name == main_sol.name);

    // Track existing sample input hashes and next sample number for idempotent `sample = true` write-back.
    let mut existing_sample_in_hashes = collect_existing_sample_input_hashes(&prob_dir);
    let mut next_sample_num = next_available_sample_number(&prob_dir);
    let mut wrote_back_sample = false;

    for (idx, mt) in mat_tests.iter().enumerate() {
        check_cancel()?;
        let out_path = build_tests_dir.join(format!("{:03}.ans", mt.spec.index));
        let (outcome, cache_hit) = run_solution_cached_with_output(
            &cache,
            main_cr,
            &mt.input_path,
            &mt.input_hash,
            &out_path,
            limits,
            cancel.as_ref(),
        )?;

        let mut cell_verdict = outcome.verdict;
        let mut checker_msg = String::new();

        if cell_verdict == Verdict::AC {
            // If this is a sample with an existing manual `.ans`, NEVER overwrite it;
            // verify BOTH std output against manual .ans AND manual .ans against std output.
            if let TestSource::Sample {
                manual_ans_rel: Some(ref ans_rel),
                ..
            } = mt.spec.source
            {
                let manual_ans_path = prob_dir.join(ans_rel);
                let (v1, m1) = run_checker(
                    &checker_bin.binary_path,
                    &mt.input_path,
                    &out_path,
                    &manual_ans_path,
                    cancel.as_ref(),
                )?;
                let (v2, m2) = if v1 == Verdict::AC {
                    run_checker(
                        &checker_bin.binary_path,
                        &mt.input_path,
                        &manual_ans_path,
                        &out_path,
                        cancel.as_ref(),
                    )?
                } else {
                    (v1, m1.clone())
                };
                if v1 != Verdict::AC {
                    cell_verdict = v1;
                    checker_msg = format!("std vs manual {ans_rel}: {m1}");
                } else if v2 != Verdict::AC {
                    cell_verdict = v2;
                    checker_msg = format!("manual {ans_rel} vs std: {m2}");
                } else {
                    checker_msg = m1;
                }
                let ans_bytes = fs::read(&manual_ans_path)
                    .map_err(|e| io_err(format!("read {}", manual_ans_path.display()), e))?;
                ans_paths[idx] = Some((manual_ans_path, xcpc_storage::content_hash(&ans_bytes)));
            } else {
                let ans_bytes = fs::read(&out_path)
                    .map_err(|e| io_err(format!("read {}", out_path.display()), e))?;
                ans_paths[idx] = Some((out_path.clone(), xcpc_storage::content_hash(&ans_bytes)));

                // Write back to `samples/` ONLY when `[[test]]` explicitly has `sample = true`.
                // Idempotent: skip if an existing `samples/*.in` already has identical bytes,
                // and never overwrite an existing manual `.ans`.
                if mt.spec.write_back_sample
                    && !ws.is_read_only()
                    && !existing_sample_in_hashes.contains(&mt.input_hash)
                {
                    let samples_dir = prob_dir.join("samples");
                    let stem = loop {
                        let candidate = format!("{next_sample_num:02}");
                        next_sample_num += 1;
                        if !samples_dir.join(format!("{candidate}.in")).exists()
                            && !samples_dir.join(format!("{candidate}.ans")).exists()
                            && !samples_dir.join(format!("{candidate}.out")).exists()
                        {
                            break candidate;
                        }
                    };
                    let in_bytes = fs::read(&mt.input_path)
                        .map_err(|e| io_err(format!("read {}", mt.input_path.display()), e))?;
                    let in_rel = format!("{}/samples/{stem}.in", problem.dir);
                    let ans_rel = format!("{}/samples/{stem}.ans", problem.dir);
                    ws.write(&in_rel, &in_bytes)?;
                    ws.write(&ans_rel, &ans_bytes)?;
                    existing_sample_in_hashes.insert(mt.input_hash.clone());
                    wrote_back_sample = true;
                }
            }
        }

        let completed = done_runs.fetch_add(1, Ordering::Relaxed) + 1;
        emit(ProgressEvent::Running {
            solution: main_sol.name.clone(),
            test_name: mt.spec.name.clone(),
            verdict: cell_verdict,
            cpu_time_ms: outcome.cpu_time_ms,
            completed,
            total: planned_runs,
        });

        let repro = (!cell_verdict.is_ok()).then(|| {
            format!(
                "xcpc package build {slug} --tests {} --solutions {}",
                mt.spec.index, main_sol.name
            )
        });
        if let Some(ref r) = repro {
            diagnostics.push(format!(
                "main solution `{}` got {} on test #{} ({}): {}",
                main_sol.name, cell_verdict, mt.spec.index, mt.spec.name, checker_msg
            ));
            repro_commands.push(r.clone());
        }

        if include_main_in_report {
            matrix_cells.push(MatrixCell {
                test_index: mt.spec.index,
                test_name: mt.spec.name.clone(),
                solution: main_sol.name.clone(),
                verdict: cell_verdict,
                cpu_time_ms: outcome.cpu_time_ms,
                wall_time_ms: outcome.wall_time_ms,
                peak_memory_kib: outcome.peak_memory_kib,
                cache_hit,
                checker_message: checker_msg,
                stderr: outcome.stderr,
                repro_cmd: repro,
            });
        }
    }

    check_cancel()?;

    // Phase 7: Run non-main selected solutions (serial by default unless `solution_jobs > 1`).
    let other_solutions: Vec<&DiscoveredSolution> = selected_solutions
        .iter()
        .filter(|s| s.name != main_sol.name)
        .collect();

    let mut run_tasks: Vec<(&DiscoveredSolution, usize)> = Vec::new();
    for sol in &other_solutions {
        for (idx, mt) in mat_tests.iter().enumerate() {
            if solution_applies_to_test(sol, &mt.spec) {
                run_tasks.push((sol, idx));
            }
        }
    }

    let exec_one = |(sol, idx): &(&DiscoveredSolution, usize)| -> Result<MatrixCell> {
        let mt = &mat_tests[*idx];
        let sol_cr = compiled
            .get(&format!("sol:{}", sol.name))
            .ok_or_else(|| JudgeError::Internal(format!("missing binary for {}", sol.name)))?;
        let Some((ref ans_path, ref ans_hash)) = ans_paths[*idx] else {
            return Ok(MatrixCell {
                test_index: mt.spec.index,
                test_name: mt.spec.name.clone(),
                solution: sol.name.clone(),
                verdict: Verdict::JE,
                cpu_time_ms: 0,
                wall_time_ms: 0,
                peak_memory_kib: 0,
                cache_hit: false,
                checker_message: "reference answer unavailable (main solution failed)".to_owned(),
                stderr: String::new(),
                repro_cmd: Some(format!(
                    "xcpc package build {slug} --tests {} --solutions {}",
                    mt.spec.index, sol.name
                )),
            });
        };

        let cache_key = run_cache_key(
            &sol_cr.binary_hash,
            &mt.input_hash,
            limits,
            &checker_bin.binary_hash,
            ans_hash,
        );

        let (cached, cache_hit) =
            if let Some(hit) = cache.get_run_json::<CachedSolutionRun>(&cache_key) {
                (hit, true)
            } else {
                let tmp_out = tempfile::NamedTempFile::new()
                    .map_err(|e| io_err("create temp solution output", e))?;
                let outcome = run_process(
                    &sol_cr.binary_path,
                    &[],
                    Some(&mt.input_path),
                    Some(tmp_out.path()),
                    limits,
                    cancel.as_ref(),
                )?;
                let (final_verdict, checker_msg) = if outcome.verdict == Verdict::AC {
                    run_checker(
                        &checker_bin.binary_path,
                        &mt.input_path,
                        tmp_out.path(),
                        ans_path,
                        cancel.as_ref(),
                    )?
                } else {
                    (outcome.verdict, String::new())
                };
                let entry = CachedSolutionRun {
                    verdict: final_verdict,
                    cpu_time_ms: outcome.cpu_time_ms,
                    wall_time_ms: outcome.wall_time_ms,
                    peak_memory_kib: outcome.peak_memory_kib,
                    checker_message: checker_msg,
                    stderr: outcome.stderr,
                };
                let _ = cache.put_run_json(&cache_key, &entry);
                (entry, false)
            };

        let completed = done_runs.fetch_add(1, Ordering::Relaxed) + 1;
        emit(ProgressEvent::Running {
            solution: sol.name.clone(),
            test_name: mt.spec.name.clone(),
            verdict: cached.verdict,
            cpu_time_ms: cached.cpu_time_ms,
            completed,
            total: planned_runs,
        });

        Ok(MatrixCell {
            test_index: mt.spec.index,
            test_name: mt.spec.name.clone(),
            solution: sol.name.clone(),
            verdict: cached.verdict,
            cpu_time_ms: cached.cpu_time_ms,
            wall_time_ms: cached.wall_time_ms,
            peak_memory_kib: cached.peak_memory_kib,
            cache_hit,
            checker_message: cached.checker_message,
            stderr: cached.stderr,
            repro_cmd: None,
        })
    };

    let other_cells: Vec<Result<MatrixCell>> = if sol_jobs == 1 {
        run_tasks
            .iter()
            .map(|task| {
                check_cancel()?;
                exec_one(task)
            })
            .collect()
    } else {
        let sol_pool = rayon::ThreadPoolBuilder::new()
            .num_threads(sol_jobs)
            .build()
            .map_err(|e| JudgeError::Internal(format!("create solution thread pool: {e}")))?;
        sol_pool.install(|| run_tasks.par_iter().map(exec_one).collect())
    };

    for cell_res in other_cells {
        matrix_cells.push(cell_res?);
    }

    // Aggregate per-solution summaries.
    let mut solution_summaries = Vec::new();
    let mut all_solutions_matched = true;

    for sol in &selected_solutions {
        let cells: Vec<&MatrixCell> = matrix_cells
            .iter()
            .filter(|c| c.solution == sol.name)
            .collect();
        let total_tests = cells.len();
        let passed_tests = cells.iter().filter(|c| c.verdict == Verdict::AC).count();
        let max_time_ms = cells.iter().map(|c| c.cpu_time_ms).max().unwrap_or(0);
        let max_memory_kib = cells.iter().map(|c| c.peak_memory_kib).max().unwrap_or(0);
        let actual_verdict = cells
            .iter()
            .find(|c| c.verdict != Verdict::AC)
            .map(|c| c.verdict)
            .unwrap_or(Verdict::AC);
        let matched = sol.expect.matches(actual_verdict.as_str());
        if !matched {
            all_solutions_matched = false;
            let first_bad_test = cells
                .iter()
                .find(|c| c.verdict != Verdict::AC)
                .map(|c| c.test_index)
                .or_else(|| cells.first().map(|c| c.test_index));
            let cmd = match first_bad_test {
                Some(ti) => {
                    format!(
                        "xcpc package build {slug} --tests {ti} --solutions {}",
                        sol.name
                    )
                }
                None => format!("xcpc package build {slug} --solutions {}", sol.name),
            };
            diagnostics.push(format!(
                "solution `{}` expected {:?}, got {}",
                sol.name,
                sol.expect.normalized(),
                actual_verdict
            ));
            repro_commands.push(cmd.clone());
            // Attach repro_cmd to the offending cell(s).
            for c in matrix_cells.iter_mut().filter(|c| c.solution == sol.name) {
                if c.repro_cmd.is_none() && Some(c.test_index) == first_bad_test {
                    c.repro_cmd = Some(cmd.clone());
                }
            }
        }
        solution_summaries.push(SolutionSummary {
            name: sol.name.clone(),
            is_main: sol.is_main,
            expected: sol.expect.normalized(),
            actual_verdict,
            matched,
            max_time_ms,
            max_memory_kib,
            passed_tests,
            total_tests,
            tags: sol.tags.clone(),
        });
    }

    // Compute safety margins.
    let safety_margin =
        compute_safety_margin(&mat_tests, &solution_summaries, &main_sol.name, limits);

    // Phase 8: Evaluate two-tier gates (`Debug` vs `Verify`).
    let mut gates = Vec::new();
    let mut next_steps = Vec::new();

    // 1. Compile gate (already passed if we got here).
    gates.push(GateResult {
        id: "compile".to_owned(),
        title: "编译通过".to_owned(),
        status: GateStatus::Pass,
        message: format!("{} 个目标全部编译通过", compiled.len()),
    });

    // 2. Validator exists + self-tests.
    let val_valid_tests: Vec<&SelfTestResult> = self_tests
        .iter()
        .filter(|r| r.kind == "validator_valid")
        .collect();
    let val_invalid_tests: Vec<&SelfTestResult> = self_tests
        .iter()
        .filter(|r| r.kind == "validator_invalid")
        .collect();
    let val_self_all_pass =
        val_valid_tests.iter().all(|r| r.passed) && val_invalid_tests.iter().all(|r| r.passed);

    if validator_src.is_none() {
        let status = if opts.mode == BuildMode::Verify {
            GateStatus::Fail
        } else {
            GateStatus::Warn
        };
        gates.push(GateResult {
            id: "validator".to_owned(),
            title: "Validator 存在与自测".to_owned(),
            status,
            message: "缺少 validator.cpp".to_owned(),
        });
        next_steps.push(
            "编写 validator.cpp 并添加 tests/validator/{valid,invalid}/ 自测用例。".to_owned(),
        );
    } else if !val_self_all_pass {
        gates.push(GateResult {
            id: "validator".to_owned(),
            title: "Validator 存在与自测".to_owned(),
            status: GateStatus::Fail,
            message: "部分 validator 自测用例未通过".to_owned(),
        });
        next_steps.push("修复失败的 validator 或 tests/validator/ 自测用例。".to_owned());
    } else if val_valid_tests.is_empty() || val_invalid_tests.is_empty() {
        let status = if opts.mode == BuildMode::Verify {
            GateStatus::Fail
        } else {
            GateStatus::Warn
        };
        gates.push(GateResult {
            id: "validator".to_owned(),
            title: "Validator 存在与自测".to_owned(),
            status,
            message: format!(
                "validator 自测不完整（valid: {}, invalid: {}，严格验收要求各至少 1 组）",
                val_valid_tests.len(),
                val_invalid_tests.len()
            ),
        });
        next_steps
            .push("在 tests/validator/valid/ 与 invalid/ 各添加至少 1 组自测用例。".to_owned());
    } else {
        gates.push(GateResult {
            id: "validator".to_owned(),
            title: "Validator 存在与自测".to_owned(),
            status: GateStatus::Pass,
            message: format!(
                "valid {} 组、invalid {} 组全部通过",
                val_valid_tests.len(),
                val_invalid_tests.len()
            ),
        });
    }

    // 3. Custom checker self-tests (when using custom checker.cpp).
    if checker_spec.is_custom() {
        let chk_ok: Vec<&SelfTestResult> = self_tests
            .iter()
            .filter(|r| r.kind == "checker_ok")
            .collect();
        let chk_wa: Vec<&SelfTestResult> = self_tests
            .iter()
            .filter(|r| r.kind == "checker_wa")
            .collect();
        let chk_pe: Vec<&SelfTestResult> = self_tests
            .iter()
            .filter(|r| r.kind == "checker_pe")
            .collect();
        let chk_all_pass = chk_ok.iter().all(|r| r.passed)
            && chk_wa.iter().all(|r| r.passed)
            && chk_pe.iter().all(|r| r.passed);
        if !chk_all_pass {
            gates.push(GateResult {
                id: "checker_self_tests".to_owned(),
                title: "自定义 Checker 自测".to_owned(),
                status: GateStatus::Fail,
                message: "部分自定义 checker 自测未通过".to_owned(),
            });
            next_steps.push("修复自定义 checker.cpp 或 tests/checker/ 自测用例。".to_owned());
        } else if chk_ok.is_empty() || chk_wa.is_empty() {
            let status = if opts.mode == BuildMode::Verify {
                GateStatus::Fail
            } else {
                GateStatus::Warn
            };
            gates.push(GateResult {
                id: "checker_self_tests".to_owned(),
                title: "自定义 Checker 自测".to_owned(),
                status,
                message: format!(
                    "自定义 checker 自测不完整（ok: {}, wa: {}，严格验收要求各至少 1 组）",
                    chk_ok.len(),
                    chk_wa.len()
                ),
            });
            next_steps.push("在 tests/checker/ 添加至少 1 组 ok-* 与 1 组 wa-* 自测。".to_owned());
        } else {
            gates.push(GateResult {
                id: "checker_self_tests".to_owned(),
                title: "自定义 Checker 自测".to_owned(),
                status: GateStatus::Pass,
                message: format!("ok {} 组、wa {} 组全部通过", chk_ok.len(), chk_wa.len()),
            });
        }
    } else {
        gates.push(GateResult {
            id: "checker_self_tests".to_owned(),
            title: "内置 Checker".to_owned(),
            status: GateStatus::Skip,
            message: format!("使用内置标准校验器 {}", checker_spec.display_name()),
        });
    }

    // 4. Dirty inputs check (warning).
    if dirty_inputs.is_empty() {
        gates.push(GateResult {
            id: "dirty_inputs".to_owned(),
            title: "手写数据格式检查".to_owned(),
            status: GateStatus::Pass,
            message: "未发现 CRLF / BOM / 缺尾换行".to_owned(),
        });
    } else {
        gates.push(GateResult {
            id: "dirty_inputs".to_owned(),
            title: "手写数据格式检查".to_owned(),
            status: GateStatus::Warn,
            message: format!(
                "{} 个手写输入文件存在格式问题（CRLF/BOM/缺尾换行）",
                dirty_inputs.len()
            ),
        });
        next_steps.push("清理手写数据文件中的 CRLF、UTF-8 BOM 或补全末尾换行符。".to_owned());
    }

    // 5. All tests pass validator.
    if validator_src.is_some() {
        gates.push(GateResult {
            id: "validator_tests".to_owned(),
            title: "全量测试数据校验".to_owned(),
            status: if all_validator_ok {
                GateStatus::Pass
            } else {
                GateStatus::Fail
            },
            message: if all_validator_ok {
                format!("{} 组测试数据全部通过 validator", mat_tests.len())
            } else {
                "存在未通过 validator 的测试数据".to_owned()
            },
        });
    }

    // 6. All solutions match expect.
    gates.push(GateResult {
        id: "solutions_expect".to_owned(),
        title: "解法判定符合预期".to_owned(),
        status: if all_solutions_matched {
            GateStatus::Pass
        } else {
            GateStatus::Fail
        },
        message: if all_solutions_matched {
            format!("{} 个解法全部符合 expect", solution_summaries.len())
        } else {
            "存在判定与 expect 不符的解法".to_owned()
        },
    });

    // 7. Std slowest time <= 50% of time limit.
    let std_within_margin = safety_margin.std_time_ratio <= 0.50;
    let margin_pct = (safety_margin.std_time_ratio * 100.0).round();
    gates.push(GateResult {
        id: "std_time_margin".to_owned(),
        title: "标程耗时余量（≤ 50% 时限）".to_owned(),
        status: if std_within_margin {
            GateStatus::Pass
        } else if opts.mode == BuildMode::Verify {
            GateStatus::Fail
        } else {
            GateStatus::Warn
        },
        message: format!(
            "标程最慢 {}ms / {}ms（{margin_pct:.0}%）",
            safety_margin.std_max_time_ms, safety_margin.time_limit_ms
        ),
    });
    if !std_within_margin {
        next_steps.push("优化标程或评估时限设置，使标程最慢耗时不超过时限的 50%。".to_owned());
    }

    // 8. Cross-check & wrong solutions warning (prominent warning, never hard fail).
    let has_other_ac = all_solutions
        .iter()
        .any(|s| !s.is_main && s.expect.matches("AC"));
    let has_wrong_sol = all_solutions
        .iter()
        .any(|s| !s.is_main && !s.expect.matches("AC"));
    if !has_other_ac || !has_wrong_sol {
        let mut missing = Vec::new();
        if !has_other_ac {
            missing.push("独立参考解（交叉验证）");
        }
        if !has_wrong_sol {
            missing.push("错解（如 WA/TLE 用来检验数据强度）");
        }
        gates.push(GateResult {
            id: "solution_diversity".to_owned(),
            title: "解法覆盖度（交叉验证与错解）".to_owned(),
            status: GateStatus::Warn,
            message: format!("建议补充：{}", missing.join("、")),
        });
    } else {
        gates.push(GateResult {
            id: "solution_diversity".to_owned(),
            title: "解法覆盖度（交叉验证与错解）".to_owned(),
            status: GateStatus::Pass,
            message: "已包含独立参考解与错解".to_owned(),
        });
    }

    let ok = !gates.iter().any(|g| g.status == GateStatus::Fail);
    let verified = opts.mode == BuildMode::Verify && !is_subset && ok;

    if opts.mode == BuildMode::Debug && ok && !is_subset {
        next_steps.push(format!(
            "运行 `xcpc package build {slug} --strict` 完成严格验收。"
        ));
    }

    // Deduplicate repro_commands preserving order.
    let mut seen_repro = HashSet::new();
    repro_commands.retain(|c| seen_repro.insert(c.clone()));

    // If `sample = true` wrote new files into `samples/`, recompute `package_hash` so the
    // persisted report matches the post-writeback package contents on disk.
    if wrote_back_sample {
        package_hash = compute_package_hash(ws, slug)?;
    }

    let report = BuildReport {
        problem_id: slug.to_owned(),
        package_hash,
        mode: opts.mode,
        verified,
        ok,
        subset: is_subset,
        built_at_epoch_secs: now_epoch_secs(),
        compiler: Some(compiler),
        checker: checker_spec.display_name().to_owned(),
        compile_failures,
        dirty_inputs,
        self_tests,
        tests: test_summaries,
        solutions: solution_summaries,
        matrix: matrix_cells,
        gates,
        safety_margin,
        diagnostics,
        repro_commands,
        next_steps,
    };

    persist_report(ws, slug, &report)?;
    emit(ProgressEvent::Finished { ok, verified });
    Ok(report)
}

fn persist_report(ws: &Workspace, slug: &str, report: &BuildReport) -> Result<()> {
    let json = serde_json::to_vec_pretty(report)
        .map_err(|e| JudgeError::Internal(format!("serialize build report: {e}")))?;
    write_atomic(&report_path_for(ws, slug), &json)?;

    // Preserve verified state separately so Debug / subset runs NEVER overwrite it.
    let verified_path = verified_report_path_for(ws, slug);
    if report.verified {
        write_atomic(&verified_path, &json)?;
    } else if report.mode == BuildMode::Verify && !report.subset && verified_path.is_file() {
        // If a strict Verify run failed for the exact same package hash, invalidate the old
        // verified marker for that hash.
        if let Ok(bytes) = fs::read(&verified_path)
            && let Ok(prev) = serde_json::from_slice::<BuildReport>(&bytes)
            && prev.package_hash == report.package_hash
        {
            let _ = fs::remove_file(&verified_path);
        }
    }
    Ok(())
}

struct MaterializedTest {
    spec: ExpandedTest,
    input_path: PathBuf,
    input_hash: String,
    size_bytes: u64,
}

fn materialize_test(
    prob_dir: &Path,
    build_tests_dir: &Path,
    spec: &ExpandedTest,
    compiled: &HashMap<String, CompileResult>,
    cache: &JudgeCache,
    cancel: Option<&Arc<AtomicBool>>,
) -> Result<MaterializedTest> {
    let dest = build_tests_dir.join(format!("{:03}.in", spec.index));
    let bytes = match &spec.source {
        TestSource::Sample { input_rel, .. } => {
            let src = prob_dir.join(input_rel);
            fs::read(&src).map_err(|e| io_err(format!("read {}", src.display()), e))?
        }
        TestSource::File { file_rel } => {
            let src = prob_dir.join(file_rel);
            fs::read(&src).map_err(|e| io_err(format!("read {}", src.display()), e))?
        }
        TestSource::Generated {
            gen_name,
            args,
            args_raw,
        } => {
            let gen_cr = compiled
                .get(&format!("gen:{gen_name}"))
                .ok_or_else(|| JudgeError::GeneratorNotFound(gen_name.clone()))?;
            let gen_key = gen_cache_key(&gen_cr.binary_hash, args_raw);
            let cached_path = cache.gen_input_path(&gen_key);
            if cached_path.is_file() {
                fs::read(&cached_path)
                    .map_err(|e| io_err(format!("read cache {}", cached_path.display()), e))?
            } else {
                let tmp = tempfile::NamedTempFile::new()
                    .map_err(|e| io_err("create temp gen output", e))?;
                let outcome = run_process(
                    &gen_cr.binary_path,
                    args,
                    None,
                    Some(tmp.path()),
                    RunLimits::tool_defaults(),
                    cancel,
                )?;
                if outcome.verdict != Verdict::AC {
                    return Err(JudgeError::Internal(format!(
                        "generator `{gen_name}` failed on `{args_raw}` ({}): {}",
                        outcome.verdict,
                        outcome.stderr.trim()
                    )));
                }
                let data = fs::read(tmp.path()).map_err(|e| io_err("read generated input", e))?;
                let _ = cache.put_gen_input(&gen_key, &data);
                data
            }
        }
    };

    write_atomic(&dest, &bytes)?;
    let input_hash = xcpc_storage::content_hash(&bytes);
    let size_bytes = bytes.len() as u64;
    Ok(MaterializedTest {
        spec: spec.clone(),
        input_path: dest,
        input_hash,
        size_bytes,
    })
}

fn run_solution_cached_with_output(
    cache: &JudgeCache,
    sol_cr: &CompileResult,
    input_path: &Path,
    input_hash: &str,
    dest_out_path: &Path,
    limits: RunLimits,
    cancel: Option<&Arc<AtomicBool>>,
) -> Result<(RunOutcome, bool)> {
    let key = std_cache_key(&sol_cr.binary_hash, input_hash, limits);
    let out_cache = cache.root().join("run").join(format!("{key}.out"));
    if let Some(outcome) = cache.get_run_json::<RunOutcome>(&key)
        && out_cache.is_file()
    {
        let bytes = fs::read(&out_cache)
            .map_err(|e| io_err(format!("read cached std output {}", out_cache.display()), e))?;
        write_atomic(dest_out_path, &bytes)?;
        return Ok((outcome, true));
    }

    let tmp = tempfile::NamedTempFile::new().map_err(|e| io_err("create temp std output", e))?;
    let outcome = run_process(
        &sol_cr.binary_path,
        &[],
        Some(input_path),
        Some(tmp.path()),
        limits,
        cancel,
    )?;
    let bytes = fs::read(tmp.path()).map_err(|e| io_err("read std output", e))?;
    write_atomic(dest_out_path, &bytes)?;
    let _ = write_atomic(&out_cache, &bytes);
    let _ = cache.put_run_json(&key, &outcome);
    Ok((outcome, false))
}

fn run_self_tests(
    pool: &rayon::ThreadPool,
    prob_dir: &Path,
    validator_bin: Option<&Path>,
    checker_spec: &CheckerSpec,
    checker_bin: &Path,
    cancel: Option<&Arc<AtomicBool>>,
    emit: &(impl Fn(ProgressEvent) + Sync),
) -> Result<Vec<SelfTestResult>> {
    enum Task {
        ValValid {
            name: String,
            path: PathBuf,
        },
        ValInvalid {
            name: String,
            path: PathBuf,
        },
        ChkCase {
            kind: String,
            name: String,
            expected: Verdict,
            dir: PathBuf,
        },
    }

    let mut tasks = Vec::new();

    if validator_bin.is_some() {
        for (sub, is_valid) in [("valid", true), ("invalid", false)] {
            let dir = prob_dir.join("tests").join("validator").join(sub);
            if dir.is_dir() {
                let mut files: Vec<PathBuf> = fs::read_dir(&dir)
                    .into_iter()
                    .flatten()
                    .flatten()
                    .map(|e| e.path())
                    .filter(|p| {
                        p.is_file()
                            && p.extension()
                                .and_then(|s| s.to_str())
                                .is_some_and(|e| e == "in")
                    })
                    .collect();
                files.sort();
                for p in files {
                    let name = p
                        .file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or("case")
                        .to_owned();
                    if is_valid {
                        tasks.push(Task::ValValid { name, path: p });
                    } else {
                        tasks.push(Task::ValInvalid { name, path: p });
                    }
                }
            }
        }
    }

    if checker_spec.is_custom() {
        let chk_dir = prob_dir.join("tests").join("checker");
        if chk_dir.is_dir() {
            let mut dirs: Vec<PathBuf> = fs::read_dir(&chk_dir)
                .into_iter()
                .flatten()
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.is_dir())
                .collect();
            dirs.sort();
            for d in dirs {
                let name = d
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_owned();
                let (kind, expected) = if name.starts_with("ok-") {
                    ("checker_ok", Verdict::AC)
                } else if name.starts_with("wa-") {
                    ("checker_wa", Verdict::WA)
                } else if name.starts_with("pe-") {
                    ("checker_pe", Verdict::PE)
                } else {
                    continue;
                };
                tasks.push(Task::ChkCase {
                    kind: kind.to_owned(),
                    name,
                    expected,
                    dir: d,
                });
            }
        }
    }

    let total = tasks.len();
    if total == 0 {
        return Ok(Vec::new());
    }

    let done = AtomicUsize::new(0);
    let results: Vec<Result<SelfTestResult>> = pool.install(|| {
        tasks
            .par_iter()
            .map(|task| {
                let res = match task {
                    Task::ValValid { name, path } => {
                        let bin = validator_bin.expect("validator bin");
                        match run_validator(bin, path, cancel)? {
                            Ok(()) => SelfTestResult {
                                kind: "validator_valid".to_owned(),
                                name: name.clone(),
                                passed: true,
                                expected: "VALID".to_owned(),
                                actual: "VALID".to_owned(),
                                message: String::new(),
                            },
                            Err(msg) => SelfTestResult {
                                kind: "validator_valid".to_owned(),
                                name: name.clone(),
                                passed: false,
                                expected: "VALID".to_owned(),
                                actual: "INVALID".to_owned(),
                                message: msg,
                            },
                        }
                    }
                    Task::ValInvalid { name, path } => {
                        let bin = validator_bin.expect("validator bin");
                        match run_validator(bin, path, cancel)? {
                            Ok(()) => SelfTestResult {
                                kind: "validator_invalid".to_owned(),
                                name: name.clone(),
                                passed: false,
                                expected: "INVALID".to_owned(),
                                actual: "VALID".to_owned(),
                                message: "validator unexpectedly accepted invalid input".to_owned(),
                            },
                            Err(msg) => SelfTestResult {
                                kind: "validator_invalid".to_owned(),
                                name: name.clone(),
                                passed: true,
                                expected: "INVALID".to_owned(),
                                actual: "INVALID".to_owned(),
                                message: msg,
                            },
                        }
                    }
                    Task::ChkCase {
                        kind,
                        name,
                        expected,
                        dir,
                    } => {
                        let in_p = dir.join("in");
                        let out_p = dir.join("out");
                        let ans_p = dir.join("ans");
                        if !in_p.is_file() || !out_p.is_file() || !ans_p.is_file() {
                            SelfTestResult {
                                kind: kind.clone(),
                                name: name.clone(),
                                passed: false,
                                expected: expected.as_str().to_owned(),
                                actual: "JE".to_owned(),
                                message:
                                    "checker self-test dir must contain `in`, `out`, and `ans`"
                                        .to_owned(),
                            }
                        } else {
                            let (actual, msg) =
                                run_checker(checker_bin, &in_p, &out_p, &ans_p, cancel)?;
                            SelfTestResult {
                                kind: kind.clone(),
                                name: name.clone(),
                                passed: actual == *expected,
                                expected: expected.as_str().to_owned(),
                                actual: actual.as_str().to_owned(),
                                message: msg,
                            }
                        }
                    }
                };
                let completed = done.fetch_add(1, Ordering::Relaxed) + 1;
                emit(ProgressEvent::SelfTesting { completed, total });
                Ok(res)
            })
            .collect()
    });

    let mut out = Vec::with_capacity(results.len());
    for r in results {
        out.push(r?);
    }
    Ok(out)
}

fn compute_safety_margin(
    tests: &[MaterializedTest],
    solutions: &[SolutionSummary],
    main_name: &str,
    limits: RunLimits,
) -> SafetyMargin {
    let max_input_bytes = tests.iter().map(|t| t.size_bytes).max().unwrap_or(0);
    let std_summary = solutions.iter().find(|s| s.name == main_name);
    let std_max_time_ms = std_summary.map(|s| s.max_time_ms).unwrap_or(0);
    let std_max_memory_kib = std_summary.map(|s| s.max_memory_kib).unwrap_or(0);
    let std_time_ratio = if limits.time_limit_ms > 0 {
        std_max_time_ms as f64 / limits.time_limit_ms as f64
    } else {
        0.0
    };

    let slowest_ac = solutions
        .iter()
        .filter(|s| s.actual_verdict == Verdict::AC)
        .max_by_key(|s| s.max_time_ms);

    let fastest_tle = solutions
        .iter()
        .filter(|s| s.actual_verdict == Verdict::TLE)
        .min_by_key(|s| s.max_time_ms);

    SafetyMargin {
        max_input_bytes,
        time_limit_ms: limits.time_limit_ms,
        memory_limit_kib: limits.memory_limit_kib,
        std_max_time_ms,
        std_time_ratio,
        std_max_memory_kib,
        slowest_ac_solution: slowest_ac.map(|s| s.name.clone()),
        slowest_ac_time_ms: slowest_ac.map(|s| s.max_time_ms),
        fastest_tle_solution: fastest_tle.map(|s| s.name.clone()),
        fastest_tle_time_ms: fastest_tle.map(|s| s.max_time_ms),
    }
}

fn matches_test_filter(test: &ExpandedTest, filters: &[String]) -> bool {
    filters.iter().any(|f| {
        let f = f.trim();
        if let Ok(idx) = f.parse::<usize>()
            && idx == test.index
        {
            return true;
        }
        if test.name == f {
            return true;
        }
        if let Some(ref tag) = test.tag
            && tag == f
        {
            return true;
        }
        false
    })
}

fn solution_applies_to_test(sol: &DiscoveredSolution, test: &ExpandedTest) -> bool {
    let Some(ref allowed_tags) = sol.tags else {
        return true;
    };
    if allowed_tags.is_empty() {
        return true;
    }
    test.tag
        .as_deref()
        .is_some_and(|t| allowed_tags.iter().any(|a| a == t))
}

fn collect_existing_sample_input_hashes(prob_dir: &Path) -> HashSet<String> {
    let mut hashes = HashSet::new();
    let samples_dir = prob_dir.join("samples");
    let Ok(entries) = fs::read_dir(samples_dir) else {
        return hashes;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file()
            && path.extension().and_then(|s| s.to_str()) == Some("in")
            && let Ok(bytes) = fs::read(&path)
        {
            hashes.insert(xcpc_storage::content_hash(&bytes));
        }
    }
    hashes
}

fn next_available_sample_number(prob_dir: &Path) -> u32 {
    let samples_dir = prob_dir.join("samples");
    let Ok(entries) = fs::read_dir(samples_dir) else {
        return 1;
    };
    let mut max_n = 0u32;
    for entry in entries.flatten() {
        if let Some(name) = entry.file_name().to_str()
            && let Some(stem) = name.strip_suffix(".in")
            && let Ok(n) = stem.parse::<u32>()
        {
            max_n = max_n.max(n);
        }
    }
    max_n + 1
}

pub(crate) fn gen_cache_key(gen_bin_hash: &str, args_raw: &str) -> String {
    let mut h = Sha256::new();
    h.update(b"xcpc-gen-v1\0");
    h.update(gen_bin_hash.as_bytes());
    h.update(b"\0");
    h.update(args_raw.as_bytes());
    format!("{:x}", h.finalize())
}

fn std_cache_key(sol_bin_hash: &str, input_hash: &str, limits: RunLimits) -> String {
    let mut h = Sha256::new();
    h.update(b"xcpc-std-v1\0");
    h.update(sol_bin_hash.as_bytes());
    h.update(b"\0");
    h.update(input_hash.as_bytes());
    h.update(b"\0");
    h.update(limits.time_limit_ms.to_le_bytes());
    h.update(limits.memory_limit_kib.to_le_bytes());
    format!("{:x}", h.finalize())
}

fn run_cache_key(
    sol_bin_hash: &str,
    input_hash: &str,
    limits: RunLimits,
    checker_bin_hash: &str,
    ans_hash: &str,
) -> String {
    let mut h = Sha256::new();
    h.update(b"xcpc-run-v1\0");
    h.update(sol_bin_hash.as_bytes());
    h.update(b"\0");
    h.update(input_hash.as_bytes());
    h.update(b"\0");
    h.update(limits.time_limit_ms.to_le_bytes());
    h.update(limits.memory_limit_kib.to_le_bytes());
    h.update(b"\0");
    h.update(checker_bin_hash.as_bytes());
    h.update(b"\0");
    h.update(ans_hash.as_bytes());
    format!("{:x}", h.finalize())
}

pub(crate) fn now_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[allow(dead_code)]
fn _unused_btreemap_marker(_: BTreeMap<String, String>) {}

//! High-level package operations reused by CLI, MCP, and Desktop GUI:
//! - `inspect_package`
//! - `inspect_test_case`
//! - `init_package`
//! - `stress_package`

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use toml_edit::{ArrayOfTables, DocumentMut, Item, Table, value};
use xcpc_storage::Workspace;

use crate::assets::{resolve_testlib, standard_checker_source};
use crate::cache::JudgeCache;
use crate::error::{JudgeError, Result, io_err};
use crate::package::{
    CheckerSpec, ExpandedTest, PackageFreshness, TestSource, build_dir_for, check_dirty_inputs,
    check_package_freshness, compute_package_hash, discover_solutions, expand_tests,
    find_generator_source, find_validator_source, load_latest_report, resolve_checker_spec,
};
use crate::report::{BuildReport, DirtyInputIssue, GateResult};
use crate::runner::{RunLimits, Verdict, run_checker, run_process, run_validator};
use crate::toolchain::{CppStd, SourceLanguage, compile_cached, resolve_active_compiler};

/// Inventory of package files inside a problem directory.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageFiles {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub validator: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_checker: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub generators: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub solutions: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub samples: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hand_tests: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub validator_self_tests_valid: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub validator_self_tests_invalid: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub checker_self_tests: Vec<String>,
}

/// Configured metadata for one solution in a problem package (available before the first build).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageSolutionInfo {
    pub name: String,
    pub source_rel: String,
    pub is_main: bool,
    pub expected: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
}

/// Read-only overview of a problem package (`inspect_package`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PackageInspection {
    pub problem_id: String,
    pub has_package: bool,
    pub package_hash: String,
    pub freshness: PackageFreshness,
    pub checker: String,
    pub files: PackageFiles,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub solutions: Vec<PackageSolutionInfo>,
    pub expanded_tests: Vec<ExpandedTest>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dirty_inputs: Vec<DirtyInputIssue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_report: Option<BuildReport>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub gates: Vec<GateResult>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub next_steps: Vec<String>,
}

/// Safe bounded preview of a single test case (`inspect_test_case`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TestCasePreview {
    pub index: usize,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    pub is_sample: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gen_cmd: Option<String>,
    pub available_on_disk: bool,
    pub size_bytes: u64,
    pub total_lines: usize,
    pub head: String,
    pub tail: String,
    pub truncated: bool,
}

/// Result of scaffolding a new problem package (`init_package`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InitPackageResult {
    pub problem_id: String,
    pub created_files: Vec<String>,
    pub skipped_files: Vec<String>,
    pub updated_problem_toml: bool,
}

/// Options for bounded stress testing (`stress_package`).
#[derive(Debug, Clone)]
pub struct StressOptions {
    pub generator: String,
    pub args: String,
    /// Target solution names to test against `std`. If empty, selects all non-main solutions.
    pub solutions: Vec<String>,
    pub max_iter: u32,
    pub timeout_secs: u64,
    pub cxx_override: Option<PathBuf>,
    pub cache_dir: Option<PathBuf>,
}

impl Default for StressOptions {
    fn default() -> Self {
        Self {
            generator: String::new(),
            args: String::new(),
            solutions: Vec::new(),
            max_iter: 100,
            timeout_secs: 30,
            cxx_override: None,
            cache_dir: None,
        }
    }
}

/// Result of bounded stress testing (`stress_package`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StressResult {
    pub problem_id: String,
    pub found_counterexample: bool,
    pub iterations_run: u32,
    pub elapsed_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gen_cmd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub saved_test_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failing_solution: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verdict: Option<Verdict>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_preview: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub std_output_preview: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub solution_output_preview: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checker_message: Option<String>,
}

/// Pure read-only inspection of a problem package. Never compiles or executes anything.
pub fn inspect_package(ws: &Workspace, slug: &str) -> Result<PackageInspection> {
    let problem = ws.load_problem(slug)?;
    let prob_dir = ws.resolve(&problem.dir)?;
    let has_pkg = problem.meta.has_package_in_dir(&prob_dir);
    let package_hash = compute_package_hash(ws, slug)?;
    let freshness = check_package_freshness(ws, slug)?;
    let checker_spec = resolve_checker_spec(&prob_dir, &problem.meta)?;
    let expanded_tests = expand_tests(&prob_dir, &problem.meta)?;
    let dirty_inputs = check_dirty_inputs(&prob_dir);

    let files = collect_package_files(&prob_dir, &checker_spec);
    let solutions = match discover_solutions(&prob_dir, &problem.meta) {
        Ok(list) => list
            .into_iter()
            .map(|s| PackageSolutionInfo {
                name: s.name,
                source_rel: s.source_rel,
                is_main: s.is_main,
                expected: s.expect.normalized(),
                tags: s.tags,
            })
            .collect(),
        Err(_) => {
            // Fall back to declared solutions from `problem.toml` if a declared file is missing on disk.
            let mut list: Vec<PackageSolutionInfo> = problem
                .meta
                .solutions
                .iter()
                .map(|(name, spec)| PackageSolutionInfo {
                    name: name.clone(),
                    source_rel: format!("solutions/{name}.cpp"),
                    is_main: spec.main.unwrap_or(false),
                    expected: spec
                        .expect
                        .as_ref()
                        .map(|e| e.normalized())
                        .unwrap_or_else(|| vec!["AC".to_owned()]),
                    tags: spec.tags.clone(),
                })
                .collect();
            list.sort_by(|a, b| b.is_main.cmp(&a.is_main).then_with(|| a.name.cmp(&b.name)));
            list
        }
    };
    let last_report = load_latest_report(ws, slug);

    let gates = last_report
        .as_ref()
        .map(|r| r.gates.clone())
        .unwrap_or_default();

    let mut next_steps = Vec::new();
    if !has_pkg {
        next_steps.push(format!(
            "运行 `xcpc package init {slug}` 初始化题目包脚手架。"
        ));
    } else {
        match &freshness {
            PackageFreshness::VerifiedFresh { .. } => {}
            PackageFreshness::Stale { .. } => {
                next_steps.push(format!(
                    "题目包文件已变更，请重新运行 `xcpc package build {slug} --strict`。"
                ));
            }
            PackageFreshness::Unverified { .. } => {
                next_steps.push(format!(
                    "运行 `xcpc package build {slug} --strict` 执行严格验收。"
                ));
            }
            PackageFreshness::Failed { .. } => {
                if let Some(ref r) = last_report {
                    next_steps.extend(r.next_steps.clone());
                } else {
                    next_steps.push(format!(
                        "修复失败项后重新运行 `xcpc package build {slug} --strict`。"
                    ));
                }
            }
            PackageFreshness::NotAPackage => {}
        }
    }

    Ok(PackageInspection {
        problem_id: slug.to_owned(),
        has_package: has_pkg,
        package_hash,
        freshness,
        checker: checker_spec.display_name().to_owned(),
        files,
        solutions,
        expanded_tests,
        dirty_inputs,
        last_report,
        gates,
        next_steps,
    })
}

/// Safely previews the head and tail lines of a test case without overflowing context.
pub fn inspect_test_case(
    ws: &Workspace,
    slug: &str,
    selector: &str,
    head_lines: usize,
    tail_lines: usize,
) -> Result<TestCasePreview> {
    let problem = ws.load_problem(slug)?;
    let prob_dir = ws.resolve(&problem.dir)?;
    let tests = expand_tests(&prob_dir, &problem.meta)?;

    let sel = selector.trim();
    let test = tests
        .iter()
        .find(|t| sel.parse::<usize>().ok() == Some(t.index) || t.name == sel)
        .ok_or_else(|| JudgeError::TestNotFound(selector.to_owned()))?;

    let disk_path = match &test.source {
        TestSource::Sample { input_rel, .. } => prob_dir.join(input_rel),
        TestSource::File { file_rel } => prob_dir.join(file_rel),
        TestSource::Generated { .. } => build_dir_for(ws, slug)
            .join("tests")
            .join(format!("{:03}.in", test.index)),
    };

    if !disk_path.is_file() {
        return Ok(TestCasePreview {
            index: test.index,
            name: test.name.clone(),
            tag: test.tag.clone(),
            is_sample: test.is_sample,
            gen_cmd: test.gen_cmd.clone(),
            available_on_disk: false,
            size_bytes: 0,
            total_lines: 0,
            head: String::new(),
            tail: String::new(),
            truncated: false,
        });
    }

    let bytes =
        fs::read(&disk_path).map_err(|e| io_err(format!("read {}", disk_path.display()), e))?;
    let size_bytes = bytes.len() as u64;
    let text = String::from_utf8_lossy(&bytes);
    let lines: Vec<&str> = text.lines().collect();
    let total_lines = lines.len();

    let (head, tail, truncated) = slice_head_tail(&lines, head_lines, tail_lines, 4096);

    Ok(TestCasePreview {
        index: test.index,
        name: test.name.clone(),
        tag: test.tag.clone(),
        is_sample: test.is_sample,
        gen_cmd: test.gen_cmd.clone(),
        available_on_disk: true,
        size_bytes,
        total_lines,
        head,
        tail,
        truncated,
    })
}

/// Scaffolds a Polygon package with `testlib.h` best practices without overwriting existing files.
pub fn init_package(ws: &Workspace, slug: &str) -> Result<InitPackageResult> {
    let problem = ws.load_problem(slug)?;
    let prob_dir = ws.resolve(&problem.dir)?;

    let scaffold_files: &[(&str, &str)] = &[
        ("validator.cpp", TEMPLATE_VALIDATOR_CPP),
        ("generators/gen_random.cpp", TEMPLATE_GEN_RANDOM_CPP),
        ("solutions/std.cpp", TEMPLATE_STD_CPP),
        ("tests/validator/valid/01.in", "1 2\n"),
        ("tests/validator/invalid/01.in", "-1 2\n"),
    ];

    let mut created_files = Vec::new();
    let mut skipped_files = Vec::new();

    for (rel_in_prob, content) in scaffold_files {
        let abs = prob_dir.join(rel_in_prob);
        if abs.exists() {
            skipped_files.push((*rel_in_prob).to_owned());
        } else {
            let ws_rel = format!("{}/{rel_in_prob}", problem.dir);
            ws.write(&ws_rel, content.as_bytes())?;
            created_files.push((*rel_in_prob).to_owned());
        }
    }

    // Update `problem.toml` via `toml_edit` if `[solutions.std]` or `[[test]]` is absent.
    let toml_rel = format!("{}/problem.toml", problem.dir);
    let raw_toml = ws.read_text(&toml_rel)?;
    let mut doc: DocumentMut = raw_toml
        .parse()
        .map_err(|e| JudgeError::InvalidConfig(format!("parse {toml_rel}: {e}")))?;

    let mut updated_toml = false;

    if doc.get("test").is_none() {
        let mut arr = ArrayOfTables::new();
        let mut t = Table::new();
        t.insert("gen", value("gen_random"));
        t.insert("args", value("-n 10 {i}"));
        t.insert("repeat", value(3));
        t.insert("tag", value("small"));
        arr.push(t);
        doc.insert("test", Item::ArrayOfTables(arr));
        updated_toml = true;
    }

    if doc.get("solutions").is_none() {
        let mut sols = Table::new();
        sols.set_implicit(true);
        let mut std_tbl = Table::new();
        std_tbl.insert("main", value(true));
        sols.insert("std", Item::Table(std_tbl));
        doc.insert("solutions", Item::Table(sols));
        updated_toml = true;
    }

    if updated_toml {
        ws.write(&toml_rel, doc.to_string().as_bytes())?;
    }

    Ok(InitPackageResult {
        problem_id: slug.to_owned(),
        created_files,
        skipped_files,
        updated_problem_toml: updated_toml,
    })
}

/// Runs bounded stress testing (`--gen`, `--args`, `--solutions`, `--max-iter`, `--timeout`).
///
/// Saves the first counterexample to `tests/stress-<n>.in` without modifying `problem.toml`.
pub fn stress_package(
    ws: &Workspace,
    slug: &str,
    opts: &StressOptions,
    cancel: Option<Arc<AtomicBool>>,
) -> Result<StressResult> {
    if opts.generator.trim().is_empty() {
        return Err(JudgeError::InvalidConfig(
            "stress testing requires `--gen <name>`".to_owned(),
        ));
    }

    let problem = ws.load_problem(slug)?;
    let prob_dir = ws.resolve(&problem.dir)?;
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

    let all_solutions = discover_solutions(&prob_dir, &problem.meta)?;
    let main_solutions: Vec<_> = all_solutions.iter().filter(|s| s.is_main).collect();
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

    let target_solutions: Vec<_> = if opts.solutions.is_empty() {
        all_solutions
            .iter()
            .filter(|s| !s.is_main)
            .cloned()
            .collect()
    } else {
        let mut list = Vec::new();
        for name in &opts.solutions {
            let found = all_solutions
                .iter()
                .find(|s| s.name == *name)
                .ok_or_else(|| JudgeError::SolutionNotFound(name.clone()))?;
            if found.name != main_sol.name {
                list.push(found.clone());
            }
        }
        list
    };

    if target_solutions.is_empty() {
        return Err(JudgeError::InvalidConfig(
            "stress testing requires at least one non-main solution to compare against std"
                .to_owned(),
        ));
    }

    // Compile generator, validator (if present), checker, std, and target solutions.
    let (gen_path, gen_rel, gen_lang) = find_generator_source(&prob_dir, &opts.generator)?;
    let gen_bytes =
        fs::read(&gen_path).map_err(|e| io_err(format!("read {}", gen_path.display()), e))?;
    let gen_cr = compile_cached(
        &cache,
        &compiler,
        gen_lang,
        &format!("gen:{}", opts.generator),
        &gen_rel,
        &gen_bytes,
        Some(&gen_path),
        &testlib,
    )
    .map_err(|f| JudgeError::InvalidConfig(format!("generator compile failed:\n{}", f.stderr)))?;

    let val_cr = if let Some((vp, vrel, vlang)) = find_validator_source(&prob_dir) {
        let vbytes = fs::read(&vp).map_err(|e| io_err(format!("read {}", vp.display()), e))?;
        let cr = compile_cached(
            &cache,
            &compiler,
            vlang,
            "validator",
            &vrel,
            &vbytes,
            Some(&vp),
            &testlib,
        )
        .map_err(|f| {
            JudgeError::InvalidConfig(format!("validator compile failed:\n{}", f.stderr))
        })?;
        Some(cr)
    } else {
        None
    };

    let checker_spec = resolve_checker_spec(&prob_dir, &problem.meta)?;
    let checker_cr = match &checker_spec {
        CheckerSpec::Builtin(name) => {
            let src = standard_checker_source(name)
                .ok_or_else(|| JudgeError::UnknownChecker(name.clone()))?;
            compile_cached(
                &cache,
                &compiler,
                SourceLanguage::Cpp { std: CppStd::Cpp20 },
                "checker",
                &format!("<builtin:{name}>"),
                src.as_bytes(),
                None,
                &testlib,
            )
            .map_err(|f| {
                JudgeError::Internal(format!("builtin checker compile failed: {}", f.stderr))
            })?
        }
        CheckerSpec::Custom { path, display_name } => {
            let lang = SourceLanguage::from_path(path)
                .unwrap_or(SourceLanguage::Cpp { std: CppStd::Cpp20 });
            let bytes =
                fs::read(path).map_err(|e| io_err(format!("read {}", path.display()), e))?;
            compile_cached(
                &cache,
                &compiler,
                lang,
                "checker",
                display_name,
                &bytes,
                Some(path),
                &testlib,
            )
            .map_err(|f| {
                JudgeError::InvalidConfig(format!("checker compile failed:\n{}", f.stderr))
            })?
        }
    };

    let main_bytes = fs::read(&main_sol.source_path)
        .map_err(|e| io_err(format!("read {}", main_sol.source_path.display()), e))?;
    let main_cr = compile_cached(
        &cache,
        &compiler,
        main_sol.language,
        &format!("sol:{}", main_sol.name),
        &main_sol.source_rel,
        &main_bytes,
        Some(&main_sol.source_path),
        &testlib,
    )
    .map_err(|f| JudgeError::InvalidConfig(format!("std compile failed:\n{}", f.stderr)))?;

    let mut target_crs = Vec::with_capacity(target_solutions.len());
    for sol in &target_solutions {
        let bytes = fs::read(&sol.source_path)
            .map_err(|e| io_err(format!("read {}", sol.source_path.display()), e))?;
        let cr = compile_cached(
            &cache,
            &compiler,
            sol.language,
            &format!("sol:{}", sol.name),
            &sol.source_rel,
            &bytes,
            Some(&sol.source_path),
            &testlib,
        )
        .map_err(|f| {
            JudgeError::InvalidConfig(format!(
                "solution `{}` compile failed:\n{}",
                sol.name, f.stderr
            ))
        })?;
        target_crs.push((sol, cr));
    }

    let tmp_dir = tempfile::tempdir().map_err(|e| io_err("create stress temp dir", e))?;
    let in_path = tmp_dir.path().join("stress.in");
    let ans_path = tmp_dir.path().join("stress.ans");
    let out_path = tmp_dir.path().join("stress.out");

    let start = Instant::now();
    let deadline = start + Duration::from_secs(opts.timeout_secs.max(1));
    let max_iter = opts.max_iter.max(1);
    let mut iterations_run = 0u32;

    for seed in 1..=max_iter {
        if let Some(ref c) = cancel
            && c.load(Ordering::Relaxed)
        {
            return Err(JudgeError::Cancelled);
        }
        if Instant::now() >= deadline {
            break;
        }
        iterations_run = seed;

        let args_raw = expand_stress_args(&opts.args, seed);
        let args = shlex::split(&args_raw).ok_or_else(|| {
            JudgeError::InvalidConfig(format!("unclosed quote in stress args: {args_raw:?}"))
        })?;
        let gen_cmd = format!("{} {args_raw}", opts.generator);

        let gen_out = run_process(
            &gen_cr.binary_path,
            &args,
            None,
            Some(&in_path),
            RunLimits::tool_defaults(),
            cancel.as_ref(),
        )?;
        if gen_out.verdict != Verdict::AC {
            return Err(JudgeError::Internal(format!(
                "generator failed on seed {seed}: {}",
                gen_out.stderr.trim()
            )));
        }

        // Validate generated input if validator exists.
        if let Some(ref vc) = val_cr
            && let Err(vmsg) = run_validator(&vc.binary_path, &in_path, cancel.as_ref())?
        {
            let saved = save_stress_counterexample(ws, &problem.dir, &prob_dir, &in_path)?;
            return Ok(StressResult {
                problem_id: slug.to_owned(),
                found_counterexample: true,
                iterations_run,
                elapsed_ms: start.elapsed().as_millis() as u64,
                seed: Some(seed),
                gen_cmd: Some(gen_cmd),
                saved_test_path: Some(saved),
                failing_solution: Some("validator".to_owned()),
                verdict: Some(Verdict::JE),
                input_preview: Some(read_truncated_preview(&in_path, 4096)),
                std_output_preview: None,
                solution_output_preview: None,
                checker_message: Some(format!("validator rejected generated input: {vmsg}")),
            });
        }

        // Run std.
        let std_outcome = run_process(
            &main_cr.binary_path,
            &[],
            Some(&in_path),
            Some(&ans_path),
            limits,
            cancel.as_ref(),
        )?;
        if std_outcome.verdict != Verdict::AC {
            let saved = save_stress_counterexample(ws, &problem.dir, &prob_dir, &in_path)?;
            return Ok(StressResult {
                problem_id: slug.to_owned(),
                found_counterexample: true,
                iterations_run,
                elapsed_ms: start.elapsed().as_millis() as u64,
                seed: Some(seed),
                gen_cmd: Some(gen_cmd),
                saved_test_path: Some(saved),
                failing_solution: Some(main_sol.name.clone()),
                verdict: Some(std_outcome.verdict),
                input_preview: Some(read_truncated_preview(&in_path, 4096)),
                std_output_preview: Some(read_truncated_preview(&ans_path, 2048)),
                solution_output_preview: None,
                checker_message: Some(format!(
                    "main solution `{}` failed with {}",
                    main_sol.name, std_outcome.verdict
                )),
            });
        }

        // Run each target solution and compare with checker.
        for (sol, cr) in &target_crs {
            let sol_outcome = run_process(
                &cr.binary_path,
                &[],
                Some(&in_path),
                Some(&out_path),
                limits,
                cancel.as_ref(),
            )?;
            let (verdict, msg) = if sol_outcome.verdict == Verdict::AC {
                run_checker(
                    &checker_cr.binary_path,
                    &in_path,
                    &out_path,
                    &ans_path,
                    cancel.as_ref(),
                )?
            } else {
                (sol_outcome.verdict, sol_outcome.stderr.clone())
            };

            if verdict != Verdict::AC {
                let saved = save_stress_counterexample(ws, &problem.dir, &prob_dir, &in_path)?;
                return Ok(StressResult {
                    problem_id: slug.to_owned(),
                    found_counterexample: true,
                    iterations_run,
                    elapsed_ms: start.elapsed().as_millis() as u64,
                    seed: Some(seed),
                    gen_cmd: Some(gen_cmd),
                    saved_test_path: Some(saved),
                    failing_solution: Some(sol.name.clone()),
                    verdict: Some(verdict),
                    input_preview: Some(read_truncated_preview(&in_path, 4096)),
                    std_output_preview: Some(read_truncated_preview(&ans_path, 2048)),
                    solution_output_preview: Some(read_truncated_preview(&out_path, 2048)),
                    checker_message: Some(msg),
                });
            }
        }
    }

    Ok(StressResult {
        problem_id: slug.to_owned(),
        found_counterexample: false,
        iterations_run,
        elapsed_ms: start.elapsed().as_millis() as u64,
        seed: None,
        gen_cmd: None,
        saved_test_path: None,
        failing_solution: None,
        verdict: None,
        input_preview: None,
        std_output_preview: None,
        solution_output_preview: None,
        checker_message: None,
    })
}

fn expand_stress_args(template: &str, seed: u32) -> String {
    let s = seed.to_string();
    if template.contains("{i}") || template.contains("{seed}") {
        template.replace("{i}", &s).replace("{seed}", &s)
    } else if template.trim().is_empty() {
        s
    } else {
        format!("{template} {s}")
    }
}

fn save_stress_counterexample(
    ws: &Workspace,
    problem_dir_rel: &str,
    prob_dir: &Path,
    in_path: &Path,
) -> Result<String> {
    let tests_dir = prob_dir.join("tests");
    let mut next_n = 1u32;
    if let Ok(entries) = fs::read_dir(&tests_dir) {
        for e in entries.flatten() {
            if let Some(name) = e.file_name().to_str()
                && let Some(rest) = name.strip_prefix("stress-")
                && let Some(num) = rest.strip_suffix(".in")
                && let Ok(n) = num.parse::<u32>()
            {
                next_n = next_n.max(n + 1);
            }
        }
    }
    let rel_in_prob = format!("tests/stress-{next_n}.in");
    let bytes = fs::read(in_path).map_err(|e| io_err("read counterexample", e))?;
    let ws_rel = format!("{problem_dir_rel}/{rel_in_prob}");
    ws.write(&ws_rel, &bytes)?;
    Ok(rel_in_prob)
}

fn read_truncated_preview(path: &Path, max_bytes: usize) -> String {
    let Ok(bytes) = fs::read(path) else {
        return String::new();
    };
    if bytes.len() <= max_bytes {
        String::from_utf8_lossy(&bytes).into_owned()
    } else {
        let mut s = String::from_utf8_lossy(&bytes[..max_bytes]).into_owned();
        s.push_str(&format!("\n... [truncated, {} bytes total]", bytes.len()));
        s
    }
}

fn slice_head_tail(
    lines: &[&str],
    head_n: usize,
    tail_n: usize,
    max_chars_each: usize,
) -> (String, String, bool) {
    let cap_text = |slice: &[&str]| -> (String, bool) {
        let joined = slice.join("\n");
        if joined.len() <= max_chars_each {
            (joined, false)
        } else {
            let mut end = max_chars_each;
            while end > 0 && !joined.is_char_boundary(end) {
                end -= 1;
            }
            (format!("{}...", &joined[..end]), true)
        }
    };

    if lines.len() <= head_n.saturating_add(tail_n) {
        let (head, trunc) = cap_text(lines);
        return (head, String::new(), trunc);
    }

    let (head, _) = cap_text(&lines[..head_n.min(lines.len())]);
    let tail_start = lines.len().saturating_sub(tail_n);
    let (tail, _) = cap_text(&lines[tail_start..]);
    (head, tail, true)
}

fn collect_package_files(prob_dir: &Path, checker_spec: &CheckerSpec) -> PackageFiles {
    let list_files = |dir: PathBuf| -> Vec<String> {
        let Ok(entries) = fs::read_dir(&dir) else {
            return Vec::new();
        };
        let mut out: Vec<String> = entries
            .flatten()
            .filter(|e| e.file_type().is_ok_and(|ft| ft.is_file()))
            .filter_map(|e| e.file_name().to_str().map(str::to_owned))
            .collect();
        out.sort();
        out
    };

    let list_dirs = |dir: PathBuf| -> Vec<String> {
        let Ok(entries) = fs::read_dir(&dir) else {
            return Vec::new();
        };
        let mut out: Vec<String> = entries
            .flatten()
            .filter(|e| e.file_type().is_ok_and(|ft| ft.is_dir()))
            .filter_map(|e| e.file_name().to_str().map(str::to_owned))
            .collect();
        out.sort();
        out
    };

    PackageFiles {
        validator: find_validator_source(prob_dir).map(|(_, rel, _)| rel),
        custom_checker: match checker_spec {
            CheckerSpec::Custom { display_name, .. } => Some(display_name.clone()),
            CheckerSpec::Builtin(_) => None,
        },
        generators: list_files(prob_dir.join("generators")),
        solutions: list_files(prob_dir.join("solutions")),
        samples: list_files(prob_dir.join("samples")),
        hand_tests: list_files(prob_dir.join("tests"))
            .into_iter()
            .filter(|f| f.ends_with(".in"))
            .collect(),
        validator_self_tests_valid: list_files(prob_dir.join("tests/validator/valid")),
        validator_self_tests_invalid: list_files(prob_dir.join("tests/validator/invalid")),
        checker_self_tests: list_dirs(prob_dir.join("tests/checker")),
    }
}

const TEMPLATE_VALIDATOR_CPP: &str = r#"#include "testlib.h"

int main(int argc, char* argv[]) {
    registerValidation(argc, argv);
    int a = inf.readInt(0, 1'000'000'000, "a");
    inf.readSpace();
    int b = inf.readInt(0, 1'000'000'000, "b");
    inf.readEoln();
    inf.readEof();
    (void)a;
    (void)b;
    return 0;
}
"#;

const TEMPLATE_GEN_RANDOM_CPP: &str = r#"#include "testlib.h"
#include <iostream>

int main(int argc, char* argv[]) {
    registerGen(argc, argv, 1);
    int n = opt<int>("n", 10);
    int a = rnd.next(0, n);
    int b = rnd.next(0, n);
    std::cout << a << ' ' << b << '\n';
    return 0;
}
"#;

const TEMPLATE_STD_CPP: &str = r#"#include <iostream>

int main() {
    std::ios::sync_with_stdio(false);
    std::cin.tie(nullptr);
    long long a, b;
    if (std::cin >> a >> b) {
        std::cout << (a + b) << '\n';
    }
    return 0;
}
"#;

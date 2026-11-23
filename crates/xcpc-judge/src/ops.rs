//! High-level package operations reused by CLI, MCP, and Desktop GUI:
//! - `inspect_package`
//! - `inspect_test_case`
//! - `init_package`
//! - `stress_package`

use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use toml_edit::{Array, ArrayOfTables, DocumentMut, Item, Table, value};
use xcpc_storage::Workspace;

use crate::assets::{is_standard_checker, resolve_testlib, standard_checker_source};
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
    /// All self-test file paths relative to `problems/<slug>/` (validator valid/invalid & checker files).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub self_test_files: Vec<String>,
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
    #[serde(default)]
    pub ans_available: bool,
    #[serde(default)]
    pub ans_size_bytes: u64,
    #[serde(default)]
    pub ans_head: String,
    #[serde(default)]
    pub ans_tail: String,
    #[serde(default)]
    pub ans_truncated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ans_total_lines: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule_index: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_rel: Option<String>,
}

/// Kind of boilerplate source file to create inside `problems/<slug>/`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackageSourceKind {
    Solution,
    Generator,
    Validator,
    CustomChecker,
    ValidatorSelfTestValid,
    ValidatorSelfTestInvalid,
}

/// Result of appending a hand-written or generated `[[test]]` rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AddedTestInfo {
    pub rule_index: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_rel: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
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

/// Safely previews the head and tail lines of a test case (and its `.ans` if available)
/// without reading entire multi-MB files into memory or overflowing context on wide lines.
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

    let (disk_path, file_rel, ans_path) = match &test.source {
        TestSource::Sample {
            input_rel,
            manual_ans_rel,
            ..
        } => {
            let ans = manual_ans_rel
                .as_ref()
                .map(|r| prob_dir.join(r))
                .filter(|p| p.is_file())
                .unwrap_or_else(|| {
                    build_dir_for(ws, slug)
                        .join("tests")
                        .join(format!("{:03}.ans", test.index))
                });
            (prob_dir.join(input_rel), Some(input_rel.clone()), ans)
        }
        TestSource::File { file_rel } => (
            prob_dir.join(file_rel),
            Some(file_rel.clone()),
            build_dir_for(ws, slug)
                .join("tests")
                .join(format!("{:03}.ans", test.index)),
        ),
        TestSource::Generated { .. } => (
            build_dir_for(ws, slug)
                .join("tests")
                .join(format!("{:03}.in", test.index)),
            None,
            build_dir_for(ws, slug)
                .join("tests")
                .join(format!("{:03}.ans", test.index)),
        ),
    };

    let (ans_available, ans_size_bytes, ans_head, ans_tail, ans_truncated, ans_total_lines) =
        if ans_path.is_file() {
            let p = read_bounded_preview(&ans_path, head_lines, tail_lines)?;
            (
                true,
                p.size_bytes,
                p.head,
                p.tail,
                p.truncated,
                p.exact_total_lines,
            )
        } else {
            (false, 0, String::new(), String::new(), false, None)
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
            ans_available,
            ans_size_bytes,
            ans_head,
            ans_tail,
            ans_truncated,
            ans_total_lines,
            rule_index: test.rule_index,
            file_rel,
        });
    }

    let preview = read_bounded_preview(&disk_path, head_lines, tail_lines)?;

    Ok(TestCasePreview {
        index: test.index,
        name: test.name.clone(),
        tag: test.tag.clone(),
        is_sample: test.is_sample,
        gen_cmd: test.gen_cmd.clone(),
        available_on_disk: true,
        size_bytes: preview.size_bytes,
        total_lines: preview.exact_total_lines.unwrap_or(0),
        head: preview.head,
        tail: preview.tail,
        truncated: preview.truncated,
        ans_available,
        ans_size_bytes,
        ans_head,
        ans_tail,
        ans_truncated,
        ans_total_lines,
        rule_index: test.rule_index,
        file_rel,
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

const HEAD_READ_LIMIT: u64 = 64 * 1024;
const TAIL_READ_TRIGGER: u64 = 96 * 1024;
const TAIL_READ_LIMIT: u64 = 32 * 1024;
const MAX_LINE_CHARS: usize = 200;
const MAX_PREVIEW_BYTES: usize = 4096;

struct BoundedPreview {
    size_bytes: u64,
    exact_total_lines: Option<usize>,
    head: String,
    tail: String,
    truncated: bool,
}

fn truncate_line_chars(line: &str, max_chars: usize) -> (String, bool) {
    let total_chars = line.chars().count();
    if total_chars <= max_chars {
        (line.to_owned(), false)
    } else {
        let prefix: String = line.chars().take(max_chars).collect();
        let extra = total_chars - max_chars;
        (format!("{prefix}… (+{extra} chars)"), true)
    }
}

fn format_bounded_lines(lines: &[&str], max_bytes: usize) -> (String, bool) {
    let mut out = String::new();
    let mut truncated = false;
    for (i, line) in lines.iter().enumerate() {
        let (clean, line_trunc) = truncate_line_chars(line, MAX_LINE_CHARS);
        if line_trunc {
            truncated = true;
        }
        let sep_len = usize::from(i > 0);
        if out.len() + sep_len + clean.len() > max_bytes {
            let remaining = max_bytes.saturating_sub(out.len() + sep_len);
            if remaining > 0 {
                if i > 0 {
                    out.push('\n');
                }
                let mut end = remaining.min(clean.len());
                while end > 0 && !clean.is_char_boundary(end) {
                    end -= 1;
                }
                out.push_str(&clean[..end]);
                out.push('…');
            }
            truncated = true;
            break;
        }
        if i > 0 {
            out.push('\n');
        }
        out.push_str(&clean);
    }
    (out, truncated)
}

fn read_bounded_preview(
    path: &Path,
    head_lines: usize,
    tail_lines: usize,
) -> Result<BoundedPreview> {
    let mut f = File::open(path).map_err(|e| io_err(format!("open {}", path.display()), e))?;
    let size_bytes = f
        .metadata()
        .map_err(|e| io_err(format!("stat {}", path.display()), e))?
        .len();

    if size_bytes <= TAIL_READ_TRIGGER {
        let mut buf = Vec::with_capacity(size_bytes as usize);
        f.read_to_end(&mut buf)
            .map_err(|e| io_err(format!("read {}", path.display()), e))?;
        let text = String::from_utf8_lossy(&buf);
        let lines: Vec<&str> = text.lines().collect();
        let total = lines.len();

        if total <= head_lines.saturating_add(tail_lines) {
            let (head, trunc) = format_bounded_lines(&lines, MAX_PREVIEW_BYTES);
            return Ok(BoundedPreview {
                size_bytes,
                exact_total_lines: Some(total),
                head,
                tail: String::new(),
                truncated: trunc,
            });
        }

        let (head, _) = format_bounded_lines(&lines[..head_lines.min(total)], MAX_PREVIEW_BYTES);
        let tail_start = total.saturating_sub(tail_lines);
        let (tail, _) = format_bounded_lines(&lines[tail_start..], MAX_PREVIEW_BYTES);
        return Ok(BoundedPreview {
            size_bytes,
            exact_total_lines: Some(total),
            head,
            tail,
            truncated: true,
        });
    }

    // Large file (> 96 KiB): read at most 64 KiB from start and 32 KiB from end.
    let mut head_buf = vec![0u8; HEAD_READ_LIMIT as usize];
    let n_head = f
        .read(&mut head_buf)
        .map_err(|e| io_err(format!("read head {}", path.display()), e))?;
    head_buf.truncate(n_head);

    f.seek(SeekFrom::End(-(TAIL_READ_LIMIT as i64)))
        .map_err(|e| io_err(format!("seek tail {}", path.display()), e))?;
    let mut tail_buf = vec![0u8; TAIL_READ_LIMIT as usize];
    let n_tail = f
        .read(&mut tail_buf)
        .map_err(|e| io_err(format!("read tail {}", path.display()), e))?;
    tail_buf.truncate(n_tail);

    let head_text = String::from_utf8_lossy(&head_buf);
    let mut h_lines: Vec<&str> = head_text.lines().collect();
    if h_lines.len() > 1 && !head_buf.ends_with(b"\n") {
        // Drop potentially cut-off final line in the 64 KiB window.
        h_lines.pop();
    }
    let head_slice = &h_lines[..head_lines.min(h_lines.len())];
    let (head, _) = format_bounded_lines(head_slice, MAX_PREVIEW_BYTES);

    let tail = if tail_lines == 0 {
        String::new()
    } else {
        let tail_text = String::from_utf8_lossy(&tail_buf);
        if let Some(first_nl) = tail_text.find('\n') {
            let after = &tail_text[first_nl + 1..];
            let t_lines: Vec<&str> = after.lines().collect();
            let start = t_lines.len().saturating_sub(tail_lines);
            let (t, _) = format_bounded_lines(&t_lines[start..], MAX_PREVIEW_BYTES);
            t
        } else {
            // Single line spanning more than 32 KiB: show the last MAX_LINE_CHARS characters.
            let chars: Vec<char> = tail_text.chars().collect();
            let start = chars.len().saturating_sub(MAX_LINE_CHARS);
            let suffix: String = chars[start..].iter().collect();
            format!("…{suffix}")
        }
    };

    Ok(BoundedPreview {
        size_bytes,
        exact_total_lines: None,
        head,
        tail,
        truncated: true,
    })
}

/// Updates `[package].checker` in `problem.toml` via `toml_edit`.
///
/// If `"checker.cpp"` is selected and does not exist yet, creates a clean `testlib.h`
/// custom checker boilerplate (never overwriting an existing file).
pub fn set_package_checker(ws: &Workspace, slug: &str, checker: &str) -> Result<()> {
    let checker = checker.trim();
    if checker != "checker.cpp" && !is_standard_checker(checker) {
        return Err(JudgeError::UnknownChecker(checker.to_owned()));
    }

    let problem = ws.load_problem(slug)?;
    let prob_dir = ws.resolve(&problem.dir)?;
    let toml_rel = format!("{}/problem.toml", problem.dir);
    let raw_toml = ws.read_text(&toml_rel)?;
    let mut doc: DocumentMut = raw_toml
        .parse()
        .map_err(|e| JudgeError::InvalidConfig(format!("parse {toml_rel}: {e}")))?;

    let pkg_tbl = doc
        .entry("package")
        .or_insert_with(|| Item::Table(Table::new()))
        .as_table_mut()
        .ok_or_else(|| JudgeError::InvalidConfig("[package] is not a table".to_owned()))?;
    pkg_tbl.insert("checker", value(checker));

    ws.write(&toml_rel, doc.to_string().as_bytes())?;

    if checker == "checker.cpp" {
        let custom_abs = prob_dir.join("checker.cpp");
        if !custom_abs.exists() {
            let rel = format!("{}/checker.cpp", problem.dir);
            ws.write(&rel, TEMPLATE_CHECKER_CPP.as_bytes())?;
        }
    }

    Ok(())
}

/// Updates `[solutions.<name>]` in `problem.toml` via `toml_edit`.
///
/// When `is_main == Some(true)`, clears `main = true` from all other solutions and removes
/// `expect` on the new main solution.
pub fn set_solution_config(
    ws: &Workspace,
    slug: &str,
    name: &str,
    is_main: Option<bool>,
    expect: Option<Vec<String>>,
    tags: Option<Vec<String>>,
) -> Result<()> {
    let name = validate_safe_stem(name, "solution")?;
    let problem = ws.load_problem(slug)?;
    let toml_rel = format!("{}/problem.toml", problem.dir);
    let raw_toml = ws.read_text(&toml_rel)?;
    let mut doc: DocumentMut = raw_toml
        .parse()
        .map_err(|e| JudgeError::InvalidConfig(format!("parse {toml_rel}: {e}")))?;

    let sols_tbl = doc
        .entry("solutions")
        .or_insert_with(|| {
            let mut t = Table::new();
            t.set_implicit(true);
            Item::Table(t)
        })
        .as_table_mut()
        .ok_or_else(|| JudgeError::InvalidConfig("[solutions] is not a table".to_owned()))?;

    if is_main == Some(true) {
        for (k, v) in sols_tbl.iter_mut() {
            if k.get() != name
                && let Some(t) = v.as_table_mut()
            {
                t.remove("main");
            }
        }
    }

    let entry_tbl = sols_tbl
        .entry(name)
        .or_insert_with(|| Item::Table(Table::new()))
        .as_table_mut()
        .ok_or_else(|| JudgeError::InvalidConfig(format!("[solutions.{name}] is not a table")))?;

    if let Some(main_flag) = is_main {
        if main_flag {
            entry_tbl.insert("main", value(true));
            entry_tbl.remove("expect");
        } else {
            entry_tbl.remove("main");
        }
    }

    if let Some(exp_list) = expect
        && is_main != Some(true)
    {
        let cleaned: Vec<String> = exp_list
            .into_iter()
            .map(|s| s.trim().to_ascii_uppercase())
            .filter(|s| !s.is_empty())
            .collect();
        match cleaned.len() {
            0 => {
                entry_tbl.remove("expect");
            }
            1 => {
                entry_tbl.insert("expect", value(cleaned[0].as_str()));
            }
            _ => {
                let mut arr = Array::new();
                for v in cleaned {
                    arr.push(v);
                }
                entry_tbl.insert("expect", value(arr));
            }
        }
    }

    if let Some(tag_list) = tags {
        let cleaned: Vec<String> = tag_list
            .into_iter()
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty())
            .collect();
        if cleaned.is_empty() {
            entry_tbl.remove("tags");
        } else {
            let mut arr = Array::new();
            for t in cleaned {
                arr.push(t);
            }
            entry_tbl.insert("tags", value(arr));
        }
    }

    ws.write(&toml_rel, doc.to_string().as_bytes())?;
    Ok(())
}

/// Creates a boilerplate package source or self-test file inside `problems/<slug>/`
/// (never overwriting an existing file). Returns the path relative to `problems/<slug>/`.
pub fn create_package_source(
    ws: &Workspace,
    slug: &str,
    kind: PackageSourceKind,
    name: &str,
) -> Result<String> {
    let problem = ws.load_problem(slug)?;
    let prob_dir = ws.resolve(&problem.dir)?;

    let (rel_in_prob, template, solution_stem): (String, &str, Option<String>) = match kind {
        PackageSourceKind::Solution => {
            let stem = validate_safe_stem(name.strip_suffix(".cpp").unwrap_or(name), "solution")?;
            (
                format!("solutions/{stem}.cpp"),
                TEMPLATE_STD_CPP,
                Some(stem.to_owned()),
            )
        }
        PackageSourceKind::Generator => {
            let stem = validate_safe_stem(name.strip_suffix(".cpp").unwrap_or(name), "generator")?;
            (
                format!("generators/{stem}.cpp"),
                TEMPLATE_GEN_RANDOM_CPP,
                None,
            )
        }
        PackageSourceKind::Validator => ("validator.cpp".to_owned(), TEMPLATE_VALIDATOR_CPP, None),
        PackageSourceKind::CustomChecker => ("checker.cpp".to_owned(), TEMPLATE_CHECKER_CPP, None),
        PackageSourceKind::ValidatorSelfTestValid => {
            let stem = validate_safe_stem(name.strip_suffix(".in").unwrap_or(name), "self-test")?;
            (format!("tests/validator/valid/{stem}.in"), "1 2\n", None)
        }
        PackageSourceKind::ValidatorSelfTestInvalid => {
            let stem = validate_safe_stem(name.strip_suffix(".in").unwrap_or(name), "self-test")?;
            (format!("tests/validator/invalid/{stem}.in"), "-1 2\n", None)
        }
    };

    let abs = prob_dir.join(&rel_in_prob);
    if !abs.exists() {
        let ws_rel = format!("{}/{rel_in_prob}", problem.dir);
        ws.write(&ws_rel, template.as_bytes())?;
    }

    if let Some(stem) = solution_stem {
        let toml_rel = format!("{}/problem.toml", problem.dir);
        let raw_toml = ws.read_text(&toml_rel)?;
        let mut doc: DocumentMut = raw_toml
            .parse()
            .map_err(|e| JudgeError::InvalidConfig(format!("parse {toml_rel}: {e}")))?;

        let sols_tbl = doc
            .entry("solutions")
            .or_insert_with(|| {
                let mut t = Table::new();
                t.set_implicit(true);
                Item::Table(t)
            })
            .as_table_mut()
            .ok_or_else(|| JudgeError::InvalidConfig("[solutions] is not a table".to_owned()))?;

        if sols_tbl.get(&stem).is_none() {
            let mut tbl = Table::new();
            tbl.insert("expect", value("AC"));
            sols_tbl.insert(&stem, Item::Table(tbl));
            ws.write(&toml_rel, doc.to_string().as_bytes())?;
        }
    }

    Ok(rel_in_prob)
}

/// Appends a hand-written test case (`tests/hand-NN.in` + `[[test]]` in `problem.toml`).
pub fn add_hand_test(
    ws: &Workspace,
    slug: &str,
    name: Option<&str>,
    tag: Option<&str>,
    content: &str,
) -> Result<AddedTestInfo> {
    let problem = ws.load_problem(slug)?;
    let prob_dir = ws.resolve(&problem.dir)?;
    let tests_dir = prob_dir.join("tests");

    let mut next_n = 1u32;
    if let Ok(entries) = fs::read_dir(&tests_dir) {
        for e in entries.flatten() {
            if let Some(fname) = e.file_name().to_str()
                && let Some(rest) = fname.strip_prefix("hand-")
                && let Some(num) = rest.strip_suffix(".in")
                && let Ok(n) = num.parse::<u32>()
            {
                next_n = next_n.max(n + 1);
            }
        }
    }

    let file_rel = loop {
        let candidate = format!("tests/hand-{next_n:02}.in");
        if !prob_dir.join(&candidate).exists() {
            break candidate;
        }
        next_n += 1;
    };

    let normalized = normalize_input_bytes(content.as_bytes());
    let ws_rel = format!("{}/{file_rel}", problem.dir);
    ws.write(&ws_rel, &normalized)?;

    let toml_rel = format!("{}/problem.toml", problem.dir);
    let raw_toml = ws.read_text(&toml_rel)?;
    let mut doc: DocumentMut = raw_toml
        .parse()
        .map_err(|e| JudgeError::InvalidConfig(format!("parse {toml_rel}: {e}")))?;

    let arr = doc
        .entry("test")
        .or_insert_with(|| Item::ArrayOfTables(ArrayOfTables::new()))
        .as_array_of_tables_mut()
        .ok_or_else(|| {
            JudgeError::InvalidConfig("[[test]] is not an array of tables".to_owned())
        })?;

    let clean_name = name
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    let clean_tag = tag
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);

    let mut tbl = Table::new();
    tbl.insert("file", value(file_rel.as_str()));
    if let Some(ref t) = clean_tag {
        tbl.insert("tag", value(t.as_str()));
    }
    if let Some(ref n) = clean_name {
        tbl.insert("name", value(n.as_str()));
    }
    arr.push(tbl);
    let rule_index = arr.len() - 1;

    ws.write(&toml_rel, doc.to_string().as_bytes())?;

    Ok(AddedTestInfo {
        rule_index,
        file_rel: Some(file_rel),
        name: clean_name,
        tag: clean_tag,
    })
}

/// Appends a generator test rule (`[[test]]` with `gen`, `args`, `repeat`, optional `tag`/`name`).
pub fn add_generator_test(
    ws: &Workspace,
    slug: &str,
    generator: &str,
    args: &str,
    repeat: u32,
    tag: Option<&str>,
    name: Option<&str>,
) -> Result<AddedTestInfo> {
    let generator = validate_safe_stem(generator, "generator")?;
    if !(1..=200).contains(&repeat) {
        return Err(JudgeError::InvalidConfig(format!(
            "repeat must be between 1 and 200 (got {repeat})"
        )));
    }

    let problem = ws.load_problem(slug)?;
    let toml_rel = format!("{}/problem.toml", problem.dir);
    let raw_toml = ws.read_text(&toml_rel)?;
    let mut doc: DocumentMut = raw_toml
        .parse()
        .map_err(|e| JudgeError::InvalidConfig(format!("parse {toml_rel}: {e}")))?;

    let arr = doc
        .entry("test")
        .or_insert_with(|| Item::ArrayOfTables(ArrayOfTables::new()))
        .as_array_of_tables_mut()
        .ok_or_else(|| {
            JudgeError::InvalidConfig("[[test]] is not an array of tables".to_owned())
        })?;

    let clean_args = args.trim();
    let clean_name = name
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    let clean_tag = tag
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);

    let mut tbl = Table::new();
    tbl.insert("gen", value(generator));
    if !clean_args.is_empty() {
        tbl.insert("args", value(clean_args));
    }
    if repeat > 1 {
        tbl.insert("repeat", value(i64::from(repeat)));
    }
    if let Some(ref t) = clean_tag {
        tbl.insert("tag", value(t.as_str()));
    }
    if let Some(ref n) = clean_name {
        tbl.insert("name", value(n.as_str()));
    }
    arr.push(tbl);
    let rule_index = arr.len() - 1;

    ws.write(&toml_rel, doc.to_string().as_bytes())?;

    Ok(AddedTestInfo {
        rule_index,
        file_rel: None,
        name: clean_name,
        tag: clean_tag,
    })
}

/// Removes the `rule_index`-th `[[test]]` entry from `problem.toml` via `toml_edit`.
/// Does not touch `samples/`.
pub fn remove_test_rule(ws: &Workspace, slug: &str, rule_index: usize) -> Result<()> {
    let problem = ws.load_problem(slug)?;
    let toml_rel = format!("{}/problem.toml", problem.dir);
    let raw_toml = ws.read_text(&toml_rel)?;
    let mut doc: DocumentMut = raw_toml
        .parse()
        .map_err(|e| JudgeError::InvalidConfig(format!("parse {toml_rel}: {e}")))?;

    let arr = doc
        .get_mut("test")
        .and_then(Item::as_array_of_tables_mut)
        .ok_or_else(|| JudgeError::TestNotFound(format!("rule #{rule_index}")))?;

    if rule_index >= arr.len() {
        return Err(JudgeError::TestNotFound(format!("rule #{rule_index}")));
    }

    arr.remove(rule_index);
    if arr.is_empty() {
        doc.remove("test");
    }

    ws.write(&toml_rel, doc.to_string().as_bytes())?;
    Ok(())
}

/// Normalizes dirty hand-written inputs in `samples/` and `tests/` (strips UTF-8 BOM,
/// converts `\r\n`/`\r` to `\n`, ensures trailing `\n` for non-empty files).
/// Returns sorted list of fixed relative paths inside `problems/<slug>/`.
pub fn normalize_dirty_inputs(ws: &Workspace, slug: &str) -> Result<Vec<String>> {
    let problem = ws.load_problem(slug)?;
    let prob_dir = ws.resolve(&problem.dir)?;

    let mut candidates: Vec<PathBuf> = Vec::new();
    for rel_dir in [
        "samples",
        "tests",
        "tests/validator/valid",
        "tests/validator/invalid",
    ] {
        let dir = prob_dir.join(rel_dir);
        if !dir.is_dir() {
            continue;
        }
        if let Ok(entries) = fs::read_dir(&dir) {
            for e in entries.flatten() {
                let p = e.path();
                if p.is_file()
                    && p.extension()
                        .and_then(|s| s.to_str())
                        .is_some_and(|ext| ext == "in")
                {
                    candidates.push(p);
                }
            }
        }
    }
    candidates.sort();

    let mut fixed = Vec::new();
    for path in candidates {
        let raw = fs::read(&path).map_err(|e| io_err(format!("read {}", path.display()), e))?;
        let normalized = normalize_input_bytes(&raw);
        if normalized != raw
            && let Ok(rel) = path.strip_prefix(&prob_dir)
        {
            let rel_str = rel.to_string_lossy().replace('\\', "/");
            let ws_rel = format!("{}/{rel_str}", problem.dir);
            ws.write(&ws_rel, &normalized)?;
            fixed.push(rel_str);
        }
    }
    fixed.sort();
    Ok(fixed)
}

fn normalize_input_bytes(raw: &[u8]) -> Vec<u8> {
    let stripped = raw.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(raw);
    let mut out = Vec::with_capacity(stripped.len() + 1);
    let mut i = 0;
    while i < stripped.len() {
        if stripped[i] == b'\r' {
            if i + 1 < stripped.len() && stripped[i + 1] == b'\n' {
                i += 1;
            }
            out.push(b'\n');
        } else {
            out.push(stripped[i]);
        }
        i += 1;
    }
    if !out.is_empty() && !out.ends_with(b"\n") {
        out.push(b'\n');
    }
    out
}

fn validate_safe_stem<'a>(raw: &'a str, kind: &str) -> Result<&'a str> {
    let s = raw.trim();
    let valid = !s.is_empty()
        && s.len() <= 64
        && !s.contains("..")
        && !s.starts_with('.')
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.');
    if valid {
        Ok(s)
    } else {
        Err(JudgeError::InvalidConfig(format!(
            "invalid {kind} name {raw:?}: use ASCII letters, digits, '-', '_', '.' without path separators"
        )))
    }
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

    let val_valid = list_files(prob_dir.join("tests/validator/valid"));
    let val_invalid = list_files(prob_dir.join("tests/validator/invalid"));
    let chk_dirs = list_dirs(prob_dir.join("tests/checker"));

    let mut self_test_files = Vec::new();
    for f in &val_valid {
        self_test_files.push(format!("tests/validator/valid/{f}"));
    }
    for f in &val_invalid {
        self_test_files.push(format!("tests/validator/invalid/{f}"));
    }
    for d in &chk_dirs {
        for f in list_files(prob_dir.join("tests/checker").join(d)) {
            self_test_files.push(format!("tests/checker/{d}/{f}"));
        }
    }

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
        validator_self_tests_valid: val_valid,
        validator_self_tests_invalid: val_invalid,
        checker_self_tests: chk_dirs,
        self_test_files,
    }
}

const TEMPLATE_CHECKER_CPP: &str = r#"#include "testlib.h"

int main(int argc, char* argv[]) {
    registerTestlibCmd(argc, argv);
    while (!ans.seekEof() && !ouf.seekEof()) {
        std::string ja = ans.readToken();
        std::string pa = ouf.readToken();
        if (ja != pa) {
            quitf(_wa, "expected '%s', found '%s'", ja.c_str(), pa.c_str());
        }
    }
    if (!ans.seekEof()) quitf(_wa, "participant output is shorter than expected");
    if (!ouf.seekEof()) quitf(_wa, "participant output has extra tokens");
    quitf(_ok, "ok");
}
"#;

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

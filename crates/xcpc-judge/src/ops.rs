//! High-level package operations reused by CLI, MCP, and Desktop GUI:
//! - `inspect_package`
//! - `inspect_test_case`
//! - `init_package`
//! - `stress_package`

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use toml_edit::{Array, ArrayOfTables, DocumentMut, InlineTable, Item, Table, Value, value};
use xcpc_domain::{ExpectVerdict, ScoreExpect, SubtaskScoring, SubtaskSpec};
use xcpc_storage::Workspace;

use crate::assets::{is_standard_checker, resolve_testlib, standard_checker_source};
use crate::cache::JudgeCache;
use crate::error::{JudgeError, Result, io_err};
use crate::package::{
    CheckerSpec, ExpandedTest, PackageFreshness, TestSource, build_dir_for, check_dirty_inputs,
    check_package_freshness, compute_package_hash, discover_solutions, expand_tests,
    find_generator_source, find_interactor_source, find_validator_source, has_explicit_checker,
    load_latest_report, resolve_checker_spec,
};
use crate::report::{
    BuildReport, DirtyInputIssue, GateResult, InteractionTranscript, SubtaskSummary,
};
use crate::runner::{
    RunLimits, Verdict, run_checker_with_score, run_multi_run_interactive, run_multi_run_process,
    run_process, run_validator,
};
use crate::subtask::validate_subtask_config;
use crate::toolchain::{CppStd, SourceLanguage, compile_cached, resolve_active_compiler};

fn default_runs() -> u32 {
    1
}

/// Inventory of package files inside a problem directory.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageFiles {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub validator: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interactor: Option<String>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score: Option<ScoreExpect>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub subtasks: BTreeMap<String, ExpectVerdict>,
}

/// Read-only overview of a problem package (`inspect_package`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PackageInspection {
    pub problem_id: String,
    pub has_package: bool,
    pub package_hash: String,
    pub freshness: PackageFreshness,
    pub checker: String,
    #[serde(default)]
    pub interactive: bool,
    #[serde(default = "default_runs")]
    pub runs: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub subtasks: Vec<SubtaskSummary>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_score: Option<u32>,
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
    /// Exact total line count when computed (`None` only for very huge files > 16 MiB).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_lines_exact: Option<usize>,
    /// Approximate number of bytes omitted from the preview when `truncated == true`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub omitted_bytes: Option<u64>,
    pub head: String,
    pub tail: String,
    pub truncated: bool,
    #[serde(default)]
    pub ans_available: bool,
    /// Origin of `ans_*`: `"manual_sample"` or `"std"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ans_source: Option<String>,
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
    /// Built `std` output preview (`build/problems/<slug>/tests/{:03}.ans`) when available,
    /// kept distinct from manual `samples/NN.ans` so UI never mislabels manual `.ans` as `std`.
    #[serde(default)]
    pub std_output_available: bool,
    #[serde(default)]
    pub std_output_size_bytes: u64,
    #[serde(default)]
    pub std_output_head: String,
    #[serde(default)]
    pub std_output_tail: String,
    #[serde(default)]
    pub std_output_truncated: bool,
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
    Interactor,
    CustomChecker,
    ValidatorSelfTestValid,
    ValidatorSelfTestInvalid,
}

/// Options for scaffolding a problem package (`init_package_with_options`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct InitPackageOptions {
    /// Generate `interactor.cpp` (with flush/deadlock best practices) and interactive `std.cpp`.
    #[serde(default)]
    pub interactive: bool,
    /// Optional multi-run pass count (`>= 2` writes `[package].runs = N`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runs: Option<u32>,
    /// Generate `[[subtask]]` scaffold in `problem.toml` and assign tests to subtasks.
    #[serde(default)]
    pub subtasks: bool,
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
    /// Truncated interactive transcript when stress testing an interactive problem.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transcript: Option<InteractionTranscript>,
}

/// Pure read-only inspection of a problem package. Never compiles or executes anything.
pub fn inspect_package(ws: &Workspace, slug: &str) -> Result<PackageInspection> {
    let problem = ws.load_problem(slug)?;
    let prob_dir = ws.resolve(&problem.dir)?;
    let has_pkg = problem.meta.has_package_in_dir(&prob_dir);
    let package_hash = compute_package_hash(ws, slug)?;
    let freshness = check_package_freshness(ws, slug)?;
    let is_interactive = find_interactor_source(&prob_dir).is_some();
    let runs = problem.meta.run_count();
    let use_checker = !is_interactive || has_explicit_checker(&prob_dir, &problem.meta);
    let checker_spec = resolve_checker_spec(&prob_dir, &problem.meta)?;
    let checker_display = if use_checker {
        checker_spec.display_name().to_owned()
    } else {
        "interactor".to_owned()
    };
    let expanded_tests = expand_tests(&prob_dir, &problem.meta)?;
    let subtask_val = validate_subtask_config(&problem.meta.subtask, &expanded_tests);
    let has_subtasks = !problem.meta.subtask.is_empty();
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
                score: s.score,
                subtasks: s.subtasks,
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
                    score: spec.score,
                    subtasks: spec.subtasks.clone(),
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
        checker: checker_display,
        interactive: is_interactive,
        runs,
        subtasks: subtask_val.summaries,
        total_score: has_subtasks.then_some(subtask_val.total_score),
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

    let built_ans_path = build_dir_for(ws, slug)
        .join("tests")
        .join(format!("{:03}.ans", test.index));

    let (disk_path, file_rel, manual_ans_path) = match &test.source {
        TestSource::Sample {
            input_rel,
            manual_ans_rel,
            ..
        } => {
            let manual = manual_ans_rel
                .as_ref()
                .map(|r| prob_dir.join(r))
                .filter(|p| p.is_file());
            (prob_dir.join(input_rel), Some(input_rel.clone()), manual)
        }
        TestSource::File { file_rel } => (prob_dir.join(file_rel), Some(file_rel.clone()), None),
        TestSource::Generated { .. } => (
            build_dir_for(ws, slug)
                .join("tests")
                .join(format!("{:03}.in", test.index)),
            None,
            None,
        ),
    };

    // Preview built `std` output separately when available.
    let std_preview = if built_ans_path.is_file() {
        Some(read_bounded_preview(
            &built_ans_path,
            head_lines,
            tail_lines,
        )?)
    } else {
        None
    };

    // For `ans_*`: prefer manual sample `.ans` if present (labeled `"manual_sample"`),
    // otherwise use built `std` `.ans` (labeled `"std"`).
    let (ans_source, ans_preview) = if let Some(ref mp) = manual_ans_path {
        (
            Some("manual_sample".to_owned()),
            Some(read_bounded_preview(mp, head_lines, tail_lines)?),
        )
    } else if let Some(ref sp) = std_preview {
        (Some("std".to_owned()), Some(sp.clone()))
    } else {
        (None, None)
    };

    let (ans_available, ans_size_bytes, ans_head, ans_tail, ans_truncated, ans_total_lines) =
        if let Some(p) = ans_preview {
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

    let (
        std_output_available,
        std_output_size_bytes,
        std_output_head,
        std_output_tail,
        std_output_truncated,
    ) = if let Some(p) = std_preview {
        (true, p.size_bytes, p.head, p.tail, p.truncated)
    } else {
        (false, 0, String::new(), String::new(), false)
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
            total_lines_exact: Some(0),
            omitted_bytes: None,
            head: String::new(),
            tail: String::new(),
            truncated: false,
            ans_available,
            ans_source,
            ans_size_bytes,
            ans_head,
            ans_tail,
            ans_truncated,
            ans_total_lines,
            std_output_available,
            std_output_size_bytes,
            std_output_head,
            std_output_tail,
            std_output_truncated,
            rule_index: test.rule_index,
            file_rel,
        });
    }

    let preview = read_bounded_preview(&disk_path, head_lines, tail_lines)?;
    let omitted_bytes = preview.truncated.then(|| {
        let shown = (preview.head.len() + preview.tail.len()) as u64;
        preview.size_bytes.saturating_sub(shown)
    });

    Ok(TestCasePreview {
        index: test.index,
        name: test.name.clone(),
        tag: test.tag.clone(),
        is_sample: test.is_sample,
        gen_cmd: test.gen_cmd.clone(),
        available_on_disk: true,
        size_bytes: preview.size_bytes,
        total_lines: preview.exact_total_lines.unwrap_or(0),
        total_lines_exact: preview.exact_total_lines,
        omitted_bytes,
        head: preview.head,
        tail: preview.tail,
        truncated: preview.truncated,
        ans_available,
        ans_source,
        ans_size_bytes,
        ans_head,
        ans_tail,
        ans_truncated,
        ans_total_lines,
        std_output_available,
        std_output_size_bytes,
        std_output_head,
        std_output_tail,
        std_output_truncated,
        rule_index: test.rule_index,
        file_rel,
    })
}

/// Scaffolds a Polygon package with `testlib.h` best practices without overwriting existing files.
pub fn init_package(ws: &Workspace, slug: &str) -> Result<InitPackageResult> {
    init_package_with_options(ws, slug, &InitPackageOptions::default())
}

/// Scaffolds a Polygon package with optional Phase 3C features (`interactive`, `runs`, `subtasks`)
/// without overwriting existing files.
pub fn init_package_with_options(
    ws: &Workspace,
    slug: &str,
    opts: &InitPackageOptions,
) -> Result<InitPackageResult> {
    if let Some(r) = opts.runs
        && r == 0
    {
        return Err(JudgeError::InvalidConfig("runs must be >= 1".to_owned()));
    }

    let problem = ws.load_problem(slug)?;
    let prob_dir = ws.resolve(&problem.dir)?;

    let mut scaffold_files: Vec<(&str, &str)> = vec![
        ("validator.cpp", TEMPLATE_VALIDATOR_CPP),
        ("generators/gen_random.cpp", TEMPLATE_GEN_RANDOM_CPP),
        (
            "solutions/std.cpp",
            if opts.interactive {
                TEMPLATE_INTERACTIVE_STD_CPP
            } else {
                TEMPLATE_STD_CPP
            },
        ),
        ("tests/validator/valid/01.in", "1 2\n"),
        ("tests/validator/invalid/01.in", "-1 2\n"),
    ];
    if opts.interactive {
        scaffold_files.push(("interactor.cpp", TEMPLATE_INTERACTOR_CPP));
    }

    let mut created_files = Vec::new();
    let mut skipped_files = Vec::new();

    for (rel_in_prob, content) in scaffold_files {
        let abs = prob_dir.join(rel_in_prob);
        if abs.exists() {
            skipped_files.push(rel_in_prob.to_owned());
        } else {
            let ws_rel = format!("{}/{rel_in_prob}", problem.dir);
            ws.write(&ws_rel, content.as_bytes())?;
            created_files.push(rel_in_prob.to_owned());
        }
    }

    // Update `problem.toml` via `toml_edit` preserving comments & unknown fields.
    let toml_rel = format!("{}/problem.toml", problem.dir);
    let raw_toml = ws.read_text(&toml_rel)?;
    let mut doc: DocumentMut = raw_toml
        .parse()
        .map_err(|e| JudgeError::InvalidConfig(format!("parse {toml_rel}: {e}")))?;

    let mut updated_toml = false;

    if let Some(runs) = opts.runs
        && runs >= 2
    {
        let pkg_tbl = doc
            .entry("package")
            .or_insert_with(|| Item::Table(Table::new()))
            .as_table_mut()
            .ok_or_else(|| JudgeError::InvalidConfig("[package] is not a table".to_owned()))?;
        if pkg_tbl.get("runs").is_none() {
            pkg_tbl.insert("runs", value(i64::from(runs)));
            updated_toml = true;
        }
    }

    if opts.subtasks && doc.get("subtask").is_none() {
        let mut arr = ArrayOfTables::new();
        let mut s1 = Table::new();
        s1.insert("id", value("1"));
        s1.insert("score", value(30));
        s1.insert("scoring", value("min"));
        arr.push(s1);

        let mut s2 = Table::new();
        s2.insert("id", value("2"));
        s2.insert("score", value(70));
        let mut deps = Array::new();
        deps.push("1");
        s2.insert("depends", value(deps));
        s2.insert("scoring", value("min"));
        arr.push(s2);

        doc.insert("subtask", Item::ArrayOfTables(arr));
        updated_toml = true;
    }

    if doc.get("test").is_none() {
        let mut arr = ArrayOfTables::new();
        let mut t1 = Table::new();
        t1.insert("gen", value("gen_random"));
        t1.insert("args", value("-n 10 {i}"));
        t1.insert("repeat", value(3));
        t1.insert("tag", value("small"));
        if opts.subtasks {
            t1.insert("subtask", value("1"));
        }
        arr.push(t1);

        if opts.subtasks {
            let mut t2 = Table::new();
            t2.insert("gen", value("gen_random"));
            t2.insert("args", value("-n 1000 {i}"));
            t2.insert("repeat", value(5));
            t2.insert("tag", value("max"));
            t2.insert("subtask", value("2"));
            arr.push(t2);
        }

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

    let inter_cr = if let Some((ip, irel, ilang)) = find_interactor_source(&prob_dir) {
        let ibytes = fs::read(&ip).map_err(|e| io_err(format!("read {}", ip.display()), e))?;
        let cr = compile_cached(
            &cache,
            &compiler,
            ilang,
            "interactor",
            &irel,
            &ibytes,
            Some(&ip),
            &testlib,
        )
        .map_err(|f| {
            JudgeError::InvalidConfig(format!("interactor compile failed:\n{}", f.stderr))
        })?;
        Some(cr)
    } else {
        None
    };

    let runs = problem.meta.run_count();
    let use_checker = inter_cr.is_none() || has_explicit_checker(&prob_dir, &problem.meta);
    let checker_spec = resolve_checker_spec(&prob_dir, &problem.meta)?;
    let checker_cr = if use_checker {
        let cr = match &checker_spec {
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
        Some(cr)
    } else {
        None
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
                transcript: None,
            });
        }

        if let Some(ref ic) = inter_cr {
            // Interactive stress: run std with interactor first.
            let std_inter = run_multi_run_interactive(
                &ic.binary_path,
                &main_cr.binary_path,
                &in_path,
                &ans_path,
                runs,
                limits,
                cancel.as_ref(),
            )?;
            let mut std_verdict = std_inter.verdict;
            let mut std_msg = std_inter.checker_message;
            if std_verdict == Verdict::AC
                && let Some(ref chk_cr) = checker_cr
            {
                let res = run_checker_with_score(
                    &chk_cr.binary_path,
                    &in_path,
                    &ans_path,
                    &ans_path,
                    cancel.as_ref(),
                )?;
                std_verdict = res.verdict;
                std_msg = res.message;
            }
            if std_verdict != Verdict::AC {
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
                    verdict: Some(std_verdict),
                    input_preview: Some(read_truncated_preview(&in_path, 4096)),
                    std_output_preview: Some(read_truncated_preview(&ans_path, 2048)),
                    solution_output_preview: None,
                    checker_message: Some(std_msg),
                    transcript: Some(std_inter.transcript),
                });
            }

            // Run each target solution against the interactor (+ optional checker).
            for (sol, cr) in &target_crs {
                let sol_inter = run_multi_run_interactive(
                    &ic.binary_path,
                    &cr.binary_path,
                    &in_path,
                    &out_path,
                    runs,
                    limits,
                    cancel.as_ref(),
                )?;
                let mut verdict = sol_inter.verdict;
                let mut msg = sol_inter.checker_message;
                if verdict == Verdict::AC
                    && let Some(ref chk_cr) = checker_cr
                {
                    let res = run_checker_with_score(
                        &chk_cr.binary_path,
                        &in_path,
                        &out_path,
                        &ans_path,
                        cancel.as_ref(),
                    )?;
                    verdict = res.verdict;
                    msg = res.message;
                }
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
                        transcript: Some(sol_inter.transcript),
                    });
                }
            }
        } else {
            let chk_cr = checker_cr
                .as_ref()
                .expect("non-interactive stress requires checker");
            // Run std.
            let std_outcome = run_multi_run_process(
                &main_cr.binary_path,
                &in_path,
                &ans_path,
                runs,
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
                    transcript: None,
                });
            }

            // Run each target solution and compare with checker.
            for (sol, cr) in &target_crs {
                let sol_outcome = run_multi_run_process(
                    &cr.binary_path,
                    &in_path,
                    &out_path,
                    runs,
                    limits,
                    cancel.as_ref(),
                )?;
                let (verdict, msg) = if sol_outcome.verdict == Verdict::AC {
                    let res = run_checker_with_score(
                        &chk_cr.binary_path,
                        &in_path,
                        &out_path,
                        &ans_path,
                        cancel.as_ref(),
                    )?;
                    (res.verdict, res.message)
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
                        transcript: None,
                    });
                }
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
        transcript: None,
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
const LINE_COUNT_LIMIT: u64 = 16 * 1024 * 1024;
const MAX_LINE_CHARS: usize = 200;
const MAX_PREVIEW_BYTES: usize = 4096;

#[derive(Debug, Clone)]
struct BoundedPreview {
    size_bytes: u64,
    exact_total_lines: Option<usize>,
    head: String,
    tail: String,
    truncated: bool,
}

fn count_lines_streaming(path: &Path) -> Option<usize> {
    let mut f = File::open(path).ok()?;
    let mut buf = [0u8; 64 * 1024];
    let mut newlines = 0usize;
    let mut total_bytes = 0u64;
    let mut last_byte = 0u8;
    loop {
        let n = f.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        total_bytes += n as u64;
        last_byte = buf[n - 1];
        newlines += buf[..n].iter().filter(|&&b| b == b'\n').count();
    }
    if total_bytes == 0 {
        Some(0)
    } else if last_byte == b'\n' {
        Some(newlines)
    } else {
        Some(newlines + 1)
    }
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
    let head = if h_lines.is_empty() {
        String::new()
    } else if h_lines.len() == 1 && !head_buf.contains(&b'\n') {
        // Single line exceeding the 64 KiB head window (e.g. 200k-char single line without \n).
        let prefix: String = h_lines[0].chars().take(MAX_LINE_CHARS).collect();
        let extra_bytes = size_bytes.saturating_sub(prefix.len() as u64);
        format!("{prefix}… (+{extra_bytes} chars)")
    } else {
        let head_slice = &h_lines[..head_lines.min(h_lines.len())];
        let (h, _) = format_bounded_lines(head_slice, MAX_PREVIEW_BYTES);
        h
    };

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

    let exact_total_lines = if size_bytes <= LINE_COUNT_LIMIT {
        count_lines_streaming(path)
    } else {
        None
    };

    Ok(BoundedPreview {
        size_bytes,
        exact_total_lines,
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
        PackageSourceKind::Interactor => {
            ("interactor.cpp".to_owned(), TEMPLATE_INTERACTOR_CPP, None)
        }
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

/// Updates the `rule_index`-th `[[test]]` entry in `problem.toml` in-place via `toml_edit`
/// (preserving comments, order, and unknown fields).
#[allow(clippy::too_many_arguments)]
pub fn update_generator_test(
    ws: &Workspace,
    slug: &str,
    rule_index: usize,
    generator: &str,
    args: &str,
    repeat: u32,
    tag: Option<&str>,
    name: Option<&str>,
) -> Result<()> {
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
        .get_mut("test")
        .and_then(Item::as_array_of_tables_mut)
        .ok_or_else(|| JudgeError::TestNotFound(format!("rule #{rule_index}")))?;

    let tbl = arr
        .get_mut(rule_index)
        .ok_or_else(|| JudgeError::TestNotFound(format!("rule #{rule_index}")))?;

    let clean_args = args.trim();
    let clean_tag = tag.map(str::trim).filter(|s| !s.is_empty());
    let clean_name = name.map(str::trim).filter(|s| !s.is_empty());

    tbl.remove("file");
    tbl.insert("gen", value(generator));
    if clean_args.is_empty() {
        tbl.remove("args");
    } else {
        tbl.insert("args", value(clean_args));
    }
    if repeat > 1 {
        tbl.insert("repeat", value(i64::from(repeat)));
    } else {
        tbl.remove("repeat");
    }
    if let Some(t) = clean_tag {
        tbl.insert("tag", value(t));
    } else {
        tbl.remove("tag");
    }
    if let Some(n) = clean_name {
        tbl.insert("name", value(n));
    } else {
        tbl.remove("name");
    }

    ws.write(&toml_rel, doc.to_string().as_bytes())?;
    Ok(())
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

/// Deletes a solution's source file under `problems/<slug>/solutions/` (if present) and
/// removes `[solutions.<name>]` from `problem.toml` via `toml_edit`.
pub fn delete_solution(ws: &Workspace, slug: &str, name: &str) -> Result<()> {
    if ws.is_read_only() {
        return Err(JudgeError::Storage(xcpc_storage::Error::ReadOnly));
    }
    let clean = validate_safe_stem(name, "solution")?;
    let stem = SourceLanguage::logical_stem(Path::new(clean)).unwrap_or_else(|| clean.to_owned());

    let problem = ws.load_problem(slug)?;
    let prob_dir = ws.resolve(&problem.dir)?;
    let sol_dir = prob_dir.join("solutions");

    if sol_dir.is_dir() {
        let canon_sol_dir = fs::canonicalize(&sol_dir)
            .map_err(|e| io_err(format!("canonicalize {}", sol_dir.display()), e))?;
        let entries: Vec<PathBuf> = fs::read_dir(&sol_dir)
            .map_err(|e| io_err(format!("read {}", sol_dir.display()), e))?
            .flatten()
            .map(|e| e.path())
            .collect();

        for path in entries {
            let Ok(meta) = fs::symlink_metadata(&path) else {
                continue;
            };
            if !meta.file_type().is_file() {
                continue;
            }
            let fname = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
            let matches = fname == clean
                || SourceLanguage::logical_stem(&path).as_deref() == Some(stem.as_str());
            if matches {
                let canon = fs::canonicalize(&path)
                    .map_err(|e| io_err(format!("canonicalize {}", path.display()), e))?;
                if !canon.starts_with(&canon_sol_dir) {
                    return Err(JudgeError::Storage(xcpc_storage::Error::PathEscape(
                        path.display().to_string(),
                    )));
                }
                fs::remove_file(&canon)
                    .map_err(|e| io_err(format!("remove {}", canon.display()), e))?;
            }
        }
    }

    let toml_rel = format!("{}/problem.toml", problem.dir);
    let raw_toml = ws.read_text(&toml_rel)?;
    let mut doc: DocumentMut = raw_toml
        .parse()
        .map_err(|e| JudgeError::InvalidConfig(format!("parse {toml_rel}: {e}")))?;

    let mut changed = false;
    if let Some(sols_tbl) = doc.get_mut("solutions").and_then(Item::as_table_mut) {
        if sols_tbl.remove(&stem).is_some() {
            changed = true;
        }
        if sols_tbl.is_empty() {
            doc.remove("solutions");
            changed = true;
        }
    }

    if changed {
        ws.write(&toml_rel, doc.to_string().as_bytes())?;
    }
    Ok(())
}

/// Deletes `problems/<slug>/generators/<filename>`.
///
/// When `remove_referencing_tests` is `true`, also removes all `[[test]]` entries in
/// `problem.toml` whose `gen` matches the generator's logical stem, returning the count
/// of removed test rules.
pub fn delete_generator(
    ws: &Workspace,
    slug: &str,
    filename: &str,
    remove_referencing_tests: bool,
) -> Result<usize> {
    if ws.is_read_only() {
        return Err(JudgeError::Storage(xcpc_storage::Error::ReadOnly));
    }
    let clean = validate_safe_stem(filename, "generator")?;
    let problem = ws.load_problem(slug)?;
    let prob_dir = ws.resolve(&problem.dir)?;
    let gen_dir = prob_dir.join("generators");

    if !gen_dir.is_dir() {
        return Err(JudgeError::GeneratorNotFound(clean.to_owned()));
    }
    let target = gen_dir.join(clean);
    let meta = fs::symlink_metadata(&target)
        .map_err(|_| JudgeError::GeneratorNotFound(clean.to_owned()))?;
    if !meta.file_type().is_file() {
        return Err(JudgeError::GeneratorNotFound(clean.to_owned()));
    }
    let canon_gen_dir = fs::canonicalize(&gen_dir)
        .map_err(|e| io_err(format!("canonicalize {}", gen_dir.display()), e))?;
    let canon_target = fs::canonicalize(&target)
        .map_err(|e| io_err(format!("canonicalize {}", target.display()), e))?;
    if !canon_target.starts_with(&canon_gen_dir) {
        return Err(JudgeError::Storage(xcpc_storage::Error::PathEscape(
            clean.to_owned(),
        )));
    }
    fs::remove_file(&canon_target)
        .map_err(|e| io_err(format!("remove {}", canon_target.display()), e))?;

    if !remove_referencing_tests {
        return Ok(0);
    }

    let stem = SourceLanguage::logical_stem(Path::new(clean)).unwrap_or_else(|| clean.to_owned());
    let toml_rel = format!("{}/problem.toml", problem.dir);
    let raw_toml = ws.read_text(&toml_rel)?;
    let mut doc: DocumentMut = raw_toml
        .parse()
        .map_err(|e| JudgeError::InvalidConfig(format!("parse {toml_rel}: {e}")))?;

    let mut removed = 0usize;
    if let Some(arr) = doc.get_mut("test").and_then(Item::as_array_of_tables_mut) {
        let mut to_remove = Vec::new();
        for (idx, tbl) in arr.iter().enumerate() {
            if let Some(g) = tbl.get("gen").and_then(|v| v.as_str())
                && (g == stem || g == clean)
            {
                to_remove.push(idx);
            }
        }
        removed = to_remove.len();
        for idx in to_remove.into_iter().rev() {
            arr.remove(idx);
        }
        if arr.is_empty() {
            doc.remove("test");
        }
    }

    if removed > 0 {
        ws.write(&toml_rel, doc.to_string().as_bytes())?;
    }
    Ok(removed)
}

/// Deletes a validator or checker self-test file strictly inside
/// `problems/<slug>/tests/validator/` or `problems/<slug>/tests/checker/`.
/// If deleting under `tests/checker/<case>/` leaves the case directory empty, removes it too.
pub fn delete_self_test(ws: &Workspace, slug: &str, rel_path: &str) -> Result<()> {
    if ws.is_read_only() {
        return Err(JudgeError::Storage(xcpc_storage::Error::ReadOnly));
    }
    let clean = rel_path.trim().replace('\\', "/");
    let is_checker = clean.starts_with("tests/checker/");
    let is_validator = clean.starts_with("tests/validator/");
    if (!is_checker && !is_validator)
        || clean.starts_with('/')
        || Path::new(&clean).components().any(|c| {
            matches!(
                c,
                std::path::Component::ParentDir | std::path::Component::RootDir
            )
        })
    {
        return Err(JudgeError::Storage(xcpc_storage::Error::PathEscape(
            rel_path.to_owned(),
        )));
    }

    let problem = ws.load_problem(slug)?;
    let prob_dir = ws.resolve(&problem.dir)?;
    let allowed_root = if is_checker {
        prob_dir.join("tests/checker")
    } else {
        prob_dir.join("tests/validator")
    };
    if !allowed_root.is_dir() {
        return Err(JudgeError::Storage(xcpc_storage::Error::NotFound(
            rel_path.to_owned(),
        )));
    }

    let target = prob_dir.join(&clean);
    let meta = fs::symlink_metadata(&target)
        .map_err(|_| JudgeError::Storage(xcpc_storage::Error::NotFound(rel_path.to_owned())))?;
    if !meta.file_type().is_file() {
        return Err(JudgeError::Storage(xcpc_storage::Error::PathEscape(
            rel_path.to_owned(),
        )));
    }

    let canon_root = fs::canonicalize(&allowed_root)
        .map_err(|e| io_err(format!("canonicalize {}", allowed_root.display()), e))?;
    let canon_target = fs::canonicalize(&target)
        .map_err(|e| io_err(format!("canonicalize {}", target.display()), e))?;
    if !canon_target.starts_with(&canon_root) {
        return Err(JudgeError::Storage(xcpc_storage::Error::PathEscape(
            rel_path.to_owned(),
        )));
    }

    fs::remove_file(&canon_target)
        .map_err(|e| io_err(format!("remove {}", canon_target.display()), e))?;

    if is_checker
        && let Some(parent) = canon_target.parent()
        && parent != canon_root
        && parent.starts_with(&canon_root)
        && fs::read_dir(parent)
            .map(|mut it| it.next().is_none())
            .unwrap_or(false)
    {
        let _ = fs::remove_dir(parent);
    }

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

    // Validator self-tests: top level (no `--group`) plus one `<subtask_id>/` level
    // (run with `--group <subtask_id>`), listed as `name.in` and `<subtask_id>/name.in`.
    let list_self_tests = |dir: PathBuf| -> Vec<String> {
        let mut out = list_files(dir.clone());
        for sub in list_dirs(dir.clone()) {
            for f in list_files(dir.join(&sub)) {
                out.push(format!("{sub}/{f}"));
            }
        }
        out
    };
    let val_valid = list_self_tests(prob_dir.join("tests/validator/valid"));
    let val_invalid = list_self_tests(prob_dir.join("tests/validator/invalid"));
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
        interactor: find_interactor_source(prob_dir).map(|(_, rel, _)| rel),
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

/// Deletes `interactor.cpp` (or `interactor.{17,20,23}.cpp`) from `problems/<slug>/`.
pub fn delete_interactor(ws: &Workspace, slug: &str) -> Result<bool> {
    if ws.is_read_only() {
        return Err(JudgeError::Storage(xcpc_storage::Error::ReadOnly));
    }
    let problem = ws.load_problem(slug)?;
    let prob_dir = ws.resolve(&problem.dir)?;
    let canon_prob = fs::canonicalize(&prob_dir)
        .map_err(|e| io_err(format!("canonicalize {}", prob_dir.display()), e))?;

    let mut removed = false;
    for name in [
        "interactor.cpp",
        "interactor.20.cpp",
        "interactor.23.cpp",
        "interactor.17.cpp",
    ] {
        let p = prob_dir.join(name);
        if p.is_file() {
            let canon = fs::canonicalize(&p)
                .map_err(|e| io_err(format!("canonicalize {}", p.display()), e))?;
            if !canon.starts_with(&canon_prob) {
                return Err(JudgeError::Storage(xcpc_storage::Error::PathEscape(
                    name.to_owned(),
                )));
            }
            fs::remove_file(&canon)
                .map_err(|e| io_err(format!("remove {}", canon.display()), e))?;
            removed = true;
        }
    }
    Ok(removed)
}

/// Updates `[package].runs` in `problem.toml` via `toml_edit`.
/// `None` or `Some(1)` removes the key (returning to single-run default).
pub fn set_package_runs(ws: &Workspace, slug: &str, runs: Option<u32>) -> Result<()> {
    if let Some(0) = runs {
        return Err(JudgeError::InvalidConfig("runs must be >= 1".to_owned()));
    }
    let problem = ws.load_problem(slug)?;
    let toml_rel = format!("{}/problem.toml", problem.dir);
    let raw_toml = ws.read_text(&toml_rel)?;
    let mut doc: DocumentMut = raw_toml
        .parse()
        .map_err(|e| JudgeError::InvalidConfig(format!("parse {toml_rel}: {e}")))?;

    match runs {
        Some(n) if n >= 2 => {
            let pkg_tbl = doc
                .entry("package")
                .or_insert_with(|| Item::Table(Table::new()))
                .as_table_mut()
                .ok_or_else(|| JudgeError::InvalidConfig("[package] is not a table".to_owned()))?;
            pkg_tbl.insert("runs", value(i64::from(n)));
        }
        _ => {
            if let Some(pkg_tbl) = doc.get_mut("package").and_then(Item::as_table_mut) {
                pkg_tbl.remove("runs");
                if pkg_tbl.is_empty() {
                    doc.remove("package");
                }
            }
        }
    }

    ws.write(&toml_rel, doc.to_string().as_bytes())?;
    Ok(())
}

/// Adds or updates a `[[subtask]]` entry in `problem.toml` (matched by `spec.id`) via `toml_edit`.
pub fn upsert_subtask(ws: &Workspace, slug: &str, spec: &SubtaskSpec) -> Result<()> {
    let id = validate_safe_stem(&spec.id, "subtask")?;
    let problem = ws.load_problem(slug)?;
    let toml_rel = format!("{}/problem.toml", problem.dir);
    let raw_toml = ws.read_text(&toml_rel)?;
    let mut doc: DocumentMut = raw_toml
        .parse()
        .map_err(|e| JudgeError::InvalidConfig(format!("parse {toml_rel}: {e}")))?;

    let arr = doc
        .entry("subtask")
        .or_insert_with(|| Item::ArrayOfTables(ArrayOfTables::new()))
        .as_array_of_tables_mut()
        .ok_or_else(|| {
            JudgeError::InvalidConfig("[[subtask]] is not an array of tables".to_owned())
        })?;

    let fill_table = |tbl: &mut Table| {
        tbl.insert("id", value(id));
        tbl.insert("score", value(i64::from(spec.score)));
        if spec.depends.is_empty() {
            tbl.remove("depends");
        } else {
            let mut deps = Array::new();
            for d in &spec.depends {
                let d = d.trim();
                if !d.is_empty() {
                    deps.push(d);
                }
            }
            if deps.is_empty() {
                tbl.remove("depends");
            } else {
                tbl.insert("depends", value(deps));
            }
        }
        if spec.scoring == SubtaskScoring::Min {
            tbl.insert("scoring", value("min"));
        } else {
            tbl.insert("scoring", value("sum"));
        }
    };

    let existing_idx = arr
        .iter()
        .position(|t| t.get("id").and_then(|v| v.as_str()) == Some(id));
    if let Some(idx) = existing_idx {
        if let Some(tbl) = arr.get_mut(idx) {
            fill_table(tbl);
        }
    } else {
        let mut tbl = Table::new();
        fill_table(&mut tbl);
        arr.push(tbl);
    }

    ws.write(&toml_rel, doc.to_string().as_bytes())?;
    Ok(())
}

/// Deletes a `[[subtask]]` entry by `subtask_id` via `toml_edit`.
/// When `clear_references` is `true`, also removes references from `[[test]].subtask`,
/// other subtasks' `depends`, and `[solutions.*].subtasks`.
pub fn delete_subtask(
    ws: &Workspace,
    slug: &str,
    subtask_id: &str,
    clear_references: bool,
) -> Result<bool> {
    let id = subtask_id.trim();
    if id.is_empty() {
        return Err(JudgeError::InvalidConfig(
            "subtask id cannot be empty".to_owned(),
        ));
    }
    let problem = ws.load_problem(slug)?;
    let toml_rel = format!("{}/problem.toml", problem.dir);
    let raw_toml = ws.read_text(&toml_rel)?;
    let mut doc: DocumentMut = raw_toml
        .parse()
        .map_err(|e| JudgeError::InvalidConfig(format!("parse {toml_rel}: {e}")))?;

    let mut removed = false;
    if let Some(arr) = doc
        .get_mut("subtask")
        .and_then(Item::as_array_of_tables_mut)
    {
        let pos = arr
            .iter()
            .position(|t| t.get("id").and_then(|v| v.as_str()) == Some(id));
        if let Some(pos) = pos {
            arr.remove(pos);
            removed = true;
        }
        if clear_references {
            for tbl in arr.iter_mut() {
                if let Some(deps) = tbl.get_mut("depends").and_then(|v| v.as_array_mut()) {
                    deps.retain(|v| v.as_str() != Some(id));
                    if deps.is_empty() {
                        tbl.remove("depends");
                    }
                }
            }
        }
        if arr.is_empty() {
            doc.remove("subtask");
        }
    }

    if clear_references {
        if let Some(tests) = doc.get_mut("test").and_then(Item::as_array_of_tables_mut) {
            for tbl in tests.iter_mut() {
                if tbl.get("subtask").and_then(|v| v.as_str()) == Some(id) {
                    tbl.remove("subtask");
                }
            }
        }
        if let Some(sols) = doc.get_mut("solutions").and_then(Item::as_table_mut) {
            for (_, v) in sols.iter_mut() {
                if let Some(tbl) = v.as_table_mut()
                    && let Some(sub_item) = tbl.get_mut("subtasks")
                {
                    if let Some(inline) = sub_item.as_inline_table_mut() {
                        inline.remove(id);
                        if inline.is_empty() {
                            tbl.remove("subtasks");
                        }
                    } else if let Some(sub_tbl) = sub_item.as_table_mut() {
                        sub_tbl.remove(id);
                        if sub_tbl.is_empty() {
                            tbl.remove("subtasks");
                        }
                    }
                }
            }
        }
    }

    if removed || clear_references {
        ws.write(&toml_rel, doc.to_string().as_bytes())?;
    }
    Ok(removed)
}

/// Sets or clears `[[test]].subtask` on the `rule_index`-th test rule in `problem.toml`.
pub fn set_test_subtask(
    ws: &Workspace,
    slug: &str,
    rule_index: usize,
    subtask: Option<&str>,
) -> Result<()> {
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

    let tbl = arr
        .get_mut(rule_index)
        .ok_or_else(|| JudgeError::TestNotFound(format!("rule #{rule_index}")))?;

    match subtask.map(str::trim).filter(|s| !s.is_empty()) {
        Some(sid) => {
            tbl.insert("subtask", value(sid));
        }
        None => {
            tbl.remove("subtask");
        }
    }

    ws.write(&toml_rel, doc.to_string().as_bytes())?;
    Ok(())
}

/// Updates `[solutions.<name>]` OI scoring assertions (`score` and `subtasks`) via `toml_edit`.
///
/// - `score`: `None` leaves unchanged; `Some(None)` clears `score`; `Some(Some(exp))` writes exact or `[min, max]`.
/// - `subtasks`: `None` leaves unchanged; `Some(map)` replaces `subtasks` (clearing when empty).
pub fn set_solution_scoring(
    ws: &Workspace,
    slug: &str,
    name: &str,
    score: Option<Option<ScoreExpect>>,
    subtasks: Option<BTreeMap<String, ExpectVerdict>>,
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

    let entry_tbl = sols_tbl
        .entry(name)
        .or_insert_with(|| Item::Table(Table::new()))
        .as_table_mut()
        .ok_or_else(|| JudgeError::InvalidConfig(format!("[solutions.{name}] is not a table")))?;

    if let Some(score_opt) = score {
        match score_opt {
            None => {
                entry_tbl.remove("score");
            }
            Some(ScoreExpect::Exact(v)) => {
                entry_tbl.insert("score", value(i64::from(v)));
            }
            Some(ScoreExpect::Range([lo, hi])) => {
                let mut arr = Array::new();
                arr.push(i64::from(lo));
                arr.push(i64::from(hi));
                entry_tbl.insert("score", value(arr));
            }
        }
    }

    if let Some(sub_map) = subtasks {
        if sub_map.is_empty() {
            entry_tbl.remove("subtasks");
        } else {
            let mut inline = InlineTable::new();
            for (sid, exp) in sub_map {
                let norm = exp.normalized();
                match norm.len() {
                    0 => {}
                    1 => {
                        inline.insert(sid, Value::from(norm[0].as_str()));
                    }
                    _ => {
                        let mut arr = Array::new();
                        for v in norm {
                            arr.push(v);
                        }
                        inline.insert(sid, Value::Array(arr));
                    }
                }
            }
            if inline.is_empty() {
                entry_tbl.remove("subtasks");
            } else {
                entry_tbl.insert("subtasks", Item::Value(Value::InlineTable(inline)));
            }
        }
    }

    ws.write(&toml_rel, doc.to_string().as_bytes())?;
    Ok(())
}

const TEMPLATE_CHECKER_CPP: &str = r#"// TODO: 按本题题面实现自定义评测器
#include "testlib.h"

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

const TEMPLATE_VALIDATOR_CPP: &str = r#"// TODO: 按本题题面实现输入校验器
#include "testlib.h"

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

const TEMPLATE_GEN_RANDOM_CPP: &str = r#"// TODO: 按本题题面实现随机数据生成器
#include "testlib.h"
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

const TEMPLATE_STD_CPP: &str = r#"// TODO: 按本题题面实现标程
#include <iostream>

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

const TEMPLATE_INTERACTOR_CPP: &str = r#"// TODO: 按本题交互协议实现交互器（标准 testlib.h registerInteraction）
// 注意：
// 1. 每次向选手输出（stdout）后必须立即 flush（std::endl 或 std::cout.flush()），否则会导致死锁 ILE！
// 2. 切勿在选手输出答案前向 stdout 泄露隐藏信息；可将需要二段复判的数据写入 tout。
#include "testlib.h"
#include <iostream>

int main(int argc, char* argv[]) {
    registerInteraction(argc, argv);

    long long a = inf.readLong();
    long long b = inf.readLong();

    // 向选手发送初始信息并立即刷新缓冲区，防止双向等待死锁（ILE）
    std::cout << a << ' ' << b << std::endl;

    long long ans = ouf.readLong();
    tout << ans << '\n';

    if (ans != a + b) {
        quitf(_wa, "expected %lld, found %lld", a + b, ans);
    }
    quitf(_ok, "correct");
}
"#;

const TEMPLATE_INTERACTIVE_STD_CPP: &str = r#"// TODO: 按本题交互协议实现标程
// 注意：交互题每次输出后必须立即 flush（std::endl 或 std::cout.flush()），否则会触发 ILE！
#include <iostream>

int main() {
    std::ios::sync_with_stdio(false);
    std::cin.tie(nullptr);
    long long a, b;
    if (std::cin >> a >> b) {
        std::cout << (a + b) << std::endl;
    }
    return 0;
}
"#;

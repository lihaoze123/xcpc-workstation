//! External tester submission trial run (zero-pollution) and explicit atomic adopt (Phase 3D).
//!
//! - Compiles external `.cpp` files in a temporary directory and evaluates them against
//!   built problem test data without touching `solutions/` or `problem.toml`.
//! - Supports single-problem multi-file runs, whole-contest single-directory runs
//!   (`A.cpp`, `a.cpp`, `A-tree.cpp`, `tree-dp.cpp`), and multi-tester 2-level directory runs
//!   (`<dir>/<tester_name>/<prob>.cpp`).
//! - Provides clean Markdown report export and an explicit atomic `adopt_external_solution`
//!   action that copies a submission into `solutions/` and registers it in `problem.toml` via
//!   `toml_edit`.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};
use xcpc_storage::Workspace;

use crate::assets::{resolve_testlib, standard_checker_source};
use crate::build::{BuildOptions, build_package};
use crate::cache::JudgeCache;
use crate::error::{JudgeError, Result, io_err};
use crate::ops::set_solution_config;
use crate::package::{
    CheckerSpec, ExpandedTest, TestSource, build_dir_for, expand_tests, find_interactor_source,
    has_explicit_checker, load_latest_report, load_verified_report, resolve_checker_spec,
};
use crate::report::{BuildMode, MatrixCell};
use crate::runner::{
    RunLimits, Verdict, run_checker_with_score, run_multi_run_interactive, run_multi_run_process,
};
use crate::subtask::validate_subtask_config;
use crate::toolchain::{
    CppStd, SourceLanguage, compile_cached, compile_to_path, resolve_active_compiler,
};

/// Options controlling external tester trial runs.
#[derive(Debug, Clone, Default)]
pub struct ExternalRunOptions {
    /// When `true` and built test data is missing (and workspace is writable), automatically
    /// runs a `Debug` build of the problem first. When `false`, returns a clear error/hint.
    pub auto_build: bool,
    /// Optional explicit compiler path override.
    pub cxx_override: Option<PathBuf>,
    /// Optional explicit cache directory override.
    pub cache_dir: Option<PathBuf>,
    /// Parallelism for auto-building if triggered.
    pub jobs: Option<usize>,
}

/// Diagnosis of the first failing test case for an external submission.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalFirstFailure {
    pub test_index: usize,
    pub test_name: String,
    pub verdict: Verdict,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub message: String,
}

/// Evaluation result of a single external `.cpp` submission.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExternalSubmissionResult {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tester: Option<String>,
    pub problem_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub problem_label: Option<String>,
    pub file_path: String,
    pub file_name: String,
    pub verdict: Verdict,
    pub passed_tests: usize,
    pub total_tests: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_score: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_score: Option<u32>,
    pub max_time_ms: u64,
    pub max_memory_kib: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub std_max_time_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ratio_vs_std: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_failure: Option<ExternalFirstFailure>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compile_error: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cells: Vec<MatrixCell>,
}

/// External file skipped because its filename did not match any problem in the contest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnmatchedExternalFile {
    pub file_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tester: Option<String>,
    pub reason: String,
}

/// Report for running one or more external `.cpp` files against a single problem.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExternalProblemReport {
    pub problem_id: String,
    pub submissions: Vec<ExternalSubmissionResult>,
}

/// Directory layout detected by `run_external_solutions_for_contest`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalContestMode {
    SingleDirectory,
    MultiTester,
}

/// Minimal contest problem metadata included in [`ExternalContestReport`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContestProblemRef {
    pub label: String,
    pub problem_id: String,
    pub title: String,
}

/// Report for running an external directory (single-level or multi-tester 2-level) against a contest.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExternalContestReport {
    pub contest_id: String,
    pub contest_title: String,
    pub mode: ExternalContestMode,
    pub problems: Vec<ContestProblemRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub testers: Vec<String>,
    pub submissions: Vec<ExternalSubmissionResult>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unmatched: Vec<UnmatchedExternalFile>,
}

/// Options for explicitly adopting an external submission into `problems/<slug>/solutions/`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AdoptSolutionOptions {
    /// Target solution stem name (defaults to the external file's logical stem).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Expected verdicts written to `[solutions.<name>].expect` (defaults to `["AC"]`).
    #[serde(default)]
    pub expect: Vec<String>,
    /// Optional tags written to `[solutions.<name>].tags`.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Set `main = true` (default `false`).
    #[serde(default)]
    pub is_main: bool,
}

/// Result of [`adopt_external_solution`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdoptSolutionResult {
    pub problem_id: String,
    pub solution_name: String,
    pub dest_rel: String,
    pub updated_problem_toml: bool,
}

/// Runs one or more external `.cpp` files against a single problem without modifying the workspace.
pub fn run_external_solutions_for_problem(
    ws: &Workspace,
    slug: &str,
    files: &[PathBuf],
    opts: &ExternalRunOptions,
    cancel: Option<Arc<AtomicBool>>,
) -> Result<ExternalProblemReport> {
    if files.is_empty() {
        return Err(JudgeError::InvalidConfig(
            "no external solution files provided".to_owned(),
        ));
    }
    let submissions = run_external_batch_for_problem(
        ws,
        slug,
        None,
        &files
            .iter()
            .map(|p| (None, p.clone()))
            .collect::<Vec<(Option<String>, PathBuf)>>(),
        opts,
        cancel,
    )?;
    Ok(ExternalProblemReport {
        problem_id: slug.to_owned(),
        submissions,
    })
}

/// Runs an external directory (single-level or 2-level `<dir>/<tester_name>/<prob>.cpp`)
/// against all matching problems in a contest without modifying the workspace.
pub fn run_external_solutions_for_contest(
    ws: &Workspace,
    contest_id: &str,
    dir: &Path,
    opts: &ExternalRunOptions,
    cancel: Option<Arc<AtomicBool>>,
) -> Result<ExternalContestReport> {
    if !dir.is_dir() {
        return Err(JudgeError::InvalidConfig(format!(
            "external submissions path is not a directory: {}",
            dir.display()
        )));
    }

    let contest = ws.load_contest(contest_id)?;
    let mut problem_refs: Vec<ContestProblemRef> = Vec::with_capacity(contest.meta.problems.len());
    for item in &contest.meta.problems {
        let title = ws
            .load_problem(&item.id)
            .map(|p| p.meta.title)
            .unwrap_or_else(|_| item.id.clone());
        problem_refs.push(ContestProblemRef {
            label: item.label.clone(),
            problem_id: item.id.clone(),
            title,
        });
    }

    // Scan immediate entries of `dir` to detect SingleDirectory vs MultiTester layout.
    let mut top_cpp_files: Vec<PathBuf> = Vec::new();
    let mut sub_dirs: Vec<(String, Vec<PathBuf>)> = Vec::new();

    let mut entries: Vec<PathBuf> = fs::read_dir(dir)
        .map_err(|e| io_err(format!("read {}", dir.display()), e))?
        .flatten()
        .map(|e| e.path())
        .collect();
    entries.sort();

    for entry in entries {
        if entry.is_file() {
            if SourceLanguage::from_path(&entry).is_some() {
                top_cpp_files.push(entry);
            }
        } else if entry.is_dir() {
            let dir_name = entry
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or_default()
                .to_owned();
            if dir_name.starts_with('.') {
                continue;
            }
            let mut inner: Vec<PathBuf> = fs::read_dir(&entry)
                .into_iter()
                .flatten()
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.is_file() && SourceLanguage::from_path(p).is_some())
                .collect();
            inner.sort();
            if !inner.is_empty() {
                sub_dirs.push((dir_name, inner));
            }
        }
    }

    let mode = if !sub_dirs.is_empty() {
        ExternalContestMode::MultiTester
    } else {
        ExternalContestMode::SingleDirectory
    };

    let mut testers_set: BTreeSet<String> = BTreeSet::new();
    let mut candidate_files: Vec<(Option<String>, PathBuf)> = Vec::new();
    match mode {
        ExternalContestMode::SingleDirectory => {
            for f in top_cpp_files {
                candidate_files.push((None, f));
            }
        }
        ExternalContestMode::MultiTester => {
            for (tester_name, files) in sub_dirs {
                testers_set.insert(tester_name.clone());
                for f in files {
                    candidate_files.push((Some(tester_name.clone()), f));
                }
            }
            if !top_cpp_files.is_empty() {
                let default_tester = "default".to_owned();
                testers_set.insert(default_tester.clone());
                for f in top_cpp_files {
                    candidate_files.push((Some(default_tester.clone()), f));
                }
            }
        }
    }

    let available_hint = problem_refs
        .iter()
        .map(|p| format!("{} ({})", p.label, p.problem_id))
        .collect::<Vec<_>>()
        .join(", ");

    // Group matched files by problem index so each problem's test data is prepared once.
    let mut matched_by_prob: BTreeMap<usize, Vec<(Option<String>, PathBuf)>> = BTreeMap::new();
    let mut unmatched: Vec<UnmatchedExternalFile> = Vec::new();

    for (tester, path) in candidate_files {
        match match_file_to_problem(&path, &problem_refs) {
            Some(prob_idx) => {
                matched_by_prob
                    .entry(prob_idx)
                    .or_default()
                    .push((tester, path));
            }
            None => {
                unmatched.push(UnmatchedExternalFile {
                    file_path: path.to_string_lossy().into_owned(),
                    tester,
                    reason: format!(
                        "文件名未能匹配比赛中的任何题目（可用题号/slug：{available_hint}）"
                    ),
                });
            }
        }
    }

    let mut all_submissions: Vec<ExternalSubmissionResult> = Vec::new();
    for (prob_idx, group) in matched_by_prob {
        if let Some(ref c) = cancel
            && c.load(Ordering::Relaxed)
        {
            return Err(JudgeError::Cancelled);
        }
        let pref = &problem_refs[prob_idx];
        let results = run_external_batch_for_problem(
            ws,
            &pref.problem_id,
            Some(&pref.label),
            &group,
            opts,
            cancel.clone(),
        )?;
        all_submissions.extend(results);
    }

    Ok(ExternalContestReport {
        contest_id: contest.id,
        contest_title: contest.meta.name,
        mode,
        problems: problem_refs,
        testers: testers_set.into_iter().collect(),
        submissions: all_submissions,
        unmatched,
    })
}

/// Explicitly adopts an external `.cpp` file into `problems/<slug>/solutions/` atomically
/// and registers `[solutions.<name>]` in `problem.toml` via `toml_edit`.
pub fn adopt_external_solution(
    ws: &Workspace,
    slug: &str,
    source_file: &Path,
    opts: &AdoptSolutionOptions,
) -> Result<AdoptSolutionResult> {
    if ws.is_read_only() {
        return Err(JudgeError::Storage(xcpc_storage::Error::ReadOnly));
    }
    if !source_file.is_file() {
        return Err(JudgeError::InvalidConfig(format!(
            "external source file not found: {}",
            source_file.display()
        )));
    }
    let lang = SourceLanguage::from_path(source_file).ok_or_else(|| {
        JudgeError::InvalidConfig(format!(
            "unsupported source file extension (expected .cpp/.cc/.cxx): {}",
            source_file.display()
        ))
    })?;

    let raw_name = match opts
        .name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(n) => n.to_owned(),
        None => SourceLanguage::logical_stem(source_file).ok_or_else(|| {
            JudgeError::InvalidConfig(format!(
                "cannot infer solution name from {}",
                source_file.display()
            ))
        })?,
    };
    let safe_name = validate_safe_solution_stem(&raw_name)?;

    let ext = match lang {
        SourceLanguage::Cpp { std: CppStd::Cpp17 } => ".17.cpp",
        SourceLanguage::Cpp { std: CppStd::Cpp23 } => ".23.cpp",
        SourceLanguage::Cpp { std: CppStd::Cpp20 } => ".cpp",
    };
    let filename = format!("{safe_name}{ext}");

    let problem = ws.load_problem(slug)?;
    let dest_rel = format!("{}/solutions/{filename}", problem.dir);
    let dest_abs = ws.resolve(&dest_rel)?;
    if dest_abs.exists() {
        return Err(JudgeError::InvalidConfig(format!(
            "solution file already exists: {dest_rel} (choose a different name to avoid overwriting)"
        )));
    }

    let bytes =
        fs::read(source_file).map_err(|e| io_err(format!("read {}", source_file.display()), e))?;
    ws.write(&dest_rel, &bytes)?;

    let expect = if opts.is_main {
        None
    } else if opts.expect.is_empty() {
        Some(vec!["AC".to_owned()])
    } else {
        Some(opts.expect.clone())
    };
    let tags = (!opts.tags.is_empty()).then(|| opts.tags.clone());

    set_solution_config(ws, slug, &safe_name, Some(opts.is_main), expect, tags)?;

    Ok(AdoptSolutionResult {
        problem_id: slug.to_owned(),
        solution_name: safe_name,
        dest_rel,
        updated_problem_toml: true,
    })
}

/// Renders a clean Markdown verification report for a single-problem external run.
pub fn render_external_problem_markdown(report: &ExternalProblemReport) -> String {
    let mut md = String::new();
    md.push_str(&format!("# 外部验题试跑报告：{}\n\n", report.problem_id));
    md.push_str("| 文件 | 判定 | 通过/总数 | 得分 | 最慢耗时 | 标程比值 | 首个失败点 |\n");
    md.push_str("| --- | --- | ---: | ---: | ---: | ---: | --- |\n");
    for s in &report.submissions {
        let score_str = match (s.total_score, s.max_score) {
            (Some(sc), Some(mx)) => format!("{sc:.0}/{mx}"),
            _ => "-".to_owned(),
        };
        let ratio_str = s
            .ratio_vs_std
            .map(|r| format!("{r:.2}× std"))
            .unwrap_or_else(|| "-".to_owned());
        let fail_str = if let Some(ref ce) = s.compile_error {
            format!("CE: {}", first_line_trimmed(ce))
        } else if let Some(ref f) = s.first_failure {
            let msg = if f.message.is_empty() {
                String::new()
            } else {
                format!(" ({})", first_line_trimmed(&f.message))
            };
            format!("#{} {} {}{}", f.test_index, f.test_name, f.verdict, msg)
        } else {
            "-".to_owned()
        };
        md.push_str(&format!(
            "| `{}` | **{}** | {}/{} | {} | {}ms | {} | {} |\n",
            s.file_name,
            s.verdict,
            s.passed_tests,
            s.total_tests,
            score_str,
            s.max_time_ms,
            ratio_str,
            escape_md_cell(&fail_str)
        ));
    }
    md
}

/// Renders a clean Markdown verification report for a contest-level external run.
/// In `MultiTester` mode, starts with a `Tester × Problem` matrix.
pub fn render_external_contest_markdown(report: &ExternalContestReport) -> String {
    let mut md = String::new();
    md.push_str(&format!(
        "# 整场外部验题报告：{}（{}）\n\n",
        report.contest_title, report.contest_id
    ));

    if report.mode == ExternalContestMode::MultiTester && !report.testers.is_empty() {
        md.push_str("## 验题人 × 题目矩阵\n\n");
        md.push_str("| 验题人 |");
        for p in &report.problems {
            md.push_str(&format!(" {} |", p.label));
        }
        md.push('\n');
        md.push_str("| --- |");
        for _ in &report.problems {
            md.push_str(" :---: |");
        }
        md.push('\n');

        for tester in &report.testers {
            md.push_str(&format!("| {} |", escape_md_cell(tester)));
            for p in &report.problems {
                let cell = report
                    .submissions
                    .iter()
                    .find(|s| {
                        s.tester.as_deref() == Some(tester.as_str()) && s.problem_id == p.problem_id
                    })
                    .map(|s| {
                        if s.verdict == Verdict::AC {
                            if let Some(r) = s.ratio_vs_std {
                                format!("AC ({}ms, {r:.2}×)", s.max_time_ms)
                            } else {
                                format!("AC ({}ms)", s.max_time_ms)
                            }
                        } else {
                            format!("{} ({}/{})", s.verdict, s.passed_tests, s.total_tests)
                        }
                    })
                    .unwrap_or_else(|| "-".to_owned());
                md.push_str(&format!(" {} |", escape_md_cell(&cell)));
            }
            md.push('\n');
        }
        md.push('\n');
    }

    md.push_str("## 提交明细\n\n");
    md.push_str(
        "| 验题人 | 题号 | 题目 | 文件 | 判定 | 通过/总数 | 得分 | 最慢耗时 | 标程比值 | 首个失败点 |\n",
    );
    md.push_str("| --- | --- | --- | --- | --- | ---: | ---: | ---: | ---: | --- |\n");
    for s in &report.submissions {
        let tester = s.tester.as_deref().unwrap_or("-");
        let label = s.problem_label.as_deref().unwrap_or("-");
        let score_str = match (s.total_score, s.max_score) {
            (Some(sc), Some(mx)) => format!("{sc:.0}/{mx}"),
            _ => "-".to_owned(),
        };
        let ratio_str = s
            .ratio_vs_std
            .map(|r| format!("{r:.2}× std"))
            .unwrap_or_else(|| "-".to_owned());
        let fail_str = if let Some(ref ce) = s.compile_error {
            format!("CE: {}", first_line_trimmed(ce))
        } else if let Some(ref f) = s.first_failure {
            let msg = if f.message.is_empty() {
                String::new()
            } else {
                format!(" ({})", first_line_trimmed(&f.message))
            };
            format!("#{} {} {}{}", f.test_index, f.test_name, f.verdict, msg)
        } else {
            "-".to_owned()
        };
        md.push_str(&format!(
            "| {} | {} | `{}` | `{}` | **{}** | {}/{} | {} | {}ms | {} | {} |\n",
            escape_md_cell(tester),
            escape_md_cell(label),
            s.problem_id,
            s.file_name,
            s.verdict,
            s.passed_tests,
            s.total_tests,
            score_str,
            s.max_time_ms,
            ratio_str,
            escape_md_cell(&fail_str)
        ));
    }

    if !report.unmatched.is_empty() {
        md.push_str("\n## 未匹配文件（已跳过）\n\n");
        for u in &report.unmatched {
            let prefix = u
                .tester
                .as_deref()
                .map(|t| format!("[{t}] "))
                .unwrap_or_default();
            md.push_str(&format!("- {prefix}`{}`：{}\n", u.file_path, u.reason));
        }
    }

    md
}

fn run_external_batch_for_problem(
    ws: &Workspace,
    slug: &str,
    problem_label: Option<&str>,
    files: &[(Option<String>, PathBuf)],
    opts: &ExternalRunOptions,
    cancel: Option<Arc<AtomicBool>>,
) -> Result<Vec<ExternalSubmissionResult>> {
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
    let interactor_src = find_interactor_source(&prob_dir);
    let is_interactive = interactor_src.is_some();
    let runs = problem.meta.run_count();
    let use_checker = !is_interactive || has_explicit_checker(&prob_dir, &problem.meta);
    let checker_spec = resolve_checker_spec(&prob_dir, &problem.meta)?;
    let all_tests = expand_tests(&prob_dir, &problem.meta)?;
    let subtask_val = validate_subtask_config(&problem.meta.subtask, &all_tests);
    let has_subtasks = !problem.meta.subtask.is_empty();

    if all_tests.is_empty() {
        return Err(JudgeError::InvalidConfig(format!(
            "题目「{slug}」没有可用的测试点。"
        )));
    }

    // Resolve built test inputs & reference answers.
    let mut prepared_tests = resolve_built_tests(ws, slug, &prob_dir, &all_tests, is_interactive);
    if prepared_tests.is_none() {
        if opts.auto_build && !ws.is_read_only() {
            let build_opts = BuildOptions {
                mode: BuildMode::Debug,
                jobs: opts.jobs,
                cxx_override: opts.cxx_override.clone(),
                cache_dir: opts.cache_dir.clone(),
                ..Default::default()
            };
            build_package(ws, slug, &build_opts, cancel.clone(), None)?;
            prepared_tests = resolve_built_tests(ws, slug, &prob_dir, &all_tests, is_interactive);
        }
        if prepared_tests.is_none() {
            return Err(JudgeError::InvalidConfig(format!(
                "题目「{slug}」尚未构建完整测试数据（build/problems/{slug}/tests/ 缺失）。请先运行 `xcpc package build {slug}` 构建数据，或启用 auto_build。"
            )));
        }
    }
    let prepared_tests = prepared_tests.unwrap();

    // Compile checker and/or interactor (read-only cache hit in almost all cases).
    let checker_bin = if use_checker {
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
            }
        }
        .map_err(|f| JudgeError::InvalidConfig(format!("checker compile failed: {}", f.stderr)))?;
        Some(cr.binary_path)
    } else {
        None
    };

    let interactor_bin = if let Some((ref p, ref rel, lang)) = interactor_src {
        let bytes = fs::read(p).map_err(|e| io_err(format!("read {}", p.display()), e))?;
        let cr = compile_cached(
            &cache,
            &compiler,
            lang,
            "interactor",
            rel,
            &bytes,
            Some(p),
            &testlib,
        )
        .map_err(|f| {
            JudgeError::InvalidConfig(format!("interactor compile failed: {}", f.stderr))
        })?;
        Some(cr.binary_path)
    } else {
        None
    };

    let std_max_time_ms = load_verified_report(ws, slug)
        .or_else(|| load_latest_report(ws, slug))
        .map(|r| r.safety_margin.std_max_time_ms)
        .filter(|&ms| ms > 0);

    // Compile and run each external file inside an isolated temporary directory.
    let tmp_dir = tempfile::tempdir().map_err(|e| io_err("create external run temp dir", e))?;
    let mut results = Vec::with_capacity(files.len());

    for (idx, (tester, src_path)) in files.iter().enumerate() {
        if let Some(ref c) = cancel
            && c.load(Ordering::Relaxed)
        {
            return Err(JudgeError::Cancelled);
        }

        let file_name = src_path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("submission.cpp")
            .to_owned();
        let file_path_str = src_path.to_string_lossy().into_owned();

        let lang = match SourceLanguage::from_path(src_path) {
            Some(l) => l,
            None => {
                results.push(ExternalSubmissionResult {
                    tester: tester.clone(),
                    problem_id: slug.to_owned(),
                    problem_label: problem_label.map(str::to_owned),
                    file_path: file_path_str,
                    file_name,
                    verdict: Verdict::CE,
                    passed_tests: 0,
                    total_tests: prepared_tests.len(),
                    total_score: has_subtasks.then_some(0.0),
                    max_score: has_subtasks.then_some(subtask_val.total_score),
                    max_time_ms: 0,
                    max_memory_kib: 0,
                    std_max_time_ms,
                    ratio_vs_std: None,
                    first_failure: None,
                    compile_error: Some(
                        "unsupported file extension (expected .cpp/.cc/.cxx)".to_owned(),
                    ),
                    cells: vec![],
                });
                continue;
            }
        };

        let bin_name = if cfg!(windows) {
            format!("ext_{idx}.exe")
        } else {
            format!("ext_{idx}")
        };
        let bin_path = tmp_dir.path().join(bin_name);
        if let Err(fail) = compile_to_path(
            &cache, &compiler, lang, &file_name, src_path, &bin_path, &testlib,
        ) {
            results.push(ExternalSubmissionResult {
                tester: tester.clone(),
                problem_id: slug.to_owned(),
                problem_label: problem_label.map(str::to_owned),
                file_path: file_path_str,
                file_name,
                verdict: Verdict::CE,
                passed_tests: 0,
                total_tests: prepared_tests.len(),
                total_score: has_subtasks.then_some(0.0),
                max_score: has_subtasks.then_some(subtask_val.total_score),
                max_time_ms: 0,
                max_memory_kib: 0,
                std_max_time_ms,
                ratio_vs_std: None,
                first_failure: None,
                compile_error: Some(fail.stderr),
                cells: vec![],
            });
            continue;
        }

        let mut cells: Vec<MatrixCell> = Vec::with_capacity(prepared_tests.len());
        for pt in &prepared_tests {
            if let Some(ref c) = cancel
                && c.load(Ordering::Relaxed)
            {
                return Err(JudgeError::Cancelled);
            }
            let out_tmp = tempfile::NamedTempFile::new()
                .map_err(|e| io_err("create temp external output", e))?;

            let cell = if let Some(ref inter_bin) = interactor_bin {
                let inter_out = run_multi_run_interactive(
                    inter_bin,
                    &bin_path,
                    &pt.input_path,
                    out_tmp.path(),
                    runs,
                    limits,
                    cancel.as_ref(),
                )?;
                let mut verdict = inter_out.verdict;
                let mut ratio = inter_out.score_ratio;
                let mut msg = inter_out.checker_message;
                if verdict == Verdict::AC
                    && let Some(ref chk_bin) = checker_bin
                {
                    let ans_p = pt.ans_path.as_deref().unwrap_or(out_tmp.path());
                    let chk_res = run_checker_with_score(
                        chk_bin,
                        &pt.input_path,
                        out_tmp.path(),
                        ans_p,
                        cancel.as_ref(),
                    )?;
                    verdict = chk_res.verdict;
                    ratio = ratio.min(chk_res.score_ratio);
                    msg = chk_res.message;
                }
                MatrixCell {
                    test_index: pt.spec.index,
                    test_name: pt.spec.name.clone(),
                    solution: file_name.clone(),
                    verdict,
                    cpu_time_ms: inter_out.cpu_time_ms,
                    wall_time_ms: inter_out.wall_time_ms,
                    peak_memory_kib: inter_out.peak_memory_kib,
                    cache_hit: false,
                    checker_message: msg,
                    stderr: inter_out.stderr,
                    output_preview: None,
                    stale: false,
                    repro_cmd: None,
                    score_ratio: Some(ratio),
                    transcript: Some(inter_out.transcript),
                }
            } else {
                let chk_bin = checker_bin
                    .as_deref()
                    .expect("non-interactive problem requires checker");
                let ans_p = pt
                    .ans_path
                    .as_deref()
                    .expect("non-interactive prepared test has ans");
                let outcome = run_multi_run_process(
                    &bin_path,
                    &pt.input_path,
                    out_tmp.path(),
                    runs,
                    limits,
                    cancel.as_ref(),
                )?;
                let (verdict, ratio, msg) = if outcome.verdict == Verdict::AC {
                    let chk_res = run_checker_with_score(
                        chk_bin,
                        &pt.input_path,
                        out_tmp.path(),
                        ans_p,
                        cancel.as_ref(),
                    )?;
                    (chk_res.verdict, chk_res.score_ratio, chk_res.message)
                } else {
                    (outcome.verdict, 0.0, String::new())
                };
                MatrixCell {
                    test_index: pt.spec.index,
                    test_name: pt.spec.name.clone(),
                    solution: file_name.clone(),
                    verdict,
                    cpu_time_ms: outcome.cpu_time_ms,
                    wall_time_ms: outcome.wall_time_ms,
                    peak_memory_kib: outcome.peak_memory_kib,
                    cache_hit: false,
                    checker_message: msg,
                    stderr: outcome.stderr,
                    output_preview: None,
                    stale: false,
                    repro_cmd: None,
                    score_ratio: Some(ratio),
                    transcript: None,
                }
            };
            cells.push(cell);
        }

        let passed_tests = cells.iter().filter(|c| c.verdict == Verdict::AC).count();
        let total_tests = cells.len();
        let max_time_ms = cells.iter().map(|c| c.cpu_time_ms).max().unwrap_or(0);
        let max_memory_kib = cells.iter().map(|c| c.peak_memory_kib).max().unwrap_or(0);
        let first_bad = cells.iter().find(|c| c.verdict != Verdict::AC);
        let overall_verdict = first_bad.map(|c| c.verdict).unwrap_or(Verdict::AC);
        let first_failure = first_bad.map(|c| {
            let msg = if !c.checker_message.is_empty() {
                c.checker_message.clone()
            } else {
                c.stderr.trim().to_owned()
            };
            ExternalFirstFailure {
                test_index: c.test_index,
                test_name: c.test_name.clone(),
                verdict: c.verdict,
                message: msg,
            }
        });

        let (total_score, max_score) = if has_subtasks {
            let by_test: HashMap<usize, &MatrixCell> =
                cells.iter().map(|c| (c.test_index, c)).collect();
            let score = compute_external_subtask_score(&subtask_val.summaries, &by_test);
            (Some(score), Some(subtask_val.total_score))
        } else {
            (None, None)
        };

        let ratio_vs_std = std_max_time_ms.map(|std_ms| max_time_ms as f64 / std_ms as f64);

        results.push(ExternalSubmissionResult {
            tester: tester.clone(),
            problem_id: slug.to_owned(),
            problem_label: problem_label.map(str::to_owned),
            file_path: file_path_str,
            file_name,
            verdict: overall_verdict,
            passed_tests,
            total_tests,
            total_score,
            max_score,
            max_time_ms,
            max_memory_kib,
            std_max_time_ms,
            ratio_vs_std,
            first_failure,
            compile_error: None,
            cells,
        });
    }

    Ok(results)
}

struct PreparedTest {
    spec: ExpandedTest,
    input_path: PathBuf,
    ans_path: Option<PathBuf>,
}

fn resolve_built_tests(
    ws: &Workspace,
    slug: &str,
    prob_dir: &Path,
    all_tests: &[ExpandedTest],
    is_interactive: bool,
) -> Option<Vec<PreparedTest>> {
    let build_tests_dir = build_dir_for(ws, slug).join("tests");
    let mut out = Vec::with_capacity(all_tests.len());
    for t in all_tests {
        let built_in = build_tests_dir.join(format!("{:03}.in", t.index));
        let input_path = if built_in.is_file() {
            built_in
        } else {
            match &t.source {
                TestSource::Sample { input_rel, .. } => {
                    let p = prob_dir.join(input_rel);
                    if p.is_file() { p } else { return None }
                }
                TestSource::File { file_rel } => {
                    let p = prob_dir.join(file_rel);
                    if p.is_file() { p } else { return None }
                }
                TestSource::Generated { .. } => return None,
            }
        };

        let ans_path = if is_interactive {
            let built_ans = build_tests_dir.join(format!("{:03}.ans", t.index));
            built_ans.is_file().then_some(built_ans)
        } else {
            let built_ans = build_tests_dir.join(format!("{:03}.ans", t.index));
            if built_ans.is_file() {
                Some(built_ans)
            } else if let TestSource::Sample {
                manual_ans_rel: Some(ref ans_rel),
                ..
            } = t.source
            {
                let p = prob_dir.join(ans_rel);
                if p.is_file() { Some(p) } else { return None }
            } else {
                return None;
            }
        };

        out.push(PreparedTest {
            spec: t.clone(),
            input_path,
            ans_path,
        });
    }
    Some(out)
}

fn compute_external_subtask_score(
    subtasks: &[crate::report::SubtaskSummary],
    by_test: &HashMap<usize, &MatrixCell>,
) -> f64 {
    let mut passed_full: HashMap<&str, bool> = HashMap::new();
    let mut total = 0.0f64;
    for st in subtasks {
        let deps_ok = st
            .depends
            .iter()
            .all(|dep| passed_full.get(dep.as_str()).copied().unwrap_or(false));
        if !deps_ok {
            passed_full.insert(st.id.as_str(), false);
            continue;
        }
        let ratios: Vec<f64> = st
            .test_indices
            .iter()
            .map(|idx| {
                by_test
                    .get(idx)
                    .map(|c| {
                        if c.verdict == Verdict::AC {
                            c.score_ratio.unwrap_or(1.0)
                        } else {
                            0.0
                        }
                    })
                    .unwrap_or(0.0)
            })
            .collect();
        let score = if ratios.is_empty() {
            0.0
        } else {
            match st.scoring {
                xcpc_domain::SubtaskScoring::Min => {
                    let min_r = ratios.iter().copied().fold(1.0f64, f64::min);
                    min_r * (st.score as f64)
                }
                xcpc_domain::SubtaskScoring::Sum => {
                    let avg_r = ratios.iter().sum::<f64>() / (ratios.len() as f64);
                    avg_r * (st.score as f64)
                }
            }
        };
        let is_full = (score - st.score as f64).abs() < 1e-6
            && st
                .test_indices
                .iter()
                .all(|idx| by_test.get(idx).is_some_and(|c| c.verdict == Verdict::AC));
        passed_full.insert(st.id.as_str(), is_full);
        total += score;
    }
    total
}

/// Matches a filename against a contest's problems by:
/// 1. Exact problem slug match (case-insensitive), e.g. `tree-dp.cpp`
/// 2. Exact contest label match (case-insensitive), e.g. `A.cpp`, `a.cpp`
/// 3. Contest label prefix followed by `-`, `_`, `.`, or space (longest label first), e.g. `A-tree.cpp`
/// 4. Problem slug prefix followed by `-` or `_`, e.g. `tree-dp_sol2.cpp`
fn match_file_to_problem(path: &Path, problems: &[ContestProblemRef]) -> Option<usize> {
    let stem = SourceLanguage::logical_stem(path)?;
    let lower = stem.to_ascii_lowercase();

    // 1. Exact slug match.
    if let Some((idx, _)) = problems
        .iter()
        .enumerate()
        .find(|(_, p)| p.problem_id.to_ascii_lowercase() == lower)
    {
        return Some(idx);
    }

    // 2. Exact label match.
    if let Some((idx, _)) = problems
        .iter()
        .enumerate()
        .find(|(_, p)| p.label.to_ascii_lowercase() == lower)
    {
        return Some(idx);
    }

    // 3. Label prefix + separator (`-`, `_`, `.`, ` `), longest label first.
    let mut by_label_len: Vec<(usize, String)> = problems
        .iter()
        .enumerate()
        .map(|(i, p)| (i, p.label.to_ascii_lowercase()))
        .collect();
    by_label_len.sort_by_key(|b| std::cmp::Reverse(b.1.len()));
    for (idx, lbl) in by_label_len {
        if let Some(rest) = lower.strip_prefix(&lbl)
            && let Some(sep) = rest.chars().next()
            && matches!(sep, '-' | '_' | '.' | ' ')
        {
            return Some(idx);
        }
    }

    // 4. Slug prefix + separator (`-`, `_`), longest slug first.
    let mut by_slug_len: Vec<(usize, String)> = problems
        .iter()
        .enumerate()
        .map(|(i, p)| (i, p.problem_id.to_ascii_lowercase()))
        .collect();
    by_slug_len.sort_by_key(|b| std::cmp::Reverse(b.1.len()));
    for (idx, slug) in by_slug_len {
        if let Some(rest) = lower.strip_prefix(&slug)
            && let Some(sep) = rest.chars().next()
            && matches!(sep, '-' | '_' | '.')
        {
            return Some(idx);
        }
    }

    None
}

fn validate_safe_solution_stem(raw: &str) -> Result<String> {
    let s = raw.trim();
    if s.is_empty()
        || s.contains('/')
        || s.contains('\\')
        || s.contains("..")
        || !s
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(JudgeError::InvalidConfig(format!(
            "invalid solution name {raw:?}: only ASCII letters, digits, '-' and '_' are allowed"
        )));
    }
    Ok(s.to_owned())
}

fn first_line_trimmed(s: &str) -> &str {
    s.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("")
}

fn escape_md_cell(s: &str) -> String {
    s.replace('|', "\\|").replace('\n', " ")
}

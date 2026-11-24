//! Package discovery, content hashing, static freshness checking, test expansion,
//! and dirty-input inspection.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use walkdir::WalkDir;
use xcpc_domain::{ExpectVerdict, ProblemMeta, ScoreExpect};
use xcpc_storage::Workspace;

use crate::assets::is_standard_checker;
use crate::error::{JudgeError, Result, io_err};
use crate::report::{BuildReport, DirtyInputIssue};
use crate::runner::RunLimits;
use crate::toolchain::SourceLanguage;

/// Static freshness status of a problem package (never compiles or executes anything).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum PackageFreshness {
    /// Pure publishing problem (no `solutions/` files, no `[[test]]`, no `[solutions.*]`).
    NotAPackage,
    /// Last strict verification succeeded and matches current package content hash.
    VerifiedFresh {
        package_hash: String,
        built_at_epoch_secs: u64,
    },
    /// A previous strict verification exists, but package files have changed since.
    Stale {
        current_hash: String,
        verified_hash: String,
    },
    /// Package has never been strictly verified (or only debug runs exist).
    Unverified { package_hash: String },
    /// The latest build for the current package hash failed.
    Failed {
        package_hash: String,
        summary: String,
    },
}

/// Resolved checker configuration for a problem.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckerSpec {
    /// One of the 9 bundled standard checkers (`wcmp`, `ncmp`, ...).
    Builtin(String),
    /// Custom `checker.cpp` (or other path in problem dir).
    Custom { path: PathBuf, display_name: String },
}

impl CheckerSpec {
    pub fn display_name(&self) -> &str {
        match self {
            CheckerSpec::Builtin(name) => name.as_str(),
            CheckerSpec::Custom { display_name, .. } => display_name.as_str(),
        }
    }

    pub fn is_custom(&self) -> bool {
        matches!(self, CheckerSpec::Custom { .. })
    }
}

/// Source of one expanded test case.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TestSource {
    /// Leading test automatically picked up from `samples/<stem>.in`.
    Sample {
        stem: String,
        input_rel: String,
        manual_ans_rel: Option<String>,
    },
    /// Hand-written input file declared by `[[test]] file = "..."`.
    File { file_rel: String },
    /// Generated input declared by `[[test]] gen = "..."`.
    Generated {
        gen_name: String,
        args: Vec<String>,
        args_raw: String,
    },
}

/// One statically expanded test case in execution order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExpandedTest {
    /// 1-based test index.
    pub index: usize,
    /// Human-readable name (e.g. `"sample-01"`, `"min-bound"`, `"gen_tree-1"`).
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    /// Optional OI subtask ID (`[[test]].subtask`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subtask: Option<String>,
    pub is_sample: bool,
    /// When `true`, write generated `.in` and `std` `.ans` back into `samples/`.
    pub write_back_sample: bool,
    pub source: TestSource,
    /// Human-readable generator command string if generated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gen_cmd: Option<String>,
    /// 0-based index of the `[[test]]` rule in `problem.toml`, or `None` for leading `samples/`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule_index: Option<usize>,
}

/// One discovered solution combining `solutions/*.cpp` on disk with `[solutions.<name>]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredSolution {
    pub name: String,
    pub source_path: PathBuf,
    pub source_rel: String,
    pub language: SourceLanguage,
    pub is_main: bool,
    pub expect: ExpectVerdict,
    /// Explicitly configured `expect` in `problem.toml` (vs default).
    pub explicit_expect: Option<ExpectVerdict>,
    pub tags: Option<Vec<String>>,
    /// Optional expected total score for OI subtask problems.
    pub score: Option<ScoreExpect>,
    /// Optional per-subtask expected verdict assertions.
    pub subtasks: BTreeMap<String, ExpectVerdict>,
}

/// Returns `<workspace>/build/problems/<slug>`.
pub fn build_dir_for(ws: &Workspace, slug: &str) -> PathBuf {
    ws.root().join("build").join("problems").join(slug)
}

/// Path to the latest build report (`build/problems/<slug>/report.json`).
pub fn report_path_for(ws: &Workspace, slug: &str) -> PathBuf {
    build_dir_for(ws, slug).join("report.json")
}

/// Path to the preserved strict-verified report (`build/problems/<slug>/verified_report.json`).
pub fn verified_report_path_for(ws: &Workspace, slug: &str) -> PathBuf {
    build_dir_for(ws, slug).join("verified_report.json")
}

/// Path to the strict-verified report for a contest limit context (`key` from
/// [`contest_limits_key`]); `None` is the problem's own limits ([`verified_report_path_for`]).
pub fn verified_report_path_in(ws: &Workspace, slug: &str, key: Option<&str>) -> PathBuf {
    match key {
        None => verified_report_path_for(ws, slug),
        Some(k) => build_dir_for(ws, slug).join(format!("verified_report.{k}.json")),
    }
}

/// Path to the isolated sanitizer UB check report (`build/problems/<slug>/sanitizer-report.json`).
pub fn sanitizer_report_path_for(ws: &Workspace, slug: &str) -> PathBuf {
    build_dir_for(ws, slug).join("sanitizer-report.json")
}

/// Loads the latest build report if present and valid JSON.
pub fn load_latest_report(ws: &Workspace, slug: &str) -> Option<BuildReport> {
    let bytes = fs::read(report_path_for(ws, slug)).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Loads the preserved strict-verified report if present and valid JSON.
pub fn load_verified_report(ws: &Workspace, slug: &str) -> Option<BuildReport> {
    let bytes = fs::read(verified_report_path_for(ws, slug)).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Loads the strict-verified report for a contest limit context (see [`verified_report_path_in`]).
pub fn load_verified_report_in(
    ws: &Workspace,
    slug: &str,
    key: Option<&str>,
) -> Option<BuildReport> {
    let bytes = fs::read(verified_report_path_in(ws, slug, key)).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// The time and memory limits a check of `slug` runs under: the problem's own `[limits]`,
/// or, inside `contest`, the contest item's overrides over them.
pub fn effective_run_limits(
    ws: &Workspace,
    slug: &str,
    contest: Option<&str>,
) -> Result<RunLimits> {
    let problem = ws.load_problem(slug)?;
    let mut limits = problem.meta.limits.clone();
    if let Some(cid) = contest {
        let contest = ws.load_contest(cid)?;
        if let Some(item) = contest.meta.problems.iter().find(|i| i.id == slug) {
            limits = item.apply(&problem.meta).limits;
        }
    }
    RunLimits::from_problem_limits(limits.time.as_deref(), limits.memory.as_deref())
}

/// A stable, filesystem-safe key (`t1000-m262144`) for a contest whose overrides change the
/// time or memory limit of `slug`; `None` when the check runs under the problem's own limits.
///
/// A strict check under different limits is a different check: it gets its own package hash
/// and its own verified report, so checking in a contest never invalidates the library state.
pub fn contest_limits_key(
    ws: &Workspace,
    slug: &str,
    contest: Option<&str>,
) -> Result<Option<String>> {
    let Some(cid) = contest else {
        return Ok(None);
    };
    let own = effective_run_limits(ws, slug, None)?;
    let eff = effective_run_limits(ws, slug, Some(cid))?;
    if own.time_limit_ms == eff.time_limit_ms && own.memory_limit_kib == eff.memory_limit_kib {
        return Ok(None);
    }
    Ok(Some(format!(
        "t{}-m{}",
        eff.time_limit_ms, eff.memory_limit_kib
    )))
}

/// [`compute_package_hash`] for a limit context: the plain hash when `key` is `None`.
pub fn compute_package_hash_in(ws: &Workspace, slug: &str, key: Option<&str>) -> Result<String> {
    let base = compute_package_hash(ws, slug)?;
    let Some(k) = key else {
        return Ok(base);
    };
    let mut hasher = Sha256::new();
    hasher.update(b"xcpc-package-hash-limits-v1\0");
    hasher.update(base.as_bytes());
    hasher.update(b"\0");
    hasher.update(k.as_bytes());
    Ok(format!("{:x}", hasher.finalize()))
}

/// Computes a deterministic SHA-256 content hash over all package-relevant files of `slug`:
/// `problem.toml`, `validator.cpp`, `checker.cpp`, `generators/*`, `solutions/*`,
/// `samples/*`, `tests/**/*`, and user `testlib.h` (problem dir or workspace root) if present.
pub fn compute_package_hash(ws: &Workspace, slug: &str) -> Result<String> {
    let problem = ws.load_problem(slug)?;
    let prob_dir = ws.resolve(&problem.dir)?;

    let mut entries: Vec<(String, PathBuf)> = Vec::new();

    // Single files at problem root.
    for name in [
        "problem.toml",
        "validator.cpp",
        "validator.17.cpp",
        "validator.20.cpp",
        "validator.23.cpp",
        "checker.cpp",
        "checker.17.cpp",
        "checker.20.cpp",
        "checker.23.cpp",
        "interactor.cpp",
        "interactor.17.cpp",
        "interactor.20.cpp",
        "interactor.23.cpp",
        "testlib.h",
    ] {
        let p = prob_dir.join(name);
        if p.is_file() {
            entries.push((name.to_owned(), p));
        }
    }

    // Optional workspace-level testlib.h.
    let ws_testlib = ws.root().join("testlib.h");
    if ws_testlib.is_file() {
        entries.push(("../testlib.h".to_owned(), ws_testlib));
    }

    // Subdirectories: generators/, solutions/, samples/, tests/ (recursive).
    for sub in ["generators", "solutions", "samples", "tests"] {
        let dir = prob_dir.join(sub);
        if !dir.is_dir() {
            continue;
        }
        for entry in WalkDir::new(&dir).follow_links(false).into_iter().flatten() {
            if entry.file_type().is_file()
                && let Ok(rel) = entry.path().strip_prefix(&prob_dir)
            {
                let key = rel.to_string_lossy().replace('\\', "/");
                entries.push((key, entry.path().to_path_buf()));
            }
        }
    }

    entries.sort_by(|a, b| a.0.cmp(&b.0));

    let mut hasher = Sha256::new();
    hasher.update(b"xcpc-package-hash-v1\0");
    for (rel, abs) in entries {
        let bytes = fs::read(&abs).map_err(|e| io_err(format!("read {}", abs.display()), e))?;
        hasher.update(rel.as_bytes());
        hasher.update(b"\0");
        hasher.update((bytes.len() as u64).to_le_bytes());
        hasher.update(&bytes);
        hasher.update(b"\0");
    }

    Ok(format!("{:x}", hasher.finalize()))
}

/// Performs a pure static freshness check for `slug` without compiling or executing anything.
/// Used by `xcpc check` and `freeze_contest`.
pub fn check_package_freshness(ws: &Workspace, slug: &str) -> Result<PackageFreshness> {
    check_package_freshness_in(ws, slug, None)
}

/// [`check_package_freshness`] for a check inside `contest`: uses the contest's overridden
/// time and memory limits (see [`contest_limits_key`]).
pub fn check_package_freshness_in(
    ws: &Workspace,
    slug: &str,
    contest: Option<&str>,
) -> Result<PackageFreshness> {
    let problem = ws.load_problem(slug)?;
    let prob_dir = ws.resolve(&problem.dir)?;

    if !problem.meta.has_package_in_dir(&prob_dir) {
        return Ok(PackageFreshness::NotAPackage);
    }

    let key = contest_limits_key(ws, slug, contest)?;
    let key = key.as_deref();
    let current_hash = compute_package_hash_in(ws, slug, key)?;

    if let Some(verified) = load_verified_report_in(ws, slug, key)
        && verified.verified
    {
        if verified.package_hash == current_hash {
            return Ok(PackageFreshness::VerifiedFresh {
                package_hash: current_hash,
                built_at_epoch_secs: verified.built_at_epoch_secs,
            });
        }
        // Check if the latest report for the *current* hash failed.
        if let Some(latest) = load_latest_report(ws, slug)
            && latest.package_hash == current_hash
            && !latest.ok
        {
            return Ok(PackageFreshness::Failed {
                package_hash: current_hash,
                summary: summarize_failure(&latest),
            });
        }
        return Ok(PackageFreshness::Stale {
            current_hash,
            verified_hash: verified.package_hash,
        });
    }

    if let Some(latest) = load_latest_report(ws, slug)
        && latest.package_hash == current_hash
        && !latest.ok
    {
        return Ok(PackageFreshness::Failed {
            package_hash: current_hash,
            summary: summarize_failure(&latest),
        });
    }

    Ok(PackageFreshness::Unverified {
        package_hash: current_hash,
    })
}

fn summarize_failure(report: &BuildReport) -> String {
    if !report.compile_failures.is_empty() {
        let targets: Vec<&str> = report
            .compile_failures
            .iter()
            .map(|c| c.target.as_str())
            .collect();
        return format!("编译失败：{}", targets.join("、"));
    }
    // The concrete reason (gate message), never just the gate title.
    let failed_gates: Vec<&str> = report
        .gates
        .iter()
        .filter(|g| g.status == crate::report::GateStatus::Fail)
        .map(|g| {
            if g.message.trim().is_empty() {
                g.title.as_str()
            } else {
                g.message.as_str()
            }
        })
        .collect();
    if !failed_gates.is_empty() {
        return failed_gates.join("；");
    }
    "检查没有通过".to_owned()
}

/// Resolves which checker to use for a problem.
pub fn resolve_checker_spec(prob_dir: &Path, meta: &ProblemMeta) -> Result<CheckerSpec> {
    if let Some(raw) = meta
        .package
        .as_ref()
        .and_then(|p| p.checker.as_deref())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        if is_standard_checker(raw) {
            return Ok(CheckerSpec::Builtin(raw.to_owned()));
        }
        let candidate = prob_dir.join(raw);
        if candidate.is_file() {
            return Ok(CheckerSpec::Custom {
                path: candidate,
                display_name: raw.to_owned(),
            });
        }
        return Err(JudgeError::UnknownChecker(raw.to_owned()));
    }

    // Omitted: use checker.cpp (or checker.17/20/23.cpp) if present, else default `wcmp`.
    for name in [
        "checker.cpp",
        "checker.20.cpp",
        "checker.23.cpp",
        "checker.17.cpp",
    ] {
        let candidate = prob_dir.join(name);
        if candidate.is_file() {
            return Ok(CheckerSpec::Custom {
                path: candidate,
                display_name: name.to_owned(),
            });
        }
    }

    Ok(CheckerSpec::Builtin("wcmp".to_owned()))
}

/// Finds `validator.cpp` (or `validator.{17,20,23}.cpp`) in `prob_dir` if present.
pub fn find_validator_source(prob_dir: &Path) -> Option<(PathBuf, String, SourceLanguage)> {
    for name in [
        "validator.cpp",
        "validator.20.cpp",
        "validator.23.cpp",
        "validator.17.cpp",
    ] {
        let p = prob_dir.join(name);
        if p.is_file()
            && let Some(lang) = SourceLanguage::from_path(&p)
        {
            return Some((p, name.to_owned(), lang));
        }
    }
    None
}

/// Finds `interactor.cpp` (or `interactor.{17,20,23}.cpp`) in `prob_dir` if present.
/// Presence of an interactor automatically classifies the problem as interactive.
pub fn find_interactor_source(prob_dir: &Path) -> Option<(PathBuf, String, SourceLanguage)> {
    for name in [
        "interactor.cpp",
        "interactor.20.cpp",
        "interactor.23.cpp",
        "interactor.17.cpp",
    ] {
        let p = prob_dir.join(name);
        if p.is_file()
            && let Some(lang) = SourceLanguage::from_path(&p)
        {
            return Some((p, name.to_owned(), lang));
        }
    }
    None
}

/// Returns `true` if a checker is explicitly configured via `[package].checker` or if a
/// custom `checker.cpp` (or `checker.{17,20,23}.cpp`) exists in `prob_dir`.
///
/// For interactive problems, two-stage checking (passing interactor `tout` to `checker`)
/// is enabled iff this returns `true`.
pub fn has_explicit_checker(prob_dir: &Path, meta: &ProblemMeta) -> bool {
    if meta
        .package
        .as_ref()
        .and_then(|p| p.checker.as_deref())
        .map(str::trim)
        .is_some_and(|s| !s.is_empty())
    {
        return true;
    }
    [
        "checker.cpp",
        "checker.20.cpp",
        "checker.23.cpp",
        "checker.17.cpp",
    ]
    .iter()
    .any(|name| prob_dir.join(name).is_file())
}

/// Finds a generator source file `generators/<name>.cpp` (or `.17/.20/.23.cpp`).
pub fn find_generator_source(
    prob_dir: &Path,
    gen_name: &str,
) -> Result<(PathBuf, String, SourceLanguage)> {
    let gen_dir = prob_dir.join("generators");
    for suffix in [".cpp", ".20.cpp", ".23.cpp", ".17.cpp", ".cc", ".cxx"] {
        let filename = format!("{gen_name}{suffix}");
        let p = gen_dir.join(&filename);
        if p.is_file()
            && let Some(lang) = SourceLanguage::from_path(&p)
        {
            return Ok((p, format!("generators/{filename}"), lang));
        }
    }
    // Also allow passing exact filename like `gen_tree.cpp`.
    let direct = gen_dir.join(gen_name);
    if direct.is_file()
        && let Some(lang) = SourceLanguage::from_path(&direct)
    {
        return Ok((direct, format!("generators/{gen_name}"), lang));
    }
    Err(JudgeError::GeneratorNotFound(gen_name.to_owned()))
}

/// Discovers all solutions in `solutions/` and merges declarations from `[solutions.<name>]`.
pub fn discover_solutions(prob_dir: &Path, meta: &ProblemMeta) -> Result<Vec<DiscoveredSolution>> {
    let sol_dir = prob_dir.join("solutions");
    let mut files_by_name: BTreeMap<String, (PathBuf, String, SourceLanguage)> = BTreeMap::new();

    if sol_dir.is_dir() {
        let mut entries: Vec<PathBuf> = fs::read_dir(&sol_dir)
            .map_err(|e| io_err(format!("read {}", sol_dir.display()), e))?
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_file())
            .collect();
        entries.sort();

        for path in entries {
            if let (Some(lang), Some(stem)) = (
                SourceLanguage::from_path(&path),
                SourceLanguage::logical_stem(&path),
            ) {
                let fname = path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or_default()
                    .to_owned();
                files_by_name.insert(stem, (path, format!("solutions/{fname}"), lang));
            }
        }
    }

    // Verify every declared `[solutions.<name>]` exists on disk.
    for declared in meta.solutions.keys() {
        if !files_by_name.contains_key(declared) {
            return Err(JudgeError::SolutionNotFound(declared.clone()));
        }
    }

    let mut out = Vec::new();
    for (name, (path, rel, lang)) in files_by_name {
        let spec = meta.solutions.get(&name);
        let is_main = spec.and_then(|s| s.main).unwrap_or(false);
        let explicit_expect = spec.and_then(|s| s.expect.clone());
        let score = spec.and_then(|s| s.score);
        let subtasks = spec.map(|s| s.subtasks.clone()).unwrap_or_default();
        let expect = explicit_expect.clone().unwrap_or_else(|| {
            if !is_main && (score.is_some() || !subtasks.is_empty()) {
                ExpectVerdict::Single("ANY".to_owned())
            } else {
                ExpectVerdict::Single("AC".to_owned())
            }
        });
        let tags = spec.and_then(|s| s.tags.clone());
        out.push(DiscoveredSolution {
            name,
            source_path: path,
            source_rel: rel,
            language: lang,
            is_main,
            expect,
            explicit_expect,
            tags,
            score,
            subtasks,
        });
    }

    // Sort main solution first, then alphabetically by name.
    out.sort_by(|a, b| b.is_main.cmp(&a.is_main).then_with(|| a.name.cmp(&b.name)));
    Ok(out)
}

/// Statically expands `samples/` (leading tests with tag `"sample"`, skipped automatically
/// for interactive problems with `interactor.cpp`) followed by `[[test]]`.
pub fn expand_tests(prob_dir: &Path, meta: &ProblemMeta) -> Result<Vec<ExpandedTest>> {
    let mut tests = Vec::new();
    let is_interactive = find_interactor_source(prob_dir).is_some();

    // 1. Leading tests from `samples/*.in` (skipped for interactive problems unless explicitly
    // declared via `[[test]]`).
    let samples_dir = prob_dir.join("samples");
    if !is_interactive && samples_dir.is_dir() {
        let mut stems: Vec<String> = fs::read_dir(&samples_dir)
            .map_err(|e| io_err(format!("read {}", samples_dir.display()), e))?
            .flatten()
            .filter_map(|e| {
                let p = e.path();
                if !p.is_file() {
                    return None;
                }
                let name = p.file_name()?.to_str()?;
                let stem = name.strip_suffix(".in")?;
                (!stem.is_empty()).then(|| stem.to_owned())
            })
            .collect();
        stems.sort();

        for stem in stems {
            let input_rel = format!("samples/{stem}.in");
            let ans_rel = if samples_dir.join(format!("{stem}.ans")).is_file() {
                Some(format!("samples/{stem}.ans"))
            } else if samples_dir.join(format!("{stem}.out")).is_file() {
                Some(format!("samples/{stem}.out"))
            } else {
                None
            };
            let idx = tests.len() + 1;
            tests.push(ExpandedTest {
                index: idx,
                name: format!("sample-{stem}"),
                tag: Some("sample".to_owned()),
                subtask: None,
                is_sample: true,
                write_back_sample: false,
                source: TestSource::Sample {
                    stem,
                    input_rel,
                    manual_ans_rel: ans_rel,
                },
                gen_cmd: None,
                rule_index: None,
            });
        }
    }

    // 2. Expand `[[test]]` specs in order, disambiguating duplicate generated base names
    // sequentially across rules (e.g. gen_random-1..3, gen_random-4..5).
    let mut base_name_totals: BTreeMap<String, u32> = BTreeMap::new();
    for spec in &meta.test {
        if let (None, Some(gen_name)) = (&spec.file, &spec.generator) {
            let repeat = spec.repeat.unwrap_or(1).max(1);
            let base = spec.name.clone().unwrap_or_else(|| gen_name.clone());
            *base_name_totals.entry(base).or_insert(0) += repeat;
        }
    }
    let mut base_name_seq: BTreeMap<String, u32> = BTreeMap::new();

    for (spec_idx, spec) in meta.test.iter().enumerate() {
        let write_back_sample = spec.sample.unwrap_or(false);
        let subtask = spec
            .subtask
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned);
        match (&spec.file, &spec.generator) {
            (Some(file), None) => {
                let idx = tests.len() + 1;
                let default_name = Path::new(file)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("hand")
                    .to_owned();
                tests.push(ExpandedTest {
                    index: idx,
                    name: spec.name.clone().unwrap_or(default_name),
                    tag: spec.tag.clone(),
                    subtask,
                    is_sample: write_back_sample,
                    write_back_sample,
                    source: TestSource::File {
                        file_rel: file.clone(),
                    },
                    gen_cmd: None,
                    rule_index: Some(spec_idx),
                });
            }
            (None, Some(gen_name)) => {
                let repeat = spec.repeat.unwrap_or(1).max(1);
                let raw_template = spec.args.clone().unwrap_or_default();
                let has_placeholder = raw_template.contains("{i}");
                let base_name = spec.name.clone().unwrap_or_else(|| gen_name.clone());
                let total_for_base = base_name_totals.get(&base_name).copied().unwrap_or(repeat);
                for i in 1..=repeat {
                    let expanded_raw = if has_placeholder {
                        raw_template.replace("{i}", &i.to_string())
                    } else if repeat > 1 {
                        if raw_template.trim().is_empty() {
                            i.to_string()
                        } else {
                            format!("{raw_template} {i}")
                        }
                    } else {
                        raw_template.clone()
                    };
                    let args = shlex::split(&expanded_raw).ok_or_else(|| {
                        JudgeError::InvalidConfig(format!(
                            "test #{} has unclosed quote in args: {expanded_raw:?}",
                            spec_idx + 1
                        ))
                    })?;
                    let idx = tests.len() + 1;
                    let seq_num = base_name_seq.entry(base_name.clone()).or_insert(0);
                    *seq_num += 1;
                    let name = if total_for_base > 1 {
                        format!("{base_name}-{seq_num}")
                    } else {
                        base_name.clone()
                    };
                    let gen_cmd = if expanded_raw.trim().is_empty() {
                        gen_name.clone()
                    } else {
                        format!("{gen_name} {expanded_raw}")
                    };
                    tests.push(ExpandedTest {
                        index: idx,
                        name,
                        tag: spec.tag.clone(),
                        subtask: subtask.clone(),
                        is_sample: write_back_sample,
                        write_back_sample,
                        source: TestSource::Generated {
                            gen_name: gen_name.clone(),
                            args,
                            args_raw: expanded_raw,
                        },
                        gen_cmd: Some(gen_cmd),
                        rule_index: Some(spec_idx),
                    });
                }
            }
            (Some(_), Some(_)) => {
                return Err(JudgeError::InvalidConfig(format!(
                    "[[test]] #{} cannot specify both `file` and `gen`",
                    spec_idx + 1
                )));
            }
            (None, None) => {
                return Err(JudgeError::InvalidConfig(format!(
                    "[[test]] #{} must specify either `file` or `gen`",
                    spec_idx + 1
                )));
            }
        }
    }

    Ok(tests)
}

/// Checks hand-written inputs in `samples/*.in` and `tests/*.in` for CRLF, UTF-8 BOM,
/// or missing trailing newline.
pub fn check_dirty_inputs(prob_dir: &Path) -> Vec<DirtyInputIssue> {
    let mut issues = Vec::new();

    let mut check_file = |rel: String, path: &Path| {
        let Ok(bytes) = fs::read(path) else {
            return;
        };
        let has_bom = bytes.starts_with(&[0xEF, 0xBB, 0xBF]);
        let has_crlf = bytes.windows(2).any(|w| w == b"\r\n");
        let missing_trailing_newline = !bytes.is_empty() && !bytes.ends_with(b"\n");
        if has_bom || has_crlf || missing_trailing_newline {
            issues.push(DirtyInputIssue {
                path: rel,
                has_crlf,
                has_bom,
                missing_trailing_newline,
            });
        }
    };

    for (dir_name, recursive) in [("samples", false), ("tests", false)] {
        let dir = prob_dir.join(dir_name);
        if !dir.is_dir() {
            continue;
        }
        let walker = if recursive {
            WalkDir::new(&dir)
        } else {
            WalkDir::new(&dir).max_depth(1)
        };
        let mut paths: Vec<PathBuf> = walker
            .into_iter()
            .flatten()
            .filter(|e| e.file_type().is_file())
            .map(|e| e.path().to_path_buf())
            .filter(|p| {
                p.extension()
                    .and_then(|s| s.to_str())
                    .is_some_and(|ext| ext == "in")
            })
            .collect();
        paths.sort();
        for p in paths {
            if let Ok(rel) = p.strip_prefix(prob_dir) {
                check_file(rel.to_string_lossy().replace('\\', "/"), &p);
            }
        }
    }

    issues
}

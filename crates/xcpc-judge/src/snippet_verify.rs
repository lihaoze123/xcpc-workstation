//! Snippet verification engine (`self`, workspace `problem`, and global-cached `library_checker`),
//! content-hash staleness detection, and topological cascade regression.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Deserialize;
use sha2::{Digest, Sha256};
use xcpc_domain::{
    LibraryCheckerConfig, SnippetVerifyCaseResult, SnippetVerifyReport, SnippetVerifySpec,
    SnippetVerifyStatus, check_id,
};
use xcpc_storage::Workspace;

use crate::assets::{ResolvedTestlib, resolve_testlib, standard_checker_source};
use crate::build::{BuildOptions, build_package};
use crate::cache::{JudgeCache, write_atomic};
use crate::error::{JudgeError, Result, io_err};
use crate::package::{
    CheckerSpec, build_dir_for, compute_package_hash_in, effective_run_limits, resolve_checker_spec,
};
use crate::report::BuildMode;
use crate::runner::{RunLimits, Verdict, run_checker_with_score, run_process};
use crate::toolchain::{
    CppStd, DetectedCompiler, SourceLanguage, compile_cached, compile_cached_with_includes,
    resolve_active_compiler,
};

/// Options controlling snippet verification.
#[derive(Debug, Clone, Default)]
pub struct SnippetVerifyOptions {
    /// Optional explicit C++ compiler path override.
    pub cxx_override: Option<PathBuf>,
    /// Optional explicit cache root override (default: `~/.cache/xcpc`).
    pub cache_dir: Option<PathBuf>,
    /// Library Checker mirror / local repo configuration.
    pub library_checker: LibraryCheckerConfig,
    /// Optional parallelism hint for package builds.
    pub jobs: Option<usize>,
    /// When `true`, re-run verification even if a cached report matches the current content hash.
    pub force: bool,
}

/// Path to the cached snippet verification report inside `<ws>/.xcpc/snippet-verify/<slug>.json`.
pub fn snippet_verify_report_path(ws: &Workspace, slug: &str) -> PathBuf {
    ws.root()
        .join(".xcpc")
        .join("snippet-verify")
        .join(format!("{slug}.json"))
}

/// Loads the last cached verification report for `slug`, if present and valid JSON.
pub fn load_snippet_verify_report(ws: &Workspace, slug: &str) -> Option<SnippetVerifyReport> {
    let bytes = fs::read(snippet_verify_report_path(ws, slug)).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Computes the deterministic SHA-256 verification key for `slug`, incorporating:
/// - `snippets/<slug>/snippet.toml`
/// - `snippets/<slug>/code.hpp` (if present)
/// - every transitive dependency's `code.hpp`
/// - every `[[verify]]` test/adapter file and referenced workspace problem data hash.
pub fn compute_snippet_verify_hash(ws: &Workspace, slug: &str) -> Result<String> {
    check_id(slug).map_err(|e| JudgeError::Storage(e.into()))?;
    let snippet = ws.load_snippet(slug)?;
    let dag = ws.snippet_dag()?;
    let transitive = dag.transitive_deps(slug)?;

    let mut hasher = Sha256::new();
    hasher.update(b"xcpc-snippet-verify-v1\0");
    hasher.update(slug.as_bytes());
    hasher.update(b"\0");

    let meta_rel = format!("snippets/{slug}/snippet.toml");
    if let Ok(bytes) = ws.read_bytes(&meta_rel) {
        hasher.update(b"meta\0");
        hasher.update(&bytes);
        hasher.update(b"\0");
    }

    if let Some(code) = &snippet.code {
        hasher.update(b"code\0");
        hasher.update(code.text.as_bytes());
        hasher.update(b"\0");
    }

    for dep in &transitive {
        hasher.update(b"dep\0");
        hasher.update(dep.as_bytes());
        hasher.update(b"\0");
        if let Ok(bytes) = ws.read_bytes(&format!("snippets/{dep}/code.hpp")) {
            hasher.update(&bytes);
        }
        hasher.update(b"\0");
    }

    for spec in &snippet.meta.verify {
        match spec {
            SnippetVerifySpec::SelfTest { file } => {
                hasher.update(b"self\0");
                hasher.update(file.as_bytes());
                hasher.update(b"\0");
                if let Ok(bytes) = ws.read_bytes(&format!("snippets/{slug}/{file}")) {
                    hasher.update(&bytes);
                }
                hasher.update(b"\0");
            }
            SnippetVerifySpec::Problem { problem, adapter } => {
                hasher.update(b"problem\0");
                hasher.update(problem.as_bytes());
                hasher.update(b"\0");
                hasher.update(adapter.as_bytes());
                hasher.update(b"\0");
                if let Ok(bytes) = ws.read_bytes(&format!("snippets/{slug}/{adapter}")) {
                    hasher.update(&bytes);
                }
                hasher.update(b"\0");
                if let Ok(pkg_hash) = compute_package_hash_in(ws, problem, None) {
                    hasher.update(pkg_hash.as_bytes());
                } else if let Ok(prob) = ws.load_problem(problem) {
                    for s in &prob.samples {
                        hasher.update(s.name.as_bytes());
                        hasher.update(s.input.as_bytes());
                        hasher.update(s.output.as_bytes());
                    }
                }
                hasher.update(b"\0");
            }
            SnippetVerifySpec::LibraryChecker { problem, adapter } => {
                hasher.update(b"library_checker\0");
                hasher.update(problem.as_bytes());
                hasher.update(b"\0");
                hasher.update(adapter.as_bytes());
                hasher.update(b"\0");
                if let Ok(bytes) = ws.read_bytes(&format!("snippets/{slug}/{adapter}")) {
                    hasher.update(&bytes);
                }
                hasher.update(b"\0");
            }
        }
    }

    Ok(format!("{:x}", hasher.finalize()))
}

/// Statically resolves the current [`SnippetVerifyStatus`] for `slug` without compiling
/// or executing anything. Editing an upstream dependency immediately causes downstream
/// snippets to resolve as [`SnippetVerifyStatus::Stale`].
pub fn snippet_verify_status(ws: &Workspace, slug: &str) -> SnippetVerifyStatus {
    let Some(report) = load_snippet_verify_report(ws, slug) else {
        return SnippetVerifyStatus::Unverified;
    };
    let Ok(current_hash) = compute_snippet_verify_hash(ws, slug) else {
        return SnippetVerifyStatus::Stale;
    };
    if report.content_hash != current_hash {
        return SnippetVerifyStatus::Stale;
    }
    report.status
}

/// Statically resolves verification statuses for every snippet in the workspace.
pub fn all_snippet_verify_statuses(
    ws: &Workspace,
) -> Result<BTreeMap<String, SnippetVerifyStatus>> {
    let slugs = ws.list_snippets()?;
    let mut map = BTreeMap::new();
    for slug in slugs {
        let status = snippet_verify_status(ws, &slug);
        map.insert(slug, status);
    }
    Ok(map)
}

/// Verifies a single snippet across all of its `[[verify]]` declarations, caching the
/// resulting [`SnippetVerifyReport`] in `.xcpc/snippet-verify/<slug>.json`.
pub fn verify_snippet(
    ws: &Workspace,
    slug: &str,
    opts: &SnippetVerifyOptions,
) -> Result<SnippetVerifyReport> {
    check_id(slug).map_err(|e| JudgeError::Storage(e.into()))?;
    let snippet = ws.load_snippet(slug)?;
    let content_hash = compute_snippet_verify_hash(ws, slug)?;

    if !opts.force
        && let Some(cached) = load_snippet_verify_report(ws, slug)
        && cached.content_hash == content_hash
        && matches!(
            cached.status,
            SnippetVerifyStatus::Passed | SnippetVerifyStatus::Failed
        )
    {
        return Ok(cached);
    }

    let now = now_epoch_secs();
    if snippet.meta.verify.is_empty() {
        let report = SnippetVerifyReport {
            slug: slug.to_owned(),
            content_hash,
            status: SnippetVerifyStatus::Unverified,
            verified_at_epoch_secs: now,
            total_time_ms: 0,
            max_memory_kib: 0,
            cases: Vec::new(),
        };
        save_snippet_verify_report(ws, &report)?;
        return Ok(report);
    }

    let previous_report = load_snippet_verify_report(ws, slug);

    let compiler = resolve_active_compiler(opts.cxx_override.as_deref())?;
    let cache = match &opts.cache_dir {
        Some(dir) => JudgeCache::open(dir)?,
        None => JudgeCache::open_default()?,
    };
    let snippets_inc = ws.root().join("snippets");
    let dep_salt = compute_snippet_headers_salt(ws, slug)?;
    let bundled_testlib = ResolvedTestlib::Bundled {
        content_hash: xcpc_storage::content_hash(crate::assets::TESTLIB_H.as_bytes()),
    };

    let mut cases = Vec::with_capacity(snippet.meta.verify.len());
    let mut total_time_ms = 0u64;
    let mut max_memory_kib = 0u64;
    let mut any_real_failure = false;
    let mut any_unavailable = false;

    for spec in &snippet.meta.verify {
        let res = match spec {
            SnippetVerifySpec::SelfTest { file } => run_self_verify(
                ws,
                slug,
                file,
                &cache,
                &compiler,
                &snippets_inc,
                &dep_salt,
                &bundled_testlib,
            )?,
            SnippetVerifySpec::Problem { problem, adapter } => run_problem_verify(
                ws,
                slug,
                problem,
                adapter,
                opts,
                &cache,
                &compiler,
                &snippets_inc,
                &dep_salt,
                &bundled_testlib,
            )?,
            SnippetVerifySpec::LibraryChecker { problem, adapter } => run_library_checker_verify(
                ws,
                slug,
                problem,
                adapter,
                opts,
                &cache,
                &compiler,
                &snippets_inc,
                &dep_salt,
                &bundled_testlib,
            )?,
        };
        if !res.passed {
            if res.verdict == "UNAVAILABLE" {
                any_unavailable = true;
            } else {
                any_real_failure = true;
            }
        }
        total_time_ms = total_time_ms.saturating_add(res.time_ms);
        max_memory_kib = max_memory_kib.max(res.memory_kib);
        cases.push(res);
    }

    // Per review 1.4: network/fetch unavailability must never downgrade a previously
    // verified snippet to Failed. Preserve previous Passed status (and keep the valid
    // cached report on disk when content_hash matches).
    let status = if any_real_failure {
        SnippetVerifyStatus::Failed
    } else if any_unavailable {
        if let Some(prev) = &previous_report
            && prev.status == SnippetVerifyStatus::Passed
        {
            if prev.content_hash == content_hash {
                SnippetVerifyStatus::Passed
            } else {
                SnippetVerifyStatus::Stale
            }
        } else {
            SnippetVerifyStatus::Unverified
        }
    } else {
        SnippetVerifyStatus::Passed
    };

    let report = SnippetVerifyReport {
        slug: slug.to_owned(),
        content_hash: content_hash.clone(),
        status,
        verified_at_epoch_secs: now,
        total_time_ms,
        max_memory_kib,
        cases,
    };
    let should_preserve_prev_disk = !any_real_failure
        && any_unavailable
        && previous_report.as_ref().is_some_and(|p| {
            p.status == SnippetVerifyStatus::Passed && p.content_hash == content_hash
        });
    if !should_preserve_prev_disk {
        save_snippet_verify_report(ws, &report)?;
    }
    Ok(report)
}

/// Verifies `slug` and all snippets that transitively depend on it, in topological order.
pub fn verify_snippet_cascade(
    ws: &Workspace,
    slug: &str,
    opts: &SnippetVerifyOptions,
) -> Result<Vec<SnippetVerifyReport>> {
    let order = ws.snippet_dependents_topo(slug)?;
    let mut reports = Vec::with_capacity(order.len());
    for s in order {
        reports.push(verify_snippet(ws, &s, opts)?);
    }
    Ok(reports)
}

/// Verifies all snippets in the workspace in dependency order (upstream before downstream).
pub fn verify_all_snippets(
    ws: &Workspace,
    opts: &SnippetVerifyOptions,
) -> Result<Vec<SnippetVerifyReport>> {
    let slugs = ws.list_snippets()?;
    let dag = ws.snippet_dag()?;
    // Topological sort of all snippets.
    let mut ordered = Vec::with_capacity(slugs.len());
    let mut visited = std::collections::HashSet::new();
    for slug in &slugs {
        if let Ok(deps) = dag.transitive_deps(slug) {
            for d in deps {
                if visited.insert(d.clone()) {
                    ordered.push(d);
                }
            }
        }
        if visited.insert(slug.clone()) {
            ordered.push(slug.clone());
        }
    }
    let mut reports = Vec::with_capacity(ordered.len());
    for slug in ordered {
        reports.push(verify_snippet(ws, &slug, opts)?);
    }
    Ok(reports)
}

fn save_snippet_verify_report(ws: &Workspace, report: &SnippetVerifyReport) -> Result<()> {
    let path = snippet_verify_report_path(ws, &report.slug);
    let bytes = serde_json::to_vec_pretty(report)
        .map_err(|e| JudgeError::Internal(format!("serialize snippet verify report: {e}")))?;
    write_atomic(&path, &bytes)
}

fn compute_snippet_headers_salt(ws: &Workspace, slug: &str) -> Result<Vec<u8>> {
    let dag = ws.snippet_dag()?;
    let mut salt = Vec::new();
    if let Ok(bytes) = ws.read_bytes(&format!("snippets/{slug}/code.hpp")) {
        salt.extend_from_slice(slug.as_bytes());
        salt.push(0);
        salt.extend_from_slice(&bytes);
        salt.push(0);
    }
    for dep in dag.transitive_deps(slug)? {
        if let Ok(bytes) = ws.read_bytes(&format!("snippets/{dep}/code.hpp")) {
            salt.extend_from_slice(dep.as_bytes());
            salt.push(0);
            salt.extend_from_slice(&bytes);
            salt.push(0);
        }
    }
    Ok(salt)
}

// ---- 1. Self Test Verification ---------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn run_self_verify(
    ws: &Workspace,
    slug: &str,
    file: &str,
    cache: &JudgeCache,
    compiler: &DetectedCompiler,
    snippets_inc: &Path,
    dep_salt: &[u8],
    testlib: &ResolvedTestlib,
) -> Result<SnippetVerifyCaseResult> {
    let rel = format!("snippets/{slug}/{file}");
    let abs = ws.resolve(&rel)?;
    if !abs.is_file() {
        return Ok(SnippetVerifyCaseResult {
            kind: "self".to_owned(),
            target: file.to_owned(),
            adapter: None,
            passed: false,
            verdict: "JE".to_owned(),
            time_ms: 0,
            memory_kib: 0,
            passed_cases: 0,
            total_cases: 1,
            message: Some(format!("self-test file not found: {rel}")),
            compile_stderr: None,
        });
    }
    let src_bytes = fs::read(&abs).map_err(|e| io_err(format!("read {rel}"), e))?;
    let lang =
        SourceLanguage::from_path(&abs).unwrap_or(SourceLanguage::Cpp { std: CppStd::Cpp20 });

    let compiled = match compile_cached_with_includes(
        cache,
        compiler,
        lang,
        &format!("snippet-{slug}-self"),
        &rel,
        &src_bytes,
        Some(&abs),
        testlib,
        &[snippets_inc.to_path_buf()],
        dep_salt,
    ) {
        Ok(c) => c,
        Err(cf) => {
            return Ok(SnippetVerifyCaseResult {
                kind: "self".to_owned(),
                target: file.to_owned(),
                adapter: None,
                passed: false,
                verdict: "CE".to_owned(),
                time_ms: 0,
                memory_kib: 0,
                passed_cases: 0,
                total_cases: 1,
                message: Some("compilation failed".to_owned()),
                compile_stderr: Some(cf.stderr),
            });
        }
    };

    let outcome = run_process(
        &compiled.binary_path,
        &[],
        None,
        None,
        RunLimits::tool_defaults(),
        None,
    )?;
    let passed = outcome.verdict == Verdict::AC;
    let message = if passed {
        None
    } else if !outcome.stderr.trim().is_empty() {
        Some(clean_verify_stderr(&outcome.stderr))
    } else {
        Some(format!("退出状态：{}", outcome.verdict))
    };

    Ok(SnippetVerifyCaseResult {
        kind: "self".to_owned(),
        target: file.to_owned(),
        adapter: None,
        passed,
        verdict: outcome.verdict.as_str().to_owned(),
        time_ms: outcome.cpu_time_ms,
        memory_kib: outcome.peak_memory_kib,
        passed_cases: usize::from(passed),
        total_cases: 1,
        message,
        compile_stderr: None,
    })
}

// ---- 2. Workspace Problem Verification -------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn run_problem_verify(
    ws: &Workspace,
    slug: &str,
    problem_slug: &str,
    adapter: &str,
    opts: &SnippetVerifyOptions,
    cache: &JudgeCache,
    compiler: &DetectedCompiler,
    snippets_inc: &Path,
    dep_salt: &[u8],
    bundled_testlib: &ResolvedTestlib,
) -> Result<SnippetVerifyCaseResult> {
    let adapter_rel = format!("snippets/{slug}/{adapter}");
    let adapter_abs = ws.resolve(&adapter_rel)?;
    if !adapter_abs.is_file() {
        return Ok(SnippetVerifyCaseResult {
            kind: "problem".to_owned(),
            target: problem_slug.to_owned(),
            adapter: Some(adapter.to_owned()),
            passed: false,
            verdict: "JE".to_owned(),
            time_ms: 0,
            memory_kib: 0,
            passed_cases: 0,
            total_cases: 0,
            message: Some(format!("adapter file not found: {adapter_rel}")),
            compile_stderr: None,
        });
    }

    let problem = match ws.load_problem(problem_slug) {
        Ok(p) => p,
        Err(e) => {
            return Ok(SnippetVerifyCaseResult {
                kind: "problem".to_owned(),
                target: problem_slug.to_owned(),
                adapter: Some(adapter.to_owned()),
                passed: false,
                verdict: "JE".to_owned(),
                time_ms: 0,
                memory_kib: 0,
                passed_cases: 0,
                total_cases: 0,
                message: Some(format!("workspace problem `{problem_slug}` error: {e}")),
                compile_stderr: None,
            });
        }
    };

    let prob_dir = ws.resolve(&format!("problems/{problem_slug}"))?;
    let testlib = resolve_testlib(ws.root(), &prob_dir);

    // Collect test cases: prefer built `build/<problem>/tests/*.in` + `.ans` (building package if needed);
    // fall back to `problems/<problem>/samples/` for pure-statement problems.
    let mut test_pairs: Vec<(PathBuf, PathBuf)> =
        collect_in_ans_pairs(&build_dir_for(ws, problem_slug).join("tests"));
    if test_pairs.is_empty() && problem.meta.has_package_in_dir(&prob_dir) {
        let build_opts = BuildOptions {
            mode: BuildMode::Debug,
            cxx_override: opts.cxx_override.clone(),
            cache_dir: opts.cache_dir.clone(),
            jobs: opts.jobs,
            ..Default::default()
        };
        let _ = build_package(ws, problem_slug, &build_opts, None, None);
        test_pairs = collect_in_ans_pairs(&build_dir_for(ws, problem_slug).join("tests"));
    }
    if test_pairs.is_empty() {
        test_pairs = collect_in_ans_pairs(&prob_dir.join("samples"));
    }
    if test_pairs.is_empty() {
        return Ok(SnippetVerifyCaseResult {
            kind: "problem".to_owned(),
            target: problem_slug.to_owned(),
            adapter: Some(adapter.to_owned()),
            passed: false,
            verdict: "JE".to_owned(),
            time_ms: 0,
            memory_kib: 0,
            passed_cases: 0,
            total_cases: 0,
            message: Some(format!("no test cases found for problem `{problem_slug}`")),
            compile_stderr: None,
        });
    }

    // Compile adapter with `-I <ws>/snippets`.
    let adapter_bytes =
        fs::read(&adapter_abs).map_err(|e| io_err(format!("read {adapter_rel}"), e))?;
    let adapter_lang = SourceLanguage::from_path(&adapter_abs)
        .unwrap_or(SourceLanguage::Cpp { std: CppStd::Cpp20 });
    let adapter_bin = match compile_cached_with_includes(
        cache,
        compiler,
        adapter_lang,
        &format!("snippet-{slug}-prob-{problem_slug}"),
        &adapter_rel,
        &adapter_bytes,
        Some(&adapter_abs),
        bundled_testlib,
        &[snippets_inc.to_path_buf()],
        dep_salt,
    ) {
        Ok(c) => c.binary_path,
        Err(cf) => {
            return Ok(SnippetVerifyCaseResult {
                kind: "problem".to_owned(),
                target: problem_slug.to_owned(),
                adapter: Some(adapter.to_owned()),
                passed: false,
                verdict: "CE".to_owned(),
                time_ms: 0,
                memory_kib: 0,
                passed_cases: 0,
                total_cases: test_pairs.len(),
                message: Some("adapter compilation failed".to_owned()),
                compile_stderr: Some(cf.stderr),
            });
        }
    };

    // Compile checker.
    let checker_spec = resolve_checker_spec(&prob_dir, &problem.meta)?;
    let checker_bin = match compile_checker(cache, compiler, &checker_spec, &testlib) {
        Ok(b) => b,
        Err(msg) => {
            return Ok(SnippetVerifyCaseResult {
                kind: "problem".to_owned(),
                target: problem_slug.to_owned(),
                adapter: Some(adapter.to_owned()),
                passed: false,
                verdict: "JE".to_owned(),
                time_ms: 0,
                memory_kib: 0,
                passed_cases: 0,
                total_cases: test_pairs.len(),
                message: Some(msg),
                compile_stderr: None,
            });
        }
    };

    let limits = effective_run_limits(ws, problem_slug, None)?;
    run_adapter_on_cases(
        "problem",
        problem_slug,
        adapter,
        &adapter_bin,
        &checker_bin,
        &test_pairs,
        limits,
    )
}

fn compile_checker(
    cache: &JudgeCache,
    compiler: &DetectedCompiler,
    spec: &CheckerSpec,
    testlib: &ResolvedTestlib,
) -> std::result::Result<PathBuf, String> {
    match spec {
        CheckerSpec::Builtin(name) => {
            let src = standard_checker_source(name)
                .ok_or_else(|| format!("unknown builtin checker {name:?}"))?;
            compile_cached(
                cache,
                compiler,
                SourceLanguage::Cpp { std: CppStd::Cpp20 },
                &format!("checker-{name}"),
                name,
                src.as_bytes(),
                None,
                testlib,
            )
            .map(|c| c.binary_path)
            .map_err(|cf| format!("checker compile failed: {}", cf.stderr))
        }
        CheckerSpec::Custom { path, display_name } => {
            let bytes = fs::read(path).map_err(|e| format!("read checker {display_name}: {e}"))?;
            let lang = SourceLanguage::from_path(path)
                .unwrap_or(SourceLanguage::Cpp { std: CppStd::Cpp20 });
            compile_cached(
                cache,
                compiler,
                lang,
                display_name,
                display_name,
                &bytes,
                Some(path),
                testlib,
            )
            .map(|c| c.binary_path)
            .map_err(|cf| format!("checker compile failed: {}", cf.stderr))
        }
    }
}

fn collect_in_ans_pairs(dir: &Path) -> Vec<(PathBuf, PathBuf)> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut stems: Vec<String> = entries
        .filter_map(|e| e.ok())
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| !n.starts_with('.'))
        .filter_map(|n| n.strip_suffix(".in").map(str::to_owned))
        .collect();
    stems.sort();
    let mut out = Vec::new();
    for stem in stems {
        let in_path = dir.join(format!("{stem}.in"));
        let ans_path = dir.join(format!("{stem}.ans"));
        let out_path = dir.join(format!("{stem}.out"));
        if ans_path.is_file() {
            out.push((in_path, ans_path));
        } else if out_path.is_file() {
            out.push((in_path, out_path));
        }
    }
    out
}

fn run_adapter_on_cases(
    kind: &str,
    target: &str,
    adapter: &str,
    adapter_bin: &Path,
    checker_bin: &Path,
    cases: &[(PathBuf, PathBuf)],
    limits: RunLimits,
) -> Result<SnippetVerifyCaseResult> {
    let tmp = tempfile::tempdir().map_err(|e| io_err("create temp dir for snippet verify", e))?;
    let out_file = tmp.path().join("user.out");

    let mut passed_cases = 0usize;
    let mut max_time = 0u64;
    let mut max_mem = 0u64;
    let mut first_fail_verdict: Option<Verdict> = None;
    let mut first_fail_msg: Option<String> = None;

    for (idx, (in_path, ans_path)) in cases.iter().enumerate() {
        let run = run_process(
            adapter_bin,
            &[],
            Some(in_path),
            Some(&out_file),
            limits,
            None,
        )?;
        max_time = max_time.max(run.cpu_time_ms);
        max_mem = max_mem.max(run.peak_memory_kib);

        if run.verdict != Verdict::AC {
            if first_fail_verdict.is_none() {
                first_fail_verdict = Some(run.verdict);
                first_fail_msg = Some(format!(
                    "case #{}: {}{}",
                    idx + 1,
                    run.verdict,
                    if run.stderr.trim().is_empty() {
                        String::new()
                    } else {
                        format!(" ({})", run.stderr.trim())
                    }
                ));
            }
            continue;
        }

        let chk = run_checker_with_score(checker_bin, in_path, &out_file, ans_path, None)?;
        if chk.verdict == Verdict::AC {
            passed_cases += 1;
        } else if first_fail_verdict.is_none() {
            first_fail_verdict = Some(chk.verdict);
            first_fail_msg = Some(format!(
                "case #{}: {} ({})",
                idx + 1,
                chk.verdict,
                chk.message
            ));
        }
    }

    let total_cases = cases.len();
    let passed = passed_cases == total_cases && total_cases > 0;
    let verdict = if passed {
        "AC".to_owned()
    } else {
        first_fail_verdict
            .unwrap_or(Verdict::WA)
            .as_str()
            .to_owned()
    };

    Ok(SnippetVerifyCaseResult {
        kind: kind.to_owned(),
        target: target.to_owned(),
        adapter: Some(adapter.to_owned()),
        passed,
        verdict,
        time_ms: max_time,
        memory_kib: max_mem,
        passed_cases,
        total_cases,
        message: first_fail_msg,
        compile_stderr: None,
    })
}

// ---- 3. Library Checker Global-Cached Verification -------------------------------------

#[derive(Debug, Deserialize)]
struct LcInfoToml {
    #[serde(default = "default_lc_timelimit")]
    timelimit: f64,
    #[serde(default)]
    tests: Vec<LcTestEntry>,
    #[serde(default)]
    params: BTreeMap<String, toml::Value>,
}

fn default_lc_timelimit() -> f64 {
    5.0
}

#[derive(Debug, Deserialize)]
struct LcTestEntry {
    name: String,
    #[serde(default = "default_one")]
    number: u32,
}

fn default_one() -> u32 {
    1
}

/// Known top-level problem category directories in `yosupo06/library-checker-problems`.
const LC_CATEGORIES: &[&str] = &[
    "sample",
    "data_structure",
    "graph",
    "tree",
    "math",
    "convolution",
    "polynomial",
    "number_theory",
    "string",
    "geo",
    "linear_algebra",
    "enumerative_combinatorics",
    "set_power_series",
    "big_integer",
    "other",
];

#[allow(clippy::too_many_arguments)]
fn run_library_checker_verify(
    ws: &Workspace,
    slug: &str,
    lc_problem: &str,
    adapter: &str,
    opts: &SnippetVerifyOptions,
    cache: &JudgeCache,
    compiler: &DetectedCompiler,
    snippets_inc: &Path,
    dep_salt: &[u8],
    bundled_testlib: &ResolvedTestlib,
) -> Result<SnippetVerifyCaseResult> {
    let adapter_rel = format!("snippets/{slug}/{adapter}");
    let adapter_abs = ws.resolve(&adapter_rel)?;
    if !adapter_abs.is_file() {
        return Ok(SnippetVerifyCaseResult {
            kind: "library_checker".to_owned(),
            target: lc_problem.to_owned(),
            adapter: Some(adapter.to_owned()),
            passed: false,
            verdict: "JE".to_owned(),
            time_ms: 0,
            memory_kib: 0,
            passed_cases: 0,
            total_cases: 0,
            message: Some(format!("adapter file not found: {adapter_rel}")),
            compile_stderr: None,
        });
    }

    // 1. Ensure lightweight problem sources exist in `~/.cache/xcpc/library-checker/<problem>/`.
    let lc_dir =
        match ensure_library_checker_problem_cached(cache, lc_problem, &opts.library_checker) {
            Ok(dir) => dir,
            Err(LcFetchError::Network(msg)) => {
                return Ok(SnippetVerifyCaseResult {
                    kind: "library_checker".to_owned(),
                    target: lc_problem.to_owned(),
                    adapter: Some(adapter.to_owned()),
                    passed: false,
                    verdict: "UNAVAILABLE".to_owned(),
                    time_ms: 0,
                    memory_kib: 0,
                    passed_cases: 0,
                    total_cases: 0,
                    message: Some(msg),
                    compile_stderr: None,
                });
            }
            Err(LcFetchError::NotFound(msg) | LcFetchError::Other(msg)) => {
                return Ok(SnippetVerifyCaseResult {
                    kind: "library_checker".to_owned(),
                    target: lc_problem.to_owned(),
                    adapter: Some(adapter.to_owned()),
                    passed: false,
                    verdict: "JE".to_owned(),
                    time_ms: 0,
                    memory_kib: 0,
                    passed_cases: 0,
                    total_cases: 0,
                    message: Some(msg),
                    compile_stderr: None,
                });
            }
        };

    // 2. Parse `info.toml` and materialize `params.h` if needed.
    let info_raw = fs::read_to_string(lc_dir.join("info.toml"))
        .map_err(|e| io_err("read library-checker info.toml", e))?;
    let info: LcInfoToml = toml::from_str(&info_raw).map_err(|e| {
        JudgeError::InvalidConfig(format!("invalid library-checker info.toml: {e}"))
    })?;

    let params_h = lc_dir.join("params.h");
    if !params_h.is_file() {
        let mut content = String::new();
        for (k, v) in &info.params {
            match v {
                toml::Value::Integer(n) => {
                    content.push_str(&format!("#define {k} {n}LL\n"));
                }
                toml::Value::Float(f) => {
                    content.push_str(&format!("#define {k} {f}\n"));
                }
                toml::Value::String(s) => {
                    content.push_str(&format!("#define {k} \"{s}\"\n"));
                }
                _ => {}
            }
        }
        write_atomic(&params_h, content.as_bytes())?;
    }

    let common_dir = lc_dir.join("common");
    let lc_inc_dirs = vec![common_dir.clone(), lc_dir.clone()];
    let lc_testlib = if common_dir.join("testlib.h").is_file() {
        let testlib_file = common_dir.join("testlib.h");
        let bytes = fs::read(&testlib_file)
            .map_err(|e| io_err("read library-checker common/testlib.h", e))?;
        ResolvedTestlib::UserFile {
            file_path: testlib_file,
            include_dir: common_dir.clone(),
            content_hash: xcpc_storage::content_hash(&bytes),
        }
    } else {
        bundled_testlib.clone()
    };

    // 3. Compile checker.cpp & sol/correct.cpp natively via xcpc-judge cache.
    let checker_src = lc_dir.join("checker.cpp");
    let checker_bytes =
        fs::read(&checker_src).map_err(|e| io_err(format!("read {}", checker_src.display()), e))?;
    let checker_bin = match compile_cached_with_includes(
        cache,
        compiler,
        SourceLanguage::Cpp { std: CppStd::Cpp20 },
        &format!("lc-{lc_problem}-checker"),
        "checker.cpp",
        &checker_bytes,
        Some(&checker_src),
        &lc_testlib,
        &lc_inc_dirs,
        &[],
    ) {
        Ok(c) => c.binary_path,
        Err(cf) => {
            return Ok(SnippetVerifyCaseResult {
                kind: "library_checker".to_owned(),
                target: lc_problem.to_owned(),
                adapter: Some(adapter.to_owned()),
                passed: false,
                verdict: "JE".to_owned(),
                time_ms: 0,
                memory_kib: 0,
                passed_cases: 0,
                total_cases: 0,
                message: Some(format!(
                    "library-checker checker compile failed: {}",
                    cf.stderr
                )),
                compile_stderr: None,
            });
        }
    };

    let sol_src = lc_dir.join("sol").join("correct.cpp");
    let sol_bytes =
        fs::read(&sol_src).map_err(|e| io_err(format!("read {}", sol_src.display()), e))?;
    let sol_bin = match compile_cached_with_includes(
        cache,
        compiler,
        SourceLanguage::Cpp { std: CppStd::Cpp20 },
        &format!("lc-{lc_problem}-correct"),
        "sol/correct.cpp",
        &sol_bytes,
        Some(&sol_src),
        &lc_testlib,
        &lc_inc_dirs,
        &[],
    ) {
        Ok(c) => c.binary_path,
        Err(cf) => {
            return Ok(SnippetVerifyCaseResult {
                kind: "library_checker".to_owned(),
                target: lc_problem.to_owned(),
                adapter: Some(adapter.to_owned()),
                passed: false,
                verdict: "JE".to_owned(),
                time_ms: 0,
                memory_kib: 0,
                passed_cases: 0,
                total_cases: 0,
                message: Some(format!(
                    "library-checker solution compile failed: {}",
                    cf.stderr
                )),
                compile_stderr: None,
            });
        }
    };

    // 4. Generate (or reuse cached) test cases in `<lc_dir>/generated/`.
    let gen_out_dir = lc_dir.join("generated");
    fs::create_dir_all(&gen_out_dir).map_err(|e| io_err("create lc generated dir", e))?;
    let mut test_pairs: Vec<(PathBuf, PathBuf)> = Vec::new();
    let tool_limits = RunLimits::tool_defaults();

    for entry in &info.tests {
        if entry.name.ends_with(".cpp") {
            let gen_src = lc_dir.join("gen").join(&entry.name);
            let gen_bytes =
                fs::read(&gen_src).map_err(|e| io_err(format!("read {}", gen_src.display()), e))?;
            let gen_bin = match compile_cached_with_includes(
                cache,
                compiler,
                SourceLanguage::Cpp { std: CppStd::Cpp20 },
                &format!("lc-{lc_problem}-gen-{}", entry.name),
                &entry.name,
                &gen_bytes,
                Some(&gen_src),
                &lc_testlib,
                &lc_inc_dirs,
                &[],
            ) {
                Ok(c) => c.binary_path,
                Err(cf) => {
                    return Ok(SnippetVerifyCaseResult {
                        kind: "library_checker".to_owned(),
                        target: lc_problem.to_owned(),
                        adapter: Some(adapter.to_owned()),
                        passed: false,
                        verdict: "JE".to_owned(),
                        time_ms: 0,
                        memory_kib: 0,
                        passed_cases: 0,
                        total_cases: 0,
                        message: Some(format!(
                            "library-checker generator {} compile failed: {}",
                            entry.name, cf.stderr
                        )),
                        compile_stderr: None,
                    });
                }
            };
            let stem = entry.name.trim_end_matches(".cpp");
            for seed in 0..entry.number {
                let in_path = gen_out_dir.join(format!("{stem}_{seed:02}.in"));
                let out_path = gen_out_dir.join(format!("{stem}_{seed:02}.out"));
                if !in_path.is_file() {
                    let run = run_process(
                        &gen_bin,
                        &[seed.to_string()],
                        None,
                        Some(&in_path),
                        tool_limits,
                        None,
                    )?;
                    if run.verdict != Verdict::AC {
                        let _ = fs::remove_file(&in_path);
                        return Ok(SnippetVerifyCaseResult {
                            kind: "library_checker".to_owned(),
                            target: lc_problem.to_owned(),
                            adapter: Some(adapter.to_owned()),
                            passed: false,
                            verdict: "JE".to_owned(),
                            time_ms: 0,
                            memory_kib: 0,
                            passed_cases: 0,
                            total_cases: 0,
                            message: Some(format!(
                                "generator {} seed {seed} failed: {}",
                                entry.name, run.verdict
                            )),
                            compile_stderr: None,
                        });
                    }
                }
                if !out_path.is_file() {
                    let run = run_process(
                        &sol_bin,
                        &[],
                        Some(&in_path),
                        Some(&out_path),
                        tool_limits,
                        None,
                    )?;
                    if run.verdict != Verdict::AC {
                        let _ = fs::remove_file(&out_path);
                        return Ok(SnippetVerifyCaseResult {
                            kind: "library_checker".to_owned(),
                            target: lc_problem.to_owned(),
                            adapter: Some(adapter.to_owned()),
                            passed: false,
                            verdict: "JE".to_owned(),
                            time_ms: 0,
                            memory_kib: 0,
                            passed_cases: 0,
                            total_cases: 0,
                            message: Some(format!(
                                "reference solution failed on {stem}_{seed:02}: {}",
                                run.verdict
                            )),
                            compile_stderr: None,
                        });
                    }
                }
                test_pairs.push((in_path, out_path));
            }
        } else if entry.name.ends_with(".in") {
            let stem = entry.name.trim_end_matches(".in");
            let mut static_inputs: Vec<(String, PathBuf)> = Vec::new();
            for idx in 0..entry.number {
                let numbered_name = format!("{stem}_{idx:02}.in");
                let numbered_path = lc_dir.join("in").join(&numbered_name);
                if numbered_path.is_file() {
                    static_inputs.push((format!("{stem}_{idx:02}"), numbered_path));
                }
            }
            if static_inputs.is_empty() {
                let direct_path = lc_dir.join("in").join(&entry.name);
                if direct_path.is_file() {
                    static_inputs.push((stem.to_owned(), direct_path));
                }
            }
            for (case_stem, src_in) in static_inputs {
                let out_path = gen_out_dir.join(format!("{case_stem}.out"));
                if !out_path.is_file() {
                    let run = run_process(
                        &sol_bin,
                        &[],
                        Some(&src_in),
                        Some(&out_path),
                        tool_limits,
                        None,
                    )?;
                    if run.verdict != Verdict::AC {
                        let _ = fs::remove_file(&out_path);
                        return Ok(SnippetVerifyCaseResult {
                            kind: "library_checker".to_owned(),
                            target: lc_problem.to_owned(),
                            adapter: Some(adapter.to_owned()),
                            passed: false,
                            verdict: "JE".to_owned(),
                            time_ms: 0,
                            memory_kib: 0,
                            passed_cases: 0,
                            total_cases: 0,
                            message: Some(format!(
                                "reference solution failed on {case_stem}: {}",
                                run.verdict
                            )),
                            compile_stderr: None,
                        });
                    }
                }
                test_pairs.push((src_in, out_path));
            }
        }
    }

    if test_pairs.is_empty() {
        return Ok(SnippetVerifyCaseResult {
            kind: "library_checker".to_owned(),
            target: lc_problem.to_owned(),
            adapter: Some(adapter.to_owned()),
            passed: false,
            verdict: "JE".to_owned(),
            time_ms: 0,
            memory_kib: 0,
            passed_cases: 0,
            total_cases: 0,
            message: Some(format!(
                "no test cases generated for library-checker problem `{lc_problem}`"
            )),
            compile_stderr: None,
        });
    }

    // 5. Compile user's adapter with `-I <ws>/snippets`.
    let adapter_bytes =
        fs::read(&adapter_abs).map_err(|e| io_err(format!("read {adapter_rel}"), e))?;
    let adapter_lang = SourceLanguage::from_path(&adapter_abs)
        .unwrap_or(SourceLanguage::Cpp { std: CppStd::Cpp20 });
    let adapter_bin = match compile_cached_with_includes(
        cache,
        compiler,
        adapter_lang,
        &format!("snippet-{slug}-lc-{lc_problem}"),
        &adapter_rel,
        &adapter_bytes,
        Some(&adapter_abs),
        bundled_testlib,
        &[snippets_inc.to_path_buf()],
        dep_salt,
    ) {
        Ok(c) => c.binary_path,
        Err(cf) => {
            return Ok(SnippetVerifyCaseResult {
                kind: "library_checker".to_owned(),
                target: lc_problem.to_owned(),
                adapter: Some(adapter.to_owned()),
                passed: false,
                verdict: "CE".to_owned(),
                time_ms: 0,
                memory_kib: 0,
                passed_cases: 0,
                total_cases: test_pairs.len(),
                message: Some("adapter compilation failed".to_owned()),
                compile_stderr: Some(cf.stderr),
            });
        }
    };

    let tl_ms = (info.timelimit.max(0.5) * 1000.0).ceil() as u64;
    let limits = RunLimits {
        time_limit_ms: tl_ms,
        memory_limit_kib: 1024 * 1024, // Library Checker standard: 1024 MiB
        fsize_limit_bytes: 256 * 1024 * 1024,
        wall_limit_ms: tl_ms.saturating_mul(2).saturating_add(2000).max(5000),
        disable_as_limit: false,
    };

    run_adapter_on_cases(
        "library_checker",
        lc_problem,
        adapter,
        &adapter_bin,
        &checker_bin,
        &test_pairs,
        limits,
    )
}

#[derive(Debug)]
enum LcFetchError {
    Network(String),
    NotFound(String),
    Other(String),
}

/// Strips cached executable path / hash prefixes from runtime stderr (e.g. glibc `assert()`
/// output like `/home/.../.cache/xcpc/bin/snippet-modint-self-deadbeef: test.cpp:7: ...`)
/// so the UI displays a clean, readable failure reason.
fn clean_verify_stderr(raw: &str) -> String {
    let cleaned_lines: Vec<String> = raw
        .lines()
        .map(|line| {
            let trimmed = line.trim();
            // Strip leading `<cached-binary-path>: ` prefix emitted by glibc assert()
            if let Some(rest) = trimmed.strip_prefix('/')
                && let Some((head, tail)) = rest.split_once(": ")
                && (head.contains("/.cache/")
                    || head.contains("/bin/")
                    || head.contains("/snippets/"))
            {
                return strip_snippet_path_prefix(tail);
            }
            strip_snippet_path_prefix(trimmed)
        })
        .filter(|l| !l.is_empty())
        .collect();
    cleaned_lines.join("\n")
}

fn strip_snippet_path_prefix(s: &str) -> String {
    if let Some(idx) = s.find("/snippets/") {
        let after = &s[idx + "/snippets/".len()..];
        if let Some((_, file_part)) = after.split_once('/') {
            return file_part.to_owned();
        }
    }
    s.to_owned()
}

/// Ensures lightweight problem files for `lc_problem` exist in
/// `<cache_root>/library-checker/<safe_name>/`.
///
/// Order of resolution:
/// 1. Already cached on disk (`info.toml` + `checker.cpp` + `sol/correct.cpp`) -> offline hit.
/// 2. Local repo (`config.local_repo_path` or `XCPC_LC_REPO`).
/// 3. Remote HTTP fetch from `yosupo06/library-checker-problems` (with optional `mirror_url` / `XCPC_LC_MIRROR`).
fn ensure_library_checker_problem_cached(
    cache: &JudgeCache,
    lc_problem: &str,
    config: &LibraryCheckerConfig,
) -> std::result::Result<PathBuf, LcFetchError> {
    let trimmed = lc_problem.trim().trim_matches('/');
    if trimmed.is_empty() || trimmed.contains("..") || trimmed.contains('\\') {
        return Err(LcFetchError::Other(format!(
            "无效的 Library Checker 题名：{lc_problem}"
        )));
    }
    let cache_key = trimmed.rsplit('/').next().unwrap_or(trimmed);
    let dest = cache.root().join("library-checker").join(cache_key);

    if dest.join("info.toml").is_file()
        && dest.join("checker.cpp").is_file()
        && dest.join("sol").join("correct.cpp").is_file()
    {
        return Ok(dest);
    }

    // Check local repo (config takes precedence over env var `XCPC_LC_REPO`).
    let local_repo = config
        .local_repo_path
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var("XCPC_LC_REPO")
                .ok()
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty())
                .map(PathBuf::from)
        });

    if let Some(repo) = local_repo {
        return populate_lc_from_local_repo(&repo, trimmed, &dest).map_err(LcFetchError::Other);
    }

    // Remote HTTP fetch via ureq (with optional mirror prefix).
    let mirror = config
        .mirror_url
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .or_else(|| {
            std::env::var("XCPC_LC_MIRROR")
                .ok()
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty())
        });

    populate_lc_from_remote(trimmed, mirror.as_deref(), &dest)
}

fn populate_lc_from_local_repo(
    repo: &Path,
    problem: &str,
    dest: &Path,
) -> std::result::Result<PathBuf, String> {
    let prob_dir = find_lc_problem_in_repo(repo, problem).ok_or_else(|| {
        format!(
            "library-checker problem `{problem}` not found in local repo {}",
            repo.display()
        )
    })?;

    copy_file_to(&prob_dir.join("info.toml"), &dest.join("info.toml"))?;
    copy_file_to(&prob_dir.join("checker.cpp"), &dest.join("checker.cpp"))?;
    copy_file_to(
        &prob_dir.join("sol").join("correct.cpp"),
        &dest.join("sol").join("correct.cpp"),
    )?;

    // Copy optional params.h, gen/*, in/*, and repo/common/*.
    if prob_dir.join("params.h").is_file() {
        copy_file_to(&prob_dir.join("params.h"), &dest.join("params.h"))?;
    }
    copy_dir_files(&prob_dir.join("gen"), &dest.join("gen"))?;
    copy_dir_files(&prob_dir.join("in"), &dest.join("in"))?;
    copy_dir_files(&repo.join("common"), &dest.join("common"))?;

    Ok(dest.to_path_buf())
}

fn find_lc_problem_in_repo(repo: &Path, problem: &str) -> Option<PathBuf> {
    let direct = repo.join(problem);
    if direct.join("info.toml").is_file() {
        return Some(direct);
    }
    let entries = fs::read_dir(repo).ok()?;
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            let cand = p.join(problem);
            if cand.join("info.toml").is_file() {
                return Some(cand);
            }
        }
    }
    None
}

fn copy_file_to(src: &Path, dst: &Path) -> std::result::Result<(), String> {
    let bytes = fs::read(src).map_err(|e| format!("read {}: {e}", src.display()))?;
    write_atomic(dst, &bytes).map_err(|e| format!("write {}: {e}", dst.display()))
}

fn copy_dir_files(src_dir: &Path, dst_dir: &Path) -> std::result::Result<(), String> {
    if !src_dir.is_dir() {
        return Ok(());
    }
    let entries =
        fs::read_dir(src_dir).map_err(|e| format!("read dir {}: {e}", src_dir.display()))?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file()
            && let Some(name) = path.file_name()
        {
            copy_file_to(&path, &dst_dir.join(name))?;
        }
    }
    Ok(())
}

fn populate_lc_from_remote(
    problem: &str,
    mirror_prefix: Option<&str>,
    dest: &Path,
) -> std::result::Result<PathBuf, LcFetchError> {
    let raw_base = match mirror_prefix {
        Some(prefix) => format!(
            "{}/yosupo06/library-checker-problems/master",
            prefix.trim_end_matches('/')
        ),
        None => {
            "https://raw.githubusercontent.com/yosupo06/library-checker-problems/master".to_owned()
        }
    };

    // Resolve `<category>/<problem>` (either explicitly provided or probed across categories).
    let (repo_rel, info_bytes) = if problem.contains('/') {
        let url = format!("{raw_base}/{problem}/info.toml");
        let bytes = http_get_bytes(&url)?;
        (problem.to_owned(), bytes)
    } else {
        let mut found = None;
        for cat in LC_CATEGORIES {
            let rel = format!("{cat}/{problem}");
            let url = format!("{raw_base}/{rel}/info.toml");
            match http_get_bytes(&url) {
                Ok(bytes) => {
                    found = Some((rel, bytes));
                    break;
                }
                Err(LcFetchError::NotFound(_)) => continue,
                Err(LcFetchError::Network(msg)) => return Err(LcFetchError::Network(msg)),
                Err(other) => return Err(other),
            }
        }
        found.ok_or_else(|| {
            LcFetchError::NotFound(format!("未在 Library Checker 中找到题目「{problem}」"))
        })?
    };

    write_atomic(&dest.join("info.toml"), &info_bytes)
        .map_err(|e| LcFetchError::Other(e.to_string()))?;
    let info_str = String::from_utf8(info_bytes)
        .map_err(|e| LcFetchError::Other(format!("invalid UTF-8 in remote info.toml: {e}")))?;
    let info: LcInfoToml = toml::from_str(&info_str)
        .map_err(|e| LcFetchError::Other(format!("parse remote info.toml: {e}")))?;

    // Fetch checker.cpp, sol/correct.cpp, and common/random.h.
    for rel in ["checker.cpp", "sol/correct.cpp"] {
        let url = format!("{raw_base}/{repo_rel}/{rel}");
        let bytes = http_get_bytes(&url)?;
        write_atomic(&dest.join(rel), &bytes).map_err(|e| LcFetchError::Other(e.to_string()))?;
    }
    for common_file in ["random.h", "testlib.h", "fastio.h"] {
        let url = format!("{raw_base}/common/{common_file}");
        if let Ok(bytes) = http_get_bytes(&url) {
            write_atomic(&dest.join("common").join(common_file), &bytes)
                .map_err(|e| LcFetchError::Other(e.to_string()))?;
        }
    }

    // Fetch each generator or static input listed in `info.toml`.
    // Upstream `library-checker-problems` stores static `.in` entries as `in/<stem>_00.in`,
    // `in/<stem>_01.in`, ... (or `in/<name>` for single-file repos). Missing optional static
    // inputs are skipped gracefully so `gen/*.cpp` test generation still succeeds.
    for entry in &info.tests {
        if entry.name.ends_with(".cpp") {
            let url = format!("{raw_base}/{repo_rel}/gen/{}", entry.name);
            let bytes = http_get_bytes(&url)?;
            write_atomic(&dest.join("gen").join(&entry.name), &bytes)
                .map_err(|e| LcFetchError::Other(e.to_string()))?;
        } else if entry.name.ends_with(".in") {
            let stem = entry.name.trim_end_matches(".in");
            let mut fetched_any = false;
            for idx in 0..entry.number {
                let numbered = format!("{stem}_{idx:02}.in");
                let url = format!("{raw_base}/{repo_rel}/in/{numbered}");
                if let Ok(bytes) = http_get_bytes(&url) {
                    write_atomic(&dest.join("in").join(&numbered), &bytes)
                        .map_err(|e| LcFetchError::Other(e.to_string()))?;
                    fetched_any = true;
                }
            }
            if !fetched_any {
                let url = format!("{raw_base}/{repo_rel}/in/{}", entry.name);
                if let Ok(bytes) = http_get_bytes(&url) {
                    write_atomic(&dest.join("in").join(&entry.name), &bytes)
                        .map_err(|e| LcFetchError::Other(e.to_string()))?;
                }
            }
        }
    }

    Ok(dest.to_path_buf())
}

fn http_get_bytes(url: &str) -> std::result::Result<Vec<u8>, LcFetchError> {
    let resp = ureq::get(url)
        .timeout(std::time::Duration::from_secs(10))
        .call()
        .map_err(|e| match e {
            ureq::Error::Status(404, _) => LcFetchError::NotFound(format!("HTTP 404: {url}")),
            ureq::Error::Status(code, _) => LcFetchError::Network(format!(
                "连不上 Library Checker（HTTP {code}），可以在设置里配置镜像或本地目录"
            )),
            ureq::Error::Transport(_) => LcFetchError::Network(
                "连不上 Library Checker，可以在设置里配置镜像或本地目录".to_owned(),
            ),
        })?;
    let mut reader = resp.into_reader();
    let mut buf = Vec::new();
    std::io::Read::read_to_end(&mut reader, &mut buf).map_err(|_| {
        LcFetchError::Network(
            "读取 Library Checker 响应失败，可以在设置里配置镜像或本地目录".to_owned(),
        )
    })?;
    Ok(buf)
}

fn now_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use xcpc_storage::init_example_workspace;

    #[test]
    fn self_verify_and_cascade_staleness_on_example_snippets() {
        if resolve_active_compiler(None).is_err() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let cache_dir = tempfile::tempdir().unwrap();
        let ws = init_example_workspace(dir.path()).unwrap();

        // Initially unverified.
        assert_eq!(
            snippet_verify_status(&ws, "modint"),
            SnippetVerifyStatus::Unverified
        );
        assert_eq!(
            snippet_verify_status(&ws, "dsu"),
            SnippetVerifyStatus::Unverified
        );

        let opts = SnippetVerifyOptions {
            cache_dir: Some(cache_dir.path().to_path_buf()),
            ..Default::default()
        };

        // Verify modint & dsu (both have self tests).
        let mod_rep = verify_snippet(&ws, "modint", &opts).unwrap();
        assert_eq!(mod_rep.status, SnippetVerifyStatus::Passed);
        assert_eq!(mod_rep.cases.len(), 1);
        assert!(mod_rep.cases[0].passed);
        assert_eq!(
            snippet_verify_status(&ws, "modint"),
            SnippetVerifyStatus::Passed
        );

        let dsu_rep = verify_snippet(&ws, "dsu", &opts).unwrap();
        assert_eq!(dsu_rep.status, SnippetVerifyStatus::Passed);

        // Create a downstream snippet `poly_self` that depends on `modint` with only a self test.
        ws.write(
            "snippets/poly_self/snippet.toml",
            b"title = \"Poly Self\"\n\n[[verify]]\nkind = \"self\"\nfile = \"test.cpp\"\n",
        )
        .unwrap();
        ws.write(
            "snippets/poly_self/code.hpp",
            b"#pragma once\n#include \"modint/code.hpp\"\ninline mint sq(mint x) { return x * x; }\n",
        )
        .unwrap();
        ws.write(
            "snippets/poly_self/test.cpp",
            b"#include \"poly_self/code.hpp\"\n#include <cassert>\nint main() { assert(sq(3).v == 9); return 0; }\n",
        )
        .unwrap();

        let poly_rep = verify_snippet(&ws, "poly_self", &opts).unwrap();
        assert_eq!(poly_rep.status, SnippetVerifyStatus::Passed);

        // Edit upstream `modint/code.hpp` -> both `modint` and downstream `poly_self` become Stale!
        let mut mod_code = ws.read_text("snippets/modint/code.hpp").unwrap();
        mod_code.push_str("// upstream tweak\n");
        ws.write("snippets/modint/code.hpp", mod_code.as_bytes())
            .unwrap();

        assert_eq!(
            snippet_verify_status(&ws, "modint"),
            SnippetVerifyStatus::Stale
        );
        assert_eq!(
            snippet_verify_status(&ws, "poly_self"),
            SnippetVerifyStatus::Stale
        );
        // Unrelated `dsu` remains Passed.
        assert_eq!(
            snippet_verify_status(&ws, "dsu"),
            SnippetVerifyStatus::Passed
        );

        // Cascade verify from `modint` re-verifies `modint` and downstream `poly_self` (and `ntt`).
        // Remove `ntt`'s remote library_checker spec here so cascade stays 100% offline.
        ws.write(
            "snippets/ntt/snippet.toml",
            b"title = \"NTT\"\n\n[[verify]]\nkind = \"self\"\nfile = \"test.cpp\"\n",
        )
        .unwrap();

        let cascade = verify_snippet_cascade(&ws, "modint", &opts).unwrap();
        let slugs: Vec<&str> = cascade.iter().map(|r| r.slug.as_str()).collect();
        assert_eq!(slugs[0], "modint");
        assert!(slugs.contains(&"poly_self"));
        assert!(slugs.contains(&"ntt"));
        assert!(
            cascade
                .iter()
                .all(|r| r.status == SnippetVerifyStatus::Passed)
        );
        assert_eq!(
            snippet_verify_status(&ws, "poly_self"),
            SnippetVerifyStatus::Passed
        );
    }

    #[test]
    fn problem_verify_against_workspace_problem() {
        if resolve_active_compiler(None).is_err() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let cache_dir = tempfile::tempdir().unwrap();
        let ws = xcpc_storage::init_workspace(dir.path()).unwrap();

        // Create a workspace problem `a-plus-b` with samples.
        ws.write(
            "problems/a-plus-b/problem.toml",
            b"title = \"A+B\"\n[limits]\ntime = \"1s\"\nmemory = \"256MB\"\n",
        )
        .unwrap();
        ws.write("problems/a-plus-b/samples/01.in", b"2 3\n")
            .unwrap();
        ws.write("problems/a-plus-b/samples/01.ans", b"5\n")
            .unwrap();
        ws.write("problems/a-plus-b/samples/02.in", b"-10 42\n")
            .unwrap();
        ws.write("problems/a-plus-b/samples/02.ans", b"32\n")
            .unwrap();

        // Create a snippet verified against `a-plus-b`.
        ws.write(
            "snippets/adder/snippet.toml",
            b"title = \"Adder\"\n\n[[verify]]\nkind = \"problem\"\nproblem = \"a-plus-b\"\nadapter = \"verify_ab.cpp\"\n",
        )
        .unwrap();
        ws.write(
            "snippets/adder/code.hpp",
            b"#pragma once\ninline long long add_ll(long long a, long long b) { return a + b; }\n",
        )
        .unwrap();
        ws.write(
            "snippets/adder/verify_ab.cpp",
            b"#include \"adder/code.hpp\"\n#include <iostream>\nint main() { long long a, b; std::cin >> a >> b; std::cout << add_ll(a, b) << '\\n'; }\n",
        )
        .unwrap();

        let opts = SnippetVerifyOptions {
            cache_dir: Some(cache_dir.path().to_path_buf()),
            ..Default::default()
        };
        let rep = verify_snippet(&ws, "adder", &opts).unwrap();
        assert_eq!(rep.status, SnippetVerifyStatus::Passed);
        assert_eq!(rep.cases.len(), 1);
        assert_eq!(rep.cases[0].passed_cases, 2);
        assert_eq!(rep.cases[0].total_cases, 2);
    }

    #[test]
    fn library_checker_verify_using_local_mock_repo_offline() {
        if resolve_active_compiler(None).is_err() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let cache_dir = tempfile::tempdir().unwrap();
        let mock_repo = tempfile::tempdir().unwrap();
        let ws = init_example_workspace(dir.path()).unwrap();

        // Build a lightweight offline mock `library-checker-problems/convolution/convolution_mod/`.
        let prob_dir = mock_repo.path().join("convolution").join("convolution_mod");
        fs::create_dir_all(prob_dir.join("gen")).unwrap();
        fs::create_dir_all(prob_dir.join("in")).unwrap();
        fs::create_dir_all(prob_dir.join("sol")).unwrap();
        fs::create_dir_all(mock_repo.path().join("common")).unwrap();

        // Upstream-style numbered static inputs (`example_00.in`, `example_01.in`).
        fs::write(
            prob_dir.join("in").join("example_00.in"),
            "4 3\n1 2 3 4\n5 6 7\n",
        )
        .unwrap();
        fs::write(
            prob_dir.join("in").join("example_01.in"),
            "1 1\n10000000\n10000000\n",
        )
        .unwrap();

        fs::write(
            mock_repo.path().join("common").join("random.h"),
            r#"#pragma once
#include <cstdint>
struct Random {
    uint64_t s;
    explicit Random(uint64_t seed) : s(seed + 1) {}
    uint64_t next() { s ^= s << 13; s ^= s >> 7; s ^= s << 17; return s; }
    int uniform(int l, int r) { return l + int(next() % uint64_t(r - l + 1)); }
};
"#,
        )
        .unwrap();

        fs::write(
            prob_dir.join("info.toml"),
            r#"title = "Convolution"
timelimit = 5.0

[[tests]]
name = "example.in"
number = 2

[[tests]]
name = "missing_optional.in"
number = 1

[[tests]]
name = "small.cpp"
number = 3

[params]
MOD = 998244353
"#,
        )
        .unwrap();

        fs::write(
            prob_dir.join("gen").join("small.cpp"),
            r#"#include "random.h"
#include "params.h"
#include <cstdlib>
#include <iostream>
int main(int argc, char** argv) {
    int seed = argc > 1 ? std::atoi(argv[1]) : 0;
    Random rng(seed);
    int n = rng.uniform(1, 12);
    int m = rng.uniform(1, 12);
    std::cout << n << ' ' << m << '\n';
    for (int i = 0; i < n; ++i) std::cout << rng.uniform(0, 1000) << (i + 1 == n ? '\n' : ' ');
    for (int i = 0; i < m; ++i) std::cout << rng.uniform(0, 1000) << (i + 1 == m ? '\n' : ' ');
    return 0;
}
"#,
        )
        .unwrap();

        fs::write(
            prob_dir.join("sol").join("correct.cpp"),
            r#"#include <cstdint>
#include <iostream>
#include <vector>
int main() {
    int n, m;
    std::cin >> n >> m;
    std::vector<uint64_t> a(n), b(m), c(n + m - 1, 0);
    for (int i = 0; i < n; ++i) std::cin >> a[i];
    for (int i = 0; i < m; ++i) std::cin >> b[i];
    for (int i = 0; i < n; ++i)
        for (int j = 0; j < m; ++j)
            c[i + j] = (c[i + j] + a[i] * b[j]) % 998244353;
    for (size_t i = 0; i < c.size(); ++i)
        std::cout << c[i] << (i + 1 == c.size() ? '\n' : ' ');
    return 0;
}
"#,
        )
        .unwrap();

        fs::write(
            prob_dir.join("checker.cpp"),
            r#"#include "testlib.h"
int main(int argc, char** argv) {
    registerTestlibCmd(argc, argv);
    int n = inf.readInt();
    int m = inf.readInt();
    for (int i = 0; i < n + m - 1; ++i) {
        long long exp = ans.readLong();
        long long got = ouf.readLong();
        if (exp != got) quitf(_wa, "mismatch at %d: expected %lld, got %lld", i, exp, got);
    }
    quitf(_ok, "ok");
}
"#,
        )
        .unwrap();

        let opts = SnippetVerifyOptions {
            cache_dir: Some(cache_dir.path().to_path_buf()),
            library_checker: LibraryCheckerConfig {
                mirror_url: None,
                local_repo_path: Some(mock_repo.path().to_string_lossy().into_owned()),
            },
            ..Default::default()
        };

        // Example `ntt` has BOTH `self` (`test.cpp`) and `library_checker` (`convolution_mod` + `verify_conv.cpp`)!
        let rep = verify_snippet(&ws, "ntt", &opts).unwrap();
        assert_eq!(rep.status, SnippetVerifyStatus::Passed, "report: {rep:?}");
        assert_eq!(rep.cases.len(), 2);
        assert_eq!(rep.cases[0].kind, "self");
        assert!(rep.cases[0].passed);
        assert_eq!(rep.cases[1].kind, "library_checker");
        assert!(rep.cases[1].passed);
        assert_eq!(rep.cases[1].passed_cases, 5);
        assert_eq!(rep.cases[1].total_cases, 5);

        // Zero files added to workspace `problems/`!
        assert!(!ws.root().join("problems").join("convolution_mod").exists());

        // Subsequent run works 100% offline even after deleting mock_repo!
        drop(mock_repo);
        let offline_opts = SnippetVerifyOptions {
            cache_dir: Some(cache_dir.path().to_path_buf()),
            force: true,
            ..Default::default()
        };
        let rep2 = verify_snippet(&ws, "ntt", &offline_opts).unwrap();
        assert_eq!(rep2.status, SnippetVerifyStatus::Passed);
    }

    #[test]
    fn clean_verify_stderr_strips_cache_binary_and_workspace_paths() {
        let raw = "/home/user/.cache/xcpc/bin/snippet-modint-self-deadbeef1234: /tmp/ws/snippets/modint/test.cpp:7: int main(): Assertion '(a * b) / b == a' failed.\n";
        let cleaned = clean_verify_stderr(raw);
        assert_eq!(
            cleaned,
            "test.cpp:7: int main(): Assertion '(a * b) / b == a' failed."
        );
    }

    #[test]
    fn unavailable_network_preserves_previously_passed_status() {
        if resolve_active_compiler(None).is_err() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let empty_cache = tempfile::tempdir().unwrap();
        let ws = init_example_workspace(dir.path()).unwrap();

        // Seed a valid cached Passed report for `ntt` matching its current content hash.
        let hash = compute_snippet_verify_hash(&ws, "ntt").unwrap();
        let seeded = SnippetVerifyReport {
            slug: "ntt".into(),
            content_hash: hash,
            status: SnippetVerifyStatus::Passed,
            verified_at_epoch_secs: 1,
            total_time_ms: 10,
            max_memory_kib: 1024,
            cases: Vec::new(),
        };
        save_snippet_verify_report(&ws, &seeded).unwrap();

        // Point mirror_url to an unreachable local port so transport fails immediately.
        let opts = SnippetVerifyOptions {
            cache_dir: Some(empty_cache.path().to_path_buf()),
            library_checker: LibraryCheckerConfig {
                mirror_url: Some("http://127.0.0.1:1".into()),
                local_repo_path: None,
            },
            force: true,
            ..Default::default()
        };
        let rep = verify_snippet(&ws, "ntt", &opts).unwrap();
        assert_eq!(rep.status, SnippetVerifyStatus::Passed);
        assert!(rep.cases.iter().any(|c| c.verdict == "UNAVAILABLE"));
        // Disk status must still be Passed!
        assert_eq!(
            snippet_verify_status(&ws, "ntt"),
            SnippetVerifyStatus::Passed
        );
    }
}

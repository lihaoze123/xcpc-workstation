//! Minimal-set OJ package exporter for DOMjudge (`Polygon2DOMjudge`-aligned) and Hydro OJ.
//!
//! Enforces strict static/dynamic separation ("动静分离"): this module NEVER compiles or
//! executes C++ code. It checks the latest strict verification report from `xcpc-judge`
//! and packages pre-built testdata from `build/problems/<slug>/tests/`.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use xcpc_convert::publish;
use xcpc_convert::{Book, Entry};
use xcpc_domain::{ContestItem, Language, Problem, ProblemMeta, SubtaskScoring, SubtaskSpec};
use xcpc_engine::{Engine, FontOptions};
use xcpc_judge::{
    CheckerSpec, PackageFreshness, RunLimits, TESTLIB_H, build_dir_for, check_package_freshness,
    find_interactor_source, has_explicit_checker, load_latest_report, load_verified_report,
    resolve_checker_spec, standard_checker_source,
};
use xcpc_storage::Workspace;

use crate::error::{PackageError, Result, io_err};
use crate::types::{
    BatchExportEvent, BatchExportItemResult, BatchExportItemStatus, BatchExportOjResult,
    ExportOjOptions, ExportOjResult, ExportTarget, ExportedArtifact, OjFormat, PackageWarning,
};
use crate::zip_util::AtomicZipWriter;

/// Exports one problem or all problems in a contest to DOMjudge or Hydro ZIP package(s).
///
/// Never compiles or runs C++ code. Refuses unless the problem's latest strict verification
/// (`verified == true`) matches the current package content hash, unless `opts.force == true`
/// (which still requires built testdata under `build/problems/<slug>/tests/`).
pub fn export_oj_package(
    ws: &Workspace,
    target: &ExportTarget,
    opts: &ExportOjOptions,
) -> Result<ExportOjResult> {
    let mut warnings = Vec::new();

    match target {
        ExportTarget::Problem(slug) => {
            let problem = ws.load_problem(slug)?;
            let prepared = prepare_export_item(ws, &problem, None, None, opts, &mut warnings)?;
            let zip_path = if opts.output_path.is_dir() {
                opts.output_path.join(format!("{}.zip", problem.id))
            } else {
                opts.output_path.clone()
            };
            let mut engine = Engine::new(&FontOptions::from_env(), Some(ws.root().to_path_buf()));
            let artifact = write_oj_zip(ws, &prepared, &zip_path, opts, &mut engine)?;
            Ok(ExportOjResult {
                format: opts.format,
                artifacts: vec![artifact],
                warnings,
            })
        }
        ExportTarget::Contest(contest_id) => {
            let contest = ws.load_contest(contest_id)?;
            let items = ws.contest_problems(&contest)?;
            if items.is_empty() {
                return Err(PackageError::InvalidFormat(format!(
                    "contest {contest_id:?} has no problems to export"
                )));
            }

            // Pre-validate ALL problems before writing any ZIPs so a failing gate never leaves
            // partial contest output on disk.
            let mut prepared_list = Vec::with_capacity(items.len());
            for (item, problem) in &items {
                let effective_meta = item.apply(&problem.meta);
                let lang = opts.language.unwrap_or(contest.meta.language);
                let prepared = prepare_export_item(
                    ws,
                    problem,
                    Some(item.clone()),
                    Some((effective_meta, lang)),
                    opts,
                    &mut warnings,
                )?;
                prepared_list.push(prepared);
            }

            fs::create_dir_all(&opts.output_path).map_err(|e| {
                io_err(
                    format!("create output directory {}", opts.output_path.display()),
                    e,
                )
            })?;

            let mut engine = Engine::new(&FontOptions::from_env(), Some(ws.root().to_path_buf()));
            let mut artifacts = Vec::with_capacity(prepared_list.len());
            for prepared in &prepared_list {
                let label = prepared.label.as_deref().unwrap_or("P");
                let zip_path = opts
                    .output_path
                    .join(format!("{label}-{}.zip", prepared.problem.id));
                let artifact = write_oj_zip(ws, prepared, &zip_path, opts, &mut engine)?;
                artifacts.push(artifact);
            }

            Ok(ExportOjResult {
                format: opts.format,
                artifacts,
                warnings,
            })
        }
    }
}

/// Default relative output directory under the workspace for a batch OJ export:
/// `build/batch-<format>-<YYYYMMDD-HHMMSS>`.
pub fn default_batch_export_dir(format: OjFormat) -> PathBuf {
    let ts = format_utc_timestamp(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
    );
    PathBuf::from(format!("build/batch-{}-{ts}", format.as_str()))
}

/// Formats a Unix timestamp (seconds since 1970-01-01T00:00:00Z) as `YYYYMMDD-HHMMSS` (UTC).
fn format_utc_timestamp(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let hour = rem / 3600;
    let min = (rem % 3600) / 60;
    let sec = rem % 60;

    // Howard Hinnant's civil_from_days algorithm (exact proleptic Gregorian calendar).
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = (yoe as i64) + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };

    format!("{year:04}{m:02}{d:02}-{hour:02}{min:02}{sec:02}")
}

/// Batch-exports multiple problems to DOMjudge or Hydro ZIP packages (`<slug>.zip`)
/// inside a single target directory.
///
/// - Enforces `ws.is_read_only()` upfront.
/// - Strictly enforces static/dynamic separation ("动静分离"): NEVER compiles or runs C++.
///   Pure-publishing, unverified, stale, or missing-testdata problems are recorded as
///   `Skipped` (`not_a_package` / `not_verified` / `missing_testdata`) without aborting
///   the rest of the batch.
/// - Reuses one `Engine` instance across the batch and checks `cancel` between problems.
/// - When `opts.output_path` is empty, writes into `ws.root()/build/batch-<format>-<YYYYMMDD-HHMMSS>/`.
pub fn export_oj_packages_batch(
    ws: &Workspace,
    slugs: &[String],
    opts: &ExportOjOptions,
    cancel: Option<Arc<AtomicBool>>,
    progress: Option<&(dyn Fn(BatchExportEvent) + Send + Sync)>,
) -> Result<BatchExportOjResult> {
    if ws.is_read_only() {
        return Err(xcpc_storage::Error::ReadOnly.into());
    }

    // Deduplicate preserving caller order.
    let mut seen = HashSet::with_capacity(slugs.len());
    let mut unique_slugs = Vec::with_capacity(slugs.len());
    for slug in slugs {
        if seen.insert(slug.clone()) {
            unique_slugs.push(slug.clone());
        }
    }

    let output_dir = if opts.output_path.as_os_str().is_empty() {
        ws.root().join(default_batch_export_dir(opts.format))
    } else {
        opts.output_path.clone()
    };
    let output_dir_str = output_dir.display().to_string();

    let total = unique_slugs.len();
    let mut items = Vec::with_capacity(total);
    let mut warnings = Vec::new();
    let mut exported = 0usize;
    let mut skipped = 0usize;
    let mut failed = 0usize;
    let mut cancelled_count = 0usize;
    let mut cancelled = false;
    let mut dir_created = false;

    let mut engine = Engine::new(&FontOptions::from_env(), Some(ws.root().to_path_buf()));

    for (idx, slug) in unique_slugs.iter().enumerate() {
        if cancel.as_ref().is_some_and(|c| c.load(Ordering::Relaxed)) {
            for rem in &unique_slugs[idx..] {
                items.push(BatchExportItemResult {
                    problem_id: rem.clone(),
                    status: BatchExportItemStatus::Cancelled,
                    zip_path: None,
                    size_bytes: None,
                    sample_count: 0,
                    secret_count: 0,
                    verified: false,
                    forced: false,
                    code: Some("cancelled".to_owned()),
                    message: Some("已取消批量导出。".to_owned()),
                });
                cancelled_count += 1;
            }
            cancelled = true;
            if let Some(cb) = progress {
                cb(BatchExportEvent::Cancelled {
                    completed: idx,
                    total,
                });
            }
            break;
        }

        if let Some(cb) = progress {
            cb(BatchExportEvent::ProblemStarted {
                problem_id: slug.clone(),
                index: idx,
                total,
            });
        }

        let item = export_batch_single_item(
            ws,
            slug,
            opts,
            &output_dir,
            &mut dir_created,
            &mut engine,
            &mut warnings,
        );

        match item.status {
            BatchExportItemStatus::Exported => exported += 1,
            BatchExportItemStatus::Skipped => skipped += 1,
            BatchExportItemStatus::Failed => failed += 1,
            BatchExportItemStatus::Cancelled => cancelled_count += 1,
        }

        if let Some(cb) = progress {
            cb(BatchExportEvent::ProblemFinished {
                item: item.clone(),
                index: idx,
                total,
            });
        }
        items.push(item);
    }

    if let Some(cb) = progress {
        cb(BatchExportEvent::AllFinished {
            exported,
            skipped,
            failed,
            cancelled: cancelled_count,
            total,
            output_dir: output_dir_str.clone(),
        });
    }

    Ok(BatchExportOjResult {
        ok: failed == 0 && !cancelled,
        format: opts.format,
        output_dir: output_dir_str,
        cancelled,
        total,
        exported,
        skipped,
        failed,
        cancelled_count,
        items,
        warnings,
    })
}

fn export_batch_single_item(
    ws: &Workspace,
    slug: &str,
    opts: &ExportOjOptions,
    output_dir: &Path,
    dir_created: &mut bool,
    engine: &mut Engine,
    warnings: &mut Vec<PackageWarning>,
) -> BatchExportItemResult {
    let problem = match ws.load_problem(slug) {
        Ok(p) => p,
        Err(e) => {
            return BatchExportItemResult {
                problem_id: slug.to_owned(),
                status: BatchExportItemStatus::Failed,
                zip_path: None,
                size_bytes: None,
                sample_count: 0,
                secret_count: 0,
                verified: false,
                forced: false,
                code: Some(e.code().to_owned()),
                message: Some(e.human_message()),
            };
        }
    };

    let prob_dir = match ws.resolve(&problem.dir) {
        Ok(d) => d,
        Err(e) => {
            return BatchExportItemResult {
                problem_id: slug.to_owned(),
                status: BatchExportItemStatus::Failed,
                zip_path: None,
                size_bytes: None,
                sample_count: 0,
                secret_count: 0,
                verified: false,
                forced: false,
                code: Some(e.code().to_owned()),
                message: Some(e.human_message()),
            };
        }
    };

    // Pure publishing problem (no local Polygon package): always Skipped with `not_a_package`.
    if !problem.meta.has_package_in_dir(&prob_dir) {
        return BatchExportItemResult {
            problem_id: slug.to_owned(),
            status: BatchExportItemStatus::Skipped,
            zip_path: None,
            size_bytes: None,
            sample_count: 0,
            secret_count: 0,
            verified: false,
            forced: false,
            code: Some("not_a_package".to_owned()),
            message: Some(format!("题目「{slug}」不是题目包（仅含题面），已跳过。")),
        };
    }

    let prepared = match prepare_export_item(ws, &problem, None, None, opts, warnings) {
        Ok(p) => p,
        Err(e @ PackageError::NotVerified { .. }) => {
            return BatchExportItemResult {
                problem_id: slug.to_owned(),
                status: BatchExportItemStatus::Skipped,
                zip_path: None,
                size_bytes: None,
                sample_count: 0,
                secret_count: 0,
                verified: false,
                forced: false,
                code: Some(e.code().to_owned()),
                message: Some(e.human_message()),
            };
        }
        Err(e @ PackageError::MissingTestdata { .. }) => {
            return BatchExportItemResult {
                problem_id: slug.to_owned(),
                status: BatchExportItemStatus::Skipped,
                zip_path: None,
                size_bytes: None,
                sample_count: 0,
                secret_count: 0,
                verified: false,
                forced: false,
                code: Some(e.code().to_owned()),
                message: Some(e.human_message()),
            };
        }
        Err(e) => {
            return BatchExportItemResult {
                problem_id: slug.to_owned(),
                status: BatchExportItemStatus::Failed,
                zip_path: None,
                size_bytes: None,
                sample_count: 0,
                secret_count: 0,
                verified: false,
                forced: false,
                code: Some(e.code().to_owned()),
                message: Some(e.human_message()),
            };
        }
    };

    if !*dir_created {
        if let Err(e) = fs::create_dir_all(output_dir) {
            let err = io_err(
                format!("create output directory {}", output_dir.display()),
                e,
            );
            return BatchExportItemResult {
                problem_id: slug.to_owned(),
                status: BatchExportItemStatus::Failed,
                zip_path: None,
                size_bytes: None,
                sample_count: 0,
                secret_count: 0,
                verified: false,
                forced: false,
                code: Some(err.code().to_owned()),
                message: Some(err.human_message()),
            };
        }
        *dir_created = true;
    }

    let zip_path = output_dir.join(format!("{}.zip", problem.id));
    match write_oj_zip(ws, &prepared, &zip_path, opts, engine) {
        Ok(artifact) => BatchExportItemResult {
            problem_id: artifact.problem_id,
            status: BatchExportItemStatus::Exported,
            zip_path: Some(artifact.zip_path),
            size_bytes: Some(artifact.size_bytes),
            sample_count: artifact.sample_count,
            secret_count: artifact.secret_count,
            verified: artifact.verified,
            forced: artifact.forced,
            code: None,
            message: None,
        },
        Err(e) => BatchExportItemResult {
            problem_id: slug.to_owned(),
            status: BatchExportItemStatus::Failed,
            zip_path: None,
            size_bytes: None,
            sample_count: 0,
            secret_count: 0,
            verified: false,
            forced: false,
            code: Some(e.code().to_owned()),
            message: Some(e.human_message()),
        },
    }
}

struct PreparedTest {
    is_sample: bool,
    subtask: Option<String>,
    input_bytes: Vec<u8>,
    answer_bytes: Vec<u8>,
}

struct PreparedExport {
    problem: Problem,
    effective_meta: ProblemMeta,
    contest_item: Option<ContestItem>,
    label: Option<String>,
    language: Language,
    prob_dir: PathBuf,
    checker_spec: CheckerSpec,
    interactor_path: Option<PathBuf>,
    subtasks: Vec<SubtaskSpec>,
    limits: RunLimits,
    tests: Vec<PreparedTest>,
    verified: bool,
    forced: bool,
}

fn prepare_export_item(
    ws: &Workspace,
    problem: &Problem,
    contest_item: Option<ContestItem>,
    override_meta_lang: Option<(ProblemMeta, Language)>,
    opts: &ExportOjOptions,
    warnings: &mut Vec<PackageWarning>,
) -> Result<PreparedExport> {
    let slug = &problem.id;
    let prob_dir = ws.resolve(&problem.dir)?;

    let (effective_meta, language) = match override_meta_lang {
        Some((m, l)) => (m, l),
        None => (
            problem.meta.clone(),
            opts.language.unwrap_or_else(|| problem.default_language()),
        ),
    };

    // 1. Strict gate check (pure static, no execution).
    let freshness = check_package_freshness(ws, slug)?;
    let (verified, forced) = match freshness {
        PackageFreshness::VerifiedFresh { .. } => (true, false),
        other => {
            let reason = match other {
                PackageFreshness::NotAPackage => "problem has no local Polygon package".to_owned(),
                PackageFreshness::Stale { .. } => {
                    "package files changed since last strict verification".to_owned()
                }
                PackageFreshness::Unverified { .. } => {
                    "no strict verification report found".to_owned()
                }
                PackageFreshness::Failed { summary, .. } => {
                    format!("latest build failed: {summary}")
                }
                PackageFreshness::VerifiedFresh { .. } => unreachable!(),
            };
            if !opts.force {
                return Err(PackageError::NotVerified {
                    slug: slug.clone(),
                    reason,
                });
            }
            warnings.push(PackageWarning::with_path(
                "forced_unverified_export",
                format!("exported unverified package `{slug}` with --force ({reason})"),
                problem.dir.clone(),
            ));
            (false, true)
        }
    };

    // 2. Load built testdata from `build/problems/<slug>/tests/` (required even with `force`).
    let report = if verified {
        load_verified_report(ws, slug).or_else(|| load_latest_report(ws, slug))
    } else {
        load_latest_report(ws, slug).or_else(|| load_verified_report(ws, slug))
    }
    .ok_or_else(|| PackageError::MissingTestdata {
        slug: slug.clone(),
        detail: "build report not found under build/problems/<slug>/".to_owned(),
    })?;

    if report.tests.is_empty() {
        return Err(PackageError::MissingTestdata {
            slug: slug.clone(),
            detail: "build report contains 0 tests".to_owned(),
        });
    }

    let interactor_path = find_interactor_source(&prob_dir).map(|(p, _, _)| p);
    let explicit_checker = has_explicit_checker(&prob_dir, &effective_meta);
    let runs = effective_meta.run_count();
    let subtasks = effective_meta.subtask.clone();

    let tests_dir = build_dir_for(ws, slug).join("tests");
    let mut tests = Vec::with_capacity(report.tests.len());
    for t in &report.tests {
        let in_path = tests_dir.join(format!("{:03}.in", t.index));
        let ans_path = tests_dir.join(format!("{:03}.ans", t.index));
        let input_bytes = fs::read(&in_path).map_err(|_| PackageError::MissingTestdata {
            slug: slug.clone(),
            detail: format!("missing built input {}", in_path.display()),
        })?;
        let answer_bytes = if interactor_path.is_some() {
            fs::read(&ans_path).unwrap_or_default()
        } else {
            fs::read(&ans_path).map_err(|_| PackageError::MissingTestdata {
                slug: slug.clone(),
                detail: format!("missing built answer {}", ans_path.display()),
            })?
        };
        tests.push(PreparedTest {
            is_sample: t.is_sample,
            subtask: t.subtask.clone(),
            input_bytes,
            answer_bytes,
        });
    }

    // Platform-specific structured warnings for Phase 3C features.
    match opts.format {
        OjFormat::Domjudge => {
            if interactor_path.is_some() && explicit_checker {
                warnings.push(PackageWarning::with_path(
                    "domjudge_interactive_checker_ignored",
                    format!(
                        "DOMjudge `custom interactive` only runs interactor.cpp; standalone checker for `{slug}` was not packaged"
                    ),
                    problem.dir.clone(),
                ));
            }
            if runs > 1 {
                warnings.push(PackageWarning::with_path(
                    "domjudge_multi_run_unsupported",
                    format!(
                        "DOMjudge does not natively support multi-run communication (runs={runs}) for `{slug}`; exported package may require manual adaptation"
                    ),
                    problem.dir.clone(),
                ));
            }
            if !subtasks.is_empty() {
                warnings.push(PackageWarning::with_path(
                    "domjudge_subtasks_flattened",
                    format!(
                        "DOMjudge uses ICPC-style judging and does not support OI subtasks ({} subtasks declared in `{slug}`); all secret tests were flattened into data/secret/",
                        subtasks.len()
                    ),
                    problem.dir.clone(),
                ));
            }
        }
        OjFormat::Hydro => {
            if interactor_path.is_some() && explicit_checker {
                warnings.push(PackageWarning::with_path(
                    "hydro_interactive_checker_ignored",
                    format!(
                        "Hydro `type: interactive` only runs interactor.cpp; standalone checker for `{slug}` was not packaged"
                    ),
                    problem.dir.clone(),
                ));
            }
            if runs > 1 {
                warnings.push(PackageWarning::with_path(
                    "hydro_multi_run_unsupported",
                    format!(
                        "Hydro OJ does not natively support Polygon multi-run communication (runs={runs}) for `{slug}`; exported package may require manual adaptation"
                    ),
                    problem.dir.clone(),
                ));
            }
        }
    }

    let checker_spec = resolve_checker_spec(&prob_dir, &effective_meta)?;
    let limits = RunLimits::from_problem_limits(
        effective_meta.limits.time.as_deref(),
        effective_meta.limits.memory.as_deref(),
    )?;
    let label = contest_item.as_ref().map(|c| c.label.clone());

    Ok(PreparedExport {
        problem: problem.clone(),
        effective_meta,
        contest_item,
        label,
        language,
        prob_dir,
        checker_spec,
        interactor_path,
        subtasks,
        limits,
        tests,
        verified,
        forced,
    })
}

fn write_oj_zip(
    ws: &Workspace,
    prep: &PreparedExport,
    zip_path: &Path,
    opts: &ExportOjOptions,
    engine: &mut Engine,
) -> Result<ExportedArtifact> {
    let sample_count = prep.tests.iter().filter(|t| t.is_sample).count();
    let secret_count = prep.tests.len() - sample_count;

    let size_bytes = match opts.format {
        OjFormat::Domjudge => write_domjudge_zip(ws, prep, zip_path, engine)?,
        OjFormat::Hydro => write_hydro_zip(ws, prep, zip_path, engine)?,
    };

    Ok(ExportedArtifact {
        problem_id: prep.problem.id.clone(),
        label: prep.label.clone(),
        zip_path: zip_path.display().to_string(),
        sample_count,
        secret_count,
        size_bytes,
        verified: prep.verified,
        forced: prep.forced,
    })
}

// ---------------------------------------------------------------------------
// DOMjudge Export (`cn-xcpc-tools/Polygon2DOMjudge` aligned)
// ---------------------------------------------------------------------------

const DOMJUDGE_CHECKER_BUILD_SH: &str = "\
#!/bin/sh
set -e
DIR=\"$(cd \"$(dirname \"$0\")\" && pwd)\"
g++ -O2 -std=c++20 \"$DIR/checker.cpp\" -o \"$DIR/checker_bin\"
";

const DOMJUDGE_CHECKER_RUN_SH: &str = "\
#!/bin/sh
DIR=\"$(cd \"$(dirname \"$0\")\" && pwd)\"
TEST_IN=\"$1\"
TEST_ANS=\"$2\"
FEEDBACK_DIR=\"$3\"
TEAM_OUT=\"$FEEDBACK_DIR/team_out.tmp\"
cat > \"$TEAM_OUT\"
set +e
\"$DIR/checker_bin\" \"$TEST_IN\" \"$TEAM_OUT\" \"$TEST_ANS\" 2> \"$FEEDBACK_DIR/judgemessage.txt\"
RC=$?
rm -f \"$TEAM_OUT\"
if [ \"$RC\" -eq 0 ]; then
  exit 42
elif [ \"$RC\" -eq 1 ] || [ \"$RC\" -eq 2 ] || [ \"$RC\" -eq 4 ] || [ \"$RC\" -eq 8 ]; then
  exit 43
else
  exit \"$RC\"
fi
";

const DOMJUDGE_INTERACTOR_BUILD_SH: &str = "\
#!/bin/sh
set -e
DIR=\"$(cd \"$(dirname \"$0\")\" && pwd)\"
g++ -O2 -std=c++20 \"$DIR/interactor.cpp\" -o \"$DIR/interactor_bin\"
";

const DOMJUDGE_INTERACTOR_RUN_SH: &str = "\
#!/bin/sh
DIR=\"$(cd \"$(dirname \"$0\")\" && pwd)\"
TEST_IN=\"$1\"
TEST_ANS=\"$2\"
FEEDBACK_DIR=\"$3\"
TEAM_OUT=\"$FEEDBACK_DIR/team_out.tmp\"
set +e
\"$DIR/interactor_bin\" \"$TEST_IN\" \"$TEAM_OUT\" 2> \"$FEEDBACK_DIR/judgemessage.txt\"
RC=$?
rm -f \"$TEAM_OUT\"
if [ \"$RC\" -eq 0 ]; then
  exit 42
elif [ \"$RC\" -eq 1 ] || [ \"$RC\" -eq 2 ] || [ \"$RC\" -eq 4 ] || [ \"$RC\" -eq 8 ]; then
  exit 43
else
  exit \"$RC\"
fi
";

fn write_domjudge_zip(
    ws: &Workspace,
    prep: &PreparedExport,
    zip_path: &Path,
    engine: &mut Engine,
) -> Result<u64> {
    let mut zip = AtomicZipWriter::create(zip_path)?;

    // 1. `domjudge-problem.ini` (LF, NO `color`)
    let short_name = prep.label.as_deref().unwrap_or(&prep.problem.id);
    let timelimit = format_domjudge_timelimit(prep.limits.time_limit_ms);
    let ini = format!(
        "short-name = {short_name}\nexternalid = {}\ntimelimit = {timelimit}\n",
        prep.problem.id
    );
    zip.add_file("domjudge-problem.ini", ini.as_bytes())?;

    // 2. `problem.yaml`
    let memory_mib = prep.limits.memory_limit_kib.div_ceil(1024).max(1);
    let (validation, validator_flags) = if prep.interactor_path.is_some() {
        ("custom interactive", None)
    } else {
        domjudge_validation_config(&prep.checker_spec)
    };
    let mut yaml = String::new();
    yaml.push_str(&format!(
        "name: {}\nlimits:\n  memory: {memory_mib}\nvalidation: {validation}\n",
        yaml_scalar(&prep.effective_meta.title)
    ));
    if let Some(flags) = validator_flags {
        yaml.push_str(&format!("validator_flags: {flags}\n"));
    }
    zip.add_file("problem.yaml", yaml.as_bytes())?;

    // 3. `problem.pdf` compiled via `xcpc-convert` + `xcpc-engine`
    let pdf_bytes = compile_problem_pdf(ws, engine, prep)?;
    zip.add_file("problem.pdf", &pdf_bytes)?;

    // 4. `data/sample/` and `data/secret/` (adaptive zero-padding preserves lexicographical order >= 100)
    let sample_total = prep.tests.iter().filter(|t| t.is_sample).count();
    let secret_total = prep.tests.len() - sample_total;
    let sample_width = sample_total.to_string().len().max(2);
    let secret_width = secret_total.to_string().len().max(2);
    let mut sample_idx = 0usize;
    let mut secret_idx = 0usize;
    for t in &prep.tests {
        if t.is_sample {
            sample_idx += 1;
            zip.add_file(
                &format!("data/sample/{sample_idx:0sample_width$}.in"),
                &t.input_bytes,
            )?;
            zip.add_file(
                &format!("data/sample/{sample_idx:0sample_width$}.ans"),
                &t.answer_bytes,
            )?;
        } else {
            secret_idx += 1;
            zip.add_file(
                &format!("data/secret/{secret_idx:0secret_width$}.in"),
                &t.input_bytes,
            )?;
            zip.add_file(
                &format!("data/secret/{secret_idx:0secret_width$}.ans"),
                &t.answer_bytes,
            )?;
        }
    }

    // 5. `output_validators/` (interactive interactor OR custom checker)
    if let Some(ref inter_path) = prep.interactor_path {
        let inter_bytes = fs::read(inter_path)
            .map_err(|e| io_err(format!("read interactor {}", inter_path.display()), e))?;
        let testlib_bytes = read_effective_testlib(ws, &prep.prob_dir);

        zip.add_file("output_validators/interactor/interactor.cpp", &inter_bytes)?;
        zip.add_file("output_validators/interactor/testlib.h", &testlib_bytes)?;
        zip.add_file_with_mode(
            "output_validators/interactor/build",
            DOMJUDGE_INTERACTOR_BUILD_SH.as_bytes(),
            0o755,
        )?;
        zip.add_file_with_mode(
            "output_validators/interactor/run",
            DOMJUDGE_INTERACTOR_RUN_SH.as_bytes(),
            0o755,
        )?;
    } else if let CheckerSpec::Custom { ref path, .. } = prep.checker_spec {
        let checker_bytes = fs::read(path)
            .map_err(|e| io_err(format!("read custom checker {}", path.display()), e))?;
        let testlib_bytes = read_effective_testlib(ws, &prep.prob_dir);

        zip.add_file("output_validators/checker/checker.cpp", &checker_bytes)?;
        zip.add_file("output_validators/checker/testlib.h", &testlib_bytes)?;
        zip.add_file_with_mode(
            "output_validators/checker/build",
            DOMJUDGE_CHECKER_BUILD_SH.as_bytes(),
            0o755,
        )?;
        zip.add_file_with_mode(
            "output_validators/checker/run",
            DOMJUDGE_CHECKER_RUN_SH.as_bytes(),
            0o755,
        )?;
    }

    zip.finish()
}

fn domjudge_validation_config(spec: &CheckerSpec) -> (&'static str, Option<&'static str>) {
    match spec {
        CheckerSpec::Builtin(name) => match name.as_str() {
            "wcmp" | "ncmp" | "lcmp" => ("default", Some("case_sensitive")),
            "fcmp" => ("default", Some("space_change_sensitive case_sensitive")),
            "rcmp4" => ("default", Some("float_tolerance 1e-4")),
            "rcmp6" => ("default", Some("float_tolerance 1e-6")),
            "rcmp9" => ("default", Some("float_tolerance 1e-9")),
            "yesno" | "nyesno" => ("default", None),
            _ => ("default", Some("case_sensitive")),
        },
        CheckerSpec::Custom { .. } => ("custom", None),
    }
}

fn format_domjudge_timelimit(ms: u64) -> String {
    let secs = ms as f64 / 1000.0;
    if ms.is_multiple_of(1000) {
        format!("{}.0", ms / 1000)
    } else {
        let s = format!("{secs:.3}");
        let trimmed = s.trim_end_matches('0');
        if trimmed.ends_with('.') {
            format!("{trimmed}0")
        } else {
            trimmed.to_owned()
        }
    }
}

fn compile_problem_pdf(
    ws: &Workspace,
    engine: &mut Engine,
    prep: &PreparedExport,
) -> Result<Vec<u8>> {
    let mut book = Book::single(prep.language);
    let tpl_name = ws
        .default_template()
        .unwrap_or_else(|| "olymp-classic".to_owned());
    if ws.has_custom_template(&tpl_name) {
        book.template_name = tpl_name.clone();
        book.template_import = Some(format!("/templates/{tpl_name}.typ"));
        book.template_options = ws
            .workspace_template_options()
            .remove(&tpl_name)
            .unwrap_or_default();
    } else {
        // Fallback to bundled `/xcpc/olymp-classic.typ` if workspace has no custom template file.
        book.template_name = "olymp-classic".to_owned();
        book.template_import = None;
    }
    book.preamble = ws.load_preamble();

    let entry = match &prep.contest_item {
        Some(item) => Entry {
            item: item.clone(),
            problem: &prep.problem,
        },
        None => Entry::single(&prep.problem),
    };

    let doc = xcpc_convert::statements(&book, &[entry]);
    engine.reset_files();
    let (compiled, report) = publish::compile(engine, &doc);
    let Some(compiled) = compiled else {
        let msg = report
            .diagnostics
            .iter()
            .filter(|d| d.is_error())
            .map(|d| d.display())
            .collect::<Vec<_>>()
            .join("; ");
        return Err(PackageError::PdfCompileFailed {
            slug: prep.problem.id.clone(),
            detail: if msg.is_empty() {
                "unknown Typst compilation error".to_owned()
            } else {
                msg
            },
        });
    };

    compiled.pdf().map_err(|errs| {
        let msg = errs
            .iter()
            .map(|d| d.message.as_str())
            .collect::<Vec<_>>()
            .join("; ");
        PackageError::PdfCompileFailed {
            slug: prep.problem.id.clone(),
            detail: msg,
        }
    })
}

fn read_effective_testlib(ws: &Workspace, prob_dir: &Path) -> Vec<u8> {
    for candidate in [prob_dir.join("testlib.h"), ws.root().join("testlib.h")] {
        if candidate.is_file()
            && let Ok(bytes) = fs::read(&candidate)
        {
            return bytes;
        }
    }
    TESTLIB_H.as_bytes().to_vec()
}

// ---------------------------------------------------------------------------
// Hydro OJ Export
// ---------------------------------------------------------------------------

fn write_hydro_zip(
    ws: &Workspace,
    prep: &PreparedExport,
    zip_path: &Path,
    engine: &mut Engine,
) -> Result<u64> {
    let mut zip = AtomicZipWriter::create(zip_path)?;

    // 1. `problem.yaml`
    let mut prob_yaml = format!("title: {}\n", yaml_scalar(&prep.effective_meta.title));
    if !prep.effective_meta.tags.is_empty() {
        prob_yaml.push_str("tag:\n");
        for tag in &prep.effective_meta.tags {
            prob_yaml.push_str(&format!("  - {}\n", yaml_scalar(tag)));
        }
    }
    zip.add_file("problem.yaml", prob_yaml.as_bytes())?;

    // 2. Compiled PDF statement (`additional_file/problem.pdf`) + Hydro PDF embed Markdown
    let pdf_bytes = compile_problem_pdf(ws, engine, prep)?;
    zip.add_file("additional_file/problem.pdf", &pdf_bytes)?;

    let stmt_filename = match prep.language {
        Language::Chinese => "problem_zh.md",
        Language::English => "problem_en.md",
    };
    let stmt_md = "@[pdf](file://problem.pdf?noDisposition=1)\n";
    zip.add_file(stmt_filename, stmt_md.as_bytes())?;

    // 3. `testdata/config.yaml` and optional `testdata/interactor.cpp` or `testdata/checker.cpp`
    let memory_mib = prep.limits.memory_limit_kib.div_ceil(1024).max(1);
    let mut cfg = String::new();
    if let Some(ref inter_path) = prep.interactor_path {
        cfg.push_str(&format!(
            "type: interactive\ntime: {}ms\nmemory: {memory_mib}m\ninteractor: interactor.cpp\n",
            prep.limits.time_limit_ms
        ));
        let inter_bytes = fs::read(inter_path)
            .map_err(|e| io_err(format!("read interactor {}", inter_path.display()), e))?;
        let testlib_bytes = read_effective_testlib(ws, &prep.prob_dir);
        zip.add_file("testdata/interactor.cpp", &inter_bytes)?;
        zip.add_file("testdata/testlib.h", &testlib_bytes)?;
    } else {
        cfg.push_str(&format!(
            "type: default\ntime: {}ms\nmemory: {memory_mib}m\n",
            prep.limits.time_limit_ms
        ));
        match &prep.checker_spec {
            CheckerSpec::Builtin(name) if matches!(name.as_str(), "wcmp" | "ncmp" | "lcmp") => {
                cfg.push_str("checker_type: default\n");
            }
            CheckerSpec::Builtin(name) => {
                cfg.push_str("checker_type: testlib\nchecker: checker.cpp\n");
                let src = standard_checker_source(name).ok_or_else(|| {
                    PackageError::Unsupported(format!("unknown checker {name:?}"))
                })?;
                zip.add_file("testdata/checker.cpp", src.as_bytes())?;
            }
            CheckerSpec::Custom { path, .. } => {
                cfg.push_str("checker_type: testlib\nchecker: checker.cpp\n");
                let bytes = fs::read(path)
                    .map_err(|e| io_err(format!("read custom checker {}", path.display()), e))?;
                zip.add_file("testdata/checker.cpp", &bytes)?;
            }
        }
    }

    let width = prep.tests.len().to_string().len().max(2);
    if !prep.subtasks.is_empty() {
        let id_to_idx: HashMap<&str, usize> = prep
            .subtasks
            .iter()
            .enumerate()
            .map(|(i, st)| (st.id.as_str(), i + 1))
            .collect();
        cfg.push_str("subtasks:\n");
        for st in &prep.subtasks {
            let scoring_str = match st.scoring {
                SubtaskScoring::Min => "min",
                SubtaskScoring::Sum => "sum",
            };
            cfg.push_str(&format!("  - score: {}\n", st.score));
            cfg.push_str(&format!("    type: {scoring_str}\n"));
            let dep_indices: Vec<usize> = st
                .depends
                .iter()
                .filter_map(|d| id_to_idx.get(d.as_str()).copied())
                .collect();
            if !dep_indices.is_empty() {
                cfg.push_str("    if:\n");
                for dep_idx in dep_indices {
                    cfg.push_str(&format!("      - {dep_idx}\n"));
                }
            }
            cfg.push_str("    cases:\n");
            for (i, t) in prep.tests.iter().enumerate() {
                if t.subtask.as_deref() == Some(st.id.as_str()) {
                    let idx = i + 1;
                    cfg.push_str(&format!(
                        "      - input: {idx:0width$}.in\n        output: {idx:0width$}.out\n"
                    ));
                }
            }
        }
    }

    zip.add_file("testdata/config.yaml", cfg.as_bytes())?;

    // 4. `testdata/01.in`, `testdata/01.out`, ... (all built tests: samples + secret)
    for (i, t) in prep.tests.iter().enumerate() {
        let idx = i + 1;
        zip.add_file(&format!("testdata/{idx:0width$}.in"), &t.input_bytes)?;
        zip.add_file(&format!("testdata/{idx:0width$}.out"), &t.answer_bytes)?;
    }

    zip.finish()
}

/// Formats a YAML scalar safely (double-quoted with proper escaping if it contains special chars).
fn yaml_scalar(s: &str) -> String {
    let needs_quote = s.is_empty()
        || s.starts_with([
            ' ', '-', '?', ':', '#', '&', '*', '!', '|', '>', '\'', '"', '%', '@', '`', '{', '}',
            '[', ']',
        ])
        || s.ends_with(' ')
        || s.contains(": ")
        || s.contains(" #")
        || s.contains('\n')
        || s.contains('"')
        || matches!(
            s.to_ascii_lowercase().as_str(),
            "true" | "false" | "null" | "yes" | "no" | "on" | "off" | "~"
        );
    if !needs_quote {
        return s.to_owned();
    }
    let escaped = s
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n");
    format!("\"{escaped}\"")
}

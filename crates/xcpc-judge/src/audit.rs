//! Contest Readiness Audit (`audit_contest`) and Markdown report export (Phase 3D).
//!
//! Purely read-only aggregation across all problems in a contest (never compiles or executes
//! any code; safe in `--read-only` mode). Reads existing `report.json` / `verified_report.json`,
//! optional `sanitizer-report.json`, built test data sizes on disk, and `xcpc-lint` results.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use xcpc_storage::Workspace;

use crate::error::Result;
use crate::package::{
    PackageFreshness, build_dir_for, check_package_freshness_in, compute_package_hash,
    contest_limits_key, discover_solutions, effective_run_limits, expand_tests,
    find_interactor_source, find_validator_source, has_explicit_checker, load_latest_report,
    load_verified_report_in, resolve_checker_spec,
};
use crate::report::{BuildReport, GateStatus};
use crate::sanitize::{SanitizerStatus, load_sanitizer_report};

/// Default threshold for single test file size warning (20 MiB).
pub const DEFAULT_MAX_SINGLE_FILE_BYTES: u64 = 20 * 1024 * 1024;
/// Default threshold for cumulative `std` runtime across all tests warning (30 seconds).
pub const DEFAULT_MAX_CUMULATIVE_STD_MS: u64 = 30_000;

/// One lint finding summary supplied by `xcpc-lint` (or fallback lightweight checks).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditLintFinding {
    pub severity: String,
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
}

/// Aggregated lint counts for one problem.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProblemLintSummary {
    pub error_count: usize,
    pub warning_count: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub findings: Vec<AuditLintFinding>,
}

/// Options controlling [`audit_contest`].
#[derive(Debug, Clone)]
pub struct AuditOptions {
    /// Warn if any single `.in`/`.ans`/`.out` file exceeds this size (default: 20 MiB).
    pub max_single_file_bytes: u64,
    /// Warn if cumulative `std` CPU time across all tests exceeds this threshold (default: 30,000ms).
    pub max_cumulative_std_ms: u64,
    /// Optional precomputed `xcpc-lint` summaries keyed by problem slug.
    /// Callers in CLI/MCP/Desktop that depend on `xcpc-lint` (or `xcpc-convert`) can populate
    /// this map; for any problem not present here, `audit_contest` performs a lightweight
    /// built-in sample/editorial check.
    pub lint_by_problem: BTreeMap<String, ProblemLintSummary>,
}

impl Default for AuditOptions {
    fn default() -> Self {
        Self {
            max_single_file_bytes: DEFAULT_MAX_SINGLE_FILE_BYTES,
            max_cumulative_std_ms: DEFAULT_MAX_CUMULATIVE_STD_MS,
            lint_by_problem: BTreeMap::new(),
        }
    }
}

/// High-level package verification status for one problem in a contest audit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationStatus {
    Fresh,
    Stale,
    Failed,
    Unbuilt,
    StatementOnly,
}

impl VerificationStatus {
    pub fn label_zh(self) -> &'static str {
        match self {
            VerificationStatus::Fresh => "已验收",
            VerificationStatus::Stale => "已过期",
            VerificationStatus::Failed => "失败",
            VerificationStatus::Unbuilt => "未构建",
            VerificationStatus::StatementOnly => "纯出版",
        }
    }
}

/// Sanitizer UB check status for one problem in a contest audit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SanitizerAuditStatus {
    PassedFresh,
    PassedStale,
    Failed,
    Unsupported,
    NotRun,
}

impl SanitizerAuditStatus {
    pub fn label_zh(self) -> &'static str {
        match self {
            SanitizerAuditStatus::PassedFresh => "通过",
            SanitizerAuditStatus::PassedStale => "已过期",
            SanitizerAuditStatus::Failed => "发现 UB",
            SanitizerAuditStatus::Unsupported => "不支持",
            SanitizerAuditStatus::NotRun => "未运行",
        }
    }
}

/// Validator or custom checker self-test status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SelfTestAuditStatus {
    Passed,
    Failed,
    Missing,
    NotApplicable,
}

impl SelfTestAuditStatus {
    pub fn label_zh(self) -> &'static str {
        match self {
            SelfTestAuditStatus::Passed => "通过",
            SelfTestAuditStatus::Failed => "失败",
            SelfTestAuditStatus::Missing => "缺失",
            SelfTestAuditStatus::NotApplicable => "-",
        }
    }
}

/// Risk severity level for contest readiness findings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditRiskLevel {
    Error,
    Warning,
}

/// One actionable risk flag raised during [`audit_contest`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditRiskFlag {
    pub level: AuditRiskLevel,
    pub code: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub problem_label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub problem_id: Option<String>,
    pub message: String,
}

/// Per-problem row in [`ContestAuditReport`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProblemAuditRow {
    pub label: String,
    pub problem_id: String,
    pub title: String,
    pub verification_status: VerificationStatus,
    pub interactive: bool,
    pub has_subtasks: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_limit_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub std_max_time_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub std_time_ratio: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slowest_ac_time_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fastest_tle_time_ms: Option<u64>,
    /// `fastest_tle_time_ms / slowest_ac_time_ms` when both exist and `slowest_ac_time_ms > 0`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ac_tle_margin_ratio: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cumulative_std_time_ms: Option<u64>,
    pub test_count: usize,
    pub max_test_file_bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_test_file_name: Option<String>,
    pub total_test_data_bytes: u64,
    pub independent_ac_count: usize,
    pub wrong_solution_count: usize,
    pub validator_self_test: SelfTestAuditStatus,
    pub checker_self_test: SelfTestAuditStatus,
    pub sanitizer_status: SanitizerAuditStatus,
    pub sanitizer_findings_count: usize,
    pub lint_error_count: usize,
    pub lint_warning_count: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub risk_flags: Vec<AuditRiskFlag>,
}

/// Contest-level aggregated counts and freeze readiness indicator.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContestAuditSummary {
    pub total_problems: usize,
    pub fresh_count: usize,
    pub stale_count: usize,
    pub failed_count: usize,
    pub unbuilt_count: usize,
    pub statement_only_count: usize,
    pub sanitizer_passed_count: usize,
    pub sanitizer_failed_count: usize,
    pub sanitizer_not_run_count: usize,
    pub error_risk_count: usize,
    pub warning_risk_count: usize,
    pub ready_for_freeze: bool,
}

/// Complete read-only contest readiness audit report.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContestAuditReport {
    pub contest_id: String,
    pub contest_title: String,
    pub frozen: bool,
    pub audited_at_epoch_secs: u64,
    pub summary: ContestAuditSummary,
    pub problems: Vec<ProblemAuditRow>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub risk_flags: Vec<AuditRiskFlag>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub next_steps: Vec<String>,
}

/// Performs a read-only readiness audit across all problems in `contest_id`.
pub fn audit_contest(
    ws: &Workspace,
    contest_id: &str,
    opts: &AuditOptions,
) -> Result<ContestAuditReport> {
    let contest = ws.load_contest(contest_id)?;
    let mut rows = Vec::with_capacity(contest.meta.problems.len());
    let mut all_risks = Vec::new();
    let mut summary = ContestAuditSummary {
        total_problems: contest.meta.problems.len(),
        ..Default::default()
    };

    for item in &contest.meta.problems {
        let row = audit_single_problem(ws, &contest.id, &item.label, &item.id, opts);
        match row.verification_status {
            VerificationStatus::Fresh => summary.fresh_count += 1,
            VerificationStatus::Stale => summary.stale_count += 1,
            VerificationStatus::Failed => summary.failed_count += 1,
            VerificationStatus::Unbuilt => summary.unbuilt_count += 1,
            VerificationStatus::StatementOnly => summary.statement_only_count += 1,
        }
        if row.verification_status != VerificationStatus::StatementOnly {
            match row.sanitizer_status {
                SanitizerAuditStatus::PassedFresh | SanitizerAuditStatus::PassedStale => {
                    summary.sanitizer_passed_count += 1;
                }
                SanitizerAuditStatus::Failed => {
                    summary.sanitizer_failed_count += 1;
                }
                SanitizerAuditStatus::NotRun | SanitizerAuditStatus::Unsupported => {
                    summary.sanitizer_not_run_count += 1;
                }
            }
        }
        for flag in &row.risk_flags {
            match flag.level {
                AuditRiskLevel::Error => summary.error_risk_count += 1,
                AuditRiskLevel::Warning => summary.warning_risk_count += 1,
            }
            all_risks.push(flag.clone());
        }
        rows.push(row);
    }

    summary.ready_for_freeze = summary.error_risk_count == 0
        && summary.stale_count == 0
        && summary.failed_count == 0
        && summary.unbuilt_count == 0;

    let next_steps = build_next_steps(&contest.id, &rows, &summary);

    Ok(ContestAuditReport {
        contest_id: contest.id,
        contest_title: contest.meta.name,
        frozen: contest.meta.frozen,
        audited_at_epoch_secs: now_epoch_secs(),
        summary,
        problems: rows,
        risk_flags: all_risks,
        next_steps,
    })
}

fn audit_single_problem(
    ws: &Workspace,
    contest_id: &str,
    label: &str,
    slug: &str,
    opts: &AuditOptions,
) -> ProblemAuditRow {
    let mut risk_flags = Vec::new();
    let problem = match ws.load_problem(slug) {
        Ok(p) => p,
        Err(e) => {
            risk_flags.push(AuditRiskFlag {
                level: AuditRiskLevel::Error,
                code: "missing_problem".to_owned(),
                problem_label: Some(label.to_owned()),
                problem_id: Some(slug.to_owned()),
                message: format!("题目 {label}（{slug}）无法加载：{e}"),
            });
            return ProblemAuditRow {
                label: label.to_owned(),
                problem_id: slug.to_owned(),
                title: slug.to_owned(),
                verification_status: VerificationStatus::Failed,
                interactive: false,
                has_subtasks: false,
                time_limit_ms: None,
                std_max_time_ms: None,
                std_time_ratio: None,
                slowest_ac_time_ms: None,
                fastest_tle_time_ms: None,
                ac_tle_margin_ratio: None,
                cumulative_std_time_ms: None,
                test_count: 0,
                max_test_file_bytes: 0,
                max_test_file_name: None,
                total_test_data_bytes: 0,
                independent_ac_count: 0,
                wrong_solution_count: 0,
                validator_self_test: SelfTestAuditStatus::NotApplicable,
                checker_self_test: SelfTestAuditStatus::NotApplicable,
                sanitizer_status: SanitizerAuditStatus::NotRun,
                sanitizer_findings_count: 0,
                lint_error_count: 1,
                lint_warning_count: 0,
                risk_flags,
            };
        }
    };

    let prob_dir = match ws.resolve(&problem.dir) {
        Ok(d) => d,
        Err(_) => ws.root().join(&problem.dir),
    };

    let interactive = find_interactor_source(&prob_dir).is_some();
    let has_subtasks = !problem.meta.subtask.is_empty();
    // Checks inside a contest run under the contest's overridden limits.
    let time_limit_ms = effective_run_limits(ws, slug, Some(contest_id))
        .ok()
        .map(|l| l.time_limit_ms);
    let limits_key = contest_limits_key(ws, slug, Some(contest_id))
        .ok()
        .flatten();
    let limits_key = limits_key.as_deref();

    let freshness = check_package_freshness_in(ws, slug, Some(contest_id)).unwrap_or(
        PackageFreshness::Failed {
            package_hash: String::new(),
            summary: "无法读取检查记录".to_owned(),
        },
    );
    let current_hash = compute_package_hash(ws, slug).unwrap_or_default();

    let verification_status = match &freshness {
        PackageFreshness::NotAPackage => VerificationStatus::StatementOnly,
        PackageFreshness::VerifiedFresh { .. } => VerificationStatus::Fresh,
        PackageFreshness::Stale { .. } => VerificationStatus::Stale,
        PackageFreshness::Failed { .. } => VerificationStatus::Failed,
        PackageFreshness::Unverified { .. } => VerificationStatus::Unbuilt,
    };

    match &freshness {
        PackageFreshness::Stale { .. } => {
            risk_flags.push(AuditRiskFlag {
                level: AuditRiskLevel::Error,
                code: "package_stale".to_owned(),
                problem_label: Some(label.to_owned()),
                problem_id: Some(slug.to_owned()),
                message: format!("题目 {label}（{slug}）改动后还没重新检查"),
            });
        }
        PackageFreshness::Failed { summary, .. } => {
            risk_flags.push(AuditRiskFlag {
                level: AuditRiskLevel::Error,
                code: "package_failed".to_owned(),
                problem_label: Some(label.to_owned()),
                problem_id: Some(slug.to_owned()),
                message: format!("题目 {label}（{slug}）有问题：{summary}"),
            });
        }
        PackageFreshness::Unverified { .. } => {
            risk_flags.push(AuditRiskFlag {
                level: AuditRiskLevel::Error,
                code: "package_unverified".to_owned(),
                problem_label: Some(label.to_owned()),
                problem_id: Some(slug.to_owned()),
                message: format!("题目 {label}（{slug}）还没检查"),
            });
        }
        PackageFreshness::NotAPackage | PackageFreshness::VerifiedFresh { .. } => {}
    }

    // Prefer verified report when fresh; otherwise latest report.
    let report: Option<BuildReport> = match verification_status {
        VerificationStatus::Fresh => {
            load_verified_report_in(ws, slug, limits_key).or_else(|| load_latest_report(ws, slug))
        }
        VerificationStatus::StatementOnly => None,
        _ => load_latest_report(ws, slug).or_else(|| load_verified_report_in(ws, slug, limits_key)),
    };

    // Solutions coverage (independent AC & wrong solutions).
    let (independent_ac_count, wrong_solution_count) =
        if verification_status == VerificationStatus::StatementOnly {
            (0, 0)
        } else {
            match discover_solutions(&prob_dir, &problem.meta) {
                Ok(sols) => {
                    let ind_ac = sols
                        .iter()
                        .filter(|s| {
                            !s.is_main
                                && s.score.is_none()
                                && s.subtasks.is_empty()
                                && s.expect.matches("AC")
                        })
                        .count();
                    let wrong = sols
                        .iter()
                        .filter(|s| {
                            !s.is_main
                                && (!s.expect.matches("AC")
                                    || s.score.is_some()
                                    || s.subtasks.values().any(|e| !e.matches("AC")))
                        })
                        .count();
                    (ind_ac, wrong)
                }
                Err(_) => (0, 0),
            }
        };

    if verification_status != VerificationStatus::StatementOnly {
        if independent_ac_count == 0 {
            risk_flags.push(AuditRiskFlag {
                level: AuditRiskLevel::Warning,
                code: "missing_independent_ac".to_owned(),
                problem_label: Some(label.to_owned()),
                problem_id: Some(slug.to_owned()),
                message: format!("题目 {label}（{slug}）只有标程，没有另一份正确解法互相印证"),
            });
        }
        if wrong_solution_count == 0 {
            risk_flags.push(AuditRiskFlag {
                level: AuditRiskLevel::Warning,
                code: "missing_wrong_solution".to_owned(),
                problem_label: Some(label.to_owned()),
                problem_id: Some(slug.to_owned()),
                message: format!("题目 {label}（{slug}）没有错解，无法确认数据能卡掉错误做法"),
            });
        }
    }

    // Timing safety margins & cumulative std time from report.
    let (
        std_max_time_ms,
        std_time_ratio,
        slowest_ac_time_ms,
        fastest_tle_time_ms,
        ac_tle_margin_ratio,
        cumulative_std_time_ms,
    ) = if let Some(ref r) = report {
        let sm = &r.safety_margin;
        let std_max = Some(sm.std_max_time_ms);
        // Ratio against the limit this check is judged under (contest overrides included).
        let tl_ms = time_limit_ms.unwrap_or(sm.time_limit_ms);
        let std_time_ratio = if tl_ms > 0 {
            sm.std_max_time_ms as f64 / tl_ms as f64
        } else {
            sm.std_time_ratio
        };
        let ratio = Some(std_time_ratio);
        let slowest_ac = sm.slowest_ac_time_ms;
        let fastest_tle = sm.fastest_tle_time_ms;
        let ac_tle_ratio = match (slowest_ac, fastest_tle) {
            (Some(ac), Some(tle)) if ac > 0 => Some(tle as f64 / ac as f64),
            _ => None,
        };
        let main_name = r
            .solutions
            .iter()
            .find(|s| s.is_main)
            .map(|s| s.name.as_str());
        let cum_std = main_name.map(|mn| {
            r.matrix
                .iter()
                .filter(|c| c.solution == mn && !c.stale)
                .map(|c| c.cpu_time_ms)
                .sum::<u64>()
        });

        if std_time_ratio > 0.50 {
            risk_flags.push(AuditRiskFlag {
                level: AuditRiskLevel::Warning,
                code: "tight_std_time_margin".to_owned(),
                problem_label: Some(label.to_owned()),
                problem_id: Some(slug.to_owned()),
                message: format!(
                    "题目 {label}（{slug}）在 {} 时限下，标程最慢 {}（{:.0}%），建议不超过一半",
                    format_seconds(tl_ms),
                    format_seconds(sm.std_max_time_ms),
                    std_time_ratio * 100.0
                ),
            });
        }
        if let Some(margin) = ac_tle_ratio
            && margin < 1.5
        {
            risk_flags.push(AuditRiskFlag {
                level: AuditRiskLevel::Warning,
                code: "tight_ac_tle_margin".to_owned(),
                problem_label: Some(label.to_owned()),
                problem_id: Some(slug.to_owned()),
                message: format!(
                    "题目 {label}（{slug}）最慢的正确解法和最快的超时解法用时太接近（只差 {margin:.2} 倍）"
                ),
            });
        }
        if let Some(cum) = cum_std
            && cum > opts.max_cumulative_std_ms
        {
            risk_flags.push(AuditRiskFlag {
                level: AuditRiskLevel::Warning,
                code: "excessive_cumulative_std_time".to_owned(),
                problem_label: Some(label.to_owned()),
                problem_id: Some(slug.to_owned()),
                message: format!(
                    "题目 {label}（{slug}）标程跑完全部数据要 {:.1} 秒，超过 {:.1} 秒，评测会排队",
                    cum as f64 / 1000.0,
                    opts.max_cumulative_std_ms as f64 / 1000.0
                ),
            });
        }

        (
            std_max,
            ratio,
            slowest_ac,
            fastest_tle,
            ac_tle_ratio,
            cum_std,
        )
    } else {
        (None, None, None, None, None, None)
    };

    // Test count & test data disk size inspection.
    let test_count = report
        .as_ref()
        .map(|r| r.tests.len())
        .filter(|&n| n > 0)
        .unwrap_or_else(|| {
            expand_tests(&prob_dir, &problem.meta)
                .map(|v| v.len())
                .unwrap_or(0)
        });
    let (max_test_file_bytes, max_test_file_name, total_test_data_bytes) =
        scan_test_data_sizes(ws, slug, &prob_dir);

    if max_test_file_bytes > opts.max_single_file_bytes {
        let mb = max_test_file_bytes as f64 / (1024.0 * 1024.0);
        let limit_mb = opts.max_single_file_bytes as f64 / (1024.0 * 1024.0);
        risk_flags.push(AuditRiskFlag {
            level: AuditRiskLevel::Warning,
            code: "large_test_file".to_owned(),
            problem_label: Some(label.to_owned()),
            problem_id: Some(slug.to_owned()),
            message: format!(
                "题目 {label}（{slug}）的数据 {} 有 {mb:.1} MiB，超过 {limit_mb:.0} MiB",
                max_test_file_name.as_deref().unwrap_or("test")
            ),
        });
    }

    // Validator & Checker self-test statuses.
    let (validator_self_test, checker_self_test) =
        if verification_status == VerificationStatus::StatementOnly {
            (
                SelfTestAuditStatus::NotApplicable,
                SelfTestAuditStatus::NotApplicable,
            )
        } else {
            let val_status = evaluate_validator_self_test(&prob_dir, report.as_ref());
            let use_checker = !interactive || has_explicit_checker(&prob_dir, &problem.meta);
            let is_custom_checker = use_checker
                && resolve_checker_spec(&prob_dir, &problem.meta)
                    .map(|c| c.is_custom())
                    .unwrap_or(false);
            let chk_status = if is_custom_checker {
                evaluate_checker_self_test(report.as_ref())
            } else {
                SelfTestAuditStatus::NotApplicable
            };
            (val_status, chk_status)
        };

    // Sanitizer UB check status.
    let (sanitizer_status, sanitizer_findings_count) =
        if verification_status == VerificationStatus::StatementOnly {
            (SanitizerAuditStatus::NotRun, 0)
        } else if let Some(san) = load_sanitizer_report(ws, slug) {
            let fc = san.findings.len();
            let st = match san.status {
                SanitizerStatus::Unsupported => SanitizerAuditStatus::Unsupported,
                SanitizerStatus::Failed => {
                    risk_flags.push(AuditRiskFlag {
                        level: AuditRiskLevel::Warning,
                        code: "sanitizer_ub_found".to_owned(),
                        problem_label: Some(label.to_owned()),
                        problem_id: Some(slug.to_owned()),
                        message: format!("题目 {label}（{slug}）内存检查发现 {fc} 处问题"),
                    });
                    SanitizerAuditStatus::Failed
                }
                SanitizerStatus::Passed => {
                    if !current_hash.is_empty() && san.package_hash == current_hash && !san.subset {
                        SanitizerAuditStatus::PassedFresh
                    } else {
                        SanitizerAuditStatus::PassedStale
                    }
                }
            };
            (st, fc)
        } else {
            (SanitizerAuditStatus::NotRun, 0)
        };

    // Lint counts (injected from xcpc-lint if available, else lightweight fallback).
    let lint = opts
        .lint_by_problem
        .get(slug)
        .cloned()
        .unwrap_or_else(|| fallback_lightweight_lint(ws, &problem.dir));
    if lint.error_count > 0 {
        risk_flags.push(AuditRiskFlag {
            level: AuditRiskLevel::Error,
            code: "lint_errors".to_owned(),
            problem_label: Some(label.to_owned()),
            problem_id: Some(slug.to_owned()),
            message: format!("题目 {label}（{slug}）的题面有 {} 处错误", lint.error_count),
        });
    } else if lint.warning_count > 0 {
        risk_flags.push(AuditRiskFlag {
            level: AuditRiskLevel::Warning,
            code: "lint_warnings".to_owned(),
            problem_label: Some(label.to_owned()),
            problem_id: Some(slug.to_owned()),
            message: format!(
                "题目 {label}（{slug}）的题面、题解或样例有 {} 处提醒",
                lint.warning_count
            ),
        });
    }

    ProblemAuditRow {
        label: label.to_owned(),
        problem_id: slug.to_owned(),
        title: problem.meta.title,
        verification_status,
        interactive,
        has_subtasks,
        time_limit_ms,
        std_max_time_ms,
        std_time_ratio,
        slowest_ac_time_ms,
        fastest_tle_time_ms,
        ac_tle_margin_ratio,
        cumulative_std_time_ms,
        test_count,
        max_test_file_bytes,
        max_test_file_name,
        total_test_data_bytes,
        independent_ac_count,
        wrong_solution_count,
        validator_self_test,
        checker_self_test,
        sanitizer_status,
        sanitizer_findings_count,
        lint_error_count: lint.error_count,
        lint_warning_count: lint.warning_count,
        risk_flags,
    }
}

fn evaluate_validator_self_test(
    prob_dir: &Path,
    report: Option<&BuildReport>,
) -> SelfTestAuditStatus {
    if find_validator_source(prob_dir).is_none() {
        return SelfTestAuditStatus::Missing;
    }
    if let Some(r) = report
        && let Some(gate) = r.gates.iter().find(|g| g.id == "validator")
    {
        return match gate.status {
            GateStatus::Pass => SelfTestAuditStatus::Passed,
            GateStatus::Fail => SelfTestAuditStatus::Failed,
            GateStatus::Warn => SelfTestAuditStatus::Missing,
            GateStatus::Skip => SelfTestAuditStatus::NotApplicable,
        };
    }
    SelfTestAuditStatus::Missing
}

fn evaluate_checker_self_test(report: Option<&BuildReport>) -> SelfTestAuditStatus {
    if let Some(r) = report
        && let Some(gate) = r.gates.iter().find(|g| g.id == "checker_self_tests")
    {
        return match gate.status {
            GateStatus::Pass => SelfTestAuditStatus::Passed,
            GateStatus::Fail => SelfTestAuditStatus::Failed,
            GateStatus::Warn => SelfTestAuditStatus::Missing,
            GateStatus::Skip => SelfTestAuditStatus::NotApplicable,
        };
    }
    SelfTestAuditStatus::Missing
}

fn scan_test_data_sizes(ws: &Workspace, slug: &str, prob_dir: &Path) -> (u64, Option<String>, u64) {
    let mut max_bytes = 0u64;
    let mut max_name: Option<String> = None;
    let mut total_bytes = 0u64;

    let build_tests_dir = build_dir_for(ws, slug).join("tests");
    let scan_dir = if build_tests_dir.is_dir() {
        build_tests_dir
    } else {
        prob_dir.join("samples")
    };

    if let Ok(rd) = fs::read_dir(&scan_dir) {
        for entry in rd.flatten() {
            let p = entry.path();
            if !p.is_file() {
                continue;
            }
            let ext = p.extension().and_then(|s| s.to_str()).unwrap_or("");
            if !matches!(ext, "in" | "ans" | "out") {
                continue;
            }
            if let Ok(meta) = entry.metadata() {
                let sz = meta.len();
                total_bytes = total_bytes.saturating_add(sz);
                if sz > max_bytes {
                    max_bytes = sz;
                    max_name = p.file_name().and_then(|s| s.to_str()).map(str::to_owned);
                }
            }
        }
    }

    (max_bytes, max_name, total_bytes)
}

fn fallback_lightweight_lint(ws: &Workspace, prob_dir_rel: &str) -> ProblemLintSummary {
    let mut summary = ProblemLintSummary::default();
    let samples_rel = format!("{prob_dir_rel}/samples");
    if let Ok(inv) = ws.inspect_samples(&samples_rel) {
        for orphan in inv.orphan_inputs.iter().chain(inv.orphan_outputs.iter()) {
            summary.warning_count += 1;
            summary.findings.push(AuditLintFinding {
                severity: "warning".to_owned(),
                code: "sample_unpaired".to_owned(),
                message: format!("unpaired sample file {orphan}"),
                file: Some(format!("{samples_rel}/{orphan}")),
            });
        }
        if inv.non_contiguous {
            summary.warning_count += 1;
            summary.findings.push(AuditLintFinding {
                severity: "warning".to_owned(),
                code: "sample_non_contiguous".to_owned(),
                message: "sample numbering is not contiguous from 01".to_owned(),
                file: Some(samples_rel),
            });
        }
    }
    let editorial_rel = format!("{prob_dir_rel}/editorial.md");
    if ws.resolve(&editorial_rel).is_ok_and(|p| !p.is_file()) {
        summary.warning_count += 1;
        summary.findings.push(AuditLintFinding {
            severity: "warning".to_owned(),
            code: "missing_editorial".to_owned(),
            message: "problem has no editorial.md".to_owned(),
            file: Some(editorial_rel),
        });
    }
    summary
}

fn build_next_steps(
    contest_id: &str,
    rows: &[ProblemAuditRow],
    summary: &ContestAuditSummary,
) -> Vec<String> {
    let mut steps = Vec::new();
    let need_verify: Vec<&str> = rows
        .iter()
        .filter(|r| {
            matches!(
                r.verification_status,
                VerificationStatus::Stale
                    | VerificationStatus::Unbuilt
                    | VerificationStatus::Failed
            )
        })
        .map(|r| r.problem_id.as_str())
        .collect();
    if !need_verify.is_empty() {
        steps.push(format!(
            "对 {} 道未通过/已过期题目执行严格验收：{}",
            need_verify.len(),
            need_verify.join("、")
        ));
    }
    if summary.sanitizer_failed_count > 0 {
        let failed_san: Vec<&str> = rows
            .iter()
            .filter(|r| r.sanitizer_status == SanitizerAuditStatus::Failed)
            .map(|r| r.problem_id.as_str())
            .collect();
        steps.push(format!(
            "修复 Sanitizer UB 体检报错的题目：{}",
            failed_san.join("、")
        ));
    } else if summary.sanitizer_not_run_count > 0 {
        steps.push(format!(
            "有 {} 道题目包尚未运行 UB 体检，建议在冻结前执行整场 Sanitizer 体检。",
            summary.sanitizer_not_run_count
        ));
    }
    if summary.ready_for_freeze {
        steps.push(format!(
            "整场比赛已满足冻结前置条件，可执行 `xcpc contest freeze {contest_id}`。"
        ));
    }
    steps
}

/// Renders a clean Markdown report for [`ContestAuditReport`].
pub fn render_contest_audit_markdown(report: &ContestAuditReport) -> String {
    let mut md = String::new();
    md.push_str(&format!(
        "# 整场验题体检报告：{}（{}）\n\n",
        report.contest_title, report.contest_id
    ));
    md.push_str(&format!(
        "- 题目总数：{}（已验收 {} / 已过期 {} / 失败 {} / 未构建 {} / 纯出版 {}）\n",
        report.summary.total_problems,
        report.summary.fresh_count,
        report.summary.stale_count,
        report.summary.failed_count,
        report.summary.unbuilt_count,
        report.summary.statement_only_count
    ));
    md.push_str(&format!(
        "- Sanitizer UB 体检：通过 {} / 发现 UB {} / 未运行 {}\n",
        report.summary.sanitizer_passed_count,
        report.summary.sanitizer_failed_count,
        report.summary.sanitizer_not_run_count
    ));
    md.push_str(&format!(
        "- 冻结就绪状态：{}\n\n",
        if report.summary.ready_for_freeze {
            "已就绪"
        } else {
            "未就绪（存在阻断项）"
        }
    ));

    md.push_str("## 逐题体检总览\n\n");
    md.push_str(
        "| 题号 | 题目 | 验收状态 | 标程最慢 / 时限 | 累计标程耗时 | 最大单点 | 独立AC / 错解 | 自测(V/C) | UB 体检 | Lint (E/W) |\n",
    );
    md.push_str("| --- | --- | --- | ---: | ---: | ---: | ---: | :---: | --- | ---: |\n");
    for r in &report.problems {
        let timing = match (r.std_max_time_ms, r.time_limit_ms, r.std_time_ratio) {
            (Some(ms), Some(tl), Some(ratio)) => {
                format!("{ms}ms / {tl}ms ({:.0}%)", ratio * 100.0)
            }
            (_, Some(tl), _) => format!("- / {tl}ms"),
            _ => "-".to_owned(),
        };
        let cum_str = r
            .cumulative_std_time_ms
            .map(|ms| format!("{:.2}s", ms as f64 / 1000.0))
            .unwrap_or_else(|| "-".to_owned());
        let max_file = if r.max_test_file_bytes == 0 {
            "-".to_owned()
        } else if r.max_test_file_bytes >= 1024 * 1024 {
            format!(
                "{:.1} MiB",
                r.max_test_file_bytes as f64 / (1024.0 * 1024.0)
            )
        } else {
            format!("{:.1} KiB", r.max_test_file_bytes as f64 / 1024.0)
        };
        let self_test = format!(
            "{} / {}",
            r.validator_self_test.label_zh(),
            r.checker_self_test.label_zh()
        );
        let san = if r.verification_status == VerificationStatus::StatementOnly {
            "-".to_owned()
        } else if r.sanitizer_findings_count > 0 {
            format!(
                "{} ({})",
                r.sanitizer_status.label_zh(),
                r.sanitizer_findings_count
            )
        } else {
            r.sanitizer_status.label_zh().to_owned()
        };

        md.push_str(&format!(
            "| {} | `{}` {} | {} | {} | {} | {} | {} / {} | {} | {} | {} / {} |\n",
            r.label,
            r.problem_id,
            r.title.replace('|', "\\|"),
            r.verification_status.label_zh(),
            timing,
            cum_str,
            max_file,
            r.independent_ac_count,
            r.wrong_solution_count,
            self_test,
            san,
            r.lint_error_count,
            r.lint_warning_count
        ));
    }

    if !report.risk_flags.is_empty() {
        md.push_str("\n## 风险与预警\n\n");
        for f in &report.risk_flags {
            let tag = match f.level {
                AuditRiskLevel::Error => "[ERROR]",
                AuditRiskLevel::Warning => "[WARN]",
            };
            md.push_str(&format!("- **{tag}** `{}`: {}\n", f.code, f.message));
        }
    }

    if !report.next_steps.is_empty() {
        md.push_str("\n## 下一步建议\n\n");
        for step in &report.next_steps {
            md.push_str(&format!("- {step}\n"));
        }
    }

    md
}

/// `800` → `0.8 秒`, `1000` → `1 秒`.
fn format_seconds(ms: u64) -> String {
    let s = ms as f64 / 1000.0;
    let text = format!("{s:.2}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    format!("{text} 秒")
}

fn now_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

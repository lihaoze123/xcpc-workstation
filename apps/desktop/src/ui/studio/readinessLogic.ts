// Pure types and formatting/aggregation helpers for Phase 3D:
// - Sanitizer (ASan/UBSan) UB check reports
// - External tester trial runs (single-problem, contest single-dir, contest multi-tester)
// - Contest readiness audit (per-problem risk highlighting, batch target selection)

export type SanitizerStatus = "passed" | "failed" | "unsupported";
export type SanitizerTargetKind = "generator" | "validator" | "checker" | "interactor" | "solution";

export interface SanitizerFindingData {
  target: string;
  kind: SanitizerTargetKind;
  test_index?: number;
  test_name?: string;
  summary: string;
  stack_summary?: string[];
  raw_stderr?: string;
}

export interface SanitizerTargetResultData {
  target: string;
  kind: SanitizerTargetKind;
  source_path: string;
  passed: boolean;
  checked_runs: number;
  findings_count: number;
}

export interface SanitizerReportData {
  problem_id: string;
  package_hash: string;
  status: SanitizerStatus;
  ok: boolean;
  subset: boolean;
  checked_at_epoch_secs: number;
  unsupported_reason?: string;
  relaxed_time_limit_ms: number;
  compile_failures?: Array<{ target: string; source: string; stderr: string }>;
  targets?: SanitizerTargetResultData[];
  findings?: SanitizerFindingData[];
  diagnostics?: string[];
  next_steps?: string[];
}

export interface ExternalFirstFailureData {
  test_index: number;
  test_name: string;
  verdict: string;
  message?: string;
}

export interface ExternalSubmissionResultData {
  tester?: string;
  problem_id: string;
  problem_label?: string;
  file_path: string;
  file_name: string;
  verdict: string;
  passed_tests: number;
  total_tests: number;
  total_score?: number;
  max_score?: number;
  max_time_ms: number;
  max_memory_kib: number;
  std_max_time_ms?: number;
  ratio_vs_std?: number;
  first_failure?: ExternalFirstFailureData;
  compile_error?: string;
}

export interface UnmatchedExternalFileData {
  file_path: string;
  tester?: string;
  reason: string;
}

export interface ExternalProblemReportData {
  problem_id: string;
  submissions: ExternalSubmissionResultData[];
}

export type ExternalContestMode = "single_directory" | "multi_tester";

export interface ContestProblemRefData {
  label: string;
  problem_id: string;
  title: string;
}

export interface ExternalContestReportData {
  contest_id: string;
  contest_title: string;
  mode: ExternalContestMode;
  problems: ContestProblemRefData[];
  testers?: string[];
  submissions: ExternalSubmissionResultData[];
  unmatched?: UnmatchedExternalFileData[];
}

export type VerificationStatus = "fresh" | "stale" | "failed" | "unbuilt" | "statement_only";
export type SanitizerAuditStatus = "passed_fresh" | "passed_stale" | "failed" | "unsupported" | "not_run";
export type SelfTestAuditStatus = "passed" | "failed" | "missing" | "not_applicable";
export type AuditRiskLevel = "error" | "warning";

export interface AuditRiskFlagData {
  level: AuditRiskLevel;
  code: string;
  problem_label?: string;
  problem_id?: string;
  message: string;
}

export interface ProblemAuditRowData {
  label: string;
  problem_id: string;
  title: string;
  verification_status: VerificationStatus;
  interactive: boolean;
  has_subtasks: boolean;
  time_limit_ms?: number;
  std_max_time_ms?: number;
  std_time_ratio?: number;
  slowest_ac_time_ms?: number;
  fastest_tle_time_ms?: number;
  ac_tle_margin_ratio?: number;
  cumulative_std_time_ms?: number;
  test_count: number;
  max_test_file_bytes: number;
  max_test_file_name?: string;
  total_test_data_bytes: number;
  independent_ac_count: number;
  wrong_solution_count: number;
  validator_self_test: SelfTestAuditStatus;
  checker_self_test: SelfTestAuditStatus;
  sanitizer_status: SanitizerAuditStatus;
  sanitizer_findings_count: number;
  lint_error_count: number;
  lint_warning_count: number;
  risk_flags?: AuditRiskFlagData[];
}

export interface ContestAuditSummaryData {
  total_problems: number;
  fresh_count: number;
  stale_count: number;
  failed_count: number;
  unbuilt_count: number;
  statement_only_count: number;
  sanitizer_passed_count: number;
  sanitizer_failed_count: number;
  sanitizer_not_run_count: number;
  error_risk_count: number;
  warning_risk_count: number;
  ready_for_freeze: boolean;
}

export interface ContestAuditReportData {
  contest_id: string;
  contest_title: string;
  frozen: boolean;
  audited_at_epoch_secs: number;
  summary: ContestAuditSummaryData;
  problems: ProblemAuditRowData[];
  risk_flags?: AuditRiskFlagData[];
  next_steps?: string[];
}

export function parseSanitizerReport(json: string | null | undefined): SanitizerReportData | undefined {
  if (!json) return undefined;
  try {
    return JSON.parse(json) as SanitizerReportData;
  } catch {
    return undefined;
  }
}

export function parseExternalProblemReport(json: string | null | undefined): ExternalProblemReportData | undefined {
  if (!json) return undefined;
  try {
    return JSON.parse(json) as ExternalProblemReportData;
  } catch {
    return undefined;
  }
}

export function parseExternalContestReport(json: string | null | undefined): ExternalContestReportData | undefined {
  if (!json) return undefined;
  try {
    return JSON.parse(json) as ExternalContestReportData;
  } catch {
    return undefined;
  }
}

export function parseContestAuditReport(json: string | null | undefined): ContestAuditReportData | undefined {
  if (!json) return undefined;
  try {
    return JSON.parse(json) as ContestAuditReportData;
  } catch {
    return undefined;
  }
}

/** Returns contest problem IDs that need verification (`stale`, `failed`, or `unbuilt`). */
export function selectStaleOrUnverifiedProblemIds(report: ContestAuditReportData | undefined): string[] {
  if (!report) return [];
  return report.problems
    .filter(
      (row) =>
        row.verification_status === "stale" ||
        row.verification_status === "failed" ||
        row.verification_status === "unbuilt",
    )
    .map((row) => row.problem_id);
}

/** Returns all problem IDs in the contest that have a package (non-`statement_only`) for UB check. */
export function selectPackageProblemIds(report: ContestAuditReportData | undefined): string[] {
  if (!report) return [];
  return report.problems
    .filter((row) => row.verification_status !== "statement_only")
    .map((row) => row.problem_id);
}

/** Formats `std` slowest runtime against TL, e.g. `380ms (19%)`. */
export function formatStdTimeBudget(row: ProblemAuditRowData): string {
  if (row.std_max_time_ms === undefined || row.std_max_time_ms === null) return "—";
  const msStr = `${row.std_max_time_ms}ms`;
  if (row.std_time_ratio === undefined || row.std_time_ratio === null || !Number.isFinite(row.std_time_ratio)) {
    return msStr;
  }
  const pct = Math.round(row.std_time_ratio * 100);
  return `${msStr} (${pct}%)`;
}

/** Formats AC vs TLE safety margin ratio, e.g. `4.2×`. */
export function formatSafetyMarginRatio(row: ProblemAuditRowData): string {
  if (row.ac_tle_margin_ratio === undefined || row.ac_tle_margin_ratio === null || !Number.isFinite(row.ac_tle_margin_ratio)) {
    return "—";
  }
  return `${row.ac_tle_margin_ratio.toFixed(row.ac_tle_margin_ratio >= 10 ? 0 : 1)}×`;
}

/** Formats external submission runtime ratio vs `std`, e.g. `1.35× std`. */
export function formatExternalRatio(ratio: number | undefined | null): string {
  if (ratio === undefined || ratio === null || !Number.isFinite(ratio)) return "—";
  return `${ratio.toFixed(2)}× std`;
}

/** Formats cumulative `std` runtime concisely (`1.2s` or `450ms`). */
export function formatCumulativeMs(ms: number | undefined | null): string {
  if (ms === undefined || ms === null || !Number.isFinite(ms)) return "—";
  if (ms >= 1000) return `${(ms / 1000).toFixed(1)}s`;
  return `${Math.round(ms)}ms`;
}

/** Determines the highest risk severity of an audit row for visual highlighting. */
export function auditRowRiskSeverity(row: ProblemAuditRowData): "error" | "warning" | "ok" {
  const flags = row.risk_flags ?? [];
  if (
    row.verification_status === "failed" ||
    row.sanitizer_status === "failed" ||
    row.lint_error_count > 0 ||
    flags.some((f) => f.level === "error")
  ) {
    return "error";
  }
  if (
    row.verification_status === "stale" ||
    row.verification_status === "unbuilt" ||
    row.lint_warning_count > 0 ||
    flags.some((f) => f.level === "warning")
  ) {
    return "warning";
  }
  return "ok";
}

/** Suggests default `expect` verdict list when adopting an external submission. */
export function inferAdoptExpect(verdict: string): string[] {
  const upper = verdict.trim().toUpperCase();
  if (upper === "AC" || upper === "WA" || upper === "TLE" || upper === "MLE" || upper === "RE" || upper === "PE") {
    return [upper];
  }
  return ["AC"];
}

/** Builds a lookup map of `(tester, problem_id) -> submissions[]` for multi-tester matrix display. */
export function buildMultiTesterMatrix(report: ExternalContestReportData): {
  testers: string[];
  problems: ContestProblemRefData[];
  cells: Map<string, ExternalSubmissionResultData[]>;
} {
  const testers = report.testers ?? [];
  const cells = new Map<string, ExternalSubmissionResultData[]>();
  for (const sub of report.submissions) {
    const tester = sub.tester ?? "";
    const key = `${tester}::${sub.problem_id}`;
    const list = cells.get(key);
    if (list) list.push(sub);
    else cells.set(key, [sub]);
  }
  return { testers, problems: report.problems, cells };
}

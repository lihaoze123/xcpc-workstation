import { expect, test } from "nsh/test";
import {
  auditRowRiskSeverity,
  buildMultiTesterMatrix,
  formatCumulativeMs,
  formatExternalRatio,
  formatSafetyMarginRatio,
  formatStdTimeBudget,
  inferAdoptExpect,
  parseContestAuditReport,
  parseSanitizerReport,
  selectPackageProblemIds,
  selectStaleOrUnverifiedProblemIds,
  type ContestAuditReportData,
  type ExternalContestReportData,
  type ProblemAuditRowData,
} from "./studio/readinessLogic";

function makeRow(overrides: Partial<ProblemAuditRowData>): ProblemAuditRowData {
  return {
    label: "A",
    problem_id: "prob-a",
    title: "Problem A",
    verification_status: "fresh",
    interactive: false,
    has_subtasks: false,
    test_count: 10,
    max_test_file_bytes: 1024,
    total_test_data_bytes: 8192,
    independent_ac_count: 1,
    wrong_solution_count: 1,
    validator_self_test: "passed",
    checker_self_test: "not_applicable",
    sanitizer_status: "passed_fresh",
    sanitizer_findings_count: 0,
    lint_error_count: 0,
    lint_warning_count: 0,
    risk_flags: [],
    ...overrides,
  };
}

test("selectStaleOrUnverifiedProblemIds picks stale, failed, and unbuilt in order", () => {
  const report: ContestAuditReportData = {
    contest_id: "c1",
    contest_title: "Contest 1",
    frozen: false,
    audited_at_epoch_secs: 1000,
    summary: {
      total_problems: 5,
      fresh_count: 1,
      stale_count: 1,
      failed_count: 1,
      unbuilt_count: 1,
      statement_only_count: 1,
      sanitizer_passed_count: 1,
      sanitizer_failed_count: 0,
      sanitizer_not_run_count: 3,
      error_risk_count: 1,
      warning_risk_count: 2,
      ready_for_freeze: false,
    },
    problems: [
      makeRow({ label: "A", problem_id: "a", verification_status: "fresh" }),
      makeRow({ label: "B", problem_id: "b", verification_status: "stale" }),
      makeRow({ label: "C", problem_id: "c", verification_status: "statement_only" }),
      makeRow({ label: "D", problem_id: "d", verification_status: "failed" }),
      makeRow({ label: "E", problem_id: "e", verification_status: "unbuilt" }),
    ],
  };
  expect(selectStaleOrUnverifiedProblemIds(report)).toEqual(["b", "d", "e"]);
  expect(selectPackageProblemIds(report)).toEqual(["a", "b", "d", "e"]);
});

test("formatting helpers render tabular metrics cleanly", () => {
  const row = makeRow({
    time_limit_ms: 2000,
    std_max_time_ms: 380,
    std_time_ratio: 0.19,
    ac_tle_margin_ratio: 4.25,
  });
  expect(formatStdTimeBudget(row)).toBe("380ms (19%)");
  expect(formatSafetyMarginRatio(row)).toBe("4.3×");
  expect(formatExternalRatio(1.354)).toBe("1.35× std");
  expect(formatExternalRatio(undefined)).toBe("—");
  expect(formatCumulativeMs(450)).toBe("450ms");
  expect(formatCumulativeMs(32500)).toBe("32.5s");
});

test("auditRowRiskSeverity highlights error, warning, and ok rows accurately", () => {
  expect(auditRowRiskSeverity(makeRow({}))).toBe("ok");
  expect(auditRowRiskSeverity(makeRow({ verification_status: "stale" }))).toBe("warning");
  expect(auditRowRiskSeverity(makeRow({ sanitizer_status: "failed", sanitizer_findings_count: 1 }))).toBe("error");
  expect(auditRowRiskSeverity(makeRow({ lint_error_count: 1 }))).toBe("error");
});

test("inferAdoptExpect maps standard verdicts and falls back to AC", () => {
  expect(inferAdoptExpect("AC")).toEqual(["AC"]);
  expect(inferAdoptExpect("tle")).toEqual(["TLE"]);
  expect(inferAdoptExpect("WA")).toEqual(["WA"]);
  expect(inferAdoptExpect("CE")).toEqual(["AC"]);
});

test("buildMultiTesterMatrix indexes submissions by tester and problem", () => {
  const report: ExternalContestReportData = {
    contest_id: "c1",
    contest_title: "C1",
    mode: "multi_tester",
    problems: [
      { label: "A", problem_id: "a", title: "A" },
      { label: "B", problem_id: "b", title: "B" },
    ],
    testers: ["alice", "bob"],
    submissions: [
      {
        tester: "alice",
        problem_id: "a",
        problem_label: "A",
        file_path: "/tmp/alice/A.cpp",
        file_name: "A.cpp",
        verdict: "AC",
        passed_tests: 10,
        total_tests: 10,
        max_time_ms: 42,
        max_memory_kib: 2048,
        ratio_vs_std: 1.1,
      },
    ],
    unmatched: [],
  };
  const matrix = buildMultiTesterMatrix(report);
  expect(matrix.testers).toEqual(["alice", "bob"]);
  expect(matrix.cells.get("alice::a")?.length).toBe(1);
  expect(matrix.cells.get("bob::a")).toBeUndefined();
});

test("JSON parsers handle invalid or empty inputs safely", () => {
  expect(parseSanitizerReport(null)).toBeUndefined();
  expect(parseSanitizerReport("{bad")).toBeUndefined();
  expect(parseContestAuditReport(undefined)).toBeUndefined();
});

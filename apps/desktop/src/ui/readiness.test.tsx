import { expect, test } from "nsh/test";
import { languagePref, systemLocale } from "./i18n";
import {
  idsToCheck,
  inferAdoptExpect,
  parseContestAuditReport,
  parseSanitizerReport,
  type ContestAuditReportData,
  type ProblemAuditRowData,
} from "./studio/readinessLogic";
import { contestStatusModel, markForAuditRow } from "./studio/statusLogic";

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
    sanitizer_status: "not_run",
    sanitizer_findings_count: 0,
    lint_error_count: 0,
    lint_warning_count: 0,
    risk_flags: [],
    ...overrides,
  };
}

function makeReport(problems: ProblemAuditRowData[]): ContestAuditReportData {
  return {
    contest_id: "c",
    contest_title: "C",
    frozen: false,
    audited_at_epoch_secs: 0,
    summary: {
      total_problems: problems.length,
      fresh_count: 0,
      stale_count: 0,
      failed_count: 0,
      unbuilt_count: 0,
      statement_only_count: 0,
      sanitizer_passed_count: 0,
      sanitizer_failed_count: 0,
      sanitizer_not_run_count: 0,
      error_risk_count: 0,
      warning_risk_count: 0,
      ready_for_freeze: false,
    },
    problems,
  };
}

const zh = () => {
  systemLocale.set("zh-CN");
  languagePref.set("system");
};
const noActions = { goProblem: () => {}, goSettings: () => {} };

test("outline marks follow the three states (§3.1)", () => {
  expect(markForAuditRow(makeRow({}))).toBe("passed");
  expect(markForAuditRow(makeRow({ verification_status: "stale" }))).toBe("unchecked");
  expect(markForAuditRow(makeRow({ verification_status: "unbuilt" }))).toBe("unchecked");
  expect(markForAuditRow(makeRow({ verification_status: "failed" }))).toBe("issues");
  expect(markForAuditRow(makeRow({ verification_status: "statement_only" }))).toBe("none");
  expect(
    markForAuditRow(makeRow({ risk_flags: [{ level: "error", code: "lint_errors", message: "x" }] })),
  ).toBe("issues");
  // 「还没检查」 is not an issue.
  expect(
    markForAuditRow(
      makeRow({ verification_status: "unbuilt", risk_flags: [{ level: "error", code: "package_unverified", message: "x" }] }),
    ),
  ).toBe("unchecked");
});

test("「检查」 runs unchecked and failing problems; 全部重新检查 runs all", () => {
  const report = makeReport([
    makeRow({ problem_id: "a" }),
    makeRow({ problem_id: "b", verification_status: "stale" }),
    makeRow({ problem_id: "c", verification_status: "failed" }),
    makeRow({ problem_id: "d", verification_status: "statement_only" }),
  ]);
  expect(idsToCheck(report, false)).toEqual(["b", "c"]);
  expect(idsToCheck(report, true)).toEqual(["a", "b", "c"]);
  expect(idsToCheck(undefined, true)).toEqual([]);
});

test("contest status slot: issues first, then not checked, else 可以定稿", () => {
  zh();
  const issues = contestStatusModel(
    {
      report: makeReport([
        makeRow({
          label: "B",
          problem_id: "b",
          title: "区间求和",
          verification_status: "failed",
          risk_flags: [{ level: "error", code: "package_failed", message: "题目 B（b）有问题：校验器缺少自测用例" }],
        }),
        makeRow({ label: "C", problem_id: "c", verification_status: "unbuilt" }),
      ]),
      compilerMissing: false,
      tester: new Map(),
    },
    noActions,
  );
  expect(issues.mark).toBe("issues");
  expect(issues.label).toBe("1 题有问题");
  expect(issues.items[0].text).toBe("校验器缺少自测用例");
  expect(issues.items[0].where).toBe("B　区间求和");

  const unchecked = contestStatusModel(
    { report: makeReport([makeRow({ verification_status: "stale" })]), compilerMissing: false, tester: new Map() },
    noActions,
  );
  expect(unchecked).toMatchObject({ mark: "unchecked", label: "1 题未检查" });

  const ready = contestStatusModel({ report: makeReport([makeRow({})]), compilerMissing: false, tester: new Map() }, noActions);
  expect(ready).toMatchObject({ mark: "passed", label: "可以定稿" });

  const running = contestStatusModel(
    { report: makeReport([makeRow({})]), running: { done: 2, total: 4 }, compilerMissing: false, tester: new Map() },
    noActions,
  );
  expect(running).toMatchObject({ mark: "running", label: "检查中 2/4" });
});

test("tester code that does not pass is summarized per tester", () => {
  zh();
  const sub = (tester: string, verdict: string) => ({
    tester,
    problem_id: "b",
    file_path: `/t/${tester}/B.cpp`,
    file_name: "B.cpp",
    verdict,
    passed_tests: 0,
    total_tests: 1,
    max_time_ms: 1,
    max_memory_kib: 1,
  });
  const m = contestStatusModel(
    {
      report: makeReport([makeRow({ label: "B", problem_id: "b" })]),
      compilerMissing: false,
      tester: new Map([["b", [sub("alice", "WA"), sub("bob", "AC")]]]),
    },
    noActions,
  );
  expect(m.reminders.map((r) => r.text)).toEqual(["alice 有 1 题未通过（B）"]);
});

test("inferAdoptExpect maps standard verdicts and falls back to AC", () => {
  expect(inferAdoptExpect("WA")).toEqual(["WA"]);
  expect(inferAdoptExpect(" tle ")).toEqual(["TLE"]);
  expect(inferAdoptExpect("ILE")).toEqual(["AC"]);
});

test("JSON parsers handle invalid or empty inputs safely", () => {
  expect(parseContestAuditReport(undefined)).toBeUndefined();
  expect(parseContestAuditReport("not json")).toBeUndefined();
  expect(parseSanitizerReport("")).toBeUndefined();
});

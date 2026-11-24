import { describe, expect, test } from "nsh/test";
import { languagePref, systemLocale, t } from "./i18n";
import { currentToast, dismissToast, showToast } from "./Toast";
import { markForPackageStatus, problemStatusModel, problemTypeSummary, type ProblemStatusInput } from "./studio/statusLogic";

function input(over: Partial<ProblemStatusInput> = {}): ProblemStatusInput {
  return {
    hasPackage: true,
    running: false,
    compilerMissing: false,
    status: { kind: "unverified", issueCount: 0 },
    failedGates: [],
    warnGates: [],
    compileFailures: [],
    isZh: true,
    ...over,
  };
}

function zh() {
  systemLocale.set("zh-CN");
  languagePref.set("system");
}

const noop = { goSettings: () => {} };

describe("Problem status slot (authoring-ui-refactor §3, §6)", () => {
  test("no compiler is said up front, with a way to Settings → Toolchain", () => {
    zh();
    let went = false;
    const m = problemStatusModel(input({ compilerMissing: true }), { goSettings: () => (went = true) });
    expect(m.mark).toBe("issues");
    expect(m.label).toBe("没有找到 C++ 编译器");
    expect(m.action?.label).toBe("去设置");
    m.action!.run();
    expect(went).toBe(true);
  });

  test("an action error shows the Core's words, never the generic line", () => {
    zh();
    const m = problemStatusModel(
      input({ error: { code: "no_main_solution", error: "题目「a」还没有标程。请在解法中把一份解法设为标程。" } }),
      noop,
    );
    expect(m.mark).toBe("issues");
    expect(m.label).toContain("还没有标程");
    expect(m.label).not.toBe(t("error.generic"));
    expect(m.details).toContain("code: no_main_solution");
  });

  test("a cancelled check is not an error", () => {
    zh();
    const m = problemStatusModel(input({ error: { code: "cancelled", error: "检查已取消。" } }), noop);
    expect(m.mark).toBe("unchecked");
  });

  test("issues list the concrete reasons; warnings only remind", () => {
    zh();
    const m = problemStatusModel(
      input({
        status: { kind: "issues", issueCount: 1 },
        failedGates: [{ id: "validator", title: "校验器与自测", status: "fail", message: "校验器缺少自测用例" }],
        warnGates: [{ id: "solutions", title: "参考解与错解", status: "warn", message: "没有错解" }],
      }),
      noop,
    );
    expect(m.label).toBe("1 处问题");
    expect(m.items.map((i) => i.text)).toEqual(["校验器缺少自测用例"]);
    expect(m.reminders.map((i) => i.text)).toEqual(["没有错解"]);
  });

  test("passed is quiet; running and not-checked use their marks", () => {
    zh();
    expect(problemStatusModel(input({ status: { kind: "ready", issueCount: 0 } }), noop)).toMatchObject({ mark: "passed", label: "已通过", items: [] });
    expect(problemStatusModel(input({ running: true, progress: { done: 2, total: 5 } }), noop).label).toBe("检查中 2/5");
    expect(problemStatusModel(input({ status: { kind: "needs_reverify", issueCount: 0 } }), noop).mark).toBe("unchecked");
    expect(problemStatusModel(input({ hasPackage: false }), noop).mark).toBe("none");
  });

  test("package status maps to the three marks", () => {
    expect(["verified_fresh", "failed", "stale", "unverified", null].map(markForPackageStatus)).toEqual([
      "passed",
      "issues",
      "unchecked",
      "unchecked",
      "none",
    ]);
  });
});

describe("题型 summary uses spacing, never `·` (design-system §1.4)", () => {
  test("joins parts with a full-width space", () => {
    systemLocale.set("zh-CN");
    languagePref.set("system");
    const s = problemTypeSummary({ interactive: true, runs: 2, subtaskScore: 100 });
    expect(s).toBe("交互　两轮　子任务 100 分");
    expect(s.includes("·")).toBe(false);
  });
});

describe("Single-slot Toast (design-system §6.5 tier 3)", () => {
  test("replaces previous toast without stacking and uses tone-specific durations", () => {
    dismissToast();
    expect(currentToast()).toBeUndefined();

    showToast({ tone: "success", text: "比赛检查通过，可以定稿" });
    const first = currentToast();
    expect(first?.tone).toBe("success");
    expect(first?.text).toBe("比赛检查通过，可以定稿");
    expect(first?.durationMs).toBe(3000);

    showToast({ tone: "issues", text: "检查完成：2 题有问题", action: { label: "查看", run: () => {} } });
    const second = currentToast();
    expect(second?.id).not.toBe(first?.id);
    expect(second?.tone).toBe("issues");
    expect(second?.durationMs).toBe(4500);
    expect(second?.action?.label).toBe("查看");

    dismissToast();
    expect(currentToast()).toBeUndefined();
  });
});

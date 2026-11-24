// What a problem's status slot says (authoring-ui-refactor §3, §6): one pure function from
// the authoring state to a `StatusModel`, so every case — no compiler, an error, checking,
// passed, has issues, not checked — is unit-tested without the UI.

import { locale, t } from "../i18n";
import { errorCopy, errorDetails } from "../logic";
import type { UiError } from "../store";
import type { GateResultData, SingleStatusKind } from "./buildLogic";
import { humanizeCheckText, isMissingCompilerError } from "./buildLogic";
import type { StatusItem, StatusMarkKind, StatusModel } from "./StatusSlot";

export interface ProblemStatusInput {
  hasPackage: boolean;
  running: boolean;
  /** The running check is a memory check. */
  memoryCheck?: boolean;
  progress?: { done: number; total: number };
  compilerMissing: boolean;
  status: { kind: SingleStatusKind; issueCount: number };
  failedGates: readonly GateResultData[];
  warnGates: readonly GateResultData[];
  compileFailures: readonly { source_path: string }[];
  /** The last action's error (a failed start, a failed file write…). */
  error?: UiError;
  isZh: boolean;
}

export interface ProblemStatusActions {
  /** Where 「前往」 goes for a check item; none when there is nowhere better. */
  goForGate?: (gate: GateResultData) => (() => void) | undefined;
  openFile?: (relPath: string) => void;
  goSettings: () => void;
}

export function problemStatusModel(input: ProblemStatusInput, actions: ProblemStatusActions): StatusModel {
  const none: StatusModel = { mark: "none", label: "", items: [], reminders: [] };
  if (!input.hasPackage) return none;

  if (input.running) {
    const p = input.progress;
    return {
      mark: "running",
      label: input.memoryCheck
        ? t("status.memoryChecking")
        : p && p.total > 0
          ? t("status.checkingProgress", { done: p.done, total: p.total })
          : t("status.checking"),
      items: [],
      reminders: [],
    };
  }

  const settings = { label: t("status.goSettings"), run: actions.goSettings };
  if (input.compilerMissing) {
    return {
      mark: "issues",
      label: t("status.noCompiler"),
      items: [{ id: "no_compiler", text: t("status.noCompilerDetail") }],
      reminders: [],
      action: settings,
    };
  }

  const err = input.error;
  if (err && err.code !== "cancelled" && (err.code || err.error)) {
    const words = errorCopy(err.code, err.error);
    const noCompiler = err.code === "no_compiler" || err.code === "compiler_not_found" || isMissingCompilerError(err.error);
    return {
      mark: "issues",
      label: noCompiler ? t("status.noCompiler") : words.message,
      items: [{ id: "error", text: words.next ? `${words.message}　${words.next}` : words.message }],
      reminders: [],
      action: noCompiler ? settings : undefined,
      details: errorDetails(err.code, err.error, err.context),
    };
  }

  const gateItem = (g: GateResultData): StatusItem => ({
    id: g.id,
    text: humanizeCheckText(g.message.trim() || g.title, input.isZh),
    onGo: actions.goForGate?.(g),
  });
  const reminders = input.warnGates.map(gateItem);

  switch (input.status.kind) {
    case "ready":
      return { mark: "passed", label: t("status.passed"), items: [], reminders };
    case "issues": {
      const items: StatusItem[] = [
        ...input.compileFailures.map((cf) => ({
          id: `compile:${cf.source_path}`,
          text: t("status.compileFailed", { file: cf.source_path }),
          onGo: actions.openFile ? () => actions.openFile!(cf.source_path) : undefined,
        })),
        ...input.failedGates.filter((g) => g.id !== "compile" || input.compileFailures.length === 0).map(gateItem),
      ];
      return {
        mark: "issues",
        label: t("status.issues", { count: Math.max(1, items.length) }),
        items,
        reminders,
      };
    }
    case "needs_reverify":
      return { mark: "unchecked", label: t("status.stale"), items: [{ id: "stale", text: t("status.staleDetail") }], reminders: [] };
    default:
      return {
        mark: "unchecked",
        label: t("status.notChecked"),
        items: [{ id: "unchecked", text: t("status.notCheckedDetail") }],
        reminders: [],
      };
  }
}

/** The status mark for a problem's `packageStatus` (library table, contest outline). */
export function markForPackageStatus(status: string | null | undefined): StatusMarkKind {
  switch (status) {
    case "verified_fresh":
      return "passed";
    case "failed":
      return "issues";
    case "stale":
    case "unverified":
      return "unchecked";
    default:
      return "none";
  }
}

export type JudgeMode = "standard" | "custom" | "interactive";

/** How answers are judged: built-in comparison, a custom checker, or an interactor. */
export function judgeModeOf(interactive: boolean, checker: string | null | undefined): JudgeMode {
  if (interactive) return "interactive";
  return checker === "checker.cpp" ? "custom" : "standard";
}

/** The 题型 summary next to the sidebar entry: 「传统」「交互　两轮　子任务 100 分」 (spaced, no separators; design-system §1.4). */
export function problemTypeSummary(input: { interactive: boolean; runs: number; subtaskScore: number | null }): string {
  const parts = [input.interactive ? t("build.type.interactive") : t("build.type.standard")];
  if (input.runs >= 2) parts.push(runsLabel(input.runs));
  if (input.subtaskScore != null) parts.push(t("build.type.subtasksScore", { score: input.subtaskScore }));
  return parts.join(locale() === "zh-CN" ? "　" : "  ");
}

/** 「一轮」「两轮」「三轮」… */
export function runsLabel(runs: number): string {
  const zh = ["", "一", "两", "三", "四", "五", "六", "七", "八", "九"];
  return t("build.type.runs", { n: locale() === "zh-CN" ? (zh[runs] ?? String(runs)) : String(runs) });
}

/** The expectation next to a solution: 「应 AC」「应 WA/TLE」「应得 30 分」「应得 60–70 分」; none for the main solution. */
export function solutionExpectLabel(spec: {
  is_main: boolean;
  expected: readonly string[];
  score?: number | [number, number] | null;
}): string | undefined {
  if (spec.is_main) return undefined;
  if (spec.score != null) {
    const s = Array.isArray(spec.score) ? `${spec.score[0]}–${spec.score[1]}` : String(spec.score);
    return t("build.expect.score", { score: s });
  }
  const v = spec.expected.length > 0 ? spec.expected.join("/") : "AC";
  return t("build.expect.verdict", { verdict: v });
}

/** A verdict in plain words: 「答案错误（WA）」「卡住了（未刷新输出或协议死锁）」. */
export function verdictWords(verdict: string): string {
  const v = verdict.toUpperCase();
  switch (v) {
    case "AC":
      return t("verdict.ac");
    case "WA":
      return t("verdict.wa");
    case "PE":
      return t("verdict.pe");
    case "TLE":
      return t("verdict.tle");
    case "MLE":
      return t("verdict.mle");
    case "RE":
      return t("verdict.re");
    case "ILE":
      return t("verdict.ile");
    case "OLE":
      return t("verdict.ole");
    default:
      return v;
  }
}

/**
 * The diagnostic pane's first line (§5.4): the verdict in plain words when it matches the
 * expectation; 「应 AC，实际 WA」 plus a plain reason and where to look when it does not.
 */
export function cellHeadline(
  verdict: string,
  expected: readonly string[],
  isAnomaly: boolean,
): { text: string; mismatch: boolean; explain?: string } {
  const v = verdict.toUpperCase();
  if (!isAnomaly) return { text: v === "ILE" ? verdictWords(v) : `${verdictWords(v)}（${v}）`, mismatch: false };
  const exp = (expected.length > 0 ? expected : ["AC"]).join("/");
  const text = t("build.mismatch.verdict", { expected: exp, actual: v });
  const explainKey =
    v === "AC"
      ? "build.diag.explainAc"
      : v === "WA" || v === "PE"
        ? "build.diag.explainWa"
        : v === "TLE"
          ? "build.diag.explainTle"
          : v === "ILE"
            ? "build.diag.explainIle"
            : v === "RE"
              ? "build.diag.explainRe"
              : v === "MLE"
                ? "build.diag.explainMle"
                : undefined;
  return { text, mismatch: true, explain: explainKey ? t(explainKey) : undefined };
}

/** Transcript entries grouped by run (「第 1 轮」), in order; one unnamed group for single-run problems. */
export function groupTranscriptByRun<T extends { run?: number | null }>(entries: readonly T[]): { run: number | null; entries: T[] }[] {
  const groups: { run: number | null; entries: T[] }[] = [];
  for (const e of entries) {
    const run = e.run ?? null;
    const last = groups[groups.length - 1];
    if (last && last.run === run) last.entries.push(e);
    else groups.push({ run, entries: [e] });
  }
  return groups;
}

/** A memory-check target by its object name: 「标程 std」「解法 brute」「生成器 gen_n」「校验器」 (§5.6). */
export function sanitizerTargetName(target: string, mainSolution?: string): string {
  if (target.startsWith("sol:")) {
    const name = target.slice(4);
    return name === mainSolution ? t("build.object.std", { name }) : t("build.object.solution", { name });
  }
  if (target.startsWith("gen:")) return t("build.object.generator", { name: target.slice(4) });
  if (target.startsWith("validator")) return t("build.tree.validator");
  if (target.startsWith("checker")) return t("build.tree.checker");
  if (target.startsWith("interactor")) return t("build.tree.interactor");
  return target;
}

/** Speed against the main solution in words (§2): 「与标程相近」 under 2×, else 「比标程慢 3.5 倍」. */
export function stdRatioWords(ratio: number | null | undefined): string {
  if (ratio == null || !Number.isFinite(ratio)) return "–";
  if (ratio < 2) return t("build.external.ratioNear");
  const n = Math.round(ratio * 10) / 10;
  return t("build.external.ratioSlower", { n: Number.isInteger(n) ? String(n) : n.toFixed(1) });
}

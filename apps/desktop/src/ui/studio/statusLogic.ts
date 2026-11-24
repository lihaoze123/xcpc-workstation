// What a problem's status slot says (authoring-ui-refactor §3, §6): one pure function from
// the authoring state to a `StatusModel`, so every case — no compiler, an error, checking,
// passed, has issues, not checked — is unit-tested without the UI.

import { t } from "../i18n";
import { errorCopy, errorDetails } from "../logic";
import type { UiError } from "../store";
import type { GateResultData, SingleStatusKind } from "./buildLogic";
import { humanizeCheckText, isMissingCompilerError } from "./buildLogic";
import type { StatusItem, StatusMarkKind, StatusModel } from "./StatusSlot";

export interface ProblemStatusInput {
  hasPackage: boolean;
  running: boolean;
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
      label: p && p.total > 0 ? t("status.checkingProgress", { done: p.done, total: p.total }) : t("status.checking"),
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

// Checking a whole contest from its studio (authoring-ui-refactor §4): the audit behind the
// title-bar status slot and the outline marks, the batch check that spins those marks in
// place, and tester code whose results go back to each problem. Tester results live only
// for this session — nothing is written to the workspace (phase-3d §3.2).

import { signal } from "nsh";
import type { BatchTaskSnapshotJs } from "xcpc-workstation/native";
import { t } from "../i18n";
import { onExternalChange, preview, type UiError } from "../store";
import { showToast } from "../Toast";
import {
  idsToCheck,
  parseContestAuditReport,
  parseExternalContestReport,
  type ContestAuditReportData,
  type ExternalSubmissionResultData,
} from "./readinessLogic";

export interface ContestCheckState {
  contestId: string;
  report?: ContestAuditReportData;
  markdown?: string;
  error?: UiError;
}

export const contestCheck = signal<ContestCheckState | undefined>(undefined);
export const contestBatch = signal<BatchTaskSnapshotJs | undefined>(undefined);
export const testerRunning = signal(false);
export const testerMarkdown = signal<{ contestId: string; markdown: string } | undefined>(undefined);
/** Tester submissions by problem ID, for the problem's 「验题人代码 (N)」 page. */
export const testerResults = signal<ReadonlyMap<string, ExternalSubmissionResultData[]>>(new Map());

let auditSeq = 0;

/** Reads the contest's check state again (its overridden limits included). */
export { idsToCheck };

export async function refreshContestCheck(contestId: string): Promise<void> {
  const seq = ++auditSeq;
  const res = await preview.auditContest(contestId);
  if (seq !== auditSeq) return;
  if (!res.ok) {
    contestCheck.set({ contestId, error: { code: res.code, error: res.error, context: t("contestCheck.context") } });
    return;
  }
  contestCheck.set({ contestId, report: parseContestAuditReport(res.reportJson), markdown: res.markdown ?? undefined });
}

/** Loads the state for `contestId` when the studio shows a different contest. */
export function ensureContestCheck(contestId: string): void {
  if (contestCheck()?.contestId === contestId) return;
  contestCheck.set({ contestId });
  testerResults.set(new Map());
  testerMarkdown.set(undefined);
  void refreshContestCheck(contestId);
}

onExternalChange(() => {
  const cur = contestCheck();
  if (cur && !contestBatch()?.running) void refreshContestCheck(cur.contestId);
});

let streamSeq = 0;

/** Checks `ids` one after another under the contest's limits; marks spin in place. */
export function startContestCheck(
  contestId: string,
  ids: string[],
  mode: "verify" | "sanitize",
  onOpenStatus?: () => void,
): UiError | undefined {
  if (ids.length === 0 || contestBatch()?.running) return undefined;
  const res = preview.startBatchBuild(ids, mode, contestId);
  if (!res.ok) return { code: res.code, error: res.error, context: t("contestCheck.context") };
  contestBatch.set(preview.batchTaskStatus());
  const mine = ++streamSeq;
  void (async () => {
    while (mine === streamSeq) {
      const ev = await preview.nextBatchEvent(res.taskId);
      if (mine !== streamSeq) return;
      contestBatch.set(ev.snapshot);
      if (ev.kind === "problem_finished") void refreshContestCheck(contestId);
      if (ev.kind === "all_finished" || ev.kind === "cancelled" || ev.kind === "error" || ev.kind === "closed") {
        contestBatch.set({ ...ev.snapshot, running: false });
        await refreshContestCheck(contestId);
        if (mine !== streamSeq || ev.kind === "cancelled" || ev.kind === "closed") return;
        const viewAction = onOpenStatus ? { label: t("toast.view"), run: onOpenStatus } : undefined;
        if (mode === "sanitize") {
          const failed = ev.snapshot.failed;
          if (failed > 0) {
            showToast({ tone: "issues", text: t("toast.contestMemoryIssues", { count: failed }), action: viewAction });
          } else {
            showToast({ tone: "success", text: t("toast.memoryClean") });
          }
        } else {
          const rep = contestCheck()?.contestId === contestId ? contestCheck()?.report : undefined;
          const issues = rep ? rep.summary.failed_count : ev.snapshot.failed;
          if (issues > 0 || ev.snapshot.failed > 0) {
            showToast({
              tone: "issues",
              text: t("toast.contestCheckIssues", { count: Math.max(issues, ev.snapshot.failed) }),
              action: viewAction,
            });
          } else if (rep?.summary.ready_for_freeze) {
            showToast({ tone: "success", text: t("toast.contestCheckReady") });
          } else {
            showToast({ tone: "success", text: t("toast.contestCheckDone", { count: ev.snapshot.succeeded }) });
          }
        }
        return;
      }
    }
  })();
  return undefined;
}

export function cancelContestCheck(): void {
  preview.cancelBatchTask();
}

/** Runs a folder of tester code (`A.cpp`…, or one sub-folder per tester) against the contest. */
export async function runTesterFolder(
  contestId: string,
  readOnly: boolean,
  onOpenStatus?: () => void,
): Promise<UiError | undefined> {
  if (testerRunning()) return undefined;
  const picked = await preview.pickExternalFolder({
    title: t("contestCheck.folderTitle"),
    buttonLabel: t("contestCheck.folderButton"),
  });
  if (picked.cancelled || !picked.path) return undefined;
  if (picked.error) return { error: picked.error, context: t("contestCheck.tryTesterCode") };
  testerRunning.set(true);
  const res = await preview.runExternalContestSolutions(contestId, picked.path, !readOnly);
  testerRunning.set(false);
  if (!res.ok) return { code: res.code, error: res.error, context: t("contestCheck.tryTesterCode") };
  const report = parseExternalContestReport(res.reportJson);
  const subs = report?.submissions ?? [];
  const byProblem = new Map<string, ExternalSubmissionResultData[]>();
  for (const sub of subs) {
    const list = byProblem.get(sub.problem_id);
    if (list) list.push(sub);
    else byProblem.set(sub.problem_id, [sub]);
  }
  testerResults.set(byProblem);
  testerMarkdown.set(res.markdown ? { contestId, markdown: res.markdown } : undefined);
  if (subs.length === 0) {
    showToast({ tone: "info", text: t("toast.testerEmpty") });
  } else {
    const failed = subs.filter((s) => s.verdict !== "AC").length;
    if (failed > 0) {
      showToast({
        tone: "issues",
        text: t("toast.testerFailed", { count: failed }),
        action: onOpenStatus ? { label: t("toast.view"), run: onOpenStatus } : undefined,
      });
    } else {
      showToast({ tone: "success", text: t("toast.testerAllPassed", { count: subs.length }) });
    }
  }
  return undefined;
}

/** 「导出检查报告」: the audit, plus the tester × problem matrix when tester code was tried. */
export async function exportCheckReport(contestId: string): Promise<UiError | undefined> {
  const audit = contestCheck()?.contestId === contestId ? contestCheck()?.markdown : undefined;
  const tester = testerMarkdown()?.contestId === contestId ? testerMarkdown()?.markdown : undefined;
  const md = [audit, tester].filter(Boolean).join("\n\n");
  if (!md) return undefined;
  const res = await preview.saveMarkdownReport(`${contestId}-check-report.md`, md, {
    title: t("contestCheck.exportTitle"),
    buttonLabel: t("contestCheck.exportButton"),
    filterLabel: t("contestCheck.exportFilter"),
  });
  if (res.cancelled) return undefined;
  if (res.ok) {
    showToast({ tone: "success", text: t("toast.reportExported") });
    return undefined;
  }
  return { error: res.error, context: t("contestCheck.exportReport") };
}

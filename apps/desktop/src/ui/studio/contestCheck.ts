// Checking a whole contest from its studio (authoring-ui-refactor §4): the audit behind the
// title-bar status slot and the outline marks, the batch check that spins those marks in
// place, and tester code whose results go back to each problem. Tester results live only
// for this session — nothing is written to the workspace (phase-3d §3.2).

import { signal } from "nsh";
import type { BatchTaskSnapshotJs } from "xcpc-workstation/native";
import { t } from "../i18n";
import { onExternalChange, preview, type UiError } from "../store";
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
export function startContestCheck(contestId: string, ids: string[], mode: "verify" | "sanitize"): UiError | undefined {
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
        void refreshContestCheck(contestId);
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
export async function runTesterFolder(contestId: string, readOnly: boolean): Promise<UiError | undefined> {
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
  const byProblem = new Map<string, ExternalSubmissionResultData[]>();
  for (const sub of report?.submissions ?? []) {
    const list = byProblem.get(sub.problem_id);
    if (list) list.push(sub);
    else byProblem.set(sub.problem_id, [sub]);
  }
  testerResults.set(byProblem);
  testerMarkdown.set(res.markdown ? { contestId, markdown: res.markdown } : undefined);
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
  if (res.cancelled || res.ok) return undefined;
  return { error: res.error, context: t("contestCheck.exportReport") };
}

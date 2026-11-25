// Shared snippet verification runner & poller (used by SnippetsView, SnippetStudio, and CommandPalette).

import { t } from "../i18n";
import { bumpSnippetsRevision, goSettings, preview } from "../store";
import { showToast } from "../Toast";
import { parseJsonPayload } from "./snippetLogic";
import type { SnippetVerifyJobState } from "./types";

let activePollTimer: ReturnType<typeof setInterval> | undefined;

export function cancelSnippetVerifyPoll() {
  if (activePollTimer !== undefined) {
    clearInterval(activePollTimer);
    activePollTimer = undefined;
  }
}

export function runSnippetVerifyJob(opts: {
  mode: "single" | "cascade" | "all";
  slug: string | null;
  force: boolean;
  onStart?: () => void;
  onFinish?: (state: SnippetVerifyJobState | undefined) => void;
}): boolean {
  const res = preview.startSnippetVerify(opts.mode, opts.slug, opts.force);
  if (!res.ok) {
    showToast({ tone: "issues", text: res.error ?? t("snippets.error.actionFailed") });
    return false;
  }
  opts.onStart?.();
  cancelSnippetVerifyPoll();
  activePollTimer = setInterval(() => {
    const st = parseJsonPayload<SnippetVerifyJobState>(preview.snippetVerifyStatusJson());
    if (!st || st.running) return;
    cancelSnippetVerifyPoll();
    bumpSnippetsRevision();
    opts.onFinish?.(st);

    if (st.error) {
      showToast({
        tone: "issues",
        text: st.error,
        action:
          st.errorCode === "no_compiler"
            ? { label: t("snippets.verify.goSettings"), run: () => goSettings("toolchain") }
            : undefined,
      });
      return;
    }

    const reports = st.reports ?? [];
    const failedReports = reports.filter((r) => r.status === "failed");
    const unavailableCases = reports.flatMap((r) => (r.cases ?? []).filter((c) => c.verdict === "UNAVAILABLE"));

    if (failedReports.length > 0) {
      showToast({
        tone: "issues",
        text: t("snippets.verify.doneIssues", { count: failedReports.length }),
      });
    } else if (unavailableCases.length > 0) {
      showToast({
        tone: "issues",
        text: t("snippets.verify.unavailable"),
        action: { label: t("snippets.verify.goSettings"), run: () => goSettings("toolchain", { expandAdvanced: true }) },
      });
    } else if (reports.length > 0) {
      showToast({
        tone: "success",
        text: t("snippets.studio.verifyDonePass", { count: reports.length }),
      });
    }
  }, 180);
  return true;
}

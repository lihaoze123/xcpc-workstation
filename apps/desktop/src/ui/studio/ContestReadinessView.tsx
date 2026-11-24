// Phase 3D: Contest-level Readiness Audit ("验题体检") view inside Contest Studio.
// Aggregates per-problem verification status, std time budget (% TL), AC vs TLE safety margin,
// data/runtime budget warnings, coverage (AC / wrong / self-tests / lint), and Sanitizer UB status.
// Provides the 4 high-frequency actions from spec §4.2:
//   1. Batch verify all stale/unbuilt/failed problems (polling-free streaming via nextBatchEvent)
//   2. Batch UB check across all package problems in the contest
//   3. Trial-run external tester directory (single-dir or multi-tester matrix + adopt + export MD)
//   4. Export full Contest Readiness Audit report (Markdown)

import { cleanup, colorScheme, computed, effect, Scroll, signal, Text, untracked, View } from "nsh";
import type { BatchTaskSnapshotJs } from "xcpc-workstation/native";
import { ErrorDisplay } from "../ErrorDisplay";
import { Icon } from "../Icon";
import { t } from "../i18n";
import { Btn, createTransientFlag, tip } from "../kit";
import { onExternalChange, openStudio, preview, type UiError } from "../store";
import { MONO } from "../theme";
import { formatBytesCompact } from "./buildLogic";
import {
  auditRowRiskSeverity,
  buildMultiTesterMatrix,
  formatCumulativeMs,
  formatExternalRatio,
  formatSafetyMarginRatio,
  formatStdTimeBudget,
  inferAdoptExpect,
  parseContestAuditReport,
  parseExternalContestReport,
  selectPackageProblemIds,
  selectStaleOrUnverifiedProblemIds,
  type ContestAuditReportData,
  type ExternalContestReportData,
  type ExternalSubmissionResultData,
  type ProblemAuditRowData,
  type SanitizerAuditStatus,
  type VerificationStatus,
} from "./readinessLogic";

function verificationBadgeStyle(status: VerificationStatus, dark: boolean): { label: string; bg: string; fg: string } {
  switch (status) {
    case "fresh":
      return { label: t("readiness.status.fresh"), bg: "rgba(16,185,129,0.12)", fg: dark ? "#4ade80" : "#15803d" };
    case "stale":
      return { label: t("readiness.status.stale"), bg: "rgba(245,158,11,0.12)", fg: dark ? "#fbbf24" : "#b45309" };
    case "failed":
      return { label: t("readiness.status.failed"), bg: "rgba(244,63,94,0.12)", fg: dark ? "#f87171" : "#b91c1c" };
    case "unbuilt":
      return { label: t("readiness.status.unbuilt"), bg: "rgba(113,113,122,0.12)", fg: dark ? "#a1a1aa" : "#52525b" };
    case "statement_only":
      return { label: t("readiness.status.statementOnly"), bg: "rgba(113,113,122,0.10)", fg: dark ? "#71717a" : "#71717a" };
  }
}

function sanitizerBadgeStyle(
  status: SanitizerAuditStatus,
  findingsCount: number,
  dark: boolean,
): { label: string; bg: string; fg: string } {
  switch (status) {
    case "passed_fresh":
      return { label: t("readiness.sanitizer.passedFresh"), bg: "rgba(16,185,129,0.12)", fg: dark ? "#4ade80" : "#15803d" };
    case "passed_stale":
      return { label: t("readiness.sanitizer.passedStale"), bg: "rgba(245,158,11,0.12)", fg: dark ? "#fbbf24" : "#b45309" };
    case "failed":
      return {
        label: t("readiness.sanitizer.failed", { count: Math.max(1, findingsCount) }),
        bg: "rgba(244,63,94,0.12)",
        fg: dark ? "#f87171" : "#b91c1c",
      };
    case "unsupported":
      return { label: t("readiness.sanitizer.unsupported"), bg: "rgba(245,158,11,0.10)", fg: dark ? "#fbbf24" : "#b45309" };
    case "not_run":
      return { label: t("readiness.sanitizer.notRun"), bg: "rgba(113,113,122,0.10)", fg: dark ? "#71717a" : "#71717a" };
  }
}

function verdictPillStyle(verdict: string, dark: boolean): { bg: string; fg: string } {
  if (verdict === "AC") return { bg: "rgba(16,185,129,0.12)", fg: dark ? "#4ade80" : "#15803d" };
  if (verdict === "TLE" || verdict === "MLE") return { bg: "rgba(245,158,11,0.12)", fg: dark ? "#fbbf24" : "#b45309" };
  return { bg: "rgba(244,63,94,0.12)", fg: dark ? "#f87171" : "#b91c1c" };
}

export function ContestReadinessView(props: { contestId: string; readOnly: boolean; onRefreshWorkspace?: () => void }) {
  const report = signal<ContestAuditReportData | undefined>(undefined);
  const reportMarkdown = signal<string | undefined>(undefined);
  const loading = signal(false);
  const notice = signal<UiError | undefined>(undefined);
  const exportedAudit = createTransientFlag(1600);
  const exportedExternal = createTransientFlag(1600);

  // Batch verify / sanitize streaming state
  const batchSnap = signal<BatchTaskSnapshotJs | undefined>(undefined);
  const batchRunning = computed(() => Boolean(batchSnap()?.running));
  let activeStreamSeq = 0;

  // External tester folder trial-run state
  const externalRunning = signal(false);
  const externalReport = signal<ExternalContestReportData | undefined>(undefined);
  const externalMarkdown = signal<string | undefined>(undefined);
  const adoptedKeys = signal<Record<string, boolean>>({});

  let auditReqSeq = 0;
  let disposed = false;
  cleanup(() => {
    disposed = true;
    activeStreamSeq += 1;
  });

  async function refreshAudit(cid = props.contestId) {
    const seq = ++auditReqSeq;
    loading.set(true);
    const res = await preview.auditContest(cid);
    if (disposed || seq !== auditReqSeq) return;
    loading.set(false);
    if (!res.ok) {
      notice.set({ code: res.code, error: res.error, context: t("readiness.title") });
      return;
    }
    report.set(parseContestAuditReport(res.reportJson));
    reportMarkdown.set(res.markdown ?? undefined);
  }

  // Load audit whenever contestId changes
  let lastContestId = "";
  effect(() => {
    const cid = props.contestId;
    untracked(() => {
      if (cid !== lastContestId) {
        lastContestId = cid;
        notice.set(undefined);
        externalReport.set(undefined);
        externalMarkdown.set(undefined);
        adoptedKeys.set({});
        void refreshAudit(cid);
      }
    });
  });

  // Auto-refresh audit on external filesystem changes
  const unsubWatch = onExternalChange((_paths, _rescan) => {
    if (!untracked(batchRunning)) {
      void refreshAudit(untracked(() => props.contestId));
    }
  });
  cleanup(unsubWatch);

  const staleProblemIds = computed(() => selectStaleOrUnverifiedProblemIds(report()));
  const packageProblemIds = computed(() => selectPackageProblemIds(report()));

  async function streamBatch(taskId: number) {
    const myStream = ++activeStreamSeq;
    while (!disposed && myStream === activeStreamSeq) {
      const ev = await preview.nextBatchEvent(taskId);
      if (disposed || myStream !== activeStreamSeq) return;
      batchSnap.set(ev.snapshot);
      if (ev.kind === "problem_finished") {
        void refreshAudit(props.contestId);
      }
      if (ev.kind === "all_finished" || ev.kind === "error" || ev.kind === "closed") {
        if (ev.error) {
          notice.set({ code: ev.code, error: ev.error, context: t("readiness.title") });
        }
        props.onRefreshWorkspace?.();
        void refreshAudit(props.contestId);
        return;
      }
    }
  }

  function startBatch(ids: string[], mode: "verify" | "sanitize") {
    if (props.readOnly || batchRunning() || ids.length === 0) return;
    notice.set(undefined);
    const res = preview.startBatchBuild(ids, mode, props.contestId);
    if (!res.ok) {
      notice.set({ code: res.code, error: res.error, context: t("readiness.title") });
      return;
    }
    batchSnap.set(preview.batchTaskStatus());
    void streamBatch(res.taskId);
  }

  function cancelBatch() {
    preview.cancelBatchTask();
  }

  async function pickAndRunExternalFolder() {
    if (externalRunning()) return;
    notice.set(undefined);
    const picked = await preview.pickExternalFolder({
      title: t("readiness.folderPickerTitle"),
      buttonLabel: t("readiness.folderPickerButton"),
    });
    if (picked.cancelled || !picked.path) return;
    if (picked.error) {
      notice.set({ error: picked.error, context: t("readiness.action.externalDir") });
      return;
    }
    externalRunning.set(true);
    const cid = props.contestId;
    const res = await preview.runExternalContestSolutions(cid, picked.path, !props.readOnly);
    if (disposed || cid !== props.contestId) return;
    externalRunning.set(false);
    if (!res.ok) {
      notice.set({ code: res.code, error: res.error, context: t("readiness.action.externalDir") });
      return;
    }
    externalReport.set(parseExternalContestReport(res.reportJson));
    externalMarkdown.set(res.markdown ?? undefined);
  }

  async function exportAuditMarkdown() {
    const md = reportMarkdown();
    if (!md) return;
    const res = await preview.saveMarkdownReport(`${props.contestId}-readiness-report.md`, md, {
      title: t("readiness.exportDialogTitle"),
      buttonLabel: t("readiness.exportButton"),
      filterLabel: t("readiness.exportFilter"),
    });
    if (res.cancelled) return;
    if (!res.ok) {
      notice.set({ error: res.error, context: t("readiness.action.exportMd") });
      return;
    }
    exportedAudit.trigger();
  }

  async function exportExternalMarkdown() {
    const md = externalMarkdown();
    if (!md) return;
    const res = await preview.saveMarkdownReport(`${props.contestId}-external-report.md`, md, {
      title: t("readiness.exportExternalDialogTitle"),
      buttonLabel: t("readiness.exportButton"),
      filterLabel: t("readiness.exportFilter"),
    });
    if (res.cancelled) return;
    if (!res.ok) {
      notice.set({ error: res.error, context: t("build.external.exportMd") });
      return;
    }
    exportedExternal.trigger();
  }

  function adoptExternal(sub: ExternalSubmissionResultData) {
    if (props.readOnly) return;
    const res = preview.adoptExternalSolution(
      sub.problem_id,
      sub.file_path,
      null,
      inferAdoptExpect(sub.verdict),
      null,
      false,
    );
    if (!res.ok) {
      notice.set({ code: res.code, error: res.error, context: t("build.external.adopt") });
      return;
    }
    const key = `${sub.problem_id}::${sub.file_path}`;
    adoptedKeys.set({ ...adoptedKeys(), [key]: true });
    props.onRefreshWorkspace?.();
    void refreshAudit(props.contestId);
  }

  const summary = computed(() => report()?.summary);

  return (
    <View testID="contest-readiness-view" style={{ flex: 1, minWidth: 0, minHeight: 0 }} class="bg-background">
      {/* Top summary & 4 high-frequency action bar */}
      <View
        style={{
          minHeight: 50,
          paddingLeft: 16,
          paddingRight: 16,
          paddingTop: 8,
          paddingBottom: 8,
          borderBottomWidth: 1,
          flexDirection: "row",
          alignItems: "center",
          justifyContent: "space-between",
          gap: 12,
          flexWrap: "wrap",
          flexShrink: 0,
        }}
        class="border-border bg-card"
      >
        <View style={{ flexDirection: "row", alignItems: "center", gap: 10, flexWrap: "wrap" }}>
          <If when={summary()}>
            {(s) => {
              const dark = () => colorScheme() === "dark";
              const ready = () => s().ready_for_freeze;
              return (
                <View style={{ flexDirection: "row", alignItems: "center", gap: 10 }}>
                  <View
                    testID="readiness-summary-badge"
                    style={{
                      height: 24,
                      paddingLeft: 9,
                      paddingRight: 9,
                      borderRadius: 6,
                      justifyContent: "center",
                      backgroundColor: ready() ? "rgba(16,185,129,0.12)" : "rgba(245,158,11,0.12)",
                    }}
                  >
                    <Text
                      style={{
                        fontSize: 12,
                        fontWeight: 500,
                        color: ready() ? (dark() ? "#4ade80" : "#15803d") : dark() ? "#fbbf24" : "#b45309",
                      }}
                    >
                      {ready() ? t("readiness.summary.ready") : t("readiness.summary.needsAttention")}
                    </Text>
                  </View>
                  <Text style={{ fontSize: 12.5, fontVariant: ["tabular-nums"] }} class="text-secondary-foreground">
                    {t("readiness.summary.verified", { fresh: s().fresh_count, total: s().total_problems })}
                  </Text>
                  <If when={s().error_risk_count > 0 || s().warning_risk_count > 0}>
                    <Text style={{ fontSize: 12.5, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
                      {t("readiness.summary.risks", { errors: s().error_risk_count, warnings: s().warning_risk_count })}
                    </Text>
                  </If>
                </View>
              );
            }}
          </If>
        </View>

        <View style={{ flexDirection: "row", alignItems: "center", gap: 6, flexWrap: "wrap" }}>
          <If
            when={!batchRunning()}
            else={
              <Btn
                testID="readiness-cancel-batch-btn"
                label={t("common.cancel")}
                icon="x"
                variant="outline"
                onPress={cancelBatch}
              />
            }
          >
            <Btn
              testID="readiness-verify-stale-btn"
              label={
                staleProblemIds().length > 0
                  ? t("readiness.action.verifyStaleCount", { count: staleProblemIds().length })
                  : t("readiness.action.verifyStale")
              }
              icon="play"
              variant="primary"
              disabled={props.readOnly || staleProblemIds().length === 0}
              onPress={() => startBatch(staleProblemIds(), "verify")}
            />
          </If>

          <Btn
            testID="readiness-sanitize-all-btn"
            label={t("readiness.action.sanitizeAll")}
            icon="shieldCheck"
            variant="outline"
            disabled={props.readOnly || batchRunning() || packageProblemIds().length === 0}
            onPress={() => startBatch(packageProblemIds(), "sanitize")}
          />

          <Btn
            testID="readiness-external-dir-btn"
            label={externalRunning() ? t("readiness.external.running") : t("readiness.action.externalDir")}
            icon="folderOpen"
            variant="outline"
            disabled={externalRunning()}
            onPress={() => void pickAndRunExternalFolder()}
          />

          <Btn
            testID="readiness-export-md-btn"
            label={exportedAudit.active() ? t("readiness.action.exported") : t("readiness.action.exportMd")}
            icon={exportedAudit.active() ? "check" : "fileDown"}
            variant="outline"
            disabled={!reportMarkdown()}
            onPress={() => void exportAuditMarkdown()}
          />
        </View>
      </View>

      {/* Live batch progress strip (zero polling; driven by nextBatchEvent) */}
      <If when={batchRunning() ? batchSnap() : undefined}>
        {(snap) => {
          const pct = () => (snap().total > 0 ? Math.min(100, Math.round((snap().completed / snap().total) * 100)) : 0);
          const isSan = () => snap().kind === "build_sanitize";
          return (
            <View
              testID="readiness-batch-progress"
              style={{
                paddingLeft: 16,
                paddingRight: 16,
                paddingTop: 8,
                paddingBottom: 8,
                borderBottomWidth: 1,
                gap: 6,
                flexShrink: 0,
              }}
              class="border-border bg-muted/40"
            >
              <View style={{ flexDirection: "row", alignItems: "center", justifyContent: "space-between", gap: 10 }}>
                <Text style={{ fontSize: 12.5, fontWeight: 500, fontVariant: ["tabular-nums"] }} class="text-foreground">
                  {isSan()
                    ? t("readiness.batch.sanitizing", { completed: snap().completed, total: snap().total })
                    : t("readiness.batch.verifying", { completed: snap().completed, total: snap().total })}
                </Text>
                <If when={!!snap().currentProblemId}>
                  <Text style={{ fontSize: 12, fontFamily: MONO, numberOfLines: 1, textOverflow: "ellipsis" }} class="text-muted-foreground">
                    {snap().currentProblemId!}
                    {snap().currentDetail ? `  ${snap().currentDetail}` : ""}
                  </Text>
                </If>
              </View>
              <View style={{ height: 3, borderRadius: 2, overflow: "hidden" }} class="bg-muted">
                <View style={{ height: 3, width: `${pct()}%` }} class="bg-primary" />
              </View>
            </View>
          );
        }}
      </If>

      {/* Main scrollable content */}
      <Scroll style={{ flex: 1 }} contentStyle={{ padding: 16, gap: 16 }}>
        <If when={notice()}>
          <ErrorDisplay error={notice()} onDismiss={() => notice.set(undefined)} />
        </If>

        {/* Per-Problem Readiness Table */}
        <If
          when={report() && report()!.problems.length > 0}
          else={
            <If when={!loading()}>
              <View style={{ paddingTop: 40, alignItems: "center", gap: 8 }}>
                <Icon name="shieldCheck" size={24} tone="muted-foreground" />
                <Text style={{ fontSize: 13 }} class="text-muted-foreground">
                  {t("readiness.emptyProblems")}
                </Text>
              </View>
            </If>
          }
        >
          <View testID="readiness-table" style={{ borderRadius: 10, borderWidth: 1, overflow: "hidden" }} class="border-border bg-card">
            {/* Table header */}
            <View
              style={{
                height: 36,
                paddingLeft: 12,
                paddingRight: 12,
                borderBottomWidth: 1,
                flexDirection: "row",
                alignItems: "center",
                gap: 10,
              }}
              class="border-border bg-muted/40"
            >
              <Text style={{ flex: 2.2, fontSize: 11.5, fontWeight: 500 }} class="text-muted-foreground">
                {t("readiness.col.problem")}
              </Text>
              <Text style={{ width: 78, fontSize: 11.5, fontWeight: 500 }} class="text-muted-foreground">
                {t("readiness.col.status")}
              </Text>
              <Text style={{ width: 112, fontSize: 11.5, fontWeight: 500 }} class="text-muted-foreground">
                {t("readiness.col.stdBudget")}
              </Text>
              <Text style={{ width: 84, fontSize: 11.5, fontWeight: 500 }} class="text-muted-foreground">
                {t("readiness.col.margin")}
              </Text>
              <Text style={{ width: 128, fontSize: 11.5, fontWeight: 500 }} class="text-muted-foreground">
                {t("readiness.col.dataBudget")}
              </Text>
              <Text style={{ flex: 1.4, fontSize: 11.5, fontWeight: 500 }} class="text-muted-foreground">
                {t("readiness.col.coverage")}
              </Text>
              <Text style={{ width: 96, fontSize: 11.5, fontWeight: 500 }} class="text-muted-foreground">
                {t("readiness.col.sanitizer")}
              </Text>
            </View>

            <Each of={report()?.problems ?? []} key={(row) => `${row.label}:${row.problem_id}`}>
              {(row, idx) => <AuditRow row={row()} index={idx()} />}
            </Each>
          </View>
        </If>

        {/* Risks & Warnings Summary */}
        <If when={(report()?.risk_flags?.length ?? 0) > 0}>
          <View testID="readiness-risks-section" style={{ borderRadius: 10, borderWidth: 1, padding: 12, gap: 8 }} class="border-border bg-card">
            <Text style={{ fontSize: 13, fontWeight: 500 }} class="text-foreground">
              {t("readiness.risks.title")}
            </Text>
            <View style={{ gap: 6 }}>
              <Each of={report()?.risk_flags ?? []} key={(rf, i) => `${rf.code}:${rf.problem_id ?? ""}:${i}`}>
                {(rf) => (
                  <View style={{ flexDirection: "row", alignItems: "center", gap: 8 }}>
                    <Icon
                      name={rf().level === "error" ? "circleAlert" : "triangleAlert"}
                      size={13.5}
                      tone={rf().level === "error" ? "destructive" : "chart-4"}
                    />
                    <If when={!!rf().problem_label}>
                      <View style={{ height: 18, paddingLeft: 5, paddingRight: 5, borderRadius: 4, justifyContent: "center" }} class="bg-muted">
                        <Text style={{ fontSize: 11, fontWeight: 600, fontVariant: ["tabular-nums"] }} class="text-secondary-foreground">
                          {rf().problem_label!}
                        </Text>
                      </View>
                    </If>
                    <Text style={{ fontSize: 12.5, flex: 1 }} class="text-secondary-foreground">
                      {rf().message}
                    </Text>
                  </View>
                )}
              </Each>
            </View>
          </View>
        </If>

        {/* External Tester Directory Trial-Run Results */}
        <If when={externalReport()}>
          {(ext) => (
            <ExternalContestResultsPanel
              report={ext()}
              readOnly={props.readOnly}
              exported={exportedExternal.active()}
              adoptedKeys={adoptedKeys()}
              onAdopt={adoptExternal}
              onExport={() => void exportExternalMarkdown()}
              onClose={() => {
                externalReport.set(undefined);
                externalMarkdown.set(undefined);
              }}
            />
          )}
        </If>
      </Scroll>
    </View>
  );
}

function AuditRow(props: { row: ProblemAuditRowData; index: number }) {
  const dark = () => colorScheme() === "dark";
  const r = () => props.row;
  const risk = () => auditRowRiskSeverity(r());
  const verBadge = () => verificationBadgeStyle(r().verification_status, dark());
  const sanBadge = () => sanitizerBadgeStyle(r().sanitizer_status, r().sanitizer_findings_count, dark());
  const stdHigh = () => (r().std_time_ratio ?? 0) >= 0.5;
  const marginTight = () => r().ac_tle_margin_ratio != null && r().ac_tle_margin_ratio! < 2.0;
  const dataHeavy = () => r().max_test_file_bytes > 20 * 1024 * 1024 || (r().cumulative_std_time_ms ?? 0) > 30_000;

  const leftBarColor = () => {
    if (risk() === "error") return dark() ? "#f87171" : "#dc2626";
    if (risk() === "warning") return dark() ? "#fbbf24" : "#d97706";
    return "transparent";
  };

  return (
    <View
      testID={`readiness-row-${r().label}`}
      onPress={() => openStudio({ target: "problem", id: r().problem_id })}
      tooltip={tip(t("readiness.openProblem"))}
      class="hover:bg-muted/50 border-border"
      style={{
        minHeight: 42,
        paddingLeft: 10,
        paddingRight: 12,
        paddingTop: 6,
        paddingBottom: 6,
        borderTopWidth: props.index > 0 ? 1 : 0,
        borderLeftWidth: 2,
        borderLeftColor: leftBarColor(),
        flexDirection: "row",
        alignItems: "center",
        gap: 10,
      }}
    >
      {/* Letter + Title */}
      <View style={{ flex: 2.2, minWidth: 0, flexDirection: "row", alignItems: "center", gap: 8 }}>
        <View style={{ width: 22, height: 22, borderRadius: 5, alignItems: "center", justifyContent: "center", flexShrink: 0 }} class="bg-muted">
          <Text style={{ fontSize: 11.5, fontWeight: 600, fontVariant: ["tabular-nums"] }} class="text-foreground">
            {r().label}
          </Text>
        </View>
        <Text style={{ fontSize: 13, numberOfLines: 1, textOverflow: "ellipsis", flex: 1 }} class="text-foreground">
          {r().title}
        </Text>
      </View>

      {/* Verification Status */}
      <View style={{ width: 78, flexDirection: "row" }}>
        <View style={{ height: 22, paddingLeft: 7, paddingRight: 7, borderRadius: 5, justifyContent: "center", backgroundColor: verBadge().bg }}>
          <Text style={{ fontSize: 11.5, fontWeight: 500, color: verBadge().fg }}>{verBadge().label}</Text>
        </View>
      </View>

      {/* Std Slowest (% TL) */}
      <Text
        style={{
          width: 112,
          fontSize: 12,
          fontVariant: ["tabular-nums"],
          color: stdHigh() ? (dark() ? "#fbbf24" : "#b45309") : undefined,
        }}
        class={stdHigh() ? undefined : "text-secondary-foreground"}
      >
        {formatStdTimeBudget(r())}
      </Text>

      {/* AC / TLE Safety Margin */}
      <Text
        style={{
          width: 84,
          fontSize: 12,
          fontVariant: ["tabular-nums"],
          color: marginTight() ? (dark() ? "#fbbf24" : "#b45309") : undefined,
        }}
        class={marginTight() ? undefined : "text-secondary-foreground"}
      >
        {formatSafetyMarginRatio(r())}
      </Text>

      {/* Max single test file + cumulative std runtime */}
      <Text
        style={{
          width: 128,
          fontSize: 12,
          fontVariant: ["tabular-nums"],
          color: dataHeavy() ? (dark() ? "#fbbf24" : "#b45309") : undefined,
        }}
        class={dataHeavy() ? undefined : "text-secondary-foreground"}
      >
        {r().verification_status === "statement_only"
          ? "—"
          : `${formatBytesCompact(r().max_test_file_bytes)}  ${formatCumulativeMs(r().cumulative_std_time_ms)}`}
      </Text>

      {/* Coverage (AC / wrong / self-tests / lint) */}
      <View style={{ flex: 1.4, minWidth: 0, flexDirection: "row", alignItems: "center", gap: 6, flexWrap: "wrap" }}>
        <If when={r().verification_status !== "statement_only"}>
          <Text style={{ fontSize: 12, fontVariant: ["tabular-nums"] }} class="text-secondary-foreground">
            {t("readiness.coverage.summary", { ac: r().independent_ac_count, wrong: r().wrong_solution_count })}
          </Text>
        </If>
        <If when={r().validator_self_test === "failed" || r().checker_self_test === "failed"}>
          <Text style={{ fontSize: 11.5 }} class="text-destructive">
            {t("readiness.coverage.selfTestFailed")}
          </Text>
        </If>
        <If when={r().validator_self_test === "missing" || r().checker_self_test === "missing"}>
          <Text style={{ fontSize: 11.5 }} class="text-muted-foreground">
            {t("readiness.coverage.selfTestMissing")}
          </Text>
        </If>
        <If when={r().lint_error_count > 0}>
          <Text style={{ fontSize: 11.5, fontVariant: ["tabular-nums"] }} class="text-destructive">
            {t("readiness.coverage.lintErr", { count: r().lint_error_count })}
          </Text>
        </If>
        <If when={r().lint_warning_count > 0}>
          <Text style={{ fontSize: 11.5, fontVariant: ["tabular-nums"], color: dark() ? "#fbbf24" : "#b45309" }}>
            {t("readiness.coverage.lintWarn", { count: r().lint_warning_count })}
          </Text>
        </If>
      </View>

      {/* Sanitizer UB Status */}
      <View style={{ width: 96, flexDirection: "row" }}>
        <If
          when={r().verification_status !== "statement_only"}
          else={
            <Text style={{ fontSize: 12 }} class="text-muted-foreground">
              —
            </Text>
          }
        >
          <View style={{ height: 22, paddingLeft: 7, paddingRight: 7, borderRadius: 5, justifyContent: "center", backgroundColor: sanBadge().bg }}>
            <Text style={{ fontSize: 11.5, fontWeight: 500, color: sanBadge().fg, fontVariant: ["tabular-nums"] }}>
              {sanBadge().label}
            </Text>
          </View>
        </If>
      </View>
    </View>
  );
}

function ExternalContestResultsPanel(props: {
  report: ExternalContestReportData;
  readOnly: boolean;
  exported: boolean;
  adoptedKeys: Record<string, boolean>;
  onAdopt: (sub: ExternalSubmissionResultData) => void;
  onExport: () => void;
  onClose: () => void;
}) {
  const dark = () => colorScheme() === "dark";
  const matrix = computed(() => buildMultiTesterMatrix(props.report));

  return (
    <View testID="readiness-external-results" style={{ borderRadius: 10, borderWidth: 1, overflow: "hidden" }} class="border-border bg-card">
      <View
        style={{
          height: 44,
          paddingLeft: 14,
          paddingRight: 12,
          borderBottomWidth: 1,
          flexDirection: "row",
          alignItems: "center",
          justifyContent: "space-between",
          gap: 10,
        }}
        class="border-border"
      >
        <Text style={{ fontSize: 13.5, fontWeight: 500 }} class="text-foreground">
          {props.report.mode === "multi_tester" ? t("readiness.external.multiTesterTitle") : t("readiness.external.title")}
        </Text>
        <View style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>
          <Btn
            testID="readiness-external-export-md"
            label={props.exported ? t("build.external.exported") : t("build.external.exportMd")}
            icon={props.exported ? "check" : "fileDown"}
            variant="outline"
            onPress={props.onExport}
          />
          <Btn label={t("readiness.external.close")} variant="ghost" onPress={props.onClose} />
        </View>
      </View>

      {/* Multi-tester horizontal matrix (when mode === "multi_tester") */}
      <If when={props.report.mode === "multi_tester" && matrix().testers.length > 0}>
        <Scroll horizontal style={{ borderBottomWidth: 1 }} class="border-border">
          <View style={{ padding: 12, gap: 4 }}>
            <View style={{ flexDirection: "row", alignItems: "center", gap: 8, height: 28 }}>
              <Text style={{ width: 110, fontSize: 11.5, fontWeight: 500 }} class="text-muted-foreground">
                {t("readiness.external.colTester")}
              </Text>
              <Each of={matrix().problems} key={(p) => p.problem_id}>
                {(p) => (
                  <Text style={{ width: 96, fontSize: 11.5, fontWeight: 600, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
                    {p().label}
                  </Text>
                )}
              </Each>
            </View>
            <Each of={matrix().testers} key={(tester) => tester}>
              {(tester) => (
                <View style={{ flexDirection: "row", alignItems: "center", gap: 8, height: 32 }}>
                  <Text style={{ width: 110, fontSize: 12.5, fontWeight: 500, numberOfLines: 1, textOverflow: "ellipsis" }} class="text-foreground">
                    {tester()}
                  </Text>
                  <Each of={matrix().problems} key={(p) => p.problem_id}>
                    {(p) => {
                      const subs = () => matrix().cells.get(`${tester()}::${p().problem_id}`) ?? [];
                      const first = () => subs()[0];
                      return (
                        <View style={{ width: 96, flexDirection: "row", alignItems: "center", gap: 4 }}>
                          <If
                            when={first()}
                            else={
                              <Text style={{ fontSize: 12 }} class="text-muted-foreground">
                                —
                              </Text>
                            }
                          >
                            {(s) => {
                              const st = () => verdictPillStyle(s().verdict, dark());
                              return (
                                <View style={{ flexDirection: "row", alignItems: "center", gap: 4 }}>
                                  <View style={{ height: 20, paddingLeft: 5, paddingRight: 5, borderRadius: 4, justifyContent: "center", backgroundColor: st().bg }}>
                                    <Text style={{ fontSize: 11, fontWeight: 600, color: st().fg }}>{s().verdict}</Text>
                                  </View>
                                  <Text style={{ fontSize: 11, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
                                    {s().max_time_ms}ms
                                  </Text>
                                </View>
                              );
                            }}
                          </If>
                        </View>
                      );
                    }}
                  </Each>
                </View>
              )}
            </Each>
          </View>
        </Scroll>
      </If>

      {/* Detailed submission rows with explicit Adopt action */}
      <View>
        <Each of={props.report.submissions} key={(s) => `${s.tester ?? ""}:${s.problem_id}:${s.file_path}`}>
          {(sub, idx) => {
            const st = () => verdictPillStyle(sub().verdict, dark());
            const adopted = () => !!props.adoptedKeys[`${sub().problem_id}::${sub().file_path}`];
            return (
              <View
                style={{
                  minHeight: 38,
                  paddingLeft: 14,
                  paddingRight: 12,
                  paddingTop: 5,
                  paddingBottom: 5,
                  borderTopWidth: idx() > 0 ? 1 : 0,
                  flexDirection: "row",
                  alignItems: "center",
                  gap: 10,
                }}
                class="border-border"
              >
                <If when={!!sub().tester}>
                  <Text style={{ width: 88, fontSize: 12, fontWeight: 500, numberOfLines: 1, textOverflow: "ellipsis" }} class="text-secondary-foreground">
                    {sub().tester!}
                  </Text>
                </If>
                <View style={{ width: 24, height: 20, borderRadius: 4, alignItems: "center", justifyContent: "center" }} class="bg-muted">
                  <Text style={{ fontSize: 11, fontWeight: 600, fontVariant: ["tabular-nums"] }} class="text-foreground">
                    {sub().problem_label ?? "?"}
                  </Text>
                </View>
                <Text style={{ flex: 2, fontSize: 12, fontFamily: MONO, numberOfLines: 1, textOverflow: "ellipsis" }} class="text-foreground">
                  {sub().file_name}
                </Text>
                <View style={{ width: 52, flexDirection: "row" }}>
                  <View style={{ height: 20, paddingLeft: 6, paddingRight: 6, borderRadius: 4, justifyContent: "center", backgroundColor: st().bg }}>
                    <Text style={{ fontSize: 11, fontWeight: 600, color: st().fg }}>{sub().verdict}</Text>
                  </View>
                </View>
                <Text style={{ width: 72, fontSize: 12, fontVariant: ["tabular-nums"] }} class="text-secondary-foreground">
                  {`${sub().passed_tests}/${sub().total_tests}`}
                </Text>
                <Text style={{ width: 64, fontSize: 12, fontVariant: ["tabular-nums"] }} class="text-secondary-foreground">
                  {`${sub().max_time_ms}ms`}
                </Text>
                <Text style={{ width: 78, fontSize: 12, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
                  {formatExternalRatio(sub().ratio_vs_std)}
                </Text>
                <View style={{ width: 124, flexDirection: "row", justifyContent: "flex-end" }}>
                  <Btn
                    testID={`readiness-external-adopt-${idx()}`}
                    label={adopted() ? t("build.external.adopted") : t("build.external.adopt")}
                    icon={adopted() ? "check" : "plus"}
                    variant="outline"
                    disabled={props.readOnly || adopted()}
                    onPress={() => props.onAdopt(sub())}
                  />
                </View>
              </View>
            );
          }}
        </Each>
      </View>

      {/* Unmatched files notice */}
      <If when={(props.report.unmatched?.length ?? 0) > 0}>
        <View style={{ padding: 12, borderTopWidth: 1, gap: 4 }} class="border-border bg-muted/30">
          <Text style={{ fontSize: 12, fontWeight: 500 }} class="text-muted-foreground">
            {t("readiness.external.unmatchedTitle")}
          </Text>
          <Each of={props.report.unmatched ?? []} key={(u) => u.file_path}>
            {(u) => (
              <Text style={{ fontSize: 11.5, fontFamily: MONO }} class="text-muted-foreground">
                {u().file_path}  ({u().reason})
              </Text>
            )}
          </Each>
        </View>
      </If>
    </View>
  );
}

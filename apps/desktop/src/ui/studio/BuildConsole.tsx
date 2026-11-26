// Local Polygon Authoring Console (Phase 3B polished edition):
// - Zero stacked toolbars: Authoring controls live directly in StudioView's TitleBar trailing slot
// - P0-2 Quiet Results Table ("only anomalies speak"): borderless expected cells, solid red/amber anomalies only, 2px primary selection ring
// - P0-3 Zero layout push: Checks & Filter render as anchored overlay popovers; TitleBar cluster fixed; Esc closes popovers first
// - P1-1 Fluid solution columns (120–180px), 560px (~72ch) small preview max-width
// - P1-4 Unified Detail Page Grammar across all objects + inline generator batch editor + dropdown test selector
// - P1-5 State consistency: preserves untouched summaries (`旧`) and full-run safety margin across subset runs

import { batch, cleanup, computed, effect, Scroll, signal, Text, untracked, View, type ContextMenuEvent, type JSX, type ViewRef } from "nsh";
import { Input } from "nsh/base";
import { Menu as DesktopMenu } from "nsh/desktop";
import type {
  PackageBuildStatusJs,
  PackageInspectionResultJs,
  TestCasePreviewJs,
} from "xcpc-workstation/native";
import { ErrorDisplay } from "../ErrorDisplay";
import { Icon, type IconName } from "../Icon";
import { locale, t } from "../i18n";
import { Btn, createTransientFlag, IconBtn, Segmented, tip } from "../kit";
import { EmptyState } from "../layout";
import { AppMenu, type MenuEntry } from "../Menu";
import { compilerMissing, dialog, ensureToolchainLoaded, goSettings, onExternalChange, preview, type UiError } from "../store";
import { showToast } from "../Toast";
import { MONO, popoverShadow, token } from "../theme";
import { traceInteraction, traceSpan } from "../trace";
import { CellDiagnosticDrawer, InlineFileEditor } from "./BuildDetailPane";
import { MatrixCellView } from "./MatrixCell";
import { createMatrixCellStore, stepMatrixSelection } from "./matrixStore";
import { StatusSlot, type StatusModel } from "./StatusSlot";
import { testerResults } from "./contestCheck";
import { judgeModeOf, problemStatusModel, sanitizerTargetName, stdRatioWords, verdictWords, problemTypeSummary, runsLabel, solutionExpectLabel, type JudgeMode } from "./statusLogic";
import {
  BUILTIN_CHECKERS,
  canInlineEditTest,
  cellTimeMs,
  cellVisualStyle,
  countTestsInRule,
  EXPECT_PRESETS,
  formatAcVsTleMargin,
  formatBytesCompact,
  formatScoreExpect,
  formatScoreRatioPercent,
  formatScoreValue,
  formatSolutionScoreSummary,
  formatStdTimeRatio,
  groupMatrixRowsBySubtask,
  groupTestRows,
  humanizeCheckText,
  isCellVerdictAnomaly,
  isMatrixCellAnomaly,
  isRowAnomalous,
  logicalSolutionName,
  matchSelfTestResult,
  matrixCellKey,
  mergeLiveMatrixCells,
  nextActionForCheck,
  partitionGates,
  resolveEffectiveSafetyMargin,
  resolveSingleStatusBadge,
  resolveSolutionHeaderSpecs,
  solutionRole,
  toggleSubsetSelection,
  type BuildReportData,
  type MatrixCellData,
  type PackageInspectionData,
  type SafetyMarginData,
  type SolutionHeaderSpec,
  type SolutionSummaryData,
  type SubtaskSummaryData,
  type TestSummaryData,
} from "./buildLogic";
import {
  inferAdoptExpect,
  parseExternalProblemReport,
  parseSanitizerReport,
  type ExternalProblemReportData,
  type ExternalSubmissionResultData,
  type SanitizerReportData,
} from "./readinessLogic";
import {
  AddGeneratorBatchModal,
  ConfirmDeleteRuleModal,
  ConfirmPackageDeleteModal,
  CreateSourceModal,
  SolutionScoringModal,
  SubtaskModal,
  type PendingDeleteTarget,
} from "./PackageModals";

import { CONTROL, FONT_SIZE, FONT_WEIGHT, LINE_HEIGHT, RADIUS, ROW, SPACE } from "../tokens";
export interface BuildConsoleController {
  triggerPrimaryRun(): void;
  closePopovers(): boolean;
  startRun(mode: "debug" | "verify" | "sanitize"): void;
}

interface ToolbarCtrl {
  statusModel(): StatusModel;
  statusOpen(): boolean;
  setStatusOpen(open: boolean): void;
  selectionCount(): number;
  clearSelection(): void;
  running(): boolean;
  runningExternal(): boolean;
  quickRunDone(): boolean;
  startRun(mode: "debug" | "verify" | "sanitize"): void;
  pickAndRunExternal(): void;
  cancelRun(): void;
}

/**
 * The authoring panel's 38pt toolbar (authoring-ui-refactor §5.1), same height and grammar as
 * the editor's 题面 / 样例 / 题解 row: the problem's status slot on the left, 「检查 ▾」 on the
 * right. Selecting matrix headers turns the primary into 「运行选中」; running swaps it in place
 * for 「取消」; a partial run ends with the brief 「✓ 完成」 (§6.5 tier 3).
 */
function AuthoringToolbar(props: { ctrl: ToolbarCtrl; readOnly: boolean }) {
  const running = () => props.ctrl.running();
  const selected = () => props.ctrl.selectionCount();
  const primaryLabel = () => {
    if (props.ctrl.runningExternal()) return t("build.action.externalRunning");
    return selected() > 0 ? t("build.action.runSelected", { count: selected() }) : t("build.action.check");
  };
  const W = 132;
  return (
    <View
      testID="authoring-toolbar"
      style={{
        height: ROW.table,
        flexShrink: 0,
        paddingLeft: SPACE[8],
        paddingRight: SPACE[8],
        borderBottomWidth: 1,
        flexDirection: "row",
        alignItems: "center",
        justifyContent: "space-between",
        gap: SPACE[8],
      }}
      class="border-border"
    >
      <StatusSlot
        testID="build-status-slot"
        width={260}
        model={props.ctrl.statusModel()}
        open={props.ctrl.statusOpen()}
        onOpenChange={(open) => props.ctrl.setStatusOpen(open)}
      />
      <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[4], flexShrink: 0 }}>
        <If when={selected() > 0 && !running()}>
          <IconBtn
            testID="build-clear-selection"
            icon="x"
            size="sm"
            tone="muted-foreground"
            tooltip={tip(t("build.action.clearSelection"))}
            onPress={() => props.ctrl.clearSelection()}
          />
        </If>
        <View style={{ width: W, flexDirection: "row", justifyContent: "flex-end", alignItems: "center" }}>
          <If
            when={!running()}
            else={
              <Btn
                testID="build-cancel-btn"
                label={t("common.cancel")}
                icon="x"
                variant="outline"
                onPress={() => props.ctrl.cancelRun()}
                style={{ width: W }}
              />
            }
          >
            <If
              when={!props.ctrl.quickRunDone()}
              else={
                <Btn
                  testID="build-quick-run-done-btn"
                  label={t("build.action.done")}
                  icon="check"
                  variant="outline"
                  onPress={() => props.ctrl.startRun("verify")}
                  style={{ width: W }}
                />
              }
            >
              <View style={{ flexDirection: "row", alignItems: "center" }}>
                <Btn
                  testID="build-verify-btn"
                  label={primaryLabel()}
                  variant="primary"
                  disabled={props.readOnly || props.ctrl.runningExternal()}
                  tooltip={selected() > 0 ? undefined : tip(t("build.action.check"), "mod+b")}
                  onPress={() => props.ctrl.startRun(selected() > 0 ? "debug" : "verify")}
                  style={{ width: W - 26, borderTopRightRadius: RADIUS.none, borderBottomRightRadius: RADIUS.none }}
                />
                <AppMenu
                  align="end"
                  minWidth={200}
                  entries={[
                    { id: "debug", label: t("build.action.runSelectedOnly"), icon: "play", disabled: selected() === 0 },
                    { id: "sanitize", label: t("build.action.memoryCheck"), icon: "shieldCheck" },
                    { separator: true, id: "sep" },
                    { id: "external", label: t("build.action.tryExternal"), icon: "folderOpen", disabled: props.ctrl.runningExternal() },
                  ]}
                  onSelect={(id) => {
                    if (id === "debug") props.ctrl.startRun("debug");
                    else if (id === "sanitize") props.ctrl.startRun("sanitize");
                    else if (id === "external") props.ctrl.pickAndRunExternal();
                  }}
                  trigger={
                    <View
                      testID="build-verify-menu-trigger"
                      class="bg-primary hover:bg-sidebar-primary border-l border-primary-foreground/20"
                      style={{
                        width: 26,
                        height: CONTROL.md,
                        borderTopRightRadius: RADIUS.sm,
                        borderBottomRightRadius: RADIUS.sm,
                        alignItems: "center",
                        justifyContent: "center",
                        opacity: props.readOnly ? 0.5 : 1,
                      }}
                    >
                      <Icon name="chevronDown" size="sm" tone="primary-foreground" />
                    </View>
                  }
                />
              </View>
            </If>
          </If>
        </View>
      </View>
    </View>
  );
}

type TreeSelection =
  | { kind: "problemType" }
  | { kind: "results" }
  | { kind: "sanitizer" }
  | { kind: "external" }
  | { kind: "testGroup"; groupId: string; testIndex: number }
  | { kind: "selfTests"; relPath?: string }
  | { kind: "validator" }
  | { kind: "interactor" }
  | { kind: "checker" }
  | { kind: "subtasks" }
  | { kind: "generator"; file: string }
  | { kind: "solution"; name: string };

type TestPaneTab = "input" | "sample_ans" | "std_out";

const COL_TEST_W = 200;

export function BuildConsole(props: {
  problemId: string;
  /** Inside a contest: check under its overridden limits and show its tester code. */
  contestId?: string;
  readOnly: boolean;
  inspectionResult: PackageInspectionResultJs | undefined;
  onInitPackage?: () => void;
  /** `afterBuild`: a check just finished; nothing but the package and its status changed. */
  onRefreshPackage: (opts?: { afterBuild?: boolean }) => void;
  onSwitchToStatement?: () => void;
  onCollapseInspector?: () => void;
  controller?: (c: BuildConsoleController | undefined) => void;
}) {
  const isZh = () => locale() === "zh-CN";
  ensureToolchainLoaded();

  // ── Core live state (P0-2 Single State Model) ──────────────────────────────
  const selection = signal<TreeSelection>({ kind: "results" });
  const onlyAnomalyRows = signal(false);
  const statusOpen = signal(false);
  const filterSolutions = signal<string[]>([]);
  const filterTests = signal<string[]>([]);

  const selectedCell = signal<{ testIndex: number; solution: string } | undefined>(undefined);
  const testPaneTab = signal<TestPaneTab>("input");
  const testPreview = signal<TestCasePreviewJs | undefined>(undefined);
  const fileRefreshKey = signal(0);

  // Preserved full-run snapshots so subset runs never wipe untouched column summaries or footer metrics (P1-5)
  const lastFullSummaries = signal<Map<string, SolutionSummaryData>>(new Map());
  const lastFullMargin = signal<SafetyMarginData | undefined>(undefined);

  // Inline generator batch editor state (P1-4) + Tier 2/3 transient flags (§6.5)
  const batchEditRuleIdx = signal<number | undefined>(undefined);
  const batchEditGen = signal("");
  const batchEditArgs = signal("");
  const batchEditRepeat = signal("1");
  const batchEditTag = signal("");
  const batchSaved = createTransientFlag(1500);
  const quickRunDone = createTransientFlag(1400);

  // Modals & transient feedback
  const createModalKind = signal<"solution" | "generator" | undefined>(undefined);
  const addGenModalOpen = signal(false);
  const addGenInitial = signal<string | undefined>(undefined);
  const deleteRuleIndex = signal<number | undefined>(undefined);
  const pendingDelete = signal<PendingDeleteTarget | undefined>(undefined);
  const collapsedSubtasks = signal<Record<string, boolean>>({});
  const subtaskModalOpen = signal(false);
  const editingSubtask = signal<SubtaskSummaryData | undefined>(undefined);
  const scoringModalSolution = signal<string | undefined>(undefined);
  const dirtyUndoNotice = signal<number | undefined>(undefined);
  const notice = signal<UiError | undefined>(undefined);

  // Live build runner polling state
  const buildStatus = signal<PackageBuildStatusJs>(preview.packageBuildStatus());
  const finishedReport = signal<BuildReportData | undefined>(undefined);
  const liveCells = signal<MatrixCellData[]>([]);
  const cellStore = createMatrixCellStore();
  let lastLiveCellsJson = "";
  let matrixRef: ViewRef | undefined;

  // Phase 3D: Isolated Sanitizer (UB Check) & External Trial Run state
  const sanitizerReport = signal<SanitizerReportData | undefined>(undefined);
  const externalReport = signal<ExternalProblemReportData | undefined>(undefined);
  const externalMarkdown = signal<string | undefined>(undefined);
  const externalRunning = signal(false);
  const externalExported = createTransientFlag(1600);
  const adoptedExternalPaths = signal<Record<string, boolean>>({});

  /** This problem's own trial runs plus the contest's tester code (session only, §4.1). */
  const testerSubs = computed<ExternalSubmissionResultData[]>(() => [
    ...(externalReport()?.submissions ?? []),
    ...(props.contestId ? (testerResults().get(props.problemId) ?? []) : []),
  ]);
  const showTesterColumn = computed(() => testerSubs().some((sub) => !!sub.tester));

  function refreshSanitizerReport(slug: string) {
    const res = preview.loadSanitizerReport(slug);
    if (res.ok) {
      sanitizerReport.set(parseSanitizerReport(res.reportJson));
    }
  }

  // Reset per-problem state when switching problems
  let lastSlug = "";
  effect(() => {
    const slug = props.problemId;
    untracked(() => {
      if (slug !== lastSlug) {
        lastSlug = slug;
        selection.set({ kind: "results" });
        selectedCell.set(undefined);
        testPaneTab.set("input");
        filterSolutions.set([]);
        filterTests.set([]);
        onlyAnomalyRows.set(false);
        statusOpen.set(false);
        dirtyUndoNotice.set(undefined);
        notice.set(undefined);
        finishedReport.set(undefined);
        liveCells.set([]);
        lastLiveCellsJson = "";
        cellStore.dispose();
        lastFullSummaries.set(new Map());
        lastFullMargin.set(undefined);
        batchEditRuleIdx.set(undefined);
        collapsedSubtasks.set({});
        subtaskModalOpen.set(false);
        editingSubtask.set(undefined);
        scoringModalSolution.set(undefined);
        externalReport.set(undefined);
        externalMarkdown.set(undefined);
        externalRunning.set(false);
        adoptedExternalPaths.set({});
        refreshSanitizerReport(slug);
      }
    });
  });

  // Subscribe to external filesystem changes (Fix A-2)
  const unsubWatch = onExternalChange((paths, rescan) => {
    const slug = untracked(() => props.problemId);
    const relevant =
      rescan ||
      paths.some(
        (p) =>
          p.startsWith(`problems/${slug}/`) ||
          p.startsWith(`build/problems/${slug}/`),
      );
    if (relevant) {
      finishedReport.set(undefined);
      fileRefreshKey.set(untracked(fileRefreshKey) + 1);
      refreshSanitizerReport(slug);
      props.onRefreshPackage();
    }
  });
  cleanup(unsubWatch);

  const inspection = computed<PackageInspectionData | undefined>(() => {
    const raw = props.inspectionResult?.inspectionJson;
    if (!raw) return undefined;
    try {
      return JSON.parse(raw) as PackageInspectionData;
    } catch {
      return undefined;
    }
  });

  const hasPackage = computed(() => Boolean(props.inspectionResult?.hasPackage || inspection()?.has_package));
  const freshness = computed(() => props.inspectionResult?.freshnessStatus ?? inspection()?.freshness?.status);

  const activeReport = computed<BuildReportData | undefined>(() => {
    const fin = finishedReport();
    const disk = inspection()?.last_report ?? undefined;
    if (fin && fin.problem_id === props.problemId) {
      if (!disk || fin.built_at_epoch_secs >= disk.built_at_epoch_secs) return fin;
    }
    return disk;
  });

  // Snapshot full-run summaries & safety margin whenever a non-subset (or std-bearing) report arrives (P1-5)
  effect(() => {
    const rep = activeReport();
    if (!rep) return;
    untracked(() => {
      // A partial run's summaries cover only the tests it ran: never let them judge a solution.
      if (!rep.subset && rep.solutions && rep.solutions.length > 0) {
        const next = new Map(lastFullSummaries());
        for (const s of rep.solutions) next.set(s.name, s);
        lastFullSummaries.set(next);
      }
      if (!rep.subset || rep.safety_margin.std_max_time_ms > 0) {
        lastFullMargin.set(rep.safety_margin);
      }
    });
  });

  const isRunningThis = computed(
    () => buildStatus().running && buildStatus().slug === props.problemId,
  );

  // Poll background build runner at 60ms while active, deduplicating unchanged JSON
  // snapshots so idle ticks cost ~0ms and only changed cells update via `cellStore`.
  let pollTimer: number | undefined;
  let lastSeenSeq = buildStatus().seq;
  function stopPolling() {
    if (pollTimer !== undefined) {
      clearInterval(pollTimer);
      pollTimer = undefined;
    }
  }
  function startPolling() {
    if (pollTimer !== undefined) return;
    pollTimer = setInterval(() => traceSpan("build", "poll_tick", () => {
      const next = traceSpan("build", "poll_ipc", () => preview.packageBuildStatus());
      const prev = untracked(buildStatus);
      // Skip signal writes on idle ticks where scalar progress did not change so
      // `statusModel` and footer text are not invalidated 16.7x/sec unnecessarily.
      if (
        prev.seq !== next.seq ||
        prev.running !== next.running ||
        prev.slug !== next.slug ||
        prev.mode !== next.mode ||
        prev.cancelled !== next.cancelled ||
        prev.phase !== next.phase ||
        prev.detail !== next.detail ||
        prev.completed !== next.completed ||
        prev.total !== next.total ||
        prev.error !== next.error
      ) {
        buildStatus.set(next);
      }
      if (next.liveCellsJson && next.slug === untracked(() => props.problemId)) {
        if (next.liveCellsJson !== lastLiveCellsJson) {
          lastLiveCellsJson = next.liveCellsJson;
          try {
            liveCells.set(JSON.parse(next.liveCellsJson) as MatrixCellData[]);
          } catch {
            // ignore
          }
        }
      }
      if (!next.running) {
        stopPolling();
        if (next.seq !== lastSeenSeq || next.reportJson || next.error) {
          lastSeenSeq = next.seq;
          lastLiveCellsJson = "";
          if (next.reportJson && next.slug === untracked(() => props.problemId)) {
            if (next.mode === "sanitize") {
              liveCells.set([]);
              const san = parseSanitizerReport(next.reportJson);
              if (san) {
                sanitizerReport.set(san);
                if (!next.cancelled && !next.error) {
                  if (san.status === "passed") {
                    showToast({ tone: "success", text: t("toast.memoryClean") });
                  } else if (san.status === "failed") {
                    showToast({
                      tone: "issues",
                      text: t("toast.memoryIssues", { count: Math.max(1, san.findings?.length ?? 0) }),
                      action: { label: t("toast.view"), run: () => selection.set({ kind: "sanitizer" }) },
                    });
                  }
                }
              }
            } else {
              try {
                const rep = JSON.parse(next.reportJson) as BuildReportData;
                // Commit finishedReport + clear liveCells atomically so cells reconcile
                // in-place without an empty-matrix flash.
                traceSpan("build", "report_apply", () =>
                  batch(() => {
                    finishedReport.set(rep);
                    liveCells.set([]);
                  }),
                );
                if (!next.cancelled && !next.error) {
                  if (rep.mode === "debug" || rep.subset) {
                    // Tier 2 (§6.5): brief in-place confirmation for partial / quick runs without claiming full verification.
                    quickRunDone.trigger();
                  } else if (rep.verified) {
                    showToast({ tone: "success", text: t("toast.problemCheckPassed") });
                  } else {
                    showToast({
                      tone: "issues",
                      text: t("toast.problemCheckIssues"),
                      action: { label: t("toast.view"), run: () => statusOpen.set(true) },
                    });
                  }
                }
              } catch {
                // ignore
              }
            }
          }
          if (next.error) {
            notice.set({ error: next.error, context: t("ctx.packageBuild") });
          } else {
            notice.set(undefined);
          }
          if (next.mode === "sanitize") {
            refreshSanitizerReport(untracked(() => props.problemId));
          } else {
            props.onRefreshPackage({ afterBuild: true });
          }
        }
      }
    }), 60) as unknown as number;
  }
  effect(() => {
    if (buildStatus().running) startPolling();
  });
  cleanup(() => {
    stopPolling();
    cellStore.dispose();
  });

  // ── P0-2 Single State Model derivations ────────────────────────────────────
  const solutionSpecs = computed(() => resolveSolutionHeaderSpecs(inspection(), activeReport()));

  const testRows = computed<TestSummaryData[]>(() => {
    const reportTests = new Map<number, TestSummaryData>();
    for (const rt of activeReport()?.tests ?? []) {
      reportTests.set(rt.index, rt);
    }
    const expanded = inspection()?.expanded_tests ?? [];
    if (expanded.length > 0) {
      return expanded.map((et) => {
        const rt = reportTests.get(et.index);
        return {
          index: et.index,
          name: et.name,
          tag: et.tag ?? rt?.tag ?? null,
          subtask: et.subtask ?? rt?.subtask ?? null,
          is_sample: et.is_sample,
          size_bytes: rt?.size_bytes ?? 0,
          gen_cmd: et.gen_cmd ?? rt?.gen_cmd ?? null,
          validator_ok: rt?.validator_ok ?? null,
          validator_message: rt?.validator_message ?? null,
        };
      });
    }
    return activeReport()?.tests ?? [];
  });

  const testTags = computed(() => {
    const set = new Set<string>();
    for (const r of testRows()) {
      if (r.tag && r.tag.trim()) set.add(r.tag.trim());
    }
    return Array.from(set).sort();
  });

  const testGroups = computed(() => groupTestRows(inspection()?.expanded_tests, testRows()));
  const selfTestFiles = computed(() => inspection()?.files?.self_test_files ?? []);
  const hasSelfTestFailure = computed(() =>
    (activeReport()?.self_tests ?? []).some((s) => !s.passed),
  );

  const matrixLookup = computed(() =>
    mergeLiveMatrixCells(activeReport()?.matrix, isRunningThis() ? liveCells() : undefined),
  );

  // Reconcile the merged matrix into per-cell signals inside a single batch so each
  // `MatrixCellView` and `selectedCellData` only re-evaluates when its own cell changes.
  effect(() => {
    const map = matrixLookup();
    const streaming = untracked(isRunningThis);
    traceSpan("build", "matrix_reconcile", () => cellStore.reconcile(map, streaming), {
      cells: map.size,
      streaming,
    });
  });

  const summaryBySolution = computed(() => {
    const rep = activeReport();
    if (rep?.subset) return lastFullSummaries();
    const m = new Map<string, SolutionSummaryData>();
    for (const s of rep?.solutions ?? []) m.set(s.name, s);
    return m;
  });

  const effectiveSafetyMargin = computed(() =>
    resolveEffectiveSafetyMargin(activeReport(), lastFullMargin()),
  );

  const gates = computed(() => activeReport()?.gates ?? inspection()?.gates ?? []);
  const partitionedGates = computed(() => partitionGates(gates()));
  const compileFailures = computed(() => activeReport()?.compile_failures ?? []);
  const dirtyInputs = computed(() => {
    const ins = inspection();
    if (ins) return ins.dirty_inputs ?? [];
    return activeReport()?.dirty_inputs ?? [];
  });

  const singleStatus = computed(() =>
    resolveSingleStatusBadge(
      freshness(),
      partitionedGates().failed.length,
      compileFailures().length,
      Boolean(activeReport()),
    ),
  );

  const anomalousRowIndices = computed(() => {
    const set = new Set<number>();
    const cols = solutionSpecs();
    const lookup = matrixLookup();
    const sumMap = summaryBySolution();
    for (const row of testRows()) {
      if (isRowAnomalous(row, cols, lookup, sumMap)) set.add(row.index);
    }
    return set;
  });

  const displayedTableRows = computed(() => {
    const rows = testRows();
    if (!onlyAnomalyRows()) return rows;
    const anom = anomalousRowIndices();
    return rows.filter((r) => anom.has(r.index));
  });

  const subtasks = computed<SubtaskSummaryData[]>(() => {
    const ins = inspection()?.subtasks;
    if (ins && ins.length > 0) return ins;
    return activeReport()?.subtasks ?? [];
  });

  const totalSubtaskScore = computed<number | null>(() => {
    if (inspection()?.total_score != null) return inspection()!.total_score!;
    if (activeReport()?.total_score != null) return activeReport()!.total_score!;
    const list = subtasks();
    return list.length > 0 ? list.reduce((acc, s) => acc + s.score, 0) : null;
  });

  // Problem type (§5.2): drives which objects the sidebar shows.
  const isInteractive = computed(() => Boolean(inspection()?.files?.interactor));
  const isSubtaskProblem = computed(() => subtasks().length > 0);
  const judgeMode = computed(() => judgeModeOf(isInteractive(), inspection()?.checker));
  const typeSummary = computed(() =>
    problemTypeSummary({
      interactive: isInteractive(),
      runs: inspection()?.runs ?? 1,
      subtaskScore: isSubtaskProblem() ? totalSubtaskScore() : null,
    }),
  );

  /** 「应 AC，实际 WA」 for a solution whose result does not match its expectation. */
  function solutionMismatchText(spec: SolutionHeaderSpec): string | undefined {
    const sum = summaryBySolution().get(spec.name) ?? lastFullSummaries().get(spec.name);
    if (!sum || sum.matched) return undefined;
    if (spec.score != null && sum.total_score != null) {
      return t("build.mismatch.score", { expected: formatScoreExpect(spec.score)!, actual: formatScoreValue(sum.total_score) });
    }
    const expected = (spec.expected.length > 0 ? spec.expected : ["AC"]).join("/");
    return t("build.mismatch.verdict", { expected, actual: sum.actual_verdict });
  }

  /** 「仅看异常」 counts the cells that do not match their solution's expectation (§5.3). */
  const anomalousCellCount = computed(() => {
    let n = 0;
    const lookup = matrixLookup();
    for (const spec of solutionSpecs()) {
      const sum = summaryBySolution().get(spec.name) ?? lastFullSummaries().get(spec.name);
      for (const row of testRows()) {
        const c = lookup.get(matrixCellKey(row.index, spec.name));
        if (c && isMatrixCellAnomaly(c.verdict, spec, row.subtask, sum?.matched)) n += 1;
      }
    }
    return n;
  });
  const matrixSubtaskGroups = computed(() => groupMatrixRowsBySubtask(displayedTableRows(), subtasks()));

  const selectedCellData = computed(() => {
    const sel = selectedCell();
    if (!sel) return undefined;
    return cellStore.cell(sel.testIndex, sel.solution)();
  });

  // Refresh test case preview & batch edit defaults whenever selection changes
  effect(() => {
    const sel = selection();
    const slug = props.problemId;
    void props.inspectionResult;
    void fileRefreshKey();
    untracked(() => {
      if (sel.kind !== "testGroup") {
        testPreview.set(undefined);
        batchEditRuleIdx.set(undefined);
        return;
      }
      const p = preview.inspectTestCase(slug, String(sel.testIndex), 40, 20);
      testPreview.set(p);
      const curTab = testPaneTab();
      if (curTab === "sample_ans" && (!p.ansAvailable || p.ansSource !== "manual_sample")) {
        testPaneTab.set("input");
      } else if (curTab === "std_out" && !p.stdOutputAvailable && !(p.ansAvailable && p.ansSource === "std")) {
        testPaneTab.set("input");
      }
      const grp = testGroups().find((g) => g.id === sel.groupId);
      if (grp && grp.kind === "rule" && grp.genCmd && grp.ruleIndex != null) {
        if (batchEditRuleIdx() !== grp.ruleIndex) {
          const parts = grp.genCmd.trim().split(/\s+/);
          batchEditRuleIdx.set(grp.ruleIndex);
          batchEditGen.set(parts[0] ?? "");
          batchEditArgs.set(parts.slice(1).join(" "));
          batchEditRepeat.set(String(grp.tests.length));
          batchEditTag.set(grp.tag ?? "");
        }
      } else {
        batchEditRuleIdx.set(undefined);
      }
    });
  });

  // ── Actions ────────────────────────────────────────────────────────────────

  function selectTestByIndex(testIndex: number) {
    const grp = testGroups().find((g) => g.tests.some((t) => t.index === testIndex));
    const groupId = grp?.id ?? "samples";
    testPaneTab.set("input");
    statusOpen.set(false);
    selection.set({ kind: "testGroup", groupId, testIndex });
  }

  function startRun(mode: "debug" | "verify" | "sanitize") {
    if (props.readOnly || isRunningThis()) return;
    // No compiler: 「检查」 stays enabled and explains in the status popover (§6.1).
    if (compilerMissing()) {
      statusOpen.set(true);
      return;
    }
    notice.set(undefined);
    statusOpen.set(false);
    const hasFilter = mode !== "sanitize" && (filterTests().length > 0 || filterSolutions().length > 0);
    const effectiveMode = mode === "sanitize" ? "sanitize" : hasFilter ? "debug" : mode;
    const testsArg = hasFilter && filterTests().length > 0 ? filterTests() : null;
    const solsArg = hasFilter && filterSolutions().length > 0 ? filterSolutions() : null;
    if (mode === "sanitize") {
      selection.set({ kind: "sanitizer" });
    } else {
      liveCells.set([]);
    }
    const res = preview.startPackageBuild(props.problemId, effectiveMode, testsArg, solsArg, props.contestId ?? null);
    if (!res.ok) {
      notice.set({ code: res.code, error: res.error, context: t("ctx.packageBuild") });
      return;
    }
    const next = preview.packageBuildStatus();
    lastSeenSeq = next.seq - 1;
    buildStatus.set(next);
    startPolling();
  }

  async function pickAndRunExternal() {
    if (externalRunning()) return;
    notice.set(undefined);
    statusOpen.set(false);
    const picked = await preview.pickCppFiles({
      title: t("build.external.pickerTitle"),
      buttonLabel: t("build.external.pickerButton"),
      filterLabel: t("build.external.pickerFilter"),
    });
    if (picked.cancelled) return;
    if (picked.error) {
      notice.set({ error: picked.error, context: t("build.external.button") });
      return;
    }
    if (picked.paths.length === 0) return;
    selection.set({ kind: "external" });
    externalRunning.set(true);
    const slug = props.problemId;
    const res = await preview.runExternalProblemSolutions(slug, picked.paths, !props.readOnly);
    externalRunning.set(false);
    if (slug !== props.problemId) return;
    if (!res.ok) {
      notice.set({ code: res.code, error: res.error, context: t("build.external.button") });
      return;
    }
    const parsed = parseExternalProblemReport(res.reportJson);
    externalReport.set(parsed);
    externalMarkdown.set(res.markdown ?? undefined);
    const subs = parsed?.submissions ?? [];
    if (subs.length > 0) {
      const failed = subs.filter((s) => s.verdict !== "AC").length;
      if (failed > 0) {
        showToast({
          tone: "issues",
          text: t("toast.testerFailed", { count: failed }),
          action: { label: t("toast.view"), run: () => selection.set({ kind: "external" }) },
        });
      } else {
        showToast({ tone: "success", text: t("toast.testerAllPassed", { count: subs.length }) });
      }
    }
  }

  async function exportExternalMarkdown() {
    const md = externalMarkdown();
    if (!md) return;
    const res = await preview.saveMarkdownReport(`${props.problemId}-external-report.md`, md, {
      title: t("readiness.exportExternalDialogTitle"),
      buttonLabel: t("readiness.exportButton"),
      filterLabel: t("readiness.exportFilter"),
    });
    if (res.cancelled) return;
    if (!res.ok) {
      notice.set({ error: res.error, context: t("build.external.exportMd") });
      return;
    }
    externalExported.trigger();
  }

  function adoptExternal(sub: ExternalSubmissionResultData) {
    if (props.readOnly) return;
    const res = preview.adoptExternalSolution(
      props.problemId,
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
    adoptedExternalPaths.set({ ...adoptedExternalPaths(), [sub.file_path]: true });
    props.onRefreshPackage();
  }

  function cancelRun() {
    preview.cancelPackageBuild();
    buildStatus.set(preview.packageBuildStatus());
  }

  function triggerPrimaryRun() {
    if (!hasPackage() || isRunningThis()) return;
    startRun("verify");
  }

  function closePopovers(): boolean {
    if (selectedCell()) {
      selectedCell.set(undefined);
      return true;
    }
    if (statusOpen()) {
      statusOpen.set(false);
      return true;
    }
    return false;
  }

  const activeFilterCount = computed(() => filterTests().length + filterSolutions().length);
  const progressRatio = computed(() => {
    if (!isRunningThis()) return 0;
    const tot = buildStatus().total;
    if (tot <= 0) return 0.18;
    return Math.max(0.06, Math.min(1, buildStatus().completed / tot));
  });

  const statusModel = computed<StatusModel>(() =>
    problemStatusModel(
      {
        hasPackage: hasPackage(),
        running: isRunningThis(),
        memoryCheck: buildStatus().mode === "sanitize",
        progress: { done: buildStatus().completed, total: buildStatus().total },
        compilerMissing: compilerMissing(),
        status: singleStatus(),
        failedGates: partitionedGates().failed,
        warnGates: partitionedGates().warn,
        compileFailures: compileFailures(),
        error: notice(),
        isZh: isZh(),
      },
      {
        goForGate: (gate) => {
          const act = nextActionForCheck(gate, activeReport(), inspection()?.files);
          if (!act) return undefined;
          return () => {
            if (act.kind === "view_test") selectTestByIndex(act.testIndex);
            else if (act.kind === "select_cell") {
              selection.set({ kind: "results" });
              selectedCell.set({ testIndex: act.testIndex, solution: act.solution });
            } else if (act.kind === "open_subtasks") selection.set({ kind: "subtasks" });
            else if (act.kind === "open_file") openFileInTree(act.relPath);
          };
        },
        openFile: (rel) => openFileInTree(rel),
        goSettings: () => goSettings("toolchain"),
      },
    ),
  );

  props.controller?.({ triggerPrimaryRun, closePopovers, startRun });

  const toolbarCtrl: ToolbarCtrl = {
    statusModel: () => statusModel(),
    statusOpen: () => statusOpen(),
    setStatusOpen: (open: boolean) => {
      // An action's error has been read once the popover closes; the slot falls back to the state.
      if (!open && notice()) notice.set(undefined);
      statusOpen.set(open);
    },
    selectionCount: () => activeFilterCount(),
    clearSelection: () => {
      filterSolutions.set([]);
      filterTests.set([]);
    },
    running: () => isRunningThis(),
    runningExternal: () => externalRunning(),
    quickRunDone: () => quickRunDone.active(),
    startRun,
    pickAndRunExternal: () => void pickAndRunExternal(),
    cancelRun,
  };
  cleanup(() => props.controller?.(undefined));

  function openExternal(relPath: string) {
    void preview.openPackageFile(props.problemId, relPath).then((res) => {
      if (!res.ok) notice.set({ code: res.code, error: res.error, context: t("ctx.openPackageFile") });
    });
  }

  function openFileInTree(relPath: string) {
    statusOpen.set(false);
    if (relPath === "validator.cpp" || relPath === inspection()?.files?.validator) {
      selection.set({ kind: "validator" });
      return;
    }
    if (relPath === "interactor.cpp" || relPath === inspection()?.files?.interactor) {
      selection.set({ kind: "interactor" });
      return;
    }
    if (relPath === "checker.cpp" || relPath === inspection()?.files?.custom_checker) {
      selection.set({ kind: "checker" });
      return;
    }
    if (relPath.startsWith("generators/")) {
      selection.set({ kind: "generator", file: relPath.slice("generators/".length) });
      return;
    }
    if (relPath.startsWith("solutions/")) {
      const stem = logicalSolutionName(relPath.slice("solutions/".length));
      selection.set({ kind: "solution", name: stem });
      return;
    }
    if (relPath.startsWith("tests/validator/") || relPath.startsWith("tests/checker/")) {
      selection.set({ kind: "selfTests", relPath });
      return;
    }
    openExternal(relPath);
  }

  // Fix A-1: Immediately select the newly created hand test (directly editable via InlineFileEditor!)
  async function handleAddHandTest() {
    if (props.readOnly) return;
    const pid = props.problemId;
    const cid = props.contestId ?? null;
    const res = preview.addHandTest(pid, null, "hand", "");
    if (!res.ok) {
      notice.set({ code: res.code, error: res.error, context: t("ctx.updatePackage") });
      return;
    }
    finishedReport.set(undefined);
    props.onRefreshPackage();
    let targetIndex: number | undefined;
    try {
      const freshRaw = (await preview.inspectPackageAsync(pid, cid)).inspectionJson;
      if (freshRaw) {
        const fresh = JSON.parse(freshRaw) as PackageInspectionData;
        const found = fresh.expanded_tests.find((et) => et.rule_index === res.ruleIndex);
        if (found) targetIndex = found.index;
      }
    } catch {
      // fallback below
    }
    if (props.problemId !== pid) return;
    if (targetIndex == null) targetIndex = testRows().length + 1;
    testPaneTab.set("input");
    selection.set({ kind: "testGroup", groupId: `rule:${res.ruleIndex}`, testIndex: targetIndex });
  }

  function handleAddSelfTest(kind: "validator_self_test_valid" | "validator_self_test_invalid") {
    if (props.readOnly) return;
    const existing =
      kind === "validator_self_test_valid"
        ? (inspection()?.files?.validator_self_tests_valid ?? [])
        : (inspection()?.files?.validator_self_tests_invalid ?? []);
    const nextNum = String(existing.length + 1).padStart(2, "0");
    const stem = `${nextNum}-${kind === "validator_self_test_valid" ? "valid" : "invalid"}`;
    const res = preview.createPackageSource(props.problemId, kind, stem);
    if (!res.ok) {
      notice.set({ code: res.code, error: res.error, context: t("ctx.updatePackage") });
      return;
    }
    props.onRefreshPackage();
    selection.set({ kind: "selfTests", relPath: res.relPath });
  }

  function handleSaveBatchRule() {
    const rIdx = batchEditRuleIdx();
    if (rIdx == null || props.readOnly) return;
    const gen = batchEditGen().trim();
    if (!gen) return;
    const rep = Math.max(1, Math.min(200, Number.parseInt(batchEditRepeat().trim(), 10) || 1));
    const tg = batchEditTag().trim() || null;
    const res = preview.updateGeneratorTest(props.problemId, rIdx, gen, batchEditArgs().trim(), rep, tg, null);
    if (!res.ok) {
      notice.set({ code: res.code, error: res.error, context: t("ctx.updatePackage") });
      return;
    }
    finishedReport.set(undefined);
    batchSaved.trigger();
    props.onRefreshPackage();
  }

  function handleNormalizeDirty() {
    if (props.readOnly) return;
    const res = preview.normalizeDirtyInputs(props.problemId);
    if (!res.ok) {
      notice.set({ code: res.code, error: res.error, context: t("ctx.updatePackage") });
      return;
    }
    dirtyUndoNotice.set(res.fixed.length);
    fileRefreshKey.set(fileRefreshKey() + 1);
    props.onRefreshPackage();
  }

  function handleUndoNormalizeDirty() {
    if (props.readOnly) return;
    const res = preview.undoNormalizeDirtyInputs(props.problemId);
    if (!res.ok) {
      notice.set({ code: res.code, error: res.error, context: t("ctx.updatePackage") });
      return;
    }
    dirtyUndoNotice.set(undefined);
    fileRefreshKey.set(fileRefreshKey() + 1);
    props.onRefreshPackage();
  }

  function handleSetChecker(checker: string) {
    if (props.readOnly) return;
    if (checker === "checker.cpp" && !inspection()?.files?.custom_checker) {
      const created = preview.createPackageSource(props.problemId, "custom_checker", "checker");
      if (!created.ok) {
        notice.set({ code: created.code, error: created.error, context: t("ctx.updatePackage") });
        return;
      }
    }
    const res = preview.setPackageChecker(props.problemId, checker);
    if (!res.ok) {
      notice.set({ code: res.code, error: res.error, context: t("ctx.updatePackage") });
      return;
    }
    finishedReport.set(undefined);
    props.onRefreshPackage();
  }

  function handleSolutionConfig(name: string, isMain: boolean | null, expect: string[] | null) {
    if (props.readOnly) return;
    const res = preview.setSolutionConfig(props.problemId, name, isMain, expect, null);
    if (!res.ok) {
      notice.set({ code: res.code, error: res.error, context: t("ctx.updatePackage") });
      return;
    }
    finishedReport.set(undefined);
    props.onRefreshPackage();
  }

  function handleCreateInteractor(stay = false) {
    if (props.readOnly) return;
    const res = preview.createPackageSource(props.problemId, "interactor", "interactor");
    if (!res.ok) {
      notice.set({ code: res.code, error: res.error, context: t("ctx.updatePackage") });
      return;
    }
    finishedReport.set(undefined);
    props.onRefreshPackage();
    if (!stay) selection.set({ kind: "interactor" });
  }

  /** After the interactor is deleted, the judge mode the user picked on 题型 (§5.2). */
  let pendingJudgeMode: JudgeMode | undefined;

  function handleSetJudgeMode(mode: JudgeMode) {
    if (props.readOnly || mode === judgeMode()) return;
    if (mode === "interactive") {
      handleCreateInteractor(true);
      return;
    }
    if (isInteractive()) {
      // Leaving 交互 deletes interactor.cpp: confirm first, then finish the switch.
      pendingJudgeMode = mode;
      pendingDelete.set({ kind: "interactor" });
      return;
    }
    handleSetChecker(mode === "custom" ? "checker.cpp" : "wcmp");
  }

  function handleSetScoring(bySubtask: boolean) {
    if (props.readOnly || bySubtask === isSubtaskProblem()) return;
    if (bySubtask) {
      editingSubtask.set(undefined);
      subtaskModalOpen.set(true);
    } else {
      selection.set({ kind: "subtasks" });
    }
  }

  function handleSetRuns(runs: number) {
    if (props.readOnly) return;
    const res = preview.setPackageRuns(props.problemId, runs >= 2 ? runs : null);
    if (!res.ok) {
      notice.set({ code: res.code, error: res.error, context: t("ctx.updatePackage") });
      return;
    }
    finishedReport.set(undefined);
    props.onRefreshPackage();
  }

  function handleSetTestSubtask(ruleIndex: number, subtask: string | null) {
    if (props.readOnly) return;
    const res = preview.setTestSubtask(props.problemId, ruleIndex, subtask);
    if (!res.ok) {
      notice.set({ code: res.code, error: res.error, context: t("ctx.updatePackage") });
      return;
    }
    finishedReport.set(undefined);
    props.onRefreshPackage();
  }

  function toggleSubtaskCollapsed(key: string) {
    const cur = collapsedSubtasks();
    collapsedSubtasks.set({ ...cur, [key]: !cur[key] });
  }

  function handleConfirmDeleteRule() {
    const idx = deleteRuleIndex();
    if (idx == null || props.readOnly) return;
    deleteRuleIndex.set(undefined);
    const res = preview.removeTestRule(props.problemId, idx);
    if (!res.ok) {
      notice.set({ code: res.code, error: res.error, context: t("ctx.updatePackage") });
      return;
    }
    finishedReport.set(undefined);
    selection.set({ kind: "results" });
    props.onRefreshPackage();
  }

  function countReferencingBatches(genStem: string): number {
    return testGroups().filter(
      (g) => g.kind === "rule" && g.genCmd?.trim().split(/\s+/)[0] === genStem,
    ).length;
  }

  function handleConfirmPackageDelete(target: PendingDeleteTarget) {
    pendingDelete.set(undefined);
    if (props.readOnly) return;
    if (target.kind === "solution") {
      const res = preview.deleteSolution(props.problemId, target.name);
      if (!res.ok) {
        notice.set({ code: res.code, error: res.error, context: t("ctx.updatePackage") });
        return;
      }
      finishedReport.set(undefined);
      selection.set({ kind: "results" });
      props.onRefreshPackage();
    } else if (target.kind === "generator") {
      const res = preview.deleteGenerator(props.problemId, target.file, true);
      if (!res.ok) {
        notice.set({ code: res.code, error: res.error, context: t("ctx.updatePackage") });
        return;
      }
      finishedReport.set(undefined);
      selection.set({ kind: "results" });
      props.onRefreshPackage();
    } else if (target.kind === "sample") {
      const res = preview.deleteSample(props.problemId, target.sampleIndex);
      if (!res.ok) {
        notice.set({ code: res.code, error: res.error, context: t("ctx.updatePackage") });
        return;
      }
      finishedReport.set(undefined);
      const remainingSamples = Math.max(0, (testGroups().find((g) => g.kind === "samples")?.tests.length ?? 1) - 1);
      if (remainingSamples > 0) {
        selection.set({
          kind: "testGroup",
          groupId: "samples",
          testIndex: Math.min(target.sampleIndex, remainingSamples),
        });
      } else {
        selection.set({ kind: "results" });
      }
      props.onRefreshPackage();
    } else if (target.kind === "selfTest") {
      const remaining = selfTestFiles().filter((f) => f !== target.relPath);
      const res = preview.deleteSelfTest(props.problemId, target.relPath);
      if (!res.ok) {
        notice.set({ code: res.code, error: res.error, context: t("ctx.updatePackage") });
        return;
      }
      finishedReport.set(undefined);
      selection.set({ kind: "selfTests", relPath: remaining[0] });
      props.onRefreshPackage();
    } else if (target.kind === "interactor") {
      const res = preview.deleteInteractor(props.problemId);
      if (!res.ok) {
        pendingJudgeMode = undefined;
        notice.set({ code: res.code, error: res.error, context: t("ctx.updatePackage") });
        return;
      }
      finishedReport.set(undefined);
      const next = pendingJudgeMode;
      pendingJudgeMode = undefined;
      if (next === "custom" || next === "standard") handleSetChecker(next === "custom" ? "checker.cpp" : "wcmp");
      props.onRefreshPackage();
    } else if (target.kind === "subtask") {
      const res = preview.deleteSubtask(props.problemId, target.id, true);
      if (!res.ok) {
        notice.set({ code: res.code, error: res.error, context: t("ctx.updatePackage") });
        return;
      }
      finishedReport.set(undefined);
      props.onRefreshPackage();
    }
  }

  return (
    <View style={{ flex: 1, minHeight: 0, position: "relative" }} class="bg-card" testID="build-console">
      <If
        when={hasPackage()}
        else={
          <EmptyState
            icon="database"
            title={t("build.empty.title")}
            hint={t("build.empty.desc")}
            action={
              !props.readOnly && props.onInitPackage ? (
                <Btn
                  testID="authoring-init-package-btn"
                  label={t("build.empty.action")}
                  icon="plus"
                  variant="primary"
                  size="md"
                  onPress={props.onInitPackage}
                />
              ) : undefined
            }
          />
        }
      >
        <AuthoringToolbar ctrl={toolbarCtrl} readOnly={props.readOnly} />

        {/* ── Main Split: Left Object Tree (~228pt) + Right Content Area ── */}
        <View style={{ flex: 1, minHeight: 0, flexDirection: "row" }}>
          {/* Decluttered Left Object Tree */}
          <Scroll
            testID="authoring-object-tree"
            style={{
              width: 228,
              flexShrink: 0,
              borderRightWidth: 1,
            }}
            contentStyle={{
              paddingTop: SPACE[8],
              paddingBottom: SPACE[16],
              paddingLeft: SPACE[8],
              paddingRight: SPACE[8],
              gap: SPACE[12],
            }}
            class="border-border bg-background/30"
          >
            {/* 题型 first: how this problem is judged, run and scored (§5.2) */}
            <TreeRow
              testID="tree-item-type"
              icon="settings"
              label={t("build.tree.type")}
              sub={typeSummary()}
              active={selection().kind === "problemType"}
              onPress={() => selection.set({ kind: "problemType" })}
            />

            {/* 检查结果: the matrix, plus memory check / tester code once they have results */}
            <View style={{ gap: SPACE[2] }}>
              <TreeSectionHeader label={t("build.tree.checkResults")} />
              <TreeRow
                testID="tree-item-results"
                icon="table"
                label={t("build.tree.results")}
                active={selection().kind === "results"}
                onPress={() => selection.set({ kind: "results" })}
              />
              <If when={sanitizerReport()}>
                <TreeRow
                  testID="tree-item-sanitizer"
                  icon="shieldCheck"
                  label={t("build.tree.memoryCheck")}
                  active={selection().kind === "sanitizer"}
                  statusDot={sanitizerReport()?.status === "failed" ? "fail" : undefined}
                  onPress={() => selection.set({ kind: "sanitizer" })}
                />
              </If>
              <If when={testerSubs().length > 0 || externalRunning()}>
                <TreeRow
                  testID="tree-item-external"
                  icon="folderOpen"
                  label={t("build.tree.testerCode", { count: testerSubs().length })}
                  active={selection().kind === "external"}
                  onPress={() => selection.set({ kind: "external" })}
                />
              </If>
            </View>

            {/* 校验与评测: shown by problem type (no 评测器 for an interactive problem without its own checker) */}
            <View style={{ gap: SPACE[2] }}>
              <TreeSectionHeader label={t("build.tree.validation")} />
              <TreeRow
                testID="tree-item-validator"
                icon="shieldCheck"
                label={t("build.tree.validator")}
                sub={!inspection()?.files?.validator ? t("build.tree.missing") : undefined}
                active={selection().kind === "validator"}
                onPress={() => selection.set({ kind: "validator" })}
                onContextMenu={(e) => {
                  const valFile = inspection()?.files?.validator;
                  void DesktopMenu.popup(
                    [
                      { label: t("build.menu.openEdit"), onSelect: () => selection.set({ kind: "validator" }) },
                      ...(valFile ? [{ label: t("build.quickEdit.openExternal"), onSelect: () => openExternal(valFile) }] : []),
                    ],
                    e.pagePosition,
                  );
                }}
              />
              <If when={!isInteractive() || !!inspection()?.files?.custom_checker}>
                <TreeRow
                  testID="tree-item-checker"
                  icon="scale"
                  label={t("build.tree.checker")}
                  active={selection().kind === "checker"}
                  onPress={() => selection.set({ kind: "checker" })}
                  onContextMenu={(e) => {
                    const chkFile = inspection()?.checker === "checker.cpp" ? inspection()?.files?.custom_checker : undefined;
                    void DesktopMenu.popup(
                      [
                        { label: t("build.menu.openEdit"), onSelect: () => selection.set({ kind: "checker" }) },
                        ...(chkFile ? [{ label: t("build.quickEdit.openExternal"), onSelect: () => openExternal(chkFile) }] : []),
                      ],
                      e.pagePosition,
                    );
                  }}
                />
              </If>
              <If when={isInteractive()}>
                <TreeRow
                  testID="tree-item-interactor"
                  icon="bot"
                  label={t("build.tree.interactor")}
                  active={selection().kind === "interactor"}
                  onPress={() => selection.set({ kind: "interactor" })}
                  onContextMenu={(e) => {
                    const intFile = inspection()?.files?.interactor;
                    void DesktopMenu.popup(
                      [
                        { label: t("build.menu.openEdit"), onSelect: () => selection.set({ kind: "interactor" }) },
                        ...(intFile ? [{ label: t("build.quickEdit.openExternal"), onSelect: () => openExternal(intFile) }] : []),
                        ...(intFile && !props.readOnly
                          ? [
                              { type: "separator" as const },
                              {
                                label: t("build.interactor.deleteTooltip"),
                                destructive: true,
                                onSelect: () => pendingDelete.set({ kind: "interactor" }),
                              },
                            ]
                          : []),
                      ],
                      e.pagePosition,
                    );
                  }}
                />
              </If>
            </View>

            {/* 3. Solutions (uniform `预期 WA` labels, small semantic dot, no ✓) */}
            <View style={{ gap: SPACE[2] }}>
              <TreeSectionHeader
                label={t("build.tree.solutions")}
                action={
                  !props.readOnly ? (
                    <IconBtn
                      testID="tree-add-solution-btn"
                      icon="plus"
                      size="sm"
                      tooltip={tip(t("build.programs.createSolutionTitle"))}
                      onPress={() => createModalKind.set("solution")}
                    />
                  ) : undefined
                }
              />
              <Each of={solutionSpecs()} key={(s) => s.name}>
                {(spec) => {
                  const role = () => solutionRole(spec());
                  const sum = () => summaryBySolution().get(spec().name) ?? lastFullSummaries().get(spec().name);
                  return (
                    <TreeRow
                      testID={`tree-item-solution-${spec().name}`}
                      icon={role() === "main" ? "crown" : "code"}
                      label={spec().name}
                      sub={solutionExpectLabel(spec())}
                      active={selection().kind === "solution" && (selection() as { kind: "solution"; name: string }).name === spec().name}
                      statusDot={sum() && !sum()!.matched ? "fail" : undefined}
                      onPress={() => selection.set({ kind: "solution", name: spec().name })}
                      onContextMenu={(e) => {
                        const s = spec();
                        void DesktopMenu.popup(
                          [
                            { label: t("build.menu.openEdit"), onSelect: () => selection.set({ kind: "solution", name: s.name }) },
                            ...(s.source_rel
                              ? [{ label: t("build.quickEdit.openExternal"), onSelect: () => openExternal(s.source_rel!) }]
                              : []),
                            ...(!s.is_main && !props.readOnly
                              ? [{ label: t("build.programs.setMain"), onSelect: () => handleSolutionConfig(s.name, true, ["AC"]) }]
                              : []),
                            { type: "separator" as const },
                            {
                              label: t("build.menu.deleteSolution"),
                              destructive: true,
                              disabled: props.readOnly,
                              onSelect: () => pendingDelete.set({ kind: "solution", name: s.name }),
                            },
                          ],
                          e.pagePosition,
                        );
                      }}
                    />
                  );
                }}
              </Each>
            </View>

            {/* 4. Generators */}
            <View style={{ gap: SPACE[2] }}>
              <TreeSectionHeader
                label={t("build.tree.generators")}
                action={
                  !props.readOnly ? (
                    <IconBtn
                      testID="tree-add-generator-btn"
                      icon="plus"
                      size="sm"
                      tooltip={tip(t("build.programs.createGeneratorTitle"))}
                      onPress={() => createModalKind.set("generator")}
                    />
                  ) : undefined
                }
              />
              <Each of={inspection()?.files?.generators ?? []} key={(g) => g}>
                {(file) => (
                  <TreeRow
                    testID={`tree-item-gen-${file()}`}
                    icon="sparkles"
                    label={logicalSolutionName(file())}
                    active={selection().kind === "generator" && (selection() as { kind: "generator"; file: string }).file === file()}
                    onPress={() => selection.set({ kind: "generator", file: file() })}
                    onContextMenu={(e) => {
                      const f = file();
                      const stem = logicalSolutionName(f);
                      void DesktopMenu.popup(
                        [
                          { label: t("build.menu.openEdit"), onSelect: () => selection.set({ kind: "generator", file: f }) },
                          { label: t("build.quickEdit.openExternal"), onSelect: () => openExternal(`generators/${f}`) },
                          ...(!props.readOnly
                            ? [
                                {
                                  label: t("build.menu.newBatchFromGen"),
                                  onSelect: () => {
                                    addGenInitial.set(stem);
                                    addGenModalOpen.set(true);
                                  },
                                },
                              ]
                            : []),
                          { type: "separator" as const },
                          {
                            label: t("build.menu.deleteGenerator"),
                            destructive: true,
                            disabled: props.readOnly,
                            onSelect: () =>
                              pendingDelete.set({
                                kind: "generator",
                                file: f,
                                stem,
                                refBatchCount: countReferencingBatches(stem),
                              }),
                          },
                        ],
                        e.pagePosition,
                      );
                    }}
                  />
                )}
              </Each>
            </View>

            {/* 5. Test Data — logically grouped: Samples / Disambiguated Rules / Self-tests */}
            <View style={{ gap: SPACE[2] }}>
              <TreeSectionHeader
                label={`${t("build.tree.tests")} (${testRows().length})`}
                action={
                  !props.readOnly ? (
                    <AppMenu
                      align="end"
                      minWidth={172}
                      entries={
                        [
                          { id: "hand", label: t("build.tests.addHand"), icon: "filePenLine" },
                          {
                            id: "gen",
                            label: t("build.tests.addGenerator"),
                            icon: "sparkles",
                            disabled: (inspection()?.files?.generators?.length ?? 0) === 0,
                          },
                          { id: "valid", label: t("build.tests.addValidSelfTest"), icon: "shieldCheck" },
                          { id: "invalid", label: t("build.tests.addInvalidSelfTest"), icon: "triangleAlert" },
                        ] as MenuEntry[]
                      }
                      onSelect={(id) => {
                        if (id === "hand") void handleAddHandTest();
                        else if (id === "gen") {
                          addGenInitial.set(undefined);
                          addGenModalOpen.set(true);
                        } else if (id === "valid") handleAddSelfTest("validator_self_test_valid");
                        else if (id === "invalid") handleAddSelfTest("validator_self_test_invalid");
                      }}
                      trigger={
                        <IconBtn
                          testID="tree-add-test-btn"
                          icon="plus"
                          size="sm"
                          tooltip={tip(t("build.tests.add"))}
                        />
                      }
                    />
                  ) : undefined
                }
              />

              <If when={isSubtaskProblem()}>
                <TreeRow
                  testID="tree-item-subtasks"
                  icon="listOrdered"
                  label={t("build.tree.subtasks")}
                  sub={
                    totalSubtaskScore() != null
                      ? t("build.tree.subtasksPts", { score: totalSubtaskScore()! })
                      : undefined
                  }
                  active={selection().kind === "subtasks"}
                  onPress={() => selection.set({ kind: "subtasks" })}
                />
              </If>

              <Each of={testGroups()} key={(g) => g.id}>
                {(grp) => {
                  const isActive = () =>
                    selection().kind === "testGroup" &&
                    (selection() as { kind: "testGroup"; groupId: string }).groupId === grp().id;
                  const displayLabel = () =>
                    grp().kind === "samples"
                      ? `${t("build.tree.samples")} (${grp().tests.length})`
                      : grp().label;
                  return (
                    <TreeRow
                      testID={`tree-item-group-${grp().id}`}
                      icon={grp().kind === "samples" ? "fileText" : grp().tests.length > 1 ? "sparkles" : "database"}
                      label={displayLabel()}
                      active={isActive()}
                      statusDot={grp().hasValidatorFail ? "fail" : undefined}
                      onPress={() => {
                        const firstIdx = grp().tests[0]?.index ?? 1;
                        testPaneTab.set("input");
                        selection.set({ kind: "testGroup", groupId: grp().id, testIndex: firstIdx });
                      }}
                      onContextMenu={(e) => {
                        const g = grp();
                        const firstIdx = g.tests[0]?.index ?? 1;
                        if (g.kind === "samples") {
                          void DesktopMenu.popup(
                            [
                              {
                                label: t("build.menu.viewSamples"),
                                onSelect: () => {
                                  testPaneTab.set("input");
                                  selection.set({ kind: "testGroup", groupId: g.id, testIndex: firstIdx });
                                },
                              },
                              { label: t("build.menu.openSamplesDir"), onSelect: () => openExternal("samples") },
                            ],
                            e.pagePosition,
                          );
                        } else {
                          void DesktopMenu.popup(
                            [
                              {
                                label: t("build.menu.viewTests"),
                                onSelect: () => {
                                  testPaneTab.set("input");
                                  selection.set({ kind: "testGroup", groupId: g.id, testIndex: firstIdx });
                                },
                              },
                              { type: "separator" as const },
                              {
                                label: t("build.menu.deleteBatch"),
                                destructive: true,
                                disabled: props.readOnly || g.ruleIndex == null,
                                onSelect: () => {
                                  if (g.ruleIndex != null) deleteRuleIndex.set(g.ruleIndex);
                                },
                              },
                            ],
                            e.pagePosition,
                          );
                        }
                      }}
                    />
                  );
                }}
              </Each>

              <TreeRow
                testID="tree-item-self-tests"
                icon="shieldCheck"
                label={`${t("build.tree.selfTests")} (${selfTestFiles().length})`}
                active={selection().kind === "selfTests"}
                statusDot={hasSelfTestFailure() ? "fail" : undefined}
                onPress={() => selection.set({ kind: "selfTests", relPath: selfTestFiles()[0] })}
                onContextMenu={(e) => {
                  void DesktopMenu.popup(
                    [
                      {
                        label: t("build.menu.viewSelfTests"),
                        onSelect: () => selection.set({ kind: "selfTests", relPath: selfTestFiles()[0] }),
                      },
                      ...(!props.readOnly
                        ? [
                            { type: "separator" as const },
                            {
                              label: t("build.tests.addValidSelfTest"),
                              onSelect: () => handleAddSelfTest("validator_self_test_valid"),
                            },
                            {
                              label: t("build.tests.addInvalidSelfTest"),
                              onSelect: () => handleAddSelfTest("validator_self_test_invalid"),
                            },
                          ]
                        : []),
                    ],
                    e.pagePosition,
                  );
                }}
              />
            </View>
          </Scroll>

          {/* ── Right Content Area ──────────────────────────────────────────── */}
          <View style={{ flex: 1, minWidth: 0, minHeight: 0, position: "relative" }}>
            {/* Reserved 2px hairline progress bar at very top of right pane (zero layout jump) */}
            <View style={{ height: 2, width: "100%", overflow: "hidden" }}>
              <If when={isRunningThis()}>
                <View style={{ height: 2, width: `${Math.round(progressRatio() * 100)}%` }} class="bg-primary" />
              </If>
            </View>

            {/* 题型: judge mode, runs, scoring (§5.2) */}
            <If when={selection().kind === "problemType"}>
              <Scroll style={{ flex: 1, minHeight: 0 }}>
                <View testID="problem-type-page" style={{ paddingTop: SPACE[24], paddingBottom: SPACE[32], paddingLeft: SPACE[32], paddingRight: SPACE[32], maxWidth: 600, gap: SPACE[32] }}>
                  <TypeSetting label={t("build.type.judge")} hint={judgeMode() === "interactive" ? t("build.type.judgeHintInteractive") : judgeMode() === "custom" ? t("build.type.judgeHintCustom") : t("build.type.judgeHintStandard")}>
                    <Segmented
                      items={[
                        { value: "standard", label: t("build.type.judgeStandard") },
                        { value: "custom", label: t("build.type.judgeCustom") },
                        { value: "interactive", label: t("build.type.judgeInteractive") },
                      ]}
                      value={judgeMode()}
                      onChange={(m) => handleSetJudgeMode(m as JudgeMode)}
                      itemWidth={isZh() ? 96 : 128}
                      disabled={props.readOnly}
                    />
                  </TypeSetting>
                  <TypeSetting
                    label={t("build.type.runsLabel")}
                    hint={(inspection()?.runs ?? 1) >= 2 ? t("build.type.runsHint") : undefined}
                  >
                    <Segmented
                      items={["1", "2", "3", "4"].map((n) => ({ value: n, label: runsLabel(Number(n)) }))}
                      value={String(Math.min(4, Math.max(1, inspection()?.runs ?? 1)))}
                      onChange={(n) => handleSetRuns(Number(n))}
                      itemWidth={64}
                      disabled={props.readOnly}
                    />
                  </TypeSetting>
                  <TypeSetting
                    label={t("build.type.scoring")}
                    hint={isSubtaskProblem() ? t("build.type.scoringSubtaskHint", { score: totalSubtaskScore() ?? 0 }) : undefined}
                  >
                    <Segmented
                      items={[
                        { value: "whole", label: t("build.type.scoringWhole") },
                        { value: "subtasks", label: t("build.type.scoringSubtasks") },
                      ]}
                      value={isSubtaskProblem() ? "subtasks" : "whole"}
                      onChange={(v) => handleSetScoring(v === "subtasks")}
                      itemWidth={isZh() ? 112 : 132}
                      disabled={props.readOnly}
                    />
                  </TypeSetting>
                </View>
              </Scroll>
            </If>

            {/* A. Results Overview — starts immediately at top of right pane */}
            <If when={selection().kind === "results"}>
              <View style={{ flex: 1, minHeight: 0 }}>
                {/* Dirty inputs notice + Undo (only when dirty inputs exist or just fixed) */}
                <If when={dirtyInputs().length > 0}>
                  <View
                    testID="dirty-inputs-banner"
                    style={{
                      paddingLeft: SPACE[16],
                      paddingRight: SPACE[12],
                      paddingTop: SPACE[8],
                      paddingBottom: SPACE[8],
                      borderBottomWidth: 1,
                      flexDirection: "row",
                      alignItems: "center",
                      justifyContent: "space-between",
                      gap: SPACE[8],
                    }}
                    class="border-b-border bg-background/30"
                  >
                    <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[8], flex: 1, minWidth: 0 }}>
                      <Icon name="triangleAlert" size="sm" tone="chart-4" />
                      <Text style={{ fontSize: FONT_SIZE.caption, flex: 1 }} class="text-foreground">
                        {t("build.dirtyInputs.banner", { count: dirtyInputs().length })}
                      </Text>
                    </View>
                    <If when={!props.readOnly}>
                      <Btn
                        testID="dirty-inputs-fix-btn"
                        label={t("build.dirtyInputs.fixAll")}
                        variant="outline"
                        onPress={handleNormalizeDirty}
                      />
                    </If>
                  </View>
                </If>

                <If when={dirtyUndoNotice() != null && dirtyInputs().length === 0}>
                  <View
                    testID="dirty-inputs-undo-banner"
                    style={{
                      paddingLeft: SPACE[16],
                      paddingRight: SPACE[12],
                      paddingTop: SPACE[6],
                      paddingBottom: SPACE[6],
                      borderBottomWidth: 1,
                      flexDirection: "row",
                      alignItems: "center",
                      justifyContent: "space-between",
                      gap: SPACE[8],
                    }}
                    class="border-b-border bg-background/30"
                  >
                    <Text style={{ fontSize: FONT_SIZE.caption }} class="text-foreground">
                      {t("build.dirtyInputs.fixedNotice", { count: dirtyUndoNotice()! })}
                    </Text>
                    <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[6] }}>
                      <Btn
                        testID="dirty-inputs-undo-btn"
                        label={t("common.undo")}
                        icon="rotateCcw"
                        variant="ghost"
                        onPress={handleUndoNormalizeDirty}
                      />
                      <IconBtn icon="x" size="sm" onPress={() => dirtyUndoNotice.set(undefined)} />
                    </View>
                  </View>
                </If>

                {/* Compile failures (neutral clean row, no heavy box) */}
                <If when={compileFailures().length > 0}>
                  <View style={{ borderBottomWidth: 1 }} class="border-border bg-background/20">
                    <Each of={compileFailures()} key={(cf) => cf.target}>
                      {(cf) => (
                        <View style={{ paddingLeft: SPACE[16], paddingRight: SPACE[12], paddingTop: SPACE[8], paddingBottom: SPACE[8], gap: SPACE[4] }}>
                          <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[8] }}>
                            <Icon name="circleAlert" size="sm" tone="destructive" />
                            <Text style={{ fontSize: FONT_SIZE.caption, fontWeight: FONT_WEIGHT.semibold }} class="text-destructive">
                              {t("build.compileError.title", { target: cf().source_path })}
                            </Text>
                            <Text
                              onPress={() => openFileInTree(cf().source_path)}
                              style={{ fontSize: FONT_SIZE.caption, fontWeight: FONT_WEIGHT.semibold }}
                              class="text-primary"
                            >
                              {t("build.compileError.openSource")}
                            </Text>
                          </View>
                          <Text
                            style={{ fontSize: FONT_SIZE.caption, fontFamily: MONO, lineHeight: LINE_HEIGHT.caption, numberOfLines: 4, textOverflow: "ellipsis" }}
                            class="text-secondary-foreground"
                          >
                            {cf().stderr}
                          </Text>
                        </View>
                      )}
                    </Each>
                  </View>
                </If>

                {/* P0-2 Quiet Results Table with P1-1 fluid column widths (min 120, max 180) */}
                <Scroll horizontal style={{ flex: 1, minHeight: 0 }} contentStyle={{ flexGrow: 1 }}>
                  <View
                    ref={(r) => (matrixRef = r)}
                    focusable
                    captureKeys={["up", "down", "left", "right", "j", "k"]}
                    onKeyDown={(e) => {
                      const dir =
                        e.key === "ArrowDown" || e.key === "j"
                          ? "down"
                          : e.key === "ArrowUp" || e.key === "k"
                            ? "up"
                            : e.key === "ArrowLeft"
                              ? "left"
                              : e.key === "ArrowRight"
                                ? "right"
                                : undefined;
                      if (!dir) return;
                      const rows = displayedTableRows().map((r) => r.index);
                      const sols = solutionSpecs().map((s) => s.name);
                      const next = stepMatrixSelection(
                        selectedCell(),
                        rows,
                        sols,
                        dir,
                        (ti, sol) => Boolean(untracked(cellStore.cell(ti, sol))),
                      );
                      if (next) {
                        traceInteraction("build.select_cell", () => {
                          statusOpen.set(false);
                          selectedCell.set(next);
                        });
                      }
                    }}
                    style={{ flex: 1, minHeight: 0, minWidth: COL_TEST_W + solutionSpecs().length * 120 + 32 }}
                  >
                    {/* Header: one column per solution. Its verdict lives here (red dot when it does not
                        match the expectation); subtask problems add the total score as a second line.
                        Pressing a header selects it for 「运行选中」 (§5.1, §5.3). */}
                    <View
                      style={{
                        height: isSubtaskProblem() ? 48 : 38,
                        paddingLeft: SPACE[16],
                        paddingRight: SPACE[16],
                        borderBottomWidth: 1,
                        flexDirection: "row",
                        alignItems: "center",
                      }}
                      class="border-border"
                    >
                      <View style={{ width: COL_TEST_W, flexShrink: 0 }}>
                        <Text style={{ fontSize: FONT_SIZE.caption }} class="text-muted-foreground">
                          {t("build.table.testCase")}
                        </Text>
                      </View>
                      <Each of={solutionSpecs()} key={(s) => s.name}>
                        {(spec) => {
                          const sum = () => summaryBySolution().get(spec().name) ?? lastFullSummaries().get(spec().name);
                          const mismatch = () => solutionMismatchText(spec());
                          const picked = () => filterSolutions().includes(spec().name);
                          return (
                            <View
                              testID={`matrix-col-${spec().name}`}
                              onPress={() => filterSolutions.set(toggleSubsetSelection(filterSolutions(), spec().name))}
                              tooltip={mismatch() ? tip(mismatch()!) : undefined}
                              class={picked() ? "bg-accent" : "hover:bg-muted"}
                              style={{
                                flex: 1,
                                minWidth: 120,
                                maxWidth: 180,
                                height: "100%",
                                marginRight: SPACE[4],
                                paddingLeft: SPACE[6],
                                paddingRight: SPACE[6],
                                borderRadius: RADIUS.sm,
                                justifyContent: "center",
                                gap: SPACE[2],
                              }}
                            >
                              <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[4] }}>
                                <If when={spec().is_main}>
                                  <Icon name="crown" size="sm" tone="secondary-foreground" />
                                </If>
                                <Text
                                  style={{ fontSize: FONT_SIZE.body, fontWeight: FONT_WEIGHT.semibold, numberOfLines: 1, textOverflow: "ellipsis", flexShrink: 1 }}
                                  class="text-foreground"
                                >
                                  {spec().name}
                                </Text>
                                <If when={mismatch()}>
                                  <View style={{ width: 6, height: 6, borderRadius: RADIUS.xs, flexShrink: 0 }} class="bg-destructive" />
                                </If>
                              </View>
                              <If when={isSubtaskProblem()}>
                                <Text style={{ fontSize: FONT_SIZE.caption, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
                                  {formatSolutionScoreSummary(sum()) ?? "–"}
                                </Text>
                              </If>
                            </View>
                          );
                        }}
                      </Each>
                    </View>

                    {/* Scrollable Compact Single-Line Test Rows (with optional collapsible OI subtask headers) */}
                    <Scroll style={{ flex: 1, minHeight: 0 }}>
                      <Each of={matrixSubtaskGroups()} key={(g) => g.key}>
                        {(group) => {
                          const isGrouped = () => subtasks().length > 0;
                          const isCollapsed = () => isGrouped() && Boolean(collapsedSubtasks()[group().key]);
                          return (
                            <View>
                              <If when={isGrouped()}>
                                <View
                                  testID={`matrix-subtask-header-${group().subtaskId ?? "unassigned"}`}
                                  style={{
                                    height: CONTROL.md,
                                    paddingLeft: SPACE[16],
                                    paddingRight: SPACE[16],
                                    borderBottomWidth: 1,
                                    flexDirection: "row",
                                    alignItems: "center",
                                  }}
                                  class="border-border bg-muted"
                                >
                                  <View
                                    onPress={() => toggleSubtaskCollapsed(group().key)}
                                    style={{
                                      width: COL_TEST_W,
                                      flexShrink: 0,
                                      flexDirection: "row",
                                      alignItems: "center",
                                      gap: SPACE[4],
                                      paddingRight: SPACE[12],
                                    }}
                                  >
                                    <Icon
                                      name={isCollapsed() ? "chevronRight" : "chevronDown"}
                                      size="sm"
                                      tone="secondary-foreground"
                                    />
                                    <Text
                                      style={{ fontSize: FONT_SIZE.body, fontWeight: FONT_WEIGHT.semibold, fontVariant: ["tabular-nums"], numberOfLines: 1, textOverflow: "ellipsis", flex: 1 }}
                                      class="text-foreground"
                                    >
                                      {group().subtaskId
                                        ? t("build.table.subtaskHeader", { id: group().subtaskId! })
                                        : group().samples
                                          ? t("build.tree.samples")
                                          : t("build.subtasks.unassigned")}
                                    </Text>
                                    <If when={group().summary}>
                                      {(s) => (
                                        <Text
                                          style={{ fontSize: FONT_SIZE.caption, fontVariant: ["tabular-nums"], flexShrink: 0 }}
                                          class="text-muted-foreground"
                                        >
                                          {t("build.tree.subtasksPts", { score: s().score })}
                                        </Text>
                                      )}
                                    </If>
                                  </View>

                                  {/* Per-subtask score cells aligned with each solution column */}
                                  <Each of={solutionSpecs()} key={(s) => s.name}>
                                    {(spec) => {
                                      const subScore = () => {
                                        const sid = group().subtaskId;
                                        if (!sid) return undefined;
                                        const sum = summaryBySolution().get(spec().name) ?? lastFullSummaries().get(spec().name);
                                        return sum?.subtask_scores?.find((ss) => ss.subtask_id === sid);
                                      };
                                      return (
                                        <View
                                          style={{
                                            flex: 1,
                                            minWidth: 120,
                                            maxWidth: 180,
                                            paddingRight: SPACE[12],
                                            flexDirection: "row",
                                            alignItems: "center",
                                            gap: SPACE[6],
                                          }}
                                        >
                                          <If when={subScore()}>
                                            {(ss) => (
                                              <Text
                                                style={{ fontSize: FONT_SIZE.caption, fontVariant: ["tabular-nums"], paddingLeft: SPACE[6] }}
                                                class={ss().matched === false ? "text-destructive" : "text-secondary-foreground"}
                                              >
                                                {`${formatScoreValue(ss().score)} / ${ss().max_score}`}
                                              </Text>
                                            )}
                                          </If>
                                        </View>
                                      );
                                    }}
                                  </Each>
                                </View>
                              </If>

                              <If when={!isCollapsed()}>
                                <Each of={group().rows} key={(r) => String(r.index)}>
                                  {(row) => (
                                    <View
                                      style={{
                                        height: ROW.table,
                                        paddingLeft: SPACE[12],
                                        paddingRight: SPACE[16],
                                        borderBottomWidth: 1,
                                        flexDirection: "row",
                                        alignItems: "center",
                                      }}
                                      class="border-border"
                                    >
                                      {/* Row header: pressing it selects the test for 「运行选中」 */}
                                      <View
                                        testID={`matrix-row-${row().index}`}
                                        onPress={() => filterTests.set(toggleSubsetSelection(filterTests(), String(row().index)))}
                                        class={filterTests().includes(String(row().index)) ? "bg-accent" : "hover:bg-muted"}
                                        style={{
                                          width: COL_TEST_W - 6,
                                          height: CONTROL.md,
                                          marginRight: SPACE[6],
                                          flexShrink: 0,
                                          flexDirection: "row",
                                          alignItems: "center",
                                          gap: SPACE[6],
                                          borderRadius: RADIUS.sm,
                                          paddingLeft: isGrouped() ? SPACE[16] : SPACE[6],
                                          paddingRight: SPACE[8],
                                        }}
                                      >
                                        <Text
                                          style={{ fontSize: FONT_SIZE.caption, fontVariant: ["tabular-nums"], width: 20, flexShrink: 0 }}
                                          class="text-muted-foreground"
                                        >
                                          {String(row().index).padStart(2, "0")}
                                        </Text>
                                        <Text
                                          style={{ fontSize: FONT_SIZE.body, fontWeight: FONT_WEIGHT.semibold, fontVariant: ["tabular-nums"], numberOfLines: 1, textOverflow: "ellipsis", flex: 1 }}
                                          class="text-foreground"
                                        >
                                          {row().name}
                                        </Text>
                                        <If when={row().validator_ok === false}>
                                          <Icon name="triangleAlert" size="sm" tone="destructive" />
                                        </If>
                                        <If when={row().size_bytes > 0}>
                                          <Text style={{ fontSize: FONT_SIZE.caption, fontVariant: ["tabular-nums"], flexShrink: 0 }} class="text-muted-foreground">
                                            {formatBytesCompact(row().size_bytes)}
                                          </Text>
                                        </If>
                                      </View>

                                      {/* P0-2 Quiet Solution Verdict Cells (isolated per-cell signals via MatrixCellView) */}
                                      <Each of={solutionSpecs()} key={(s) => s.name}>
                                        {(spec) => (
                                          <MatrixCellView
                                            store={cellStore}
                                            testIndex={() => row().index}
                                            subtask={() => row().subtask}
                                            spec={spec}
                                            matched={() =>
                                              (summaryBySolution().get(spec().name) ?? lastFullSummaries().get(spec().name))?.matched
                                            }
                                            isPicked={() => {
                                              const s = selectedCell();
                                              return s?.testIndex === row().index && s?.solution === spec().name;
                                            }}
                                            onPick={() => {
                                              matrixRef?.focus();
                                              traceInteraction("build.select_cell", () => {
                                                statusOpen.set(false);
                                                selectedCell.set({ testIndex: row().index, solution: spec().name });
                                              });
                                            }}
                                          />
                                        )}
                                      </Each>
                                    </View>
                                  )}
                                </Each>
                              </If>
                            </View>
                          );
                        }}
                      </Each>
                    </Scroll>
                  </View>
                </Scroll>

                {/* Quiet Footer Bar: Preserved Safety Margin metrics + Clean Progress Count (`32 / 396`) + Only Anomalies */}
                <If when={Boolean(effectiveSafetyMargin()) || anomalousRowIndices().size > 0 || isRunningThis()}>
                  <View
                    testID="results-quiet-footer"
                    style={{
                      height: CONTROL.lg,
                      paddingLeft: SPACE[16],
                      paddingRight: SPACE[12],
                      borderTopWidth: 1,
                      flexDirection: "row",
                      alignItems: "center",
                      justifyContent: "space-between",
                      gap: SPACE[12],
                    }}
                    class="border-border bg-background/30"
                  >
                    <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[12], flex: 1, minWidth: 0 }}>
                      <If
                        when={!isRunningThis()}
                        else={
                          <Text
                            style={{ fontSize: FONT_SIZE.caption, fontVariant: ["tabular-nums"] }}
                            class="text-secondary-foreground"
                          >
                            {buildStatus().total > 0
                              ? `${t("build.running.progress")} ${buildStatus().completed} / ${buildStatus().total}`
                              : t("build.running.preparing")}
                          </Text>
                        }
                      >
                        <If when={effectiveSafetyMargin()}>
                          {(m) => (
                            <>
                              <Text style={{ fontSize: FONT_SIZE.caption, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
                                {`${t("build.margin.maxInput")} ${formatBytesCompact(m().max_input_bytes)}`}
                              </Text>
                              <Text style={{ fontSize: FONT_SIZE.caption, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
                                {`${t("build.margin.stdSlowest")} ${formatStdTimeRatio(m())}`}
                              </Text>
                              <If when={formatAcVsTleMargin(m())}>
                                {(gap) => (
                                  <Text style={{ fontSize: FONT_SIZE.caption, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
                                    {`${t("build.margin.acVsTle")} ${gap()}`}
                                  </Text>
                                )}
                              </If>
                            </>
                          )}
                        </If>
                      </If>
                    </View>

                    <If when={anomalousRowIndices().size > 0 || onlyAnomalyRows()}>
                      <View
                        testID="only-anomaly-toggle"
                        onPress={() => onlyAnomalyRows.set(!onlyAnomalyRows())}
                        class={onlyAnomalyRows() ? "bg-muted border border-border" : "border border-transparent hover:bg-muted/60"}
                        style={{
                          height: CONTROL.sm,
                          paddingLeft: SPACE[8],
                          paddingRight: SPACE[8],
                          borderRadius: RADIUS.xs,
                          flexDirection: "row",
                          alignItems: "center",
                          gap: SPACE[4],
                          flexShrink: 0,
                        }}
                      >
                        <Text style={{ fontSize: FONT_SIZE.caption, fontVariant: ["tabular-nums"] }} class="text-secondary-foreground">
                          {`${t("build.filter.onlyAnomaly")} (${anomalousCellCount()})`}
                        </Text>
                      </View>
                    </If>

                  </View>
                </If>

                {/* P1-3 & P1-7 Conclusion-First Right-Overlay 3-Column Diagnostic Drawer */}
                <If when={selectedCellData()}>
                  {(cell) => {
                    const spec = () => solutionSpecs().find((s) => s.name === cell().solution);
                    const sum = () => summaryBySolution().get(cell().solution) ?? lastFullSummaries().get(cell().solution);
                    const rowMeta = () => testRows().find((r) => r.index === cell().test_index);
                    const isAnom = () => {
                      const sp = spec();
                      return sp ? isMatrixCellAnomaly(cell().verdict, sp, rowMeta()?.subtask, sum()?.matched) : false;
                    };
                    return (
                      <CellDiagnosticDrawer
                        problemId={props.problemId}
                        cell={cell()}
                        expected={spec()?.expected ?? ["AC"]}
                        isAnomaly={isAnom()}
                        genCmd={rowMeta()?.gen_cmd}
                        solutionRelPath={spec()?.source_rel}
                        onClose={() => selectedCell.set(undefined)}
                        onOpenSolution={(rel) => {
                          selectedCell.set(undefined);
                          openFileInTree(rel);
                        }}
                        onOpenTest={(idx) => {
                          selectedCell.set(undefined);
                          selectTestByIndex(idx);
                        }}
                      />
                    );
                  }}
                </If>
              </View>
            </If>

            {/* B. Logical Test Group Detail View (P1-4 unified grammar + dropdown selector + inline batch editor) */}
            <If when={selection().kind === "testGroup"}>
              {(() => {
                const curSel = () =>
                  selection().kind === "testGroup"
                    ? (selection() as { kind: "testGroup"; groupId: string; testIndex: number })
                    : undefined;
                const activeGroup = () => testGroups().find((g) => g.id === curSel()?.groupId);
                const activeIdx = () => curSel()?.testIndex ?? activeGroup()?.tests[0]?.index ?? 1;
                const row = () => testRows().find((r) => r.index === activeIdx());
                const prev = () => testPreview();
                const fileRel = () => prev()?.fileRel ?? null;
                const ruleIdx = () => activeGroup()?.ruleIndex ?? prev()?.ruleIndex ?? null;
                const directlyEditable = () =>
                  !props.readOnly && testPaneTab() === "input" && canInlineEditTest(prev()) && Boolean(fileRel());
                const isGenBatch = () => Boolean(activeGroup()?.kind === "rule" && activeGroup()?.genCmd && ruleIdx() != null);
                const genStems = () => (inspection()?.files?.generators ?? []).map(logicalSolutionName).filter(Boolean);

                const paneItems = () => {
                  const p = prev();
                  const items: { value: TestPaneTab; label: string }[] = [
                    { value: "input", label: t("build.tests.inputTab") },
                  ];
                  if (p?.ansAvailable && p.ansSource === "manual_sample") {
                    items.push({ value: "sample_ans", label: t("build.tests.sampleAnsTab") });
                  }
                  if (p?.stdOutputAvailable || (p?.ansAvailable && p.ansSource === "std")) {
                    items.push({ value: "std_out", label: t("build.tests.stdOutTab") });
                  }
                  return items;
                };
                const activeContent = () => {
                  const p = prev();
                  const tab = testPaneTab();
                  if (!p?.ok) return { head: "", tail: "", truncated: false, bytes: 0, linesExact: undefined as number | undefined, omitted: undefined as number | undefined };
                  if (tab === "sample_ans") {
                    return { head: p.ansHead, tail: p.ansTail, truncated: p.ansTruncated, bytes: p.ansSizeBytes, linesExact: p.ansTotalLines ?? undefined, omitted: undefined };
                  }
                  if (tab === "std_out") {
                    if (p.stdOutputAvailable) {
                      return { head: p.stdOutputHead, tail: p.stdOutputTail, truncated: p.stdOutputTruncated, bytes: p.stdOutputSizeBytes, linesExact: undefined, omitted: undefined };
                    }
                    return { head: p.ansHead, tail: p.ansTail, truncated: p.ansTruncated, bytes: p.ansSizeBytes, linesExact: p.ansTotalLines ?? undefined, omitted: undefined };
                  }
                  return {
                    head: p.head,
                    tail: p.tail,
                    truncated: p.truncated,
                    bytes: p.sizeBytes,
                    linesExact: p.totalLinesExact ?? (p.totalLines > 0 ? p.totalLines : undefined),
                    omitted: p.omittedBytes ?? undefined,
                  };
                };

                const testDropdownSelector = () =>
                  (activeGroup()?.tests.length ?? 0) > 1 ? (
                    <AppMenu
                      align="end"
                      minWidth={200}
                      entries={(activeGroup()?.tests ?? []).map((item) => ({
                        id: String(item.index),
                        label: `#${String(item.index).padStart(2, "0")} ${item.name}`,
                        checked: item.index === activeIdx(),
                      }))}
                      onSelect={(id) => {
                        const nextIdx = Number.parseInt(id, 10);
                        if (!Number.isNaN(nextIdx)) {
                          selection.set({
                            kind: "testGroup",
                            groupId: activeGroup()!.id,
                            testIndex: nextIdx,
                          });
                        }
                      }}
                      trigger={
                        <View
                          testID="batch-test-dropdown"
                          class="border border-border bg-card hover:bg-muted"
                          style={{
                            height: CONTROL.md,
                            paddingLeft: SPACE[8],
                            paddingRight: SPACE[8],
                            borderRadius: RADIUS.sm,
                            flexDirection: "row",
                            alignItems: "center",
                            gap: SPACE[4],
                          }}
                        >
                          <Text style={{ fontSize: FONT_SIZE.caption, fontWeight: FONT_WEIGHT.semibold, fontVariant: ["tabular-nums"] }} class="text-foreground">
                            {`#${String(activeIdx()).padStart(2, "0")} ${row()?.name ?? ""}`}
                          </Text>
                          <Icon name="chevronDown" size="sm" tone="muted-foreground" />
                        </View>
                      }
                    />
                  ) : undefined;

                const testSubtaskDropdown = () => {
                  const rIdx = ruleIdx();
                  if (props.readOnly || rIdx == null || subtasks().length === 0) return undefined;
                  const curSub = activeGroup()?.subtask ?? row()?.subtask ?? null;
                  return (
                    <AppMenu
                      align="end"
                      minWidth={172}
                      entries={[
                        { id: "__none__", label: t("build.subtasks.assignNone"), checked: curSub == null },
                        ...subtasks().map((s) => ({
                          id: s.id,
                          label: `${t("build.table.subtaskHeader", { id: s.id })}  (${s.score})`,
                          checked: curSub === s.id,
                        })),
                      ]}
                      onSelect={(id) => handleSetTestSubtask(rIdx, id === "__none__" ? null : id)}
                      trigger={
                        <View
                          testID="test-subtask-dropdown"
                          class="border border-border bg-card hover:bg-muted"
                          style={{
                            height: CONTROL.md,
                            paddingLeft: SPACE[8],
                            paddingRight: SPACE[8],
                            borderRadius: RADIUS.sm,
                            flexDirection: "row",
                            alignItems: "center",
                            gap: SPACE[4],
                          }}
                        >
                          <Text style={{ fontSize: FONT_SIZE.caption, fontWeight: FONT_WEIGHT.semibold, fontVariant: ["tabular-nums"] }} class="text-foreground">
                            {curSub ? t("build.table.subtaskHeader", { id: curSub }) : t("build.subtasks.assignNone")}
                          </Text>
                          <Icon name="chevronDown" size="sm" tone="muted-foreground" />
                        </View>
                      }
                    />
                  );
                };

                const deleteRuleBtn = () => {
                  if (props.readOnly) return undefined;
                  if (activeGroup()?.kind === "samples") {
                    return (
                      <IconBtn
                        testID="sample-delete-btn"
                        icon="trash"
                        tone="muted-foreground"
                        tooltip={tip(t("build.delete.sampleTooltip"))}
                        onPress={() => pendingDelete.set({ kind: "sample", sampleIndex: activeIdx() })}
                      />
                    );
                  }
                  if (ruleIdx() != null) {
                    return (
                      <IconBtn
                        testID="test-delete-rule-btn"
                        icon="trash"
                        tone="muted-foreground"
                        tooltip={tip(t("build.tests.deleteRule"))}
                        onPress={() => deleteRuleIndex.set(ruleIdx()!)}
                      />
                    );
                  }
                  return undefined;
                };

                return (
                  <View style={{ flex: 1, minHeight: 0 }}>
                    {/* Validator failure callout */}
                    <If when={row()?.validator_ok === false}>
                      <View
                        style={{
                          paddingLeft: SPACE[20],
                          paddingRight: SPACE[16],
                          paddingTop: SPACE[8],
                          paddingBottom: SPACE[8],
                          borderBottomWidth: 1,
                          flexDirection: "row",
                          alignItems: "center",
                          justifyContent: "space-between",
                          gap: SPACE[8],
                        }}
                        class="border-b-border bg-muted/30"
                      >
                        <View style={{ flex: 1, minWidth: 0, flexDirection: "row", alignItems: "center", gap: SPACE[8] }}>
                          <Icon name="circleAlert" size="sm" tone="destructive" />
                          <Text style={{ fontSize: FONT_SIZE.caption, fontWeight: FONT_WEIGHT.semibold }} class="text-destructive">
                            {t("build.tests.validatorFailed")}
                          </Text>
                          <If when={!!row()?.validator_message}>
                            <Text style={{ fontSize: FONT_SIZE.caption, fontFamily: MONO, flex: 1, numberOfLines: 1, textOverflow: "ellipsis" }} class="text-foreground">
                              {row()!.validator_message!}
                            </Text>
                          </If>
                        </View>
                        <Btn
                          label={t("build.tests.openValidator")}
                          icon="shieldCheck"
                          variant="ghost"
                          onPress={() => selection.set({ kind: "validator" })}
                        />
                      </View>
                    </If>

                    {/* Directly editable small hand/sample test -> Unified InlineFileEditor (NO dual 编辑/完成 mode!) */}
                    <If
                      when={directlyEditable()}
                      else={
                        <View style={{ flex: 1, minHeight: 0 }}>
                          {/* Unified Detail Header for non-editable / generated / large / output views */}
                          <View
                            style={{
                              minHeight: 52,
                              paddingLeft: SPACE[20],
                              paddingRight: SPACE[16],
                              paddingTop: SPACE[8],
                              paddingBottom: SPACE[8],
                              borderBottomWidth: 1,
                              flexDirection: "row",
                              alignItems: "center",
                              justifyContent: "space-between",
                              gap: SPACE[8],
                            }}
                            class="border-border bg-background/30"
                          >
                            <View style={{ flex: 1, minWidth: 0, gap: SPACE[2] }}>
                              <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[8] }}>
                                <Icon
                                  name={activeGroup()?.kind === "samples" ? "fileText" : isGenBatch() ? "sparkles" : "database"}
                                  size="sm"
                                  tone="secondary-foreground"
                                />
                                <Text style={{ fontSize: FONT_SIZE.body, fontWeight: FONT_WEIGHT.semibold, fontVariant: ["tabular-nums"] }} class="text-foreground">
                                  {`#${String(activeIdx()).padStart(2, "0")} ${row()?.name ?? prev()?.name ?? ""}`}
                                </Text>
                              </View>
                              <If when={fileRel() || activeGroup()?.genCmd}>
                                <Text style={{ fontSize: FONT_SIZE.caption, fontFamily: MONO, numberOfLines: 1, textOverflow: "ellipsis" }} class="text-muted-foreground">
                                  {fileRel() ?? activeGroup()?.genCmd ?? ""}
                                </Text>
                              </If>
                            </View>

                            <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[6], flexShrink: 0 }}>
                              {testSubtaskDropdown()}
                              {testDropdownSelector()}
                              <If when={paneItems().length > 1}>
                                <Segmented
                                  items={paneItems()}
                                  value={testPaneTab()}
                                  onChange={(v) => testPaneTab.set(v)}
                                  itemWidth={isZh() ? 86 : 98}
                                />
                              </If>
                              <If when={fileRel()}>
                                {(rel) => (
                                  <Btn
                                    label={t("build.quickEdit.openExternal")}
                                    icon="folderOpen"
                                    variant="ghost"
                                    onPress={() => openExternal(rel())}
                                  />
                                )}
                              </If>
                              {deleteRuleBtn()}
                            </View>
                          </View>

                          {/* P1-4 Inline Generator Batch Config Bar (editable generator / args / repeat / tag) */}
                          <If when={isGenBatch() && !props.readOnly}>
                            <View
                              testID="generator-batch-editor-bar"
                              style={{
                                paddingLeft: SPACE[20],
                                paddingRight: SPACE[16],
                                paddingTop: SPACE[8],
                                paddingBottom: SPACE[8],
                                borderBottomWidth: 1,
                                flexDirection: "row",
                                alignItems: "center",
                                gap: SPACE[8],
                                flexWrap: "wrap",
                              }}
                              class="border-border bg-background/20"
                            >
                              <AppMenu
                                align="start"
                                minWidth={180}
                                entries={genStems().map((g) => ({
                                  id: g,
                                  label: g,
                                  checked: g === batchEditGen(),
                                }))}
                                onSelect={(id) => batchEditGen.set(id)}
                                trigger={
                                  <View
                                    class="border border-border bg-card hover:bg-muted"
                                    style={{ height: CONTROL.md, paddingLeft: SPACE[8], paddingRight: SPACE[6], borderRadius: RADIUS.sm, flexDirection: "row", alignItems: "center", gap: SPACE[4] }}
                                  >
                                    <Text style={{ fontSize: FONT_SIZE.caption, fontWeight: FONT_WEIGHT.semibold }} class="text-foreground">
                                      {batchEditGen() || t("build.tests.genModalGenerator")}
                                    </Text>
                                    <Icon name="chevronDown" size="sm" tone="muted-foreground" />
                                  </View>
                                }
                              />
                              <Input
                                value={batchEditArgs()}
                                autoCapitalize="none"
                                autoCorrect={false}
                                placeholder="-n 100 {i}"
                                onChange={(e) => batchEditArgs.set(e.value)}
                                onSubmit={handleSaveBatchRule}
                                class="flex-1 min-w-[140px] h-7 px-2 rounded-md border border-border bg-card text-[12px] placeholder:text-muted-foreground focus:border-primary focus:ring-1 focus:ring-inset focus:ring-ring"
                                style={{ fontFamily: MONO }}
                              />
                              <Input
                                value={batchEditRepeat()}
                                keyboardType="number-pad"
                                onChange={(e) => batchEditRepeat.set(e.value)}
                                onSubmit={handleSaveBatchRule}
                                class="w-14 h-7 px-2 rounded-md border border-border bg-card text-[12px] focus:border-primary focus:ring-1 focus:ring-inset focus:ring-ring"
                                style={{ fontVariant: ["tabular-nums"] }}
                              />
                              <Input
                                value={batchEditTag()}
                                autoCapitalize="none"
                                autoCorrect={false}
                                placeholder={t("build.tests.genModalTag")}
                                onChange={(e) => batchEditTag.set(e.value)}
                                onSubmit={handleSaveBatchRule}
                                class="w-24 h-7 px-2 rounded-md border border-border bg-card text-[12px] placeholder:text-muted-foreground focus:border-primary focus:ring-1 focus:ring-inset focus:ring-ring"
                              />
                              <Btn
                                testID="batch-rule-save-btn"
                                label={batchSaved.active() ? t("build.batch.updated") : t("build.batch.update")}
                                icon="check"
                                variant="outline"
                                style={{ minWidth: 82 }}
                                onPress={handleSaveBatchRule}
                              />
                            </View>
                          </If>

                          {/* Bounded Preview with P1-1 maxWidth: 560 (~72ch) for small previews */}
                          <Scroll style={{ flex: 1, minHeight: 0 }} contentStyle={{ padding: SPACE[20], gap: SPACE[12] }}>
                            <Text style={{ fontSize: FONT_SIZE.caption, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
                              {activeContent().linesExact != null
                                ? `${formatBytesCompact(activeContent().bytes)}   ${t("build.tests.linesCount", { count: activeContent().linesExact! })}`
                                : formatBytesCompact(activeContent().bytes)}
                            </Text>

                            <If when={activeContent().truncated}>
                              <View
                                testID="test-truncation-banner"
                                style={{ paddingLeft: SPACE[12], paddingRight: SPACE[12], paddingTop: SPACE[6], paddingBottom: SPACE[6], borderRadius: RADIUS.sm, maxWidth: 680 }}
                                class="bg-muted/40"
                              >
                                <Text style={{ fontSize: FONT_SIZE.caption, fontVariant: ["tabular-nums"] }} class="text-secondary-foreground">
                                  {activeContent().omitted != null
                                    ? t("build.tests.truncatedWithOmitted", { bytes: formatBytesCompact(activeContent().omitted!) })
                                    : t("build.tests.truncatedBanner")}
                                </Text>
                              </View>
                            </If>

                            <View
                              style={{
                                padding: SPACE[12],
                                borderRadius: RADIUS.sm,
                                maxWidth: activeContent().truncated ? 820 : 560,
                              }}
                              class="bg-muted/40"
                            >
                              <Text style={{ fontSize: FONT_SIZE.caption, fontFamily: MONO, lineHeight: LINE_HEIGHT.body }} class="text-foreground">
                                {activeContent().head || t("build.tests.emptyContent")}
                              </Text>
                              <If when={activeContent().truncated && activeContent().tail.length > 0}>
                                <View style={{ paddingTop: SPACE[8], paddingBottom: SPACE[8] }}>
                                  <Text style={{ fontSize: FONT_SIZE.caption, fontFamily: MONO }} class="text-muted-foreground">
                                    {"…"}
                                  </Text>
                                </View>
                                <Text style={{ fontSize: FONT_SIZE.caption, fontFamily: MONO, lineHeight: LINE_HEIGHT.body }} class="text-foreground">
                                  {activeContent().tail}
                                </Text>
                              </If>
                            </View>
                          </Scroll>
                        </View>
                      }
                    >
                      <InlineFileEditor
                        problemId={props.problemId}
                        relPath={fileRel()!}
                        readOnly={props.readOnly}
                        refreshKey={fileRefreshKey()}
                        title={`#${String(activeIdx()).padStart(2, "0")} ${row()?.name ?? prev()?.name ?? ""}`}
                        icon={activeGroup()?.kind === "samples" ? "fileText" : "database"}
                        extraActions={
                          <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[6] }}>
                            {testSubtaskDropdown()}
                            {testDropdownSelector()}
                            <If when={paneItems().length > 1}>
                              <Segmented
                                items={paneItems()}
                                value={testPaneTab()}
                                onChange={(v) => testPaneTab.set(v)}
                                itemWidth={isZh() ? 86 : 98}
                              />
                            </If>
                            {deleteRuleBtn()}
                          </View>
                        }
                        onSaved={() => {
                          finishedReport.set(undefined);
                          props.onRefreshPackage();
                        }}
                        onOpenExternal={openExternal}
                      />
                    </If>
                  </View>
                );
              })()}
            </If>

            {/* C. Self-Tests Logical Group View (P1-4 unified header + crisp status dots) */}
            <If when={selection().kind === "selfTests"}>
              {(() => {
                const activeRel = () => {
                  const sel = selection();
                  if (sel.kind !== "selfTests") return undefined;
                  return sel.relPath ?? selfTestFiles()[0];
                };
                return (
                  <View style={{ flex: 1, minHeight: 0 }}>
                    <If
                      when={selfTestFiles().length > 0 && !!activeRel()}
                      else={
                        <EmptyState
                          icon="shieldCheck"
                          title={t("build.tests.selfTestsEmpty")}
                          hint={t("build.programs.validatorHint")}
                        />
                      }
                    >
                      <InlineFileEditor
                        problemId={props.problemId}
                        relPath={activeRel()!}
                        readOnly={props.readOnly}
                        refreshKey={fileRefreshKey()}
                        title={t("build.tree.selfTests")}
                        icon="shieldCheck"
                        extraActions={
                          <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[6] }}>
                            <AppMenu
                              align="end"
                              minWidth={210}
                              entries={selfTestFiles().map((rel) => ({
                                id: rel,
                                label: rel.split("/").slice(-2).join("/"),
                                checked: rel === activeRel(),
                              }))}
                              onSelect={(id) => selection.set({ kind: "selfTests", relPath: id })}
                              trigger={
                                <View
                                  class="border border-border bg-card hover:bg-muted"
                                  style={{
                                    height: CONTROL.md,
                                    paddingLeft: SPACE[8],
                                    paddingRight: SPACE[8],
                                    borderRadius: RADIUS.sm,
                                    flexDirection: "row",
                                    alignItems: "center",
                                    gap: SPACE[6],
                                  }}
                                >
                                  <If when={matchSelfTestResult(activeRel()!, activeReport()?.self_tests)?.passed === false}>
                                    <View style={{ width: 6, height: 6, borderRadius: RADIUS.xs }} class="bg-destructive" />
                                  </If>
                                  <Text style={{ fontSize: FONT_SIZE.caption, fontWeight: FONT_WEIGHT.semibold }} class="text-foreground">
                                    {activeRel()!.split("/").slice(-2).join("/")}
                                  </Text>
                                  <Icon name="chevronDown" size="sm" tone="muted-foreground" />
                                </View>
                              }
                            />
                            <If when={!props.readOnly}>
                              <IconBtn
                                testID="selftest-delete-btn"
                                icon="trash"
                                tone="muted-foreground"
                                tooltip={tip(t("build.delete.selfTestTooltip"))}
                                onPress={() => pendingDelete.set({ kind: "selfTest", relPath: activeRel()! })}
                              />
                            </If>
                          </View>
                        }
                        onSaved={() => {
                          finishedReport.set(undefined);
                          props.onRefreshPackage();
                        }}
                        onOpenExternal={openExternal}
                      />
                    </If>
                  </View>
                );
              })()}
            </If>

            {/* D. Validator Detail View (P1-4 unified header) */}
            <If when={selection().kind === "validator"}>
              <View style={{ flex: 1, minHeight: 0 }}>
                <If
                  when={inspection()?.files?.validator}
                  else={
                    <EmptyState
                      icon="shieldCheck"
                      title={t("build.tree.missing")}
                      hint={t("build.programs.validatorHint")}
                    />
                  }
                >
                  {(rel) => (
                    <InlineFileEditor
                      problemId={props.problemId}
                      relPath={rel()}
                      readOnly={props.readOnly}
                      refreshKey={fileRefreshKey()}
                      title={t("build.tree.validator")}
                      icon="shieldCheck"
                      onSaved={() => {
                        finishedReport.set(undefined);
                        props.onRefreshPackage();
                      }}
                      onOpenExternal={openExternal}
                    />
                  )}
                </If>
              </View>
            </If>

            {/* D2. Interactor & Multi-run Detail View */}
            <If when={selection().kind === "interactor"}>
              {(() => {
                const currentRuns = () => inspection()?.runs ?? 1;
                const runsDropdown = () => (
                  <AppMenu
                    align="end"
                    minWidth={160}
                    entries={[1, 2, 3, 4].map((n) => ({
                      id: String(n),
                      label: n === 1 ? t("build.interactor.runsSingle") : t("build.interactor.runsMulti", { count: n }),
                      checked: currentRuns() === n,
                    }))}
                    onSelect={(id) => handleSetRuns(Number.parseInt(id, 10) || 1)}
                    trigger={
                      <View
                        testID="interactor-runs-trigger"
                        class="border border-border bg-card hover:bg-muted"
                        style={{
                          height: CONTROL.md,
                          paddingLeft: SPACE[8],
                          paddingRight: SPACE[8],
                          borderRadius: RADIUS.sm,
                          flexDirection: "row",
                          alignItems: "center",
                          gap: SPACE[4],
                        }}
                      >
                        <Text style={{ fontSize: FONT_SIZE.caption, fontWeight: FONT_WEIGHT.semibold, fontVariant: ["tabular-nums"] }} class="text-foreground">
                          {currentRuns() === 1
                            ? t("build.interactor.runsSingle")
                            : t("build.interactor.runsMulti", { count: currentRuns() })}
                        </Text>
                        <Icon name="chevronDown" size="sm" tone="muted-foreground" />
                      </View>
                    }
                  />
                );

                return (
                  <View style={{ flex: 1, minHeight: 0 }}>
                    <If
                      when={inspection()?.files?.interactor}
                      else={
                        <View style={{ flex: 1, minHeight: 0 }}>
                          <View
                            style={{
                              minHeight: 52,
                              paddingLeft: SPACE[20],
                              paddingRight: SPACE[16],
                              paddingTop: SPACE[8],
                              paddingBottom: SPACE[8],
                              borderBottomWidth: 1,
                              flexDirection: "row",
                              alignItems: "center",
                              justifyContent: "space-between",
                              gap: SPACE[8],
                            }}
                            class="border-border bg-background/30"
                          >
                            <View style={{ gap: SPACE[2] }}>
                              <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[8] }}>
                                <Icon name="bot" size="sm" tone="secondary-foreground" />
                                <Text style={{ fontSize: FONT_SIZE.body, fontWeight: FONT_WEIGHT.semibold }} class="text-foreground">
                                  {t("build.interactor.title")}
                                </Text>
                              </View>
                              <Text style={{ fontSize: FONT_SIZE.caption }} class="text-muted-foreground">
                                {t("build.interactor.runsHint")}
                              </Text>
                            </View>
                            <If when={!props.readOnly}>{runsDropdown()}</If>
                          </View>
                          <EmptyState
                            icon="bot"
                            title={t("build.interactor.emptyTitle")}
                            hint={t("build.interactor.emptyHint")}
                            action={
                              !props.readOnly ? (
                                <Btn
                                  testID="create-interactor-btn"
                                  label={t("build.interactor.create")}
                                  icon="plus"
                                  variant="primary"
                                  size="md"
                                  onPress={() => handleCreateInteractor()}
                                />
                              ) : undefined
                            }
                          />
                        </View>
                      }
                    >
                      {(rel) => (
                        <InlineFileEditor
                          problemId={props.problemId}
                          relPath={rel()}
                          readOnly={props.readOnly}
                          refreshKey={fileRefreshKey()}
                          title={t("build.tree.interactor")}
                          icon="bot"
                          extraActions={
                            !props.readOnly ? (
                              <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[6] }}>
                                {runsDropdown()}
                                <IconBtn
                                  testID="interactor-delete-btn"
                                  icon="trash"
                                  tone="muted-foreground"
                                  tooltip={tip(t("build.interactor.deleteTooltip"))}
                                  onPress={() => pendingDelete.set({ kind: "interactor" })}
                                />
                              </View>
                            ) : undefined
                          }
                          onSaved={() => {
                            finishedReport.set(undefined);
                            props.onRefreshPackage();
                          }}
                          onOpenExternal={openExternal}
                        />
                      )}
                    </If>
                  </View>
                );
              })()}
            </If>

            {/* D3. OI Subtasks Management View */}
            <If when={selection().kind === "subtasks"}>
              <View style={{ flex: 1, minHeight: 0 }}>
                <View
                  style={{
                    minHeight: 52,
                    paddingLeft: SPACE[20],
                    paddingRight: SPACE[16],
                    paddingTop: SPACE[8],
                    paddingBottom: SPACE[8],
                    borderBottomWidth: 1,
                    flexDirection: "row",
                    alignItems: "center",
                    justifyContent: "space-between",
                    gap: SPACE[8],
                  }}
                  class="border-border bg-background/30"
                >
                  <View style={{ gap: SPACE[2] }}>
                    <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[8] }}>
                      <Icon name="listOrdered" size="sm" tone="secondary-foreground" />
                      <Text style={{ fontSize: FONT_SIZE.body, fontWeight: FONT_WEIGHT.semibold }} class="text-foreground">
                        {t("build.subtasks.title")}
                      </Text>
                      <If when={totalSubtaskScore() != null}>
                        <Text style={{ fontSize: FONT_SIZE.caption, fontWeight: FONT_WEIGHT.semibold, fontVariant: ["tabular-nums"] }} class="text-secondary-foreground">
                          {t("build.subtasks.totalScore", { score: totalSubtaskScore()! })}
                        </Text>
                      </If>
                      <If when={totalSubtaskScore() != null && totalSubtaskScore() !== 100}>
                        <Text style={{ fontSize: FONT_SIZE.caption }} class="text-chart-4">
                          {t("build.subtasks.scoreNot100")}
                        </Text>
                      </If>
                    </View>
                  </View>
                  <If when={!props.readOnly && subtasks().length > 0}>
                    <Btn
                      testID="add-subtask-btn"
                      label={t("build.subtasks.add")}
                      icon="plus"
                      variant="primary"
                      onPress={() => {
                        editingSubtask.set(undefined);
                        subtaskModalOpen.set(true);
                      }}
                    />
                  </If>
                </View>

                <If
                  when={subtasks().length > 0}
                  else={
                    <EmptyState
                      icon="listOrdered"
                      title={t("build.subtasks.emptyTitle")}
                      hint={t("build.subtasks.emptyHint")}
                      action={
                        !props.readOnly ? (
                          <Btn
                            testID="empty-add-subtask-btn"
                            label={t("build.subtasks.add")}
                            icon="plus"
                            variant="primary"
                            size="md"
                            onPress={() => {
                              editingSubtask.set(undefined);
                              subtaskModalOpen.set(true);
                            }}
                          />
                        ) : undefined
                      }
                    />
                  }
                >
                  <Scroll style={{ flex: 1 }} contentStyle={{ padding: SPACE[20], gap: SPACE[8] }}>
                    <Each of={subtasks()} key={(s) => s.id}>
                      {(sub) => (
                        <View
                          testID={`subtask-row-${sub().id}`}
                          style={{
                            paddingLeft: SPACE[12],
                            paddingRight: SPACE[12],
                            paddingTop: SPACE[12],
                            paddingBottom: SPACE[12],
                            borderRadius: RADIUS.md,
                            borderWidth: 1,
                            flexDirection: "row",
                            alignItems: "center",
                            justifyContent: "space-between",
                            gap: SPACE[12],
                          }}
                          class="border-border bg-card"
                        >
                          <View style={{ flex: 1, minWidth: 0, gap: SPACE[2] }}>
                            <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[8] }}>
                              <Text style={{ fontSize: FONT_SIZE.body, fontWeight: FONT_WEIGHT.semibold, fontVariant: ["tabular-nums"] }} class="text-foreground">
                                {t("build.table.subtaskHeader", { id: sub().id })}
                              </Text>
                              {/* token-exempt: compact subtask badge height */}
                              <View style={{ height: 20, paddingLeft: SPACE[6], paddingRight: SPACE[6], borderRadius: RADIUS.xs, justifyContent: "center" }} class="bg-muted">
                                <Text style={{ fontSize: FONT_SIZE.caption, fontWeight: FONT_WEIGHT.semibold, fontVariant: ["tabular-nums"] }} class="text-foreground">
                                  {t("build.tree.subtasksPts", { score: sub().score })}
                                </Text>
                              </View>
                              <Text style={{ fontSize: FONT_SIZE.caption }} class="text-secondary-foreground">
                                {sub().scoring === "sum" ? t("build.subtasks.scoringSum") : t("build.subtasks.scoringMin")}
                              </Text>
                            </View>
                            <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[8] }}>
                              <Text style={{ fontSize: FONT_SIZE.caption, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
                                {t("build.subtasks.testCount", { count: sub().test_count })}
                              </Text>
                              <If when={(sub().depends?.length ?? 0) > 0}>
                                <Text style={{ fontSize: FONT_SIZE.caption, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
                                  {t("build.subtasks.dependsOn", { list: sub().depends!.map((d) => t("build.table.subtaskHeader", { id: d })).join(isZh() ? "、" : ", ") })}
                                </Text>
                              </If>
                            </View>
                          </View>

                          <If when={!props.readOnly}>
                            <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[4], flexShrink: 0 }}>
                              <IconBtn
                                testID={`edit-subtask-${sub().id}`}
                                icon="pencil"
                                tone="muted-foreground"
                                onPress={() => {
                                  editingSubtask.set(sub());
                                  subtaskModalOpen.set(true);
                                }}
                              />
                              <IconBtn
                                testID={`delete-subtask-${sub().id}`}
                                icon="trash"
                                tone="muted-foreground"
                                onPress={() => pendingDelete.set({ kind: "subtask", id: sub().id })}
                              />
                            </View>
                          </If>
                        </View>
                      )}
                    </Each>
                  </Scroll>
                </If>
              </View>
            </If>

            {/* E. Checker Detail View (P1-4 unified header) */}
            <If when={selection().kind === "checker"}>
              {(() => {
                const checkerDropdown = () => (
                  <AppMenu
                    align="end"
                    minWidth={200}
                    entries={[
                      ...BUILTIN_CHECKERS.map((c) => ({
                        id: c,
                        label: c,
                        checked: (inspection()?.checker || "wcmp") === c,
                      })),
                      {
                        id: "checker.cpp",
                        label: t("build.programs.customChecker"),
                        checked: inspection()?.checker === "checker.cpp",
                      },
                    ]}
                    onSelect={(id) => handleSetChecker(id)}
                    trigger={
                      <View
                        testID="checker-select-trigger"
                        class="border border-border bg-card hover:bg-muted"
                        style={{
                          height: CONTROL.md,
                          paddingLeft: SPACE[12],
                          paddingRight: SPACE[8],
                          borderRadius: RADIUS.sm,
                          flexDirection: "row",
                          alignItems: "center",
                          gap: SPACE[6],
                        }}
                      >
                        <Text style={{ fontSize: FONT_SIZE.caption, fontWeight: FONT_WEIGHT.semibold }} class="text-foreground">
                          {inspection()?.checker || "wcmp"}
                        </Text>
                        <Icon name="chevronDown" size="sm" tone="muted-foreground" />
                      </View>
                    }
                  />
                );

                return (
                  <View style={{ flex: 1, minHeight: 0 }}>
                    <If
                      when={inspection()?.checker === "checker.cpp" ? inspection()?.files?.custom_checker : undefined}
                      else={
                        <View style={{ flex: 1, minHeight: 0 }}>
                          <View
                            style={{
                              minHeight: 52,
                              paddingLeft: SPACE[20],
                              paddingRight: SPACE[16],
                              paddingTop: SPACE[8],
                              paddingBottom: SPACE[8],
                              borderBottomWidth: 1,
                              flexDirection: "row",
                              alignItems: "center",
                              justifyContent: "space-between",
                              gap: SPACE[8],
                            }}
                            class="border-border bg-background/30"
                          >
                            <View style={{ gap: SPACE[2] }}>
                              <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[8] }}>
                                <Icon name="scale" size="sm" tone="secondary-foreground" />
                                <Text style={{ fontSize: FONT_SIZE.body, fontWeight: FONT_WEIGHT.semibold }} class="text-foreground">
                                  {t("build.tree.checker")}
                                </Text>
                              </View>
                              <Text style={{ fontSize: FONT_SIZE.caption }} class="text-muted-foreground">
                                {t("build.programs.builtinCheckerActive", { name: inspection()?.checker || "wcmp" })}
                              </Text>
                            </View>
                            {checkerDropdown()}
                          </View>
                          <View style={{ padding: SPACE[24], maxWidth: 560, gap: SPACE[8] }}>
                            <Text style={{ fontSize: FONT_SIZE.body, lineHeight: LINE_HEIGHT.body }} class="text-secondary-foreground">
                              {t("build.programs.builtinCheckerHint")}
                            </Text>
                          </View>
                        </View>
                      }
                    >
                      {(rel) => (
                        <InlineFileEditor
                          problemId={props.problemId}
                          relPath={rel()}
                          readOnly={props.readOnly}
                          refreshKey={fileRefreshKey()}
                          title={t("build.tree.checker")}
                          icon="scale"
                          extraActions={checkerDropdown()}
                          onSaved={() => {
                            finishedReport.set(undefined);
                            props.onRefreshPackage();
                          }}
                          onOpenExternal={openExternal}
                        />
                      )}
                    </If>
                  </View>
                );
              })()}
            </If>

            {/* F. Generator Detail View (P1-4 unified header) */}
            <If when={selection().kind === "generator"}>
              {(() => {
                const file = () =>
                  selection().kind === "generator" ? (selection() as { kind: "generator"; file: string }).file : "";
                return (
                  <View style={{ flex: 1, minHeight: 0 }}>
                    <InlineFileEditor
                      problemId={props.problemId}
                      relPath={`generators/${file()}`}
                      readOnly={props.readOnly}
                      refreshKey={fileRefreshKey()}
                      title={logicalSolutionName(file())}
                      icon="sparkles"
                      extraActions={
                        !props.readOnly ? (
                          <IconBtn
                            testID="generator-delete-btn"
                            icon="trash"
                            tone="muted-foreground"
                            tooltip={tip(t("build.menu.deleteGenerator"))}
                            onPress={() => {
                              const f = file();
                              const stem = logicalSolutionName(f);
                              pendingDelete.set({
                                kind: "generator",
                                file: f,
                                stem,
                                refBatchCount: countReferencingBatches(stem),
                              });
                            }}
                          />
                        ) : undefined
                      }
                      onSaved={() => {
                        finishedReport.set(undefined);
                        props.onRefreshPackage();
                      }}
                      onOpenExternal={openExternal}
                    />
                  </View>
                );
              })()}
            </If>

            {/* G. Solution Detail View (P1-4 unified header) */}
            <If when={selection().kind === "solution"}>
              {(() => {
                const solName = () =>
                  selection().kind === "solution" ? (selection() as { kind: "solution"; name: string }).name : "";
                const spec = () => solutionSpecs().find((s) => s.name === solName());
                const role = () => (spec() ? solutionRole(spec()!) : "alt");
                const roleLabel = () =>
                  role() === "main"
                    ? t("build.role.main")
                    : role() === "alt"
                      ? t("build.role.alt")
                      : t("build.role.wrong");

                return (
                  <View style={{ flex: 1, minHeight: 0 }}>
                    <If when={spec()?.source_rel}>
                      {(rel) => (
                        <InlineFileEditor
                          problemId={props.problemId}
                          relPath={rel()}
                          readOnly={props.readOnly}
                          refreshKey={fileRefreshKey()}
                          title={solName()}
                          icon={role() === "main" ? "crown" : "code"}
                          badge={roleLabel()}
                          extraActions={
                            !props.readOnly && spec() ? (
                              <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[6] }}>
                                <AppMenu
                                  align="end"
                                  minWidth={184}
                                  entries={[
                                    ...(!spec()!.is_main
                                      ? [{ id: "set_main", label: t("build.programs.setMain"), icon: "crown" as IconName }]
                                      : []),
                                    ...EXPECT_PRESETS.map((preset) => ({
                                      id: `expect:${preset.join(",")}`,
                                      label: `${t("build.programs.expectPrefix")} ${preset.join(" / ")}`,
                                      checked: spec()!.expected.join(",") === preset.join(","),
                                      disabled: spec()!.is_main && preset.join(",") !== "AC",
                                    })),
                                  ]}
                                  onSelect={(id) => {
                                    if (id === "set_main") {
                                      handleSolutionConfig(solName(), true, ["AC"]);
                                    } else if (id.startsWith("expect:")) {
                                      const exp = id.slice("expect:".length).split(",");
                                      handleSolutionConfig(solName(), null, exp);
                                    }
                                  }}
                                  trigger={
                                    <View
                                      testID={`solution-config-trigger-${solName()}`}
                                      class="border border-border bg-card hover:bg-muted"
                                      style={{
                                        height: CONTROL.md,
                                        paddingLeft: SPACE[8],
                                        paddingRight: SPACE[8],
                                        borderRadius: RADIUS.sm,
                                        flexDirection: "row",
                                        alignItems: "center",
                                        gap: SPACE[4],
                                      }}
                                    >
                                      <Text style={{ fontSize: FONT_SIZE.caption, fontWeight: FONT_WEIGHT.semibold }} class="text-foreground">
                                        {`${t("build.programs.expectPrefix")} ${spec()!.expected.join("/")}`}
                                      </Text>
                                      <Icon name="chevronDown" size="sm" tone="muted-foreground" />
                                    </View>
                                  }
                                />
                                <If when={subtasks().length > 0 && !spec()!.is_main}>
                                  <Btn
                                    testID={`solution-scoring-btn-${solName()}`}
                                    label={
                                      formatScoreExpect(spec()!.score)
                                        ? t("build.scoring.buttonValue", { score: formatScoreExpect(spec()!.score)! })
                                        : t("build.scoring.button")
                                    }
                                    icon="scale"
                                    variant="outline"
                                    onPress={() => scoringModalSolution.set(solName())}
                                  />
                                </If>
                                <IconBtn
                                  testID="solution-delete-btn"
                                  icon="trash"
                                  tone="muted-foreground"
                                  tooltip={tip(t("build.menu.deleteSolution"))}
                                  onPress={() => pendingDelete.set({ kind: "solution", name: solName() })}
                                />
                              </View>
                            ) : undefined
                          }
                          onSaved={() => {
                            finishedReport.set(undefined);
                            props.onRefreshPackage();
                          }}
                          onOpenExternal={openExternal}
                        />
                      )}
                    </If>
                  </View>
                );
              })()}
            </If>

            {/* 内存检查 (§5.6): one quiet line when clean; object-named findings when not */}
            <If when={selection().kind === "sanitizer"}>
              <View testID="build-sanitizer-view" style={{ flex: 1, minHeight: 0 }}>
                <View
                  style={{ height: 46, paddingLeft: SPACE[20], paddingRight: SPACE[16], borderBottomWidth: 1, flexDirection: "row", alignItems: "center", flexShrink: 0 }}
                  class="border-border"
                >
                  <Text style={{ fontSize: FONT_SIZE.strong, fontWeight: FONT_WEIGHT.semibold }} class="text-foreground">
                    {t("build.tree.memoryCheck")}
                  </Text>
                </View>
                <Scroll style={{ flex: 1 }} contentStyle={{ paddingTop: SPACE[16], paddingBottom: SPACE[24], paddingLeft: SPACE[20], paddingRight: SPACE[20], gap: SPACE[12] }}>
                  <If
                    when={sanitizerReport()}
                    else={
                      <Text style={{ fontSize: FONT_SIZE.body }} class="text-secondary-foreground">
                        {t("build.sanitize.running")}
                      </Text>
                    }
                  >
                    {(rep) => {
                      const mainName = () => solutionSpecs().find((sp) => sp.is_main)?.name;
                      return (
                        <View style={{ gap: SPACE[12], maxWidth: 640 }}>
                          <If when={rep().status === "passed" && (rep().findings?.length ?? 0) === 0}>
                            <Text testID="sanitizer-clean" style={{ fontSize: FONT_SIZE.body }} class="text-secondary-foreground">
                              {t("build.sanitize.noFindings")}
                            </Text>
                          </If>
                          <If when={rep().status === "unsupported"}>
                            <Text style={{ fontSize: FONT_SIZE.body, lineHeight: LINE_HEIGHT.body }} class="text-secondary-foreground">
                              {rep().unsupported_reason || t("build.sanitize.unsupported")}
                            </Text>
                          </If>
                          <Each of={rep().compile_failures ?? []} key={(cf) => `${cf.target}:${cf.source}`}>
                            {(cf) => (
                              <View style={{ gap: SPACE[6] }}>
                                <Text style={{ fontSize: FONT_SIZE.body, fontWeight: FONT_WEIGHT.semibold }} class="text-destructive">
                                  {t("status.compileFailed", { file: sanitizerTargetName(cf().target, mainName()) })}
                                </Text>
                                <View style={{ padding: SPACE[8], borderRadius: RADIUS.sm }} class="bg-muted">
                                  <Text style={{ fontSize: FONT_SIZE.caption, fontFamily: MONO, lineHeight: LINE_HEIGHT.body }} class="text-secondary-foreground">
                                    {cf().stderr.slice(0, 1200)}
                                  </Text>
                                </View>
                              </View>
                            )}
                          </Each>
                          <Each of={rep().findings ?? []} key={(f, i) => `${f.target}:${f.test_index ?? i}:${f.summary}`}>
                            {(f) => (
                              <View style={{ gap: SPACE[6], paddingBottom: SPACE[4] }}>
                                <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[8], flexWrap: "wrap" }}>
                                  <Text style={{ fontSize: FONT_SIZE.body, fontWeight: FONT_WEIGHT.semibold }} class="text-foreground">
                                    {sanitizerTargetName(f().target, mainName())}
                                  </Text>
                                  <If when={!!f().test_name}>
                                    <Text style={{ fontSize: FONT_SIZE.caption }} class="text-muted-foreground">
                                      {f().test_name!}
                                    </Text>
                                  </If>
                                </View>
                                <Text style={{ fontSize: FONT_SIZE.body, lineHeight: LINE_HEIGHT.body }} class="text-destructive">
                                  {f().summary}
                                </Text>
                                <If when={(f().stack_summary?.length ?? 0) > 0}>
                                  <View style={{ padding: SPACE[8], borderRadius: RADIUS.sm, gap: SPACE[2] }} class="bg-muted">
                                    <Each of={f().stack_summary ?? []} key="index">
                                      {(line) => (
                                        <Text style={{ fontSize: FONT_SIZE.caption, fontFamily: MONO, lineHeight: LINE_HEIGHT.body }} class="text-secondary-foreground">
                                          {line()}
                                        </Text>
                                      )}
                                    </Each>
                                  </View>
                                </If>
                              </View>
                            )}
                          </Each>
                        </View>
                      );
                    }}
                  </If>
                </Scroll>
              </View>
            </If>

            {/* 验题人代码 (§5.6): 判定 / 通过 / 最慢用时 / 与标程相比; 「加入解法」 on row hover */}
            <If when={selection().kind === "external"}>
              <View testID="build-external-view" style={{ flex: 1, minHeight: 0 }}>
                <View
                  style={{ height: 46, paddingLeft: SPACE[20], paddingRight: SPACE[16], borderBottomWidth: 1, flexDirection: "row", alignItems: "center", justifyContent: "space-between", gap: SPACE[8], flexShrink: 0 }}
                  class="border-border"
                >
                  <Text style={{ fontSize: FONT_SIZE.strong, fontWeight: FONT_WEIGHT.semibold }} class="text-foreground">
                    {t("build.tree.testerCode", { count: testerSubs().length })}
                  </Text>
                  <If when={!!externalMarkdown()}>
                    <Btn
                      testID="build-external-export-md"
                      label={externalExported.active() ? t("build.external.exported") : t("build.external.exportMd")}
                      icon={externalExported.active() ? "check" : undefined}
                      variant="ghost"
                      onPress={() => void exportExternalMarkdown()}
                    />
                  </If>
                </View>

                <Scroll style={{ flex: 1 }} contentStyle={{ paddingTop: SPACE[8], paddingBottom: SPACE[24], paddingLeft: SPACE[12], paddingRight: SPACE[12] }}>
                  <If
                    when={testerSubs().length > 0}
                    else={
                      <Text style={{ fontSize: FONT_SIZE.body, padding: SPACE[8] }} class="text-secondary-foreground">
                        {t("build.external.running")}
                      </Text>
                    }
                  >
                    <View style={{ height: CONTROL.lg, paddingLeft: SPACE[8], paddingRight: SPACE[8], flexDirection: "row", alignItems: "center", gap: SPACE[12] }}>
                      <If when={showTesterColumn()}>
                        <Text style={{ width: 88, fontSize: FONT_SIZE.caption }} class="text-muted-foreground">{t("build.external.colTester")}</Text>
                      </If>
                      <Text style={{ flex: 1, fontSize: FONT_SIZE.caption }} class="text-muted-foreground">{t("build.external.colFile")}</Text>
                      <Text style={{ width: 64, fontSize: FONT_SIZE.caption }} class="text-muted-foreground">{t("build.external.colVerdict")}</Text>
                      <Text style={{ width: 72, fontSize: FONT_SIZE.caption, textAlign: "right" }} class="text-muted-foreground">{t("build.external.colPassed")}</Text>
                      <Text style={{ width: 80, fontSize: FONT_SIZE.caption, textAlign: "right" }} class="text-muted-foreground">{t("build.external.colTime")}</Text>
                      <Text style={{ width: 112, fontSize: FONT_SIZE.caption }} class="text-muted-foreground">{t("build.external.colRatio")}</Text>
                      <View style={{ width: 28 }} />
                    </View>
                    <Each of={testerSubs()} key={(s) => `${s.tester ?? ""}:${s.file_path}`}>
                      {(sub, idx) => {
                        const adopted = () => !!adoptedExternalPaths()[sub().file_path];
                        const failText = () => {
                          if (sub().compile_error) return t("build.sanitize.compileFailed");
                          const ff = sub().first_failure;
                          if (!ff) return undefined;
                          return `${ff.test_name}：${verdictWords(ff.verdict)}${ff.message ? `　${ff.message}` : ""}`;
                        };
                        return (
                          <View
                            class="group border-t border-border hover:bg-muted"
                            style={{ height: ROW.table, paddingLeft: SPACE[8], paddingRight: SPACE[8], borderRadius: RADIUS.sm, flexDirection: "row", alignItems: "center", gap: SPACE[12] }}
                          >
                            <If when={showTesterColumn()}>
                              <Text style={{ width: 88, fontSize: FONT_SIZE.body, numberOfLines: 1, textOverflow: "ellipsis" }} class="text-secondary-foreground">
                                {sub().tester ?? ""}
                              </Text>
                            </If>
                            <Text style={{ flex: 1, fontSize: FONT_SIZE.body, fontFamily: MONO, numberOfLines: 1, textOverflow: "ellipsis" }} class="text-foreground">
                              {sub().file_name}
                            </Text>
                            <Text
                              tooltip={failText() ? tip(failText()!) : undefined}
                              style={{ width: 64, fontSize: FONT_SIZE.body, fontVariant: ["tabular-nums"] }}
                              class={sub().verdict === "AC" ? "text-foreground" : "text-secondary-foreground"}
                            >
                              {sub().compile_error ? "CE" : sub().verdict}
                            </Text>
                            <Text style={{ width: 72, fontSize: FONT_SIZE.body, fontVariant: ["tabular-nums"], textAlign: "right" }} class="text-secondary-foreground">
                              {sub().max_score != null ? `${sub().total_score ?? 0} / ${sub().max_score}` : `${sub().passed_tests} / ${sub().total_tests}`}
                            </Text>
                            <Text style={{ width: 80, fontSize: FONT_SIZE.body, fontVariant: ["tabular-nums"], textAlign: "right" }} class="text-secondary-foreground">
                              {`${sub().max_time_ms} ms`}
                            </Text>
                            <Text style={{ width: 112, fontSize: FONT_SIZE.body }} class="text-secondary-foreground">
                              {stdRatioWords(sub().ratio_vs_std)}
                            </Text>
                            <View style={{ width: 28, alignItems: "flex-end" }} class={adopted() ? "" : "opacity-0 group-hover:opacity-100"}>
                              <IconBtn
                                testID={`external-adopt-${idx()}`}
                                icon={adopted() ? "check" : "plus"}
                                size="sm"
                                tone="secondary-foreground"
                                disabled={props.readOnly || adopted()}
                                tooltip={tip(adopted() ? t("build.external.adopted") : t("build.external.adopt"))}
                                onPress={() => adoptExternal(sub())}
                              />
                            </View>
                          </View>
                        );
                      }}
                    </Each>
                  </If>
                </Scroll>
              </View>
            </If>
          </View>
        </View>

        {/* Modals */}
        <CreateSourceModal
          open={createModalKind() !== undefined}
          kind={createModalKind() ?? "solution"}
          problemId={props.problemId}
          onClose={() => createModalKind.set(undefined)}
          onCreated={(relPath) => {
            finishedReport.set(undefined);
            props.onRefreshPackage();
            openFileInTree(relPath);
          }}
        />
        <AddGeneratorBatchModal
          open={addGenModalOpen()}
          problemId={props.problemId}
          generators={inspection()?.files?.generators ?? []}
          subtasks={subtasks()}
          initialGenerator={addGenInitial()}
          onClose={() => {
            addGenModalOpen.set(false);
            addGenInitial.set(undefined);
          }}
          onAdded={() => {
            finishedReport.set(undefined);
            props.onRefreshPackage();
          }}
        />
        <SubtaskModal
          open={subtaskModalOpen()}
          problemId={props.problemId}
          editing={editingSubtask()}
          existingSubtasks={subtasks()}
          onClose={() => {
            subtaskModalOpen.set(false);
            editingSubtask.set(undefined);
          }}
          onSaved={() => {
            finishedReport.set(undefined);
            props.onRefreshPackage();
          }}
        />
        <SolutionScoringModal
          open={scoringModalSolution() !== undefined}
          problemId={props.problemId}
          solutionName={scoringModalSolution() ?? ""}
          initialScore={solutionSpecs().find((s) => s.name === scoringModalSolution())?.score}
          initialSubtasks={solutionSpecs().find((s) => s.name === scoringModalSolution())?.subtasks}
          subtasks={subtasks()}
          onClose={() => scoringModalSolution.set(undefined)}
          onSaved={() => {
            finishedReport.set(undefined);
            props.onRefreshPackage();
          }}
        />
        <ConfirmDeleteRuleModal
          open={deleteRuleIndex() !== undefined}
          testCount={countTestsInRule(inspection()?.expanded_tests, deleteRuleIndex())}
          onClose={() => deleteRuleIndex.set(undefined)}
          onConfirm={handleConfirmDeleteRule}
        />
        <ConfirmPackageDeleteModal
          target={pendingDelete()}
          onClose={() => pendingDelete.set(undefined)}
          onConfirm={handleConfirmPackageDelete}
        />
      </If>
    </View>
  );
}

/** One setting on the 题型 page: a label, the control, an optional plain-language hint. */
function TypeSetting(props: { label: string; hint?: string; children?: JSX.Element }) {
  return (
    <View style={{ gap: SPACE[8] }}>
      <Text style={{ fontSize: FONT_SIZE.body, fontWeight: FONT_WEIGHT.semibold }} class="text-foreground">
        {props.label}
      </Text>
      <View style={{ flexDirection: "row" }}>{props.children}</View>
      <If when={props.hint}>
        <Text style={{ fontSize: FONT_SIZE.caption, lineHeight: LINE_HEIGHT.body }} class="text-muted-foreground">
          {props.hint}
        </Text>
      </If>
    </View>
  );
}

// ── Left Object Tree subcomponents ───────────────────────────────────────────

function TreeSectionHeader(props: { label: string; action?: ReturnType<typeof IconBtn> | ReturnType<typeof AppMenu> }) {
  return (
    <View
      style={{
        height: CONTROL.sm,
        paddingLeft: SPACE[8],
        paddingRight: SPACE[2],
        flexDirection: "row",
        alignItems: "center",
        justifyContent: "space-between",
      }}
    >
      {/* P2: 11px muted regular weight for clear hierarchy */}
      <Text style={{ fontSize: FONT_SIZE.caption, fontWeight: FONT_WEIGHT.regular, letterSpacing: 0.2 }} class="text-muted-foreground">
        {props.label}
      </Text>
      {props.action}
    </View>
  );
}

function TreeRow(props: {
  icon: IconName;
  label: string;
  sub?: string;
  active: boolean;
  statusDot?: "fail";
  onPress: () => void;
  onContextMenu?: (e: ContextMenuEvent) => void;
  testID?: string;
}) {
  return (
    <View
      testID={props.testID}
      onPress={props.onPress}
      onContextMenu={props.onContextMenu}
      class={props.active ? "bg-muted" : "hover:bg-muted/60"}
      style={{
        height: CONTROL.md,
        paddingLeft: SPACE[8],
        paddingRight: SPACE[8],
        borderRadius: RADIUS.sm,
        flexDirection: "row",
        alignItems: "center",
        gap: SPACE[8],
      }}
    >
      <Icon
        name={props.icon}
        size="sm"
        tone={props.active ? "foreground" : "secondary-foreground"}
      />
      <Text
        style={{
          flex: 1,
          fontSize: FONT_SIZE.body,
          fontWeight: FONT_WEIGHT.semibold,
          fontVariant: ["tabular-nums"],
          numberOfLines: 1,
          textOverflow: "ellipsis",
        }}
        class={props.active ? "text-foreground" : "text-secondary-foreground"}
      >
        {props.label}
      </Text>
      <If when={!!props.sub}>
        <Text style={{ fontSize: FONT_SIZE.caption, flexShrink: 0 }} class="text-muted-foreground">
          {props.sub!}
        </Text>
      </If>
      {/* P1-6: small semantic dot instead of checkmark ✓ */}
      <If when={props.statusDot === "fail"}>
        <View style={{ width: 6, height: 6, borderRadius: RADIUS.xs, flexShrink: 0 }} class="bg-destructive" />
      </If>
    </View>
  );
}

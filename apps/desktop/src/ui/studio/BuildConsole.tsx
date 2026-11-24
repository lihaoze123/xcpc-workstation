// Local Polygon Authoring Console (Phase 3B polished edition):
// - Zero stacked toolbars: Authoring controls live directly in StudioView's TitleBar trailing slot
// - P0-2 Quiet Results Table ("only anomalies speak"): borderless expected cells, solid red/amber anomalies only, 2px primary selection ring
// - P0-3 Zero layout push: Checks & Filter render as anchored overlay popovers; TitleBar cluster fixed; Esc closes popovers first
// - P1-1 Fluid solution columns (120–180px), 560px (~72ch) small preview max-width
// - P1-4 Unified Detail Page Grammar across all objects + inline generator batch editor + dropdown test selector
// - P1-5 State consistency: preserves untouched summaries (`旧`) and full-run safety margin across subset runs

import { cleanup, colorScheme, computed, effect, Scroll, signal, Text, untracked, View, type ContextMenuEvent } from "nsh";
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
import { dialog, goSpace, onExternalChange, preview, type UiError } from "../store";
import { MONO, popoverShadow, token } from "../theme";
import { CellDiagnosticDrawer, InlineFileEditor } from "./BuildDetailPane";
import {
  buildPackageModeBadges,
  BUILTIN_CHECKERS,
  canInlineEditTest,
  cellVisualStyle,
  countTestsInRule,
  EXPECT_PRESETS,
  formatAcVsTleMargin,
  formatBytesCompact,
  formatScoreExpect,
  formatScoreRatioPercent,
  formatScoreValue,
  formatSolutionConclusion,
  formatSolutionScoreSummary,
  formatStdTimeRatio,
  groupMatrixRowsBySubtask,
  groupTestRows,
  humanizeCheckText,
  isCellVerdictAnomaly,
  isMissingCompilerError,
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
  type SolutionSummaryData,
  type SubtaskSummaryData,
  type TestSummaryData,
} from "./buildLogic";
import {
  formatExternalRatio,
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

export interface BuildConsoleController {
  triggerPrimaryRun(): void;
  closePopovers(): boolean;
  statusPill(): { label: string; bg: string; fg: string; dotColor?: string; expanded: boolean };
  toggleChecks(): void;
  canFilter(): boolean;
  filterCount(): number;
  toggleFilter(): void;
  running(): boolean;
  runningSanitize(): boolean;
  runningExternal(): boolean;
  sanitizerStatus(): "passed" | "failed" | "unsupported" | "not_run";
  quickRunDone(): boolean;
  startRun(mode: "debug" | "verify" | "sanitize"): void;
  pickAndRunExternal(): void;
  cancelRun(): void;
}

/**
 * Authoring actions rendered directly in StudioView's TitleBar trailing slot:
 * - Status pill: neutral surface + small semantic dot (green tint ONLY when ready for contest)
 * - Phase 3D: UB Check & Trial Run External buttons (fixed slots, zero layout shift)
 * - Filter button: stays present in fixed slot (disabled when not on Results view)
 * - Primary CTA: constant "验证" label (swaps in-place with outline "取消" while running)
 */
export function AuthoringTitleBarControls(props: { ctrl: BuildConsoleController; readOnly: boolean }) {
  const pill = () => props.ctrl.statusPill();
  const running = () => props.ctrl.running();
  const filterCount = () => props.ctrl.filterCount();
  const canFilter = () => props.ctrl.canFilter();
  const sanStatus = () => props.ctrl.sanitizerStatus();
  const sanDotColor = () => {
    const dark = colorScheme() === "dark";
    const s = sanStatus();
    if (s === "passed") return dark ? "#4ade80" : "#16a34a";
    if (s === "failed") return dark ? "#f87171" : "#dc2626";
    if (s === "unsupported") return dark ? "#fbbf24" : "#d97706";
    return undefined;
  };

  return (
    <View style={{ flexDirection: "row", alignItems: "center", gap: 6, flexShrink: 0 }}>
      {/* Single Status Pill (neutral surface + small semantic dot; green tint only when ready) */}
      <View
        testID="build-single-status-pill"
        onPress={() => props.ctrl.toggleChecks()}
        style={{
          height: 26,
          minWidth: 102,
          paddingLeft: 9,
          paddingRight: 7,
          borderRadius: 6,
          backgroundColor: pill().bg,
          flexDirection: "row",
          alignItems: "center",
          justifyContent: "center",
          gap: 6,
          flexShrink: 0,
        }}
      >
        <If when={!!pill().dotColor}>
          <View style={{ width: 6, height: 6, borderRadius: 3, backgroundColor: pill().dotColor, flexShrink: 0 }} />
        </If>
        <Text style={{ fontSize: 12, fontWeight: 500, color: pill().fg, fontVariant: ["tabular-nums"], numberOfLines: 1 }}>
          {pill().label}
        </Text>
        <Icon name={pill().expanded ? "chevronDown" : "chevronRight"} size={12} tone="secondary-foreground" />
      </View>

      {/* Phase 3D: UB Check button */}
      <View style={{ position: "relative", flexShrink: 0 }}>
        <Btn
          testID="build-sanitize-btn"
          label={t("build.sanitize.button")}
          icon="shieldCheck"
          variant="outline"
          disabled={props.readOnly || running()}
          tooltip={tip(t("build.sanitize.tooltip"))}
          onPress={() => props.ctrl.startRun("sanitize")}
        />
        <If when={!!sanDotColor()}>
          <View
            style={{
              position: "absolute",
              top: 3,
              right: 3,
              width: 6,
              height: 6,
              borderRadius: 3,
              backgroundColor: sanDotColor(),
            }}
          />
        </If>
      </View>

      {/* Phase 3D: Trial Run External Solutions button */}
      <Btn
        testID="build-external-btn"
        label={t("build.external.button")}
        icon="play"
        variant="outline"
        disabled={props.ctrl.runningExternal()}
        tooltip={tip(t("build.external.tooltip"))}
        onPress={() => props.ctrl.pickAndRunExternal()}
      />

      {/* Filter button — stays in place (disabled when on detail view so status pill never shifts!) */}
      <Btn
        testID="build-filter-toggle"
        label={filterCount() > 0 ? `${t("build.filter.button")} ${filterCount()}` : t("build.filter.button")}
        icon="list"
        variant={filterCount() > 0 ? "outline" : "ghost"}
        disabled={!canFilter()}
        onPress={() => props.ctrl.toggleFilter()}
      />

      {/* Fixed-width primary CTA slot: constant "验证" <-> outline "取消" / Tier 3 "✓ 完成" in-place */}
      <View style={{ width: 108, flexDirection: "row", justifyContent: "flex-end", alignItems: "center" }}>
        <If
          when={!running()}
          else={
            <Btn
              testID="build-cancel-btn"
              label={t("common.cancel")}
              icon="x"
              variant="outline"
              onPress={() => props.ctrl.cancelRun()}
              style={{ width: 108 }}
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
                style={{ width: 108 }}
              />
            }
          >
            <View style={{ flexDirection: "row", alignItems: "center" }}>
              <Btn
                testID="build-verify-btn"
                label={t("build.action.verify")}
                icon="play"
                variant="primary"
                disabled={props.readOnly}
                tooltip={tip(t("build.action.verify"), "mod+b")}
                onPress={() => props.ctrl.startRun("verify")}
                style={{
                  width: 82,
                  borderTopRightRadius: 0,
                  borderBottomRightRadius: 0,
                }}
              />
              <AppMenu
                align="end"
                minWidth={184}
                entries={[
                  {
                    id: "debug",
                    label:
                      filterCount() > 0
                        ? t("build.action.verifySelected", { count: filterCount() })
                        : t("build.action.quickRun"),
                    icon: "play",
                  },
                ]}
                onSelect={(id) => {
                  if (id === "debug") props.ctrl.startRun("debug");
                }}
                trigger={
                  <View
                    testID="build-verify-menu-trigger"
                    class="bg-primary hover:bg-sidebar-primary"
                    style={{
                      width: 26,
                      height: 28,
                      borderTopRightRadius: 6,
                      borderBottomRightRadius: 6,
                      borderLeftWidth: 1,
                      borderLeftColor: "rgba(255,255,255,0.18)",
                      alignItems: "center",
                      justifyContent: "center",
                      opacity: props.readOnly ? 0.5 : 1,
                    }}
                  >
                    <Icon name="chevronDown" size={13} tone="primary-foreground" />
                  </View>
                }
              />
            </View>
          </If>
        </If>
      </View>
    </View>
  );
}

type TreeSelection =
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
  readOnly: boolean;
  inspectionResult: PackageInspectionResultJs | undefined;
  onInitPackage?: () => void;
  onRefreshPackage: () => void;
  onSwitchToStatement?: () => void;
  onCollapseInspector?: () => void;
  controller?: (c: BuildConsoleController | undefined) => void;
}) {
  const isZh = () => locale() === "zh-CN";

  // ── Core live state (P0-2 Single State Model) ──────────────────────────────
  const selection = signal<TreeSelection>({ kind: "results" });
  const onlyAnomalyRows = signal(false);
  const checksExpanded = signal(false);
  const passedChecksExpanded = signal(false);
  const filterOpen = signal(false);
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

  // Phase 3D: Isolated Sanitizer (UB Check) & External Trial Run state
  const sanitizerReport = signal<SanitizerReportData | undefined>(undefined);
  const externalReport = signal<ExternalProblemReportData | undefined>(undefined);
  const externalMarkdown = signal<string | undefined>(undefined);
  const externalRunning = signal(false);
  const externalExported = createTransientFlag(1600);
  const adoptedExternalPaths = signal<Record<string, boolean>>({});

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
        filterOpen.set(false);
        onlyAnomalyRows.set(false);
        checksExpanded.set(false);
        dirtyUndoNotice.set(undefined);
        notice.set(undefined);
        finishedReport.set(undefined);
        liveCells.set([]);
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
      if (rep.solutions && rep.solutions.length > 0) {
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

  // Poll background build runner at 120ms while active, streaming liveCells in real time (P1-8)
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
    pollTimer = setInterval(() => {
      const next = preview.packageBuildStatus();
      buildStatus.set(next);
      if (next.liveCellsJson && next.slug === untracked(() => props.problemId)) {
        try {
          liveCells.set(JSON.parse(next.liveCellsJson) as MatrixCellData[]);
        } catch {
          // ignore
        }
      }
      if (!next.running) {
        stopPolling();
        if (next.seq !== lastSeenSeq || next.reportJson || next.error) {
          lastSeenSeq = next.seq;
          liveCells.set([]);
          if (next.reportJson && next.slug === untracked(() => props.problemId)) {
            if (next.mode === "sanitize") {
              const san = parseSanitizerReport(next.reportJson);
              if (san) sanitizerReport.set(san);
            } else {
              try {
                const rep = JSON.parse(next.reportJson) as BuildReportData;
                finishedReport.set(rep);
                // Tier 3 (§6.5): brief in-place confirmation for partial / quick runs without claiming full verification.
                if (!next.cancelled && !next.error && (rep.mode === "debug" || rep.subset)) {
                  quickRunDone.trigger();
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
            props.onRefreshPackage();
          }
        }
      }
    }, 120) as unknown as number;
  }
  effect(() => {
    if (buildStatus().running) startPolling();
  });
  cleanup(stopPolling);

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

  const summaryBySolution = computed(() => {
    const m = new Map<string, SolutionSummaryData>();
    for (const s of activeReport()?.solutions ?? []) m.set(s.name, s);
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

  const modeBadges = computed(() => buildPackageModeBadges(inspection(), activeReport(), isZh()));
  const matrixSubtaskGroups = computed(() => groupMatrixRowsBySubtask(displayedTableRows(), subtasks()));

  const selectedCellData = computed(() => {
    const sel = selectedCell();
    if (!sel) return undefined;
    return matrixLookup().get(matrixCellKey(sel.testIndex, sel.solution));
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
    checksExpanded.set(false);
    filterOpen.set(false);
    selection.set({ kind: "testGroup", groupId, testIndex });
  }

  function startRun(mode: "debug" | "verify" | "sanitize") {
    if (props.readOnly || isRunningThis()) return;
    notice.set(undefined);
    checksExpanded.set(false);
    filterOpen.set(false);
    const hasFilter = mode !== "sanitize" && (filterTests().length > 0 || filterSolutions().length > 0);
    const effectiveMode = mode === "sanitize" ? "sanitize" : hasFilter ? "debug" : mode;
    const testsArg = hasFilter && filterTests().length > 0 ? filterTests() : null;
    const solsArg = hasFilter && filterSolutions().length > 0 ? filterSolutions() : null;
    if (mode === "sanitize") {
      selection.set({ kind: "sanitizer" });
    } else {
      liveCells.set([]);
    }
    const res = preview.startPackageBuild(props.problemId, effectiveMode, testsArg, solsArg, null);
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
    checksExpanded.set(false);
    filterOpen.set(false);
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
    externalReport.set(parseExternalProblemReport(res.reportJson));
    externalMarkdown.set(res.markdown ?? undefined);
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
    if (checksExpanded()) {
      checksExpanded.set(false);
      return true;
    }
    if (filterOpen()) {
      filterOpen.set(false);
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

  // P1-6: Neutral surface + small semantic dot; tinted green ONLY for ready-for-contest
  const statusPillStyle = computed(() => {
    const st = singleStatus();
    const dark = colorScheme() === "dark";
    if (st.kind === "ready") {
      return {
        label: t("build.status.ready"),
        bg: dark ? "rgba(34, 197, 94, 0.14)" : "rgba(22, 163, 74, 0.10)",
        fg: dark ? "#4ade80" : "#15803d",
        dotColor: dark ? "#4ade80" : "#16a34a",
        expanded: checksExpanded(),
      };
    }
    const neutralBg = dark ? "rgba(255, 255, 255, 0.06)" : "rgba(24, 24, 27, 0.05)";
    const neutralFg = dark ? "#e4e4e7" : "#27272a";
    if (st.kind === "issues") {
      return {
        label: t("build.status.issues", { count: st.issueCount }),
        bg: neutralBg,
        fg: neutralFg,
        dotColor: dark ? "#f87171" : "#dc2626",
        expanded: checksExpanded(),
      };
    }
    if (st.kind === "needs_reverify") {
      return {
        label: t("build.status.needsReverify"),
        bg: neutralBg,
        fg: neutralFg,
        dotColor: dark ? "#fbbf24" : "#d97706",
        expanded: checksExpanded(),
      };
    }
    return {
      label: t("build.status.unverified"),
      bg: neutralBg,
      fg: dark ? "#a1a1aa" : "#52525b",
      dotColor: dark ? "#71717a" : "#a1a1aa",
      expanded: checksExpanded(),
    };
  });

  props.controller?.({
    triggerPrimaryRun,
    closePopovers,
    statusPill: () => statusPillStyle(),
    toggleChecks: () => {
      filterOpen.set(false);
      checksExpanded.set(!checksExpanded());
    },
    canFilter: () => selection().kind === "results",
    filterCount: () => activeFilterCount(),
    toggleFilter: () => {
      checksExpanded.set(false);
      filterOpen.set(!filterOpen());
    },
    running: () => isRunningThis(),
    runningSanitize: () => isRunningThis() && buildStatus().mode === "sanitize",
    runningExternal: () => externalRunning(),
    sanitizerStatus: () => sanitizerReport()?.status ?? "not_run",
    quickRunDone: () => quickRunDone.active(),
    startRun,
    pickAndRunExternal: () => void pickAndRunExternal(),
    cancelRun,
  });
  cleanup(() => props.controller?.(undefined));

  function openExternal(relPath: string) {
    void preview.openPackageFile(props.problemId, relPath).then((res) => {
      if (!res.ok) notice.set({ code: res.code, error: res.error, context: t("ctx.openPackageFile") });
    });
  }

  function openFileInTree(relPath: string) {
    checksExpanded.set(false);
    filterOpen.set(false);
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
  function handleAddHandTest() {
    if (props.readOnly) return;
    const res = preview.addHandTest(props.problemId, null, "hand", "");
    if (!res.ok) {
      notice.set({ code: res.code, error: res.error, context: t("ctx.updatePackage") });
      return;
    }
    finishedReport.set(undefined);
    props.onRefreshPackage();
    const freshRaw = preview.inspectPackage(props.problemId).inspectionJson;
    let targetIndex: number | undefined;
    if (freshRaw) {
      try {
        const fresh = JSON.parse(freshRaw) as PackageInspectionData;
        const found = fresh.expanded_tests.find((et) => et.rule_index === res.ruleIndex);
        if (found) targetIndex = found.index;
      } catch {
        // fallback below
      }
    }
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

  function handleCreateInteractor() {
    if (props.readOnly) return;
    const res = preview.createPackageSource(props.problemId, "interactor", "interactor");
    if (!res.ok) {
      notice.set({ code: res.code, error: res.error, context: t("ctx.updatePackage") });
      return;
    }
    finishedReport.set(undefined);
    props.onRefreshPackage();
    selection.set({ kind: "interactor" });
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
        notice.set({ code: res.code, error: res.error, context: t("ctx.updatePackage") });
        return;
      }
      finishedReport.set(undefined);
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
        {/* Global error notice (when present) */}
        <If when={notice()}>
          <View
            style={{
              paddingLeft: 14,
              paddingRight: 14,
              paddingTop: 8,
              paddingBottom: 8,
              borderBottomWidth: 1,
              flexDirection: "row",
              alignItems: "center",
              justifyContent: "space-between",
              gap: 10,
            }}
            class="border-b-border bg-muted/30"
          >
            <View style={{ flex: 1, minWidth: 0 }}>
              <ErrorDisplay error={notice()} onDismiss={() => notice.set(undefined)} />
            </View>
            <If when={isMissingCompilerError(notice()?.error)}>
              <Btn
                label={t("build.openToolchainSettings")}
                icon="settings"
                variant="outline"
                onPress={() => goSpace("settings")}
              />
            </If>
          </View>
        </If>

        {/* ── Main Split: Left Object Tree (~228pt) + Right Content Area (directly under TitleBar) ── */}
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
              paddingTop: 8,
              paddingBottom: 16,
              paddingLeft: 8,
              paddingRight: 8,
              gap: 14,
            }}
            class="border-border bg-background/30"
          >
            {/* 1. Results overview */}
            <TreeRow
              testID="tree-item-results"
              icon="table"
              label={t("build.tree.results")}
              active={selection().kind === "results"}
              onPress={() => selection.set({ kind: "results" })}
            />

            {/* 2. Validator & Checker */}
            <View style={{ gap: 2 }}>
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
              <TreeRow
                testID="tree-item-checker"
                icon="scale"
                label={t("build.tree.checker")}
                sub={inspection()?.checker || "wcmp"}
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
              <TreeRow
                testID="tree-item-interactor"
                icon="bot"
                label={t("build.tree.interactor")}
                sub={
                  (inspection()?.runs ?? 1) > 1
                    ? `×${inspection()?.runs}`
                    : inspection()?.files?.interactor
                      ? undefined
                      : t("build.tree.interactorOff")
                }
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
            </View>

            {/* 3. Solutions (uniform `预期 WA` labels, small semantic dot, no ✓) */}
            <View style={{ gap: 2 }}>
              <TreeSectionHeader
                label={t("build.tree.solutions")}
                action={
                  !props.readOnly ? (
                    <IconBtn
                      testID="tree-add-solution-btn"
                      icon="plus"
                      size={22}
                      iconSize={13}
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
                      sub={
                        role() === "main"
                          ? undefined
                          : formatScoreExpect(spec().score)
                            ? t("build.tree.subtasksPts", { score: formatScoreExpect(spec().score)! })
                            : `${t("build.programs.expectPrefix")} ${spec().expected.join("/")}`
                      }
                      active={selection().kind === "solution" && (selection() as { kind: "solution"; name: string }).name === spec().name}
                      statusDot={sum() ? (sum()!.matched ? "ok" : "fail") : undefined}
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
            <View style={{ gap: 2 }}>
              <TreeSectionHeader
                label={t("build.tree.generators")}
                action={
                  !props.readOnly ? (
                    <IconBtn
                      testID="tree-add-generator-btn"
                      icon="plus"
                      size={22}
                      iconSize={13}
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
            <View style={{ gap: 2 }}>
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
                        if (id === "hand") handleAddHandTest();
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
                          size={22}
                          iconSize={13}
                          tooltip={tip(t("build.tests.add"))}
                        />
                      }
                    />
                  ) : undefined
                }
              />

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
              <View style={{ height: 1, marginTop: 6, marginBottom: 6 }} class="bg-border" />
              <TreeRow
                testID="tree-item-sanitizer"
                icon="shieldCheck"
                label={t("build.sanitize.button")}
                active={selection().kind === "sanitizer"}
                statusDot={
                  sanitizerReport()?.status === "failed"
                    ? "fail"
                    : sanitizerReport()?.status === "passed"
                      ? "ok"
                      : undefined
                }
                onPress={() => selection.set({ kind: "sanitizer" })}
              />
              <TreeRow
                testID="tree-item-external"
                icon="play"
                label={
                  externalReport()?.submissions.length
                    ? `${t("build.external.button")} (${externalReport()!.submissions.length})`
                    : t("build.external.button")
                }
                active={selection().kind === "external"}
                onPress={() => selection.set({ kind: "external" })}
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

            {/* A. Results Overview — starts immediately at top of right pane */}
            <If when={selection().kind === "results"}>
              <View style={{ flex: 1, minHeight: 0 }}>
                {/* Compact Phase 3C mode badges (interactive / multi-run / OI subtasks) — hidden for standard problems */}
                <If when={modeBadges().length > 0}>
                  <View
                    testID="build-mode-badges-bar"
                    style={{
                      height: 32,
                      paddingLeft: 16,
                      paddingRight: 14,
                      borderBottomWidth: 1,
                      flexDirection: "row",
                      alignItems: "center",
                      gap: 6,
                    }}
                    class="border-b-border bg-background/25"
                  >
                    <Each of={modeBadges()} key={(b) => b.id}>
                      {(badge) => (
                        <View
                          testID={`build-mode-badge-${badge().id}`}
                          onPress={() => {
                            if (badge().id === "subtasks") selection.set({ kind: "subtasks" });
                            else selection.set({ kind: "interactor" });
                          }}
                          style={{
                            height: 21,
                            paddingLeft: 7,
                            paddingRight: 7,
                            borderRadius: 5,
                            justifyContent: "center",
                          }}
                          class="bg-muted hover:bg-accent border border-border/60"
                        >
                          <Text
                            style={{ fontSize: 11.5, fontWeight: 500, fontVariant: ["tabular-nums"] }}
                            class="text-secondary-foreground"
                          >
                            {badge().label}
                          </Text>
                        </View>
                      )}
                    </Each>
                  </View>
                </If>

                {/* Dirty inputs notice + Undo (only when dirty inputs exist or just fixed) */}
                <If when={dirtyInputs().length > 0}>
                  <View
                    testID="dirty-inputs-banner"
                    style={{
                      paddingLeft: 16,
                      paddingRight: 14,
                      paddingTop: 7,
                      paddingBottom: 7,
                      borderBottomWidth: 1,
                      flexDirection: "row",
                      alignItems: "center",
                      justifyContent: "space-between",
                      gap: 10,
                    }}
                    class="border-b-border bg-background/30"
                  >
                    <View style={{ flexDirection: "row", alignItems: "center", gap: 7, flex: 1, minWidth: 0 }}>
                      <Icon name="triangleAlert" size={14} tone="chart-4" />
                      <Text style={{ fontSize: 12, flex: 1 }} class="text-foreground">
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
                      paddingLeft: 16,
                      paddingRight: 14,
                      paddingTop: 6,
                      paddingBottom: 6,
                      borderBottomWidth: 1,
                      flexDirection: "row",
                      alignItems: "center",
                      justifyContent: "space-between",
                      gap: 10,
                    }}
                    class="border-b-border bg-background/30"
                  >
                    <Text style={{ fontSize: 12 }} class="text-foreground">
                      {t("build.dirtyInputs.fixedNotice", { count: dirtyUndoNotice()! })}
                    </Text>
                    <View style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>
                      <Btn
                        testID="dirty-inputs-undo-btn"
                        label={t("common.undo")}
                        icon="rotateCcw"
                        variant="ghost"
                        onPress={handleUndoNormalizeDirty}
                      />
                      <IconBtn icon="x" size={24} iconSize={13} onPress={() => dirtyUndoNotice.set(undefined)} />
                    </View>
                  </View>
                </If>

                {/* Compile failures (neutral clean row, no heavy box) */}
                <If when={compileFailures().length > 0}>
                  <View style={{ borderBottomWidth: 1 }} class="border-border bg-background/20">
                    <Each of={compileFailures()} key={(cf) => cf.target}>
                      {(cf) => (
                        <View style={{ paddingLeft: 16, paddingRight: 14, paddingTop: 8, paddingBottom: 8, gap: 4 }}>
                          <View style={{ flexDirection: "row", alignItems: "center", gap: 8 }}>
                            <Icon name="circleAlert" size={13} tone="destructive" />
                            <Text style={{ fontSize: 12, fontWeight: 600 }} class="text-destructive">
                              {t("build.compileError.title", { target: cf().source_path })}
                            </Text>
                            <Text
                              onPress={() => openFileInTree(cf().source_path)}
                              style={{ fontSize: 12, fontWeight: 500 }}
                              class="text-primary"
                            >
                              {t("build.compileError.openSource")}
                            </Text>
                          </View>
                          <Text
                            style={{ fontSize: 11, fontFamily: MONO, lineHeight: 16, numberOfLines: 4, textOverflow: "ellipsis" }}
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
                  <View style={{ flex: 1, minHeight: 0, minWidth: COL_TEST_W + solutionSpecs().length * 120 + 32 }}>
                    {/* Sticky Header Row */}
                    <View
                      style={{
                        height: 36,
                        paddingLeft: 16,
                        paddingRight: 16,
                        borderBottomWidth: 1,
                        flexDirection: "row",
                        alignItems: "center",
                      }}
                      class="border-border bg-background/50"
                    >
                      <View style={{ width: COL_TEST_W, flexShrink: 0 }}>
                        <Text style={{ fontSize: 12, fontWeight: 500 }} class="text-secondary-foreground">
                          {t("build.table.testCase")}
                        </Text>
                      </View>
                      <Each of={solutionSpecs()} key={(s) => s.name}>
                        {(spec) => (
                          <View
                            onPress={() => selection.set({ kind: "solution", name: spec().name })}
                            style={{
                              flex: 1,
                              minWidth: 120,
                              maxWidth: 180,
                              flexDirection: "row",
                              alignItems: "center",
                              gap: 4,
                              paddingRight: 10,
                            }}
                          >
                            <If when={spec().is_main}>
                              <Icon name="crown" size={12} tone="secondary-foreground" />
                            </If>
                            <Text
                              style={{ fontSize: 12, fontWeight: 600, numberOfLines: 1, textOverflow: "ellipsis" }}
                              class="text-foreground"
                            >
                              {spec().name}
                            </Text>
                          </View>
                        )}
                      </Each>
                    </View>

                    {/* Sticky Summary Conclusion Row (P1-5: preserves previous summary with `旧` on subset runs) */}
                    <View
                      style={{
                        height: 30,
                        paddingLeft: 16,
                        paddingRight: 16,
                        borderBottomWidth: 1,
                        flexDirection: "row",
                        alignItems: "center",
                      }}
                      class="border-border bg-muted/20"
                    >
                      <View style={{ width: COL_TEST_W, flexShrink: 0 }}>
                        <Text style={{ fontSize: 11, fontWeight: 500 }} class="text-secondary-foreground">
                          {t("build.table.conclusion")}
                        </Text>
                      </View>
                      <Each of={solutionSpecs()} key={(s) => s.name}>
                        {(spec) => {
                          const conclusion = () => {
                            const sum = summaryBySolution().get(spec().name);
                            const prevSum = lastFullSummaries().get(spec().name);
                            const cells: MatrixCellData[] = [];
                            const lookup = matrixLookup();
                            for (const row of testRows()) {
                              const c = lookup.get(matrixCellKey(row.index, spec().name));
                              if (c) cells.push(c);
                            }
                            return formatSolutionConclusion(sum, cells, isZh(), isRunningThis(), prevSum);
                          };
                          return (
                            <View
                              style={{
                                flex: 1,
                                minWidth: 120,
                                maxWidth: 180,
                                paddingRight: 10,
                                flexDirection: "row",
                                alignItems: "center",
                                gap: 5,
                                opacity: conclusion().stale ? 0.55 : 1,
                              }}
                            >
                              <Text
                                style={{ fontSize: 11, fontWeight: 500, fontVariant: ["tabular-nums"] }}
                                class={
                                  conclusion().tone === "fail"
                                    ? "text-destructive"
                                    : conclusion().tone === "ok"
                                      ? "text-secondary-foreground"
                                      : "text-muted-foreground"
                                }
                              >
                                {conclusion().text}
                              </Text>
                              <If when={Boolean(conclusion().stale)}>
                                <Text style={{ fontSize: 10.5 }} class="text-muted-foreground">
                                  {t("build.table.staleTag")}
                                </Text>
                              </If>
                            </View>
                          );
                        }}
                      </Each>
                    </View>

                    {/* Sticky OI Total Score Row (only when subtasks are declared) */}
                    <If when={subtasks().length > 0}>
                      <View
                        testID="build-total-score-row"
                        style={{
                          height: 28,
                          paddingLeft: 16,
                          paddingRight: 16,
                          borderBottomWidth: 1,
                          flexDirection: "row",
                          alignItems: "center",
                        }}
                        class="border-border bg-muted/15"
                      >
                        <View style={{ width: COL_TEST_W, flexShrink: 0 }}>
                          <Text style={{ fontSize: 11, fontWeight: 500 }} class="text-secondary-foreground">
                            {t("build.table.totalScore")}
                          </Text>
                        </View>
                        <Each of={solutionSpecs()} key={(s) => s.name}>
                          {(spec) => {
                            const sum = () => summaryBySolution().get(spec().name) ?? lastFullSummaries().get(spec().name);
                            const scoreText = () => formatSolutionScoreSummary(sum());
                            return (
                              <View
                                style={{
                                  flex: 1,
                                  minWidth: 120,
                                  maxWidth: 180,
                                  paddingRight: 10,
                                  flexDirection: "row",
                                  alignItems: "center",
                                }}
                              >
                                <Text
                                  style={{ fontSize: 11.5, fontWeight: 600, fontVariant: ["tabular-nums"] }}
                                  class={sum() && !sum()!.matched ? "text-destructive" : "text-foreground"}
                                >
                                  {scoreText() ?? "-"}
                                </Text>
                              </View>
                            );
                          }}
                        </Each>
                      </View>
                    </If>

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
                                    height: 30,
                                    paddingLeft: 16,
                                    paddingRight: 16,
                                    borderBottomWidth: 1,
                                    flexDirection: "row",
                                    alignItems: "center",
                                  }}
                                  class="border-border/80 bg-muted/30"
                                >
                                  <View
                                    onPress={() => toggleSubtaskCollapsed(group().key)}
                                    style={{
                                      width: COL_TEST_W,
                                      flexShrink: 0,
                                      flexDirection: "row",
                                      alignItems: "center",
                                      gap: 5,
                                      paddingRight: 10,
                                    }}
                                  >
                                    <Icon
                                      name={isCollapsed() ? "chevronRight" : "chevronDown"}
                                      size={12}
                                      tone="secondary-foreground"
                                    />
                                    <Text
                                      style={{ fontSize: 12, fontWeight: 600, fontVariant: ["tabular-nums"], numberOfLines: 1, textOverflow: "ellipsis", flex: 1 }}
                                      class="text-foreground"
                                    >
                                      {group().subtaskId
                                        ? t("build.table.subtaskHeader", { id: group().subtaskId! })
                                        : t("build.subtasks.unassigned")}
                                    </Text>
                                    <If when={group().summary}>
                                      {(s) => (
                                        <Text
                                          style={{ fontSize: 11, fontVariant: ["tabular-nums"], flexShrink: 0 }}
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
                                            paddingRight: 10,
                                            flexDirection: "row",
                                            alignItems: "center",
                                            gap: 6,
                                          }}
                                        >
                                          <If when={subScore()}>
                                            {(ss) => (
                                              <Text
                                                style={{ fontSize: 11, fontWeight: 500, fontVariant: ["tabular-nums"] }}
                                                class={
                                                  ss().matched === false
                                                    ? "text-destructive"
                                                    : ss().full_score
                                                      ? "text-secondary-foreground"
                                                      : "text-foreground"
                                                }
                                              >
                                                {ss().full_score
                                                  ? `${formatScoreValue(ss().score)} / ${ss().max_score}`
                                                  : `${formatScoreValue(ss().score)} / ${ss().max_score}  ${ss().verdict}`}
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
                                        height: 34,
                                        paddingLeft: 16,
                                        paddingRight: 16,
                                        borderBottomWidth: 1,
                                        flexDirection: "row",
                                        alignItems: "center",
                                      }}
                                      class="border-border/60 hover:bg-muted/30"
                                    >
                                      {/* Test case label cell */}
                                      <View
                                        onPress={() => selectTestByIndex(row().index)}
                                        style={{
                                          width: COL_TEST_W,
                                          flexShrink: 0,
                                          flexDirection: "row",
                                          alignItems: "center",
                                          gap: 6,
                                          paddingLeft: isGrouped() ? 10 : 0,
                                          paddingRight: 10,
                                        }}
                                      >
                                        <Text
                                          style={{ fontSize: 12, fontVariant: ["tabular-nums"], width: 20, flexShrink: 0 }}
                                          class="text-muted-foreground"
                                        >
                                          {String(row().index).padStart(2, "0")}
                                        </Text>
                                        <Text
                                          style={{ fontSize: 13, fontWeight: 500, fontVariant: ["tabular-nums"], numberOfLines: 1, textOverflow: "ellipsis", flex: 1 }}
                                          class="text-foreground"
                                        >
                                          {row().name}
                                        </Text>
                                        <If when={row().validator_ok === false}>
                                          <Icon name="triangleAlert" size={12} tone="destructive" />
                                        </If>
                                        <If when={row().size_bytes > 0}>
                                          <Text style={{ fontSize: 11, fontVariant: ["tabular-nums"], flexShrink: 0 }} class="text-muted-foreground">
                                            {formatBytesCompact(row().size_bytes)}
                                          </Text>
                                        </If>
                                      </View>

                                      {/* P0-2 Quiet Solution Verdict Cells (fluid 120–180px, only anomalies have solid fill!) */}
                                      <Each of={solutionSpecs()} key={(s) => s.name}>
                                        {(spec) => {
                                          const cell = () => matrixLookup().get(matrixCellKey(row().index, spec().name));
                                          const sum = () => summaryBySolution().get(spec().name) ?? lastFullSummaries().get(spec().name);
                                          const isAnom = () => {
                                            const c = cell();
                                            if (!c) return false;
                                            return isCellVerdictAnomaly(c.verdict, spec().expected, spec().is_main, sum()?.matched);
                                          };
                                          const isPicked = () => {
                                            const s = selectedCell();
                                            return s?.testIndex === row().index && s?.solution === spec().name;
                                          };
                                          return (
                                            <View
                                              onPress={() => {
                                                if (cell()) {
                                                  checksExpanded.set(false);
                                                  filterOpen.set(false);
                                                  selectedCell.set({ testIndex: row().index, solution: spec().name });
                                                }
                                              }}
                                              style={{
                                                flex: 1,
                                                minWidth: 120,
                                                maxWidth: 180,
                                                paddingRight: 10,
                                                justifyContent: "center",
                                              }}
                                            >
                                              <If
                                                when={cell()}
                                                else={
                                                  <Text style={{ fontSize: 12, paddingLeft: 6 }} class="text-muted-foreground">
                                                    {"-"}
                                                  </Text>
                                                }
                                              >
                                                {(c) => {
                                                  const st = () => cellVisualStyle(c().verdict, isAnom(), colorScheme() === "dark");
                                                  const partial = () => formatScoreRatioPercent(c().score_ratio, false);
                                                  return (
                                                    <View
                                                      style={{
                                                        height: 24,
                                                        paddingLeft: 7,
                                                        paddingRight: 7,
                                                        borderRadius: 5,
                                                        borderWidth: isPicked() ? 2 : 0,
                                                        backgroundColor: st().bg,
                                                        borderColor: isPicked() ? token("primary") : "transparent",
                                                        opacity: c().stale ? 0.5 : 1,
                                                        flexDirection: "row",
                                                        alignItems: "center",
                                                        justifyContent: "space-between",
                                                        gap: 6,
                                                      }}
                                                    >
                                                      <Text
                                                        style={{
                                                          fontSize: 11.5,
                                                          fontWeight: st().bold ? 600 : 500,
                                                          color: st().fg,
                                                          fontVariant: ["tabular-nums"],
                                                        }}
                                                      >
                                                        {partial() ? `${c().verdict} ${partial()!}` : c().verdict}
                                                      </Text>
                                                      <Text
                                                        style={{
                                                          fontSize: 11,
                                                          color: st().fg,
                                                          opacity: st().solid ? 0.9 : 0.72,
                                                          fontVariant: ["tabular-nums"],
                                                        }}
                                                      >
                                                        {`${c().cpu_time_ms} ms`}
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
                      height: 32,
                      paddingLeft: 16,
                      paddingRight: 14,
                      borderTopWidth: 1,
                      flexDirection: "row",
                      alignItems: "center",
                      justifyContent: "space-between",
                      gap: 12,
                    }}
                    class="border-border bg-background/30"
                  >
                    <View style={{ flexDirection: "row", alignItems: "center", gap: 14, flex: 1, minWidth: 0 }}>
                      <If
                        when={!isRunningThis()}
                        else={
                          <Text
                            style={{ fontSize: 11.5, fontVariant: ["tabular-nums"] }}
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
                              <Text style={{ fontSize: 11.5, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
                                {`${t("build.margin.maxInput")} ${formatBytesCompact(m().max_input_bytes)}`}
                              </Text>
                              <Text style={{ fontSize: 11.5, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
                                {`${t("build.margin.stdSlowest")} ${formatStdTimeRatio(m())}`}
                              </Text>
                              <If when={formatAcVsTleMargin(m())}>
                                {(gap) => (
                                  <Text style={{ fontSize: 11.5, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
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
                          height: 22,
                          paddingLeft: 7,
                          paddingRight: 7,
                          borderRadius: 5,
                          flexDirection: "row",
                          alignItems: "center",
                          gap: 5,
                          flexShrink: 0,
                        }}
                      >
                        <Icon
                          name="triangleAlert"
                          size={11.5}
                          tone={anomalousRowIndices().size > 0 ? "chart-4" : "muted-foreground"}
                        />
                        <Text style={{ fontSize: 11.5, fontWeight: 500, fontVariant: ["tabular-nums"] }} class="text-secondary-foreground">
                          {`${t("build.filter.onlyAnomaly")} (${anomalousRowIndices().size})`}
                        </Text>
                      </View>
                    </If>

                    <If when={!isRunningThis() && singleStatus().kind === "ready" && anomalousRowIndices().size === 0 && !onlyAnomalyRows()}>
                      <View testID="build-verified-next-steps" style={{ flexDirection: "row", alignItems: "center", gap: 12, flexShrink: 0 }}>
                        <If when={!!props.onSwitchToStatement}>
                          <Text
                            onPress={() => props.onSwitchToStatement?.()}
                            style={{ fontSize: 11.5, fontWeight: 500 }}
                            class="text-secondary-foreground hover:text-foreground"
                          >
                            {t("build.verified.backToStatement")}
                          </Text>
                        </If>
                        <If when={!props.readOnly}>
                          <Text
                            onPress={() => dialog.set({ kind: "addToContest", problemId: props.problemId })}
                            style={{ fontSize: 11.5, fontWeight: 500 }}
                            class="text-primary"
                          >
                            {t("build.verified.addToContest")}
                          </Text>
                        </If>
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
                      return sp ? isCellVerdictAnomaly(cell().verdict, sp.expected, sp.is_main, sum()?.matched) : false;
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
                            height: 28,
                            paddingLeft: 9,
                            paddingRight: 7,
                            borderRadius: 6,
                            flexDirection: "row",
                            alignItems: "center",
                            gap: 5,
                          }}
                        >
                          <Text style={{ fontSize: 12, fontWeight: 500, fontVariant: ["tabular-nums"] }} class="text-foreground">
                            {`#${String(activeIdx()).padStart(2, "0")} ${row()?.name ?? ""}`}
                          </Text>
                          <Icon name="chevronDown" size={12} tone="muted-foreground" />
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
                            height: 28,
                            paddingLeft: 9,
                            paddingRight: 7,
                            borderRadius: 6,
                            flexDirection: "row",
                            alignItems: "center",
                            gap: 5,
                          }}
                        >
                          <Text style={{ fontSize: 12, fontWeight: 500, fontVariant: ["tabular-nums"] }} class="text-foreground">
                            {curSub ? t("build.table.subtaskHeader", { id: curSub }) : t("build.subtasks.assignNone")}
                          </Text>
                          <Icon name="chevronDown" size={12} tone="muted-foreground" />
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
                          paddingLeft: 20,
                          paddingRight: 16,
                          paddingTop: 8,
                          paddingBottom: 8,
                          borderBottomWidth: 1,
                          flexDirection: "row",
                          alignItems: "center",
                          justifyContent: "space-between",
                          gap: 10,
                        }}
                        class="border-b-border bg-muted/30"
                      >
                        <View style={{ flex: 1, minWidth: 0, flexDirection: "row", alignItems: "center", gap: 8 }}>
                          <Icon name="circleAlert" size={14} tone="destructive" />
                          <Text style={{ fontSize: 12, fontWeight: 600 }} class="text-destructive">
                            {t("build.tests.validatorFailed")}
                          </Text>
                          <If when={!!row()?.validator_message}>
                            <Text style={{ fontSize: 12, fontFamily: MONO, flex: 1, numberOfLines: 1, textOverflow: "ellipsis" }} class="text-foreground">
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
                              paddingLeft: 20,
                              paddingRight: 16,
                              paddingTop: 8,
                              paddingBottom: 8,
                              borderBottomWidth: 1,
                              flexDirection: "row",
                              alignItems: "center",
                              justifyContent: "space-between",
                              gap: 10,
                            }}
                            class="border-border bg-background/30"
                          >
                            <View style={{ flex: 1, minWidth: 0, gap: 2 }}>
                              <View style={{ flexDirection: "row", alignItems: "center", gap: 7 }}>
                                <Icon
                                  name={activeGroup()?.kind === "samples" ? "fileText" : isGenBatch() ? "sparkles" : "database"}
                                  size={14.5}
                                  tone="secondary-foreground"
                                />
                                <Text style={{ fontSize: 13, fontWeight: 600, fontVariant: ["tabular-nums"] }} class="text-foreground">
                                  {`#${String(activeIdx()).padStart(2, "0")} ${row()?.name ?? prev()?.name ?? ""}`}
                                </Text>
                              </View>
                              <If when={fileRel() || activeGroup()?.genCmd}>
                                <Text style={{ fontSize: 11.5, fontFamily: MONO, numberOfLines: 1, textOverflow: "ellipsis" }} class="text-muted-foreground">
                                  {fileRel() ?? activeGroup()?.genCmd ?? ""}
                                </Text>
                              </If>
                            </View>

                            <View style={{ flexDirection: "row", alignItems: "center", gap: 6, flexShrink: 0 }}>
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
                                paddingLeft: 20,
                                paddingRight: 16,
                                paddingTop: 8,
                                paddingBottom: 8,
                                borderBottomWidth: 1,
                                flexDirection: "row",
                                alignItems: "center",
                                gap: 8,
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
                                    style={{ height: 28, paddingLeft: 8, paddingRight: 6, borderRadius: 6, flexDirection: "row", alignItems: "center", gap: 5 }}
                                  >
                                    <Text style={{ fontSize: 12, fontWeight: 500 }} class="text-foreground">
                                      {batchEditGen() || t("build.tests.genModalGenerator")}
                                    </Text>
                                    <Icon name="chevronDown" size={12} tone="muted-foreground" />
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
                          <Scroll style={{ flex: 1, minHeight: 0 }} contentStyle={{ padding: 20, gap: 12 }}>
                            <Text style={{ fontSize: 12, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
                              {activeContent().linesExact != null
                                ? `${formatBytesCompact(activeContent().bytes)}   ${t("build.tests.linesCount", { count: activeContent().linesExact! })}`
                                : formatBytesCompact(activeContent().bytes)}
                            </Text>

                            <If when={activeContent().truncated}>
                              <View
                                testID="test-truncation-banner"
                                style={{ paddingLeft: 10, paddingRight: 10, paddingTop: 6, paddingBottom: 6, borderRadius: 6, maxWidth: 680 }}
                                class="bg-muted/40"
                              >
                                <Text style={{ fontSize: 12, fontVariant: ["tabular-nums"] }} class="text-secondary-foreground">
                                  {activeContent().omitted != null
                                    ? t("build.tests.truncatedWithOmitted", { bytes: formatBytesCompact(activeContent().omitted!) })
                                    : t("build.tests.truncatedBanner")}
                                </Text>
                              </View>
                            </If>

                            <View
                              style={{
                                padding: 14,
                                borderRadius: 6,
                                maxWidth: activeContent().truncated ? 820 : 560,
                              }}
                              class="bg-muted/40"
                            >
                              <Text style={{ fontSize: 12, fontFamily: MONO, lineHeight: 18 }} class="text-foreground">
                                {activeContent().head || t("build.tests.emptyContent")}
                              </Text>
                              <If when={activeContent().truncated && activeContent().tail.length > 0}>
                                <View style={{ paddingTop: 8, paddingBottom: 8 }}>
                                  <Text style={{ fontSize: 11, fontFamily: MONO }} class="text-muted-foreground">
                                    {"…"}
                                  </Text>
                                </View>
                                <Text style={{ fontSize: 12, fontFamily: MONO, lineHeight: 18 }} class="text-foreground">
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
                          <View style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>
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
                          <View style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>
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
                                    height: 28,
                                    paddingLeft: 9,
                                    paddingRight: 7,
                                    borderRadius: 6,
                                    flexDirection: "row",
                                    alignItems: "center",
                                    gap: 6,
                                  }}
                                >
                                  <If when={Boolean(matchSelfTestResult(activeRel()!, activeReport()?.self_tests))}>
                                    <View
                                      style={{
                                        width: 6,
                                        height: 6,
                                        borderRadius: 3,
                                        backgroundColor: matchSelfTestResult(activeRel()!, activeReport()?.self_tests)?.passed
                                          ? "#16a34a"
                                          : "#dc2626",
                                      }}
                                    />
                                  </If>
                                  <Text style={{ fontSize: 12, fontWeight: 500 }} class="text-foreground">
                                    {activeRel()!.split("/").slice(-2).join("/")}
                                  </Text>
                                  <Icon name="chevronDown" size={12} tone="muted-foreground" />
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
                          height: 28,
                          paddingLeft: 9,
                          paddingRight: 7,
                          borderRadius: 6,
                          flexDirection: "row",
                          alignItems: "center",
                          gap: 5,
                        }}
                      >
                        <Text style={{ fontSize: 12, fontWeight: 500, fontVariant: ["tabular-nums"] }} class="text-foreground">
                          {currentRuns() === 1
                            ? t("build.interactor.runsSingle")
                            : t("build.interactor.runsMulti", { count: currentRuns() })}
                        </Text>
                        <Icon name="chevronDown" size={12} tone="muted-foreground" />
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
                              paddingLeft: 20,
                              paddingRight: 16,
                              paddingTop: 8,
                              paddingBottom: 8,
                              borderBottomWidth: 1,
                              flexDirection: "row",
                              alignItems: "center",
                              justifyContent: "space-between",
                              gap: 10,
                            }}
                            class="border-border bg-background/30"
                          >
                            <View style={{ gap: 2 }}>
                              <View style={{ flexDirection: "row", alignItems: "center", gap: 7 }}>
                                <Icon name="bot" size={14.5} tone="secondary-foreground" />
                                <Text style={{ fontSize: 13, fontWeight: 600 }} class="text-foreground">
                                  {t("build.interactor.title")}
                                </Text>
                              </View>
                              <Text style={{ fontSize: 11.5 }} class="text-muted-foreground">
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
                                  onPress={handleCreateInteractor}
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
                              <View style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>
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
                    paddingLeft: 20,
                    paddingRight: 16,
                    paddingTop: 8,
                    paddingBottom: 8,
                    borderBottomWidth: 1,
                    flexDirection: "row",
                    alignItems: "center",
                    justifyContent: "space-between",
                    gap: 10,
                  }}
                  class="border-border bg-background/30"
                >
                  <View style={{ gap: 2 }}>
                    <View style={{ flexDirection: "row", alignItems: "center", gap: 8 }}>
                      <Icon name="listOrdered" size={14.5} tone="secondary-foreground" />
                      <Text style={{ fontSize: 13, fontWeight: 600 }} class="text-foreground">
                        {t("build.subtasks.title")}
                      </Text>
                      <If when={totalSubtaskScore() != null}>
                        <Text style={{ fontSize: 12, fontWeight: 500, fontVariant: ["tabular-nums"] }} class="text-secondary-foreground">
                          {t("build.subtasks.totalScore", { score: totalSubtaskScore()! })}
                        </Text>
                      </If>
                      <If when={totalSubtaskScore() != null && totalSubtaskScore() !== 100}>
                        <Text style={{ fontSize: 11.5 }} class="text-chart-4">
                          {t("build.subtasks.scoreNot100")}
                        </Text>
                      </If>
                    </View>
                  </View>
                  <If when={!props.readOnly}>
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
                  <Scroll style={{ flex: 1 }} contentStyle={{ padding: 20, gap: 8 }}>
                    <Each of={subtasks()} key={(s) => s.id}>
                      {(sub) => (
                        <View
                          testID={`subtask-row-${sub().id}`}
                          style={{
                            paddingLeft: 14,
                            paddingRight: 10,
                            paddingTop: 10,
                            paddingBottom: 10,
                            borderRadius: 8,
                            borderWidth: 1,
                            flexDirection: "row",
                            alignItems: "center",
                            justifyContent: "space-between",
                            gap: 12,
                          }}
                          class="border-border bg-card"
                        >
                          <View style={{ flex: 1, minWidth: 0, gap: 3 }}>
                            <View style={{ flexDirection: "row", alignItems: "center", gap: 8 }}>
                              <Text style={{ fontSize: 13, fontWeight: 600, fontVariant: ["tabular-nums"] }} class="text-foreground">
                                {t("build.table.subtaskHeader", { id: sub().id })}
                              </Text>
                              <View style={{ height: 20, paddingLeft: 6, paddingRight: 6, borderRadius: 4, justifyContent: "center" }} class="bg-muted">
                                <Text style={{ fontSize: 11.5, fontWeight: 600, fontVariant: ["tabular-nums"] }} class="text-foreground">
                                  {t("build.tree.subtasksPts", { score: sub().score })}
                                </Text>
                              </View>
                              <Text style={{ fontSize: 11.5 }} class="text-secondary-foreground">
                                {sub().scoring === "sum" ? t("build.subtasks.scoringSum") : t("build.subtasks.scoringMin")}
                              </Text>
                            </View>
                            <View style={{ flexDirection: "row", alignItems: "center", gap: 10 }}>
                              <Text style={{ fontSize: 11.5, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
                                {t("build.subtasks.testCount", { count: sub().test_count })}
                              </Text>
                              <If when={(sub().depends?.length ?? 0) > 0}>
                                <Text style={{ fontSize: 11.5, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
                                  {`${t("build.subtasks.dependsLabel")}: ${sub().depends!.join(", ")}`}
                                </Text>
                              </If>
                            </View>
                          </View>

                          <If when={!props.readOnly}>
                            <View style={{ flexDirection: "row", alignItems: "center", gap: 4, flexShrink: 0 }}>
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
                          height: 28,
                          paddingLeft: 10,
                          paddingRight: 8,
                          borderRadius: 6,
                          flexDirection: "row",
                          alignItems: "center",
                          gap: 6,
                        }}
                      >
                        <Text style={{ fontSize: 12, fontWeight: 500 }} class="text-foreground">
                          {inspection()?.checker || "wcmp"}
                        </Text>
                        <Icon name="chevronDown" size={13} tone="muted-foreground" />
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
                              paddingLeft: 20,
                              paddingRight: 16,
                              paddingTop: 8,
                              paddingBottom: 8,
                              borderBottomWidth: 1,
                              flexDirection: "row",
                              alignItems: "center",
                              justifyContent: "space-between",
                              gap: 10,
                            }}
                            class="border-border bg-background/30"
                          >
                            <View style={{ gap: 2 }}>
                              <View style={{ flexDirection: "row", alignItems: "center", gap: 7 }}>
                                <Icon name="scale" size={14.5} tone="secondary-foreground" />
                                <Text style={{ fontSize: 13, fontWeight: 600 }} class="text-foreground">
                                  {t("build.tree.checker")}
                                </Text>
                              </View>
                              <Text style={{ fontSize: 11.5 }} class="text-muted-foreground">
                                {t("build.programs.builtinCheckerActive", { name: inspection()?.checker || "wcmp" })}
                              </Text>
                            </View>
                            {checkerDropdown()}
                          </View>
                          <View style={{ padding: 24, maxWidth: 560, gap: 8 }}>
                            <Text style={{ fontSize: 12.5, lineHeight: 19 }} class="text-secondary-foreground">
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
                              <View style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>
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
                                        height: 28,
                                        paddingLeft: 9,
                                        paddingRight: 7,
                                        borderRadius: 6,
                                        flexDirection: "row",
                                        alignItems: "center",
                                        gap: 5,
                                      }}
                                    >
                                      <Text style={{ fontSize: 12, fontWeight: 500 }} class="text-foreground">
                                        {`${t("build.programs.expectPrefix")} ${spec()!.expected.join("/")}`}
                                      </Text>
                                      <Icon name="chevronDown" size={12} tone="muted-foreground" />
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

            {/* Phase 3D: Isolated Sanitizer (ASan/UBSan) UB Check View */}
            <If when={selection().kind === "sanitizer"}>
              <View testID="build-sanitizer-view" style={{ flex: 1, minHeight: 0 }}>
                <View
                  style={{
                    height: 46,
                    paddingLeft: 16,
                    paddingRight: 16,
                    borderBottomWidth: 1,
                    flexDirection: "row",
                    alignItems: "center",
                    justifyContent: "space-between",
                    gap: 10,
                    flexShrink: 0,
                  }}
                  class="border-border"
                >
                  <View style={{ flexDirection: "row", alignItems: "center", gap: 8, minWidth: 0 }}>
                    <Text style={{ fontSize: 14, fontWeight: 500 }} class="text-foreground">
                      {t("build.sanitize.panelTitle")}
                    </Text>
                    <If when={sanitizerReport()}>
                      {(rep) => {
                        const dark = () => colorScheme() === "dark";
                        const st = () => rep().status;
                        const bg = () =>
                          st() === "passed"
                            ? "rgba(16,185,129,0.12)"
                            : st() === "failed"
                              ? "rgba(244,63,94,0.12)"
                              : "rgba(245,158,11,0.12)";
                        const fg = () =>
                          st() === "passed"
                            ? dark()
                              ? "#4ade80"
                              : "#15803d"
                            : st() === "failed"
                              ? dark()
                                ? "#f87171"
                                : "#b91c1c"
                              : dark()
                                ? "#fbbf24"
                                : "#b45309";
                        const label = () =>
                          st() === "passed"
                            ? t("build.sanitize.passed")
                            : st() === "failed"
                              ? t("build.sanitize.failed", {
                                  count: (rep().findings?.length ?? 0) + (rep().compile_failures?.length ?? 0),
                                })
                              : t("build.sanitize.unsupported");
                        return (
                          <View style={{ height: 22, paddingLeft: 8, paddingRight: 8, borderRadius: 5, justifyContent: "center", backgroundColor: bg() }}>
                            <Text style={{ fontSize: 11.5, fontWeight: 500, color: fg(), fontVariant: ["tabular-nums"] }}>{label()}</Text>
                          </View>
                        );
                      }}
                    </If>
                    <If when={sanitizerReport()}>
                      {(rep) => (
                        <Text style={{ fontSize: 12, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
                          {t("build.sanitize.relaxedLimit", { ms: rep().relaxed_time_limit_ms })}
                        </Text>
                      )}
                    </If>
                  </View>
                  <Btn
                    testID="build-sanitize-rerun-btn"
                    label={isRunningThis() && buildStatus().mode === "sanitize" ? t("build.sanitize.running") : t("build.sanitize.rerun")}
                    icon="shieldCheck"
                    variant="outline"
                    disabled={props.readOnly || isRunningThis()}
                    onPress={() => startRun("sanitize")}
                  />
                </View>

                <Scroll style={{ flex: 1 }} contentStyle={{ padding: 16, gap: 12 }}>
                  <If
                    when={sanitizerReport()}
                    else={
                      <View style={{ paddingTop: 28, alignItems: "center", gap: 10 }}>
                        <Icon name="shieldCheck" size={24} tone="muted-foreground" />
                        <Text style={{ fontSize: 13 }} class="text-secondary-foreground">
                          {isRunningThis() && buildStatus().mode === "sanitize"
                            ? t("build.sanitize.running")
                            : t("build.sanitize.notRun")}
                        </Text>
                        <If when={!props.readOnly && !isRunningThis()}>
                          <Btn
                            label={t("build.sanitize.button")}
                            icon="shieldCheck"
                            variant="primary"
                            onPress={() => startRun("sanitize")}
                          />
                        </If>
                      </View>
                    }
                  >
                    {(rep) => (
                      <View style={{ gap: 12 }}>
                        <If when={rep().status === "unsupported" && rep().unsupported_reason}>
                          <View style={{ padding: 12, borderRadius: 8, borderWidth: 1 }} class="bg-muted/50 border-border">
                            <Text style={{ fontSize: 12.5, lineHeight: 18 }} class="text-secondary-foreground">
                              {rep().unsupported_reason!}
                            </Text>
                          </View>
                        </If>

                        <If when={(rep().compile_failures?.length ?? 0) > 0}>
                          <View style={{ gap: 8 }}>
                            <Each of={rep().compile_failures ?? []} key={(cf) => `${cf.target}:${cf.source}`}>
                              {(cf) => (
                                <View style={{ padding: 12, borderRadius: 8, borderWidth: 1, gap: 6 }} class="bg-card border-border">
                                  <View style={{ flexDirection: "row", alignItems: "center", gap: 8 }}>
                                    <Text style={{ fontSize: 12.5, fontWeight: 500, fontFamily: MONO }} class="text-destructive">
                                      {cf().target}
                                    </Text>
                                    <Text style={{ fontSize: 11.5 }} class="text-muted-foreground">
                                      {t("build.sanitize.compileFailed")}
                                    </Text>
                                  </View>
                                  <Text style={{ fontSize: 11.5, fontFamily: MONO, lineHeight: 16 }} class="text-secondary-foreground">
                                    {cf().stderr.slice(0, 1200)}
                                  </Text>
                                </View>
                              )}
                            </Each>
                          </View>
                        </If>

                        <If when={(rep().findings?.length ?? 0) > 0}>
                          <View style={{ gap: 8 }}>
                            <Each of={rep().findings ?? []} key={(f, i) => `${f.target}:${f.test_index ?? i}:${f.summary}`}>
                              {(f) => (
                                <View style={{ padding: 12, borderRadius: 8, borderWidth: 1, gap: 6 }} class="bg-card border-border">
                                  <View style={{ flexDirection: "row", alignItems: "center", gap: 8, flexWrap: "wrap" }}>
                                    <Text style={{ fontSize: 12.5, fontWeight: 600, fontFamily: MONO }} class="text-foreground">
                                      {f().target}
                                    </Text>
                                    <If when={!!f().test_name}>
                                      <Text style={{ fontSize: 11.5, fontFamily: MONO }} class="text-muted-foreground">
                                        {f().test_name!}
                                      </Text>
                                    </If>
                                  </View>
                                  <Text style={{ fontSize: 12.5, lineHeight: 18 }} class="text-destructive">
                                    {f().summary}
                                  </Text>
                                  <If when={(f().stack_summary?.length ?? 0) > 0}>
                                    <View style={{ padding: 8, borderRadius: 6, gap: 2 }} class="bg-muted">
                                      <Each of={f().stack_summary ?? []} key="index">
                                        {(line) => (
                                          <Text style={{ fontSize: 11.5, fontFamily: MONO, lineHeight: 16 }} class="text-secondary-foreground">
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
                        </If>

                        <If when={rep().status === "passed" && (rep().findings?.length ?? 0) === 0}>
                          <View style={{ padding: 12, borderRadius: 8, borderWidth: 1, gap: 4 }} class="bg-card border-border">
                            <Text style={{ fontSize: 13, fontWeight: 500 }} class="text-foreground">
                              {t("build.sanitize.noFindings")}
                            </Text>
                            <Text style={{ fontSize: 12, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
                              {t("build.sanitize.targetsChecked", { count: rep().targets?.length ?? 0 })}
                            </Text>
                          </View>
                        </If>

                        <If when={(rep().targets?.length ?? 0) > 0}>
                          <View style={{ borderRadius: 8, borderWidth: 1, overflow: "hidden" }} class="border-border bg-card">
                            <Each of={rep().targets ?? []} key={(tr) => `${tr.kind}:${tr.target}`}>
                              {(tr, idx) => (
                                <View
                                  style={{
                                    height: 36,
                                    paddingLeft: 12,
                                    paddingRight: 12,
                                    borderTopWidth: idx() > 0 ? 1 : 0,
                                    flexDirection: "row",
                                    alignItems: "center",
                                    justifyContent: "space-between",
                                    gap: 10,
                                  }}
                                  class="border-border"
                                >
                                  <View style={{ flexDirection: "row", alignItems: "center", gap: 8, minWidth: 0, flex: 1 }}>
                                    <View
                                      style={{
                                        width: 6,
                                        height: 6,
                                        borderRadius: 3,
                                        backgroundColor: tr().passed
                                          ? colorScheme() === "dark"
                                            ? "#4ade80"
                                            : "#16a34a"
                                          : colorScheme() === "dark"
                                            ? "#f87171"
                                            : "#dc2626",
                                      }}
                                    />
                                    <Text style={{ fontSize: 12.5, fontFamily: MONO, numberOfLines: 1, textOverflow: "ellipsis" }} class="text-foreground">
                                      {tr().target}
                                    </Text>
                                    <Text style={{ fontSize: 11.5 }} class="text-muted-foreground">
                                      {tr().kind}
                                    </Text>
                                  </View>
                                  <Text style={{ fontSize: 12, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
                                    {tr().findings_count > 0 ? `${tr().findings_count} UB` : `${tr().checked_runs} runs`}
                                  </Text>
                                </View>
                              )}
                            </Each>
                          </View>
                        </If>
                      </View>
                    )}
                  </If>
                </Scroll>
              </View>
            </If>

            {/* Phase 3D: Zero-Pollution External Solution Trial Run View */}
            <If when={selection().kind === "external"}>
              <View testID="build-external-view" style={{ flex: 1, minHeight: 0 }}>
                <View
                  style={{
                    height: 46,
                    paddingLeft: 16,
                    paddingRight: 16,
                    borderBottomWidth: 1,
                    flexDirection: "row",
                    alignItems: "center",
                    justifyContent: "space-between",
                    gap: 10,
                    flexShrink: 0,
                  }}
                  class="border-border"
                >
                  <Text style={{ fontSize: 14, fontWeight: 500 }} class="text-foreground">
                    {t("build.external.panelTitle")}
                  </Text>
                  <View style={{ flexDirection: "row", alignItems: "center", gap: 8 }}>
                    <If when={!!externalMarkdown()}>
                      <Btn
                        testID="build-external-export-md"
                        label={externalExported.active() ? t("build.external.exported") : t("build.external.exportMd")}
                        icon={externalExported.active() ? "check" : "fileDown"}
                        variant="outline"
                        onPress={() => void exportExternalMarkdown()}
                      />
                    </If>
                    <Btn
                      testID="build-external-pick-another"
                      label={externalRunning() ? t("build.external.running") : t("build.external.pickAnother")}
                      icon="play"
                      variant="primary"
                      disabled={externalRunning()}
                      onPress={() => void pickAndRunExternal()}
                    />
                  </View>
                </View>

                <Scroll style={{ flex: 1 }} contentStyle={{ padding: 16, gap: 12 }}>
                  <If
                    when={externalReport() && externalReport()!.submissions.length > 0}
                    else={
                      <View style={{ paddingTop: 28, alignItems: "center", gap: 10 }}>
                        <Icon name="play" size={24} tone="muted-foreground" />
                        <Text style={{ fontSize: 13 }} class="text-secondary-foreground">
                          {externalRunning() ? t("build.external.running") : t("build.external.empty")}
                        </Text>
                      </View>
                    }
                  >
                    <View style={{ borderRadius: 8, borderWidth: 1, overflow: "hidden" }} class="border-border bg-card">
                      {/* Table header */}
                      <View
                        style={{
                          height: 34,
                          paddingLeft: 12,
                          paddingRight: 12,
                          borderBottomWidth: 1,
                          flexDirection: "row",
                          alignItems: "center",
                          gap: 10,
                        }}
                        class="border-border bg-muted/40"
                      >
                        <Text style={{ flex: 2, fontSize: 11.5, fontWeight: 500 }} class="text-muted-foreground">
                          {t("build.external.colFile")}
                        </Text>
                        <Text style={{ width: 64, fontSize: 11.5, fontWeight: 500 }} class="text-muted-foreground">
                          {t("build.external.colVerdict")}
                        </Text>
                        <Text style={{ width: 84, fontSize: 11.5, fontWeight: 500 }} class="text-muted-foreground">
                          {t("build.external.colPassed")}
                        </Text>
                        <Text style={{ width: 72, fontSize: 11.5, fontWeight: 500 }} class="text-muted-foreground">
                          {t("build.external.colTime")}
                        </Text>
                        <Text style={{ width: 82, fontSize: 11.5, fontWeight: 500 }} class="text-muted-foreground">
                          {t("build.external.colRatio")}
                        </Text>
                        <Text style={{ flex: 2, fontSize: 11.5, fontWeight: 500 }} class="text-muted-foreground">
                          {t("build.external.colFailure")}
                        </Text>
                        <View style={{ width: 124 }} />
                      </View>
                      <Each of={externalReport()?.submissions ?? []} key={(s) => s.file_path}>
                        {(sub, idx) => {
                          const dark = () => colorScheme() === "dark";
                          const v = () => sub().verdict;
                          const vBg = () =>
                            v() === "AC"
                              ? "rgba(16,185,129,0.12)"
                              : v() === "TLE" || v() === "MLE"
                                ? "rgba(245,158,11,0.12)"
                                : "rgba(244,63,94,0.12)";
                          const vFg = () =>
                            v() === "AC"
                              ? dark()
                                ? "#4ade80"
                                : "#15803d"
                              : v() === "TLE" || v() === "MLE"
                                ? dark()
                                  ? "#fbbf24"
                                  : "#b45309"
                                : dark()
                                  ? "#f87171"
                                  : "#b91c1c";
                          const adopted = () => !!adoptedExternalPaths()[sub().file_path];
                          const failText = () => {
                            if (sub().compile_error) return t("build.sanitize.compileFailed");
                            const ff = sub().first_failure;
                            if (!ff) return "—";
                            return `${ff.test_name} (${ff.verdict})${ff.message ? ` ${ff.message}` : ""}`;
                          };
                          return (
                            <View
                              style={{
                                minHeight: 40,
                                paddingLeft: 12,
                                paddingRight: 12,
                                paddingTop: 6,
                                paddingBottom: 6,
                                borderTopWidth: idx() > 0 ? 1 : 0,
                                flexDirection: "row",
                                alignItems: "center",
                                gap: 10,
                              }}
                              class="border-border"
                            >
                              <Text style={{ flex: 2, fontSize: 12.5, fontFamily: MONO, numberOfLines: 1, textOverflow: "ellipsis" }} class="text-foreground">
                                {sub().file_name}
                              </Text>
                              <View style={{ width: 64, flexDirection: "row" }}>
                                <View style={{ height: 20, paddingLeft: 6, paddingRight: 6, borderRadius: 4, justifyContent: "center", backgroundColor: vBg() }}>
                                  <Text style={{ fontSize: 11.5, fontWeight: 600, color: vFg() }}>{v()}</Text>
                                </View>
                              </View>
                              <Text style={{ width: 84, fontSize: 12, fontVariant: ["tabular-nums"] }} class="text-secondary-foreground">
                                {sub().max_score != null
                                  ? `${sub().total_score ?? 0}/${sub().max_score}`
                                  : `${sub().passed_tests}/${sub().total_tests}`}
                              </Text>
                              <Text style={{ width: 72, fontSize: 12, fontVariant: ["tabular-nums"] }} class="text-secondary-foreground">
                                {`${sub().max_time_ms}ms`}
                              </Text>
                              <Text style={{ width: 82, fontSize: 12, fontVariant: ["tabular-nums"] }} class="text-secondary-foreground">
                                {formatExternalRatio(sub().ratio_vs_std)}
                              </Text>
                              <Text style={{ flex: 2, fontSize: 11.5, numberOfLines: 1, textOverflow: "ellipsis" }} class="text-muted-foreground">
                                {failText()}
                              </Text>
                              <View style={{ width: 124, flexDirection: "row", justifyContent: "flex-end" }}>
                                <Btn
                                  testID={`external-adopt-${idx()}`}
                                  label={adopted() ? t("build.external.adopted") : t("build.external.adopt")}
                                  icon={adopted() ? "check" : "plus"}
                                  variant="outline"
                                  disabled={props.readOnly || adopted()}
                                  onPress={() => adoptExternal(sub())}
                                />
                              </View>
                            </View>
                          );
                        }}
                      </Each>
                    </View>
                  </If>
                </Scroll>
              </View>
            </If>
          </View>
        </View>

        {/* ── P0-3 Anchored Overlay Popover: Issues / Checks (NEVER pushes table down!) ── */}
        <If when={checksExpanded()}>
          <View
            testID="build-checks-panel"
            style={{
              position: "absolute",
              top: 6,
              right: 14,
              width: 440,
              maxHeight: 380,
              borderRadius: 10,
              borderWidth: 1,
              boxShadow: popoverShadow(),
              overflow: "hidden",
            }}
            class="bg-popover border-border"
          >
            <Scroll style={{ maxHeight: 380 }} contentStyle={{ padding: 12, gap: 8 }}>
              <If when={singleStatus().kind === "ready"}>
                <View
                  testID="build-checks-verified-header"
                  style={{
                    flexDirection: "row",
                    alignItems: "center",
                    justifyContent: "space-between",
                    gap: 8,
                    paddingBottom: gates().length > 0 ? 8 : 0,
                    borderBottomWidth: gates().length > 0 ? 1 : 0,
                  }}
                  class="border-border"
                >
                  <View style={{ flexDirection: "row", alignItems: "center", gap: 6, flex: 1, minWidth: 0 }}>
                    <Icon name="check" size={13.5} style={{ tintColor: colorScheme() === "dark" ? "#4ade80" : "#16a34a" }} />
                    <Text style={{ fontSize: 12.5, fontWeight: 500 }} class="text-foreground">
                      {t("build.verified.headline")}
                    </Text>
                  </View>
                  <View style={{ flexDirection: "row", alignItems: "center", gap: 10, flexShrink: 0 }}>
                    <If when={!!props.onSwitchToStatement}>
                      <Text
                        onPress={() => {
                          checksExpanded.set(false);
                          props.onSwitchToStatement?.();
                        }}
                        style={{ fontSize: 12, fontWeight: 500 }}
                        class="text-secondary-foreground hover:text-foreground"
                      >
                        {t("build.verified.backToStatement")}
                      </Text>
                    </If>
                    <If when={!props.readOnly}>
                      <Text
                        onPress={() => {
                          checksExpanded.set(false);
                          dialog.set({ kind: "addToContest", problemId: props.problemId });
                        }}
                        style={{ fontSize: 12, fontWeight: 500 }}
                        class="text-primary"
                      >
                        {t("build.verified.addToContest")}
                      </Text>
                    </If>
                  </View>
                </View>
              </If>
              <If
                when={gates().length > 0}
                else={
                  <Text style={{ fontSize: 12 }} class="text-secondary-foreground">
                    {t("build.checks.noChecksYet")}
                  </Text>
                }
              >
                <Each of={partitionedGates().attention} key={(g) => g.id}>
                  {(gate) => {
                    const action = () => nextActionForCheck(gate(), activeReport(), inspection()?.files);
                    return (
                      <View style={{ flexDirection: "row", alignItems: "flex-start", gap: 8, paddingTop: 2, paddingBottom: 2 }}>
                        <View style={{ paddingTop: 2 }}>
                          <Icon
                            name={gate().status === "fail" ? "circleAlert" : "triangleAlert"}
                            size={13.5}
                            tone={gate().status === "fail" ? "destructive" : "chart-4"}
                          />
                        </View>
                        <View style={{ flex: 1, minWidth: 0, gap: 2 }}>
                          <View style={{ flexDirection: "row", alignItems: "center", flexWrap: "wrap", gap: 6 }}>
                            <Text style={{ fontSize: 12.5, fontWeight: 500 }} class="text-foreground">
                              {humanizeCheckText(gate().title, isZh())}
                            </Text>
                            <If when={action()}>
                              {(act) => (
                                <Text
                                  onPress={() => {
                                    checksExpanded.set(false);
                                    const a = act();
                                    if (a.kind === "view_test") selectTestByIndex(a.testIndex);
                                    else if (a.kind === "select_cell") {
                                      selection.set({ kind: "results" });
                                      selectedCell.set({ testIndex: a.testIndex, solution: a.solution });
                                    } else if (a.kind === "open_subtasks") {
                                      selection.set({ kind: "subtasks" });
                                    } else if (a.kind === "open_file") openFileInTree(a.relPath);
                                  }}
                                  style={{ fontSize: 12, fontWeight: 500 }}
                                  class="text-primary"
                                >
                                  {t("build.check.fix")}
                                </Text>
                              )}
                            </If>
                          </View>
                          <Text style={{ fontSize: 12, lineHeight: 17 }} class="text-secondary-foreground">
                            {humanizeCheckText(gate().message, isZh())}
                          </Text>
                        </View>
                      </View>
                    );
                  }}
                </Each>

                <If when={partitionedGates().passed.length > 0}>
                  <View style={{ borderTopWidth: partitionedGates().attention.length > 0 ? 1 : 0, paddingTop: partitionedGates().attention.length > 0 ? 8 : 0, gap: 4 }} class="border-border">
                    <View
                      onPress={() => passedChecksExpanded.set(!passedChecksExpanded())}
                      style={{ flexDirection: "row", alignItems: "center", gap: 6 }}
                    >
                      <Icon name="check" size={13} tone="secondary-foreground" />
                      <Text style={{ fontSize: 12, fontVariant: ["tabular-nums"] }} class="text-secondary-foreground">
                        {t("build.checks.passedCollapsed", { count: partitionedGates().passed.length })}
                      </Text>
                      <Icon
                        name={passedChecksExpanded() ? "chevronDown" : "chevronRight"}
                        size={12}
                        tone="muted-foreground"
                      />
                    </View>
                    <If when={passedChecksExpanded()}>
                      <View style={{ paddingLeft: 18, gap: 3 }}>
                        <Each of={partitionedGates().passed} key={(g) => g.id}>
                          {(gate) => (
                            <Text style={{ fontSize: 12 }} class="text-secondary-foreground">
                              {`${humanizeCheckText(gate().title, isZh())}  ${humanizeCheckText(gate().message, isZh())}`}
                            </Text>
                          )}
                        </Each>
                      </View>
                    </If>
                  </View>
                </If>
              </If>
            </Scroll>
          </View>
        </If>

        {/* ── P0-3 Anchored Overlay Popover: Subset Filter (Solutions + Test Tags, NEVER pushes table!) ── */}
        <If when={filterOpen()}>
          <View
            testID="build-filter-popover"
            style={{
              position: "absolute",
              top: 6,
              right: 14,
              width: 360,
              borderRadius: 10,
              borderWidth: 1,
              boxShadow: popoverShadow(),
              padding: 12,
              gap: 10,
            }}
            class="bg-popover border-border"
          >
            <View style={{ gap: 6 }}>
              <Text style={{ fontSize: 11, fontWeight: 500 }} class="text-muted-foreground">
                {t("build.tree.solutions")}
              </Text>
              <View style={{ flexDirection: "row", flexWrap: "wrap", gap: 6 }}>
                <Each of={solutionSpecs()} key={(s) => s.name}>
                  {(spec) => {
                    const active = () => filterSolutions().includes(spec().name);
                    return (
                      <View
                        onPress={() => filterSolutions.set(toggleSubsetSelection(filterSolutions(), spec().name))}
                        class={active() ? "bg-primary text-primary-foreground" : "bg-muted hover:bg-accent"}
                        style={{ height: 24, paddingLeft: 8, paddingRight: 8, borderRadius: 5, justifyContent: "center" }}
                      >
                        <Text
                          style={{ fontSize: 12, fontWeight: 500 }}
                          class={active() ? "text-primary-foreground" : "text-foreground"}
                        >
                          {spec().name}
                        </Text>
                      </View>
                    );
                  }}
                </Each>
              </View>
            </View>

            <If when={testTags().length > 0}>
              <View style={{ gap: 6 }}>
                <Text style={{ fontSize: 11, fontWeight: 500 }} class="text-muted-foreground">
                  {t("build.filter.testsGroup")}
                </Text>
                <View style={{ flexDirection: "row", flexWrap: "wrap", gap: 6 }}>
                  <Each of={testTags()} key={(tg) => tg}>
                    {(tg) => {
                      const active = () => filterTests().includes(tg());
                      return (
                        <View
                          onPress={() => filterTests.set(toggleSubsetSelection(filterTests(), tg()))}
                          class={active() ? "bg-primary text-primary-foreground" : "bg-muted hover:bg-accent"}
                          style={{ height: 24, paddingLeft: 8, paddingRight: 8, borderRadius: 5, justifyContent: "center" }}
                        >
                          <Text
                            style={{ fontSize: 12, fontWeight: 500 }}
                            class={active() ? "text-primary-foreground" : "text-foreground"}
                          >
                            {tg()}
                          </Text>
                        </View>
                      );
                    }}
                  </Each>
                </View>
              </View>
            </If>

            <If when={activeFilterCount() > 0}>
              <View style={{ flexDirection: "row", justifyContent: "flex-end", borderTopWidth: 1, paddingTop: 8 }} class="border-border">
                <Btn
                  label={t("build.filter.clear")}
                  icon="filterX"
                  variant="ghost"
                  onPress={() => {
                    filterSolutions.set([]);
                    filterTests.set([]);
                  }}
                />
              </View>
            </If>
          </View>
        </If>

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

// ── Left Object Tree subcomponents ───────────────────────────────────────────

function TreeSectionHeader(props: { label: string; action?: ReturnType<typeof IconBtn> | ReturnType<typeof AppMenu> }) {
  return (
    <View
      style={{
        height: 24,
        paddingLeft: 8,
        paddingRight: 2,
        flexDirection: "row",
        alignItems: "center",
        justifyContent: "space-between",
      }}
    >
      {/* P2: 11px muted regular weight for clear hierarchy */}
      <Text style={{ fontSize: 11, fontWeight: 400, letterSpacing: 0.2 }} class="text-muted-foreground">
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
  statusDot?: "ok" | "fail";
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
        height: 28,
        paddingLeft: 8,
        paddingRight: 8,
        borderRadius: 6,
        flexDirection: "row",
        alignItems: "center",
        gap: 7,
      }}
    >
      <Icon
        name={props.icon}
        size={13.5}
        tone={props.active ? "foreground" : "secondary-foreground"}
      />
      <Text
        style={{
          flex: 1,
          fontSize: 12.5,
          fontWeight: 500,
          fontVariant: ["tabular-nums"],
          numberOfLines: 1,
          textOverflow: "ellipsis",
        }}
        class={props.active ? "text-foreground" : "text-secondary-foreground"}
      >
        {props.label}
      </Text>
      <If when={!!props.sub}>
        <Text style={{ fontSize: 11, flexShrink: 0 }} class="text-muted-foreground">
          {props.sub!}
        </Text>
      </If>
      {/* P1-6: small semantic dot instead of checkmark ✓ */}
      <If when={props.statusDot === "fail"}>
        <View style={{ width: 6, height: 6, borderRadius: 3, backgroundColor: "#dc2626", flexShrink: 0 }} />
      </If>
      <If when={props.statusDot === "ok"}>
        <View style={{ width: 6, height: 6, borderRadius: 3, backgroundColor: "#16a34a", opacity: 0.65, flexShrink: 0 }} />
      </If>
    </View>
  );
}

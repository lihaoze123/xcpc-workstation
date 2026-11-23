// Local Polygon Authoring Console (Phase 3B "less is more" architecture):
// - Zero stacked toolbars: Authoring controls live directly in StudioView's TitleBar trailing slot
// - Decluttered Left Object Tree (~228pt) with logical test groups (Samples / Rules / Self-tests)
// - P0-2 Single State Model: structure from live disk `inspection`, verdicts merge `liveCells` + `report.matrix`
// - Zero-jitter live runs: reserved 2px progress hairline at top of right pane, in-place Verify/Cancel swap
// - P1-7 Right-overlay 3-column diagnostic drawer (never squeezes the table)
// - Quiet footer below Results Table for Safety Margin metrics & Only-Anomalies toggle

import { cleanup, colorScheme, computed, effect, Scroll, signal, Text, untracked, View } from "nsh";
import type {
  PackageBuildStatusJs,
  PackageInspectionResultJs,
  TestCasePreviewJs,
} from "xcpc-workstation/native";
import { ErrorDisplay } from "../ErrorDisplay";
import { Icon, type IconName } from "../Icon";
import { locale, t } from "../i18n";
import { Btn, IconBtn, Segmented, tip } from "../kit";
import { EmptyState } from "../layout";
import { AppMenu, type MenuEntry } from "../Menu";
import { goSpace, onExternalChange, preview, type UiError } from "../store";
import { MONO } from "../theme";
import { CellDiagnosticDrawer, InlineFileEditor } from "./BuildDetailPane";
import {
  BUILTIN_CHECKERS,
  canInlineEditTest,
  cellVisualStyle,
  countTestsInRule,
  EXPECT_PRESETS,
  formatAcVsTleMargin,
  formatBytesCompact,
  formatSolutionConclusion,
  formatStdTimeRatio,
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
  resolveSingleStatusBadge,
  resolveSolutionHeaderSpecs,
  solutionRole,
  toggleSubsetSelection,
  type BuildReportData,
  type MatrixCellData,
  type PackageInspectionData,
  type TestSummaryData,
} from "./buildLogic";
import { AddGeneratorBatchModal, ConfirmDeleteRuleModal, CreateSourceModal } from "./PackageModals";

export interface BuildConsoleController {
  triggerPrimaryRun(): void;
  statusPill(): { label: string; bg: string; fg: string; expanded: boolean };
  toggleChecks(): void;
  showFilter(): boolean;
  filterCount(): number;
  toggleFilter(): void;
  running(): boolean;
  startRun(mode: "debug" | "verify"): void;
  cancelRun(): void;
}

/** Authoring actions rendered directly in StudioView's TitleBar trailing slot (zero stacked toolbars). */
export function AuthoringTitleBarControls(props: { ctrl: BuildConsoleController; readOnly: boolean }) {
  const pill = () => props.ctrl.statusPill();
  const running = () => props.ctrl.running();
  const filterCount = () => props.ctrl.filterCount();

  return (
    <View style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>
      {/* Single Status Pill (clickable to expand/collapse checks) */}
      <View
        testID="build-single-status-pill"
        onPress={() => props.ctrl.toggleChecks()}
        style={{
          height: 24,
          paddingLeft: 9,
          paddingRight: 7,
          borderRadius: 6,
          backgroundColor: pill().bg,
          flexDirection: "row",
          alignItems: "center",
          gap: 4,
          flexShrink: 0,
        }}
      >
        <Text style={{ fontSize: 12, fontWeight: 500, color: pill().fg, fontVariant: ["tabular-nums"] }}>
          {pill().label}
        </Text>
        <Icon name={pill().expanded ? "chevronDown" : "chevronRight"} size={12} tone="secondary-foreground" />
      </View>

      {/* Filter toggle (when on Results view) */}
      <If when={props.ctrl.showFilter()}>
        <Btn
          testID="build-filter-toggle"
          label={
            filterCount() > 0
              ? t("build.filter.activeCount", { count: filterCount() })
              : t("build.filter.button")
          }
          icon="list"
          variant={filterCount() > 0 ? "outline" : "ghost"}
          onPress={() => props.ctrl.toggleFilter()}
        />
      </If>

      {/* Fixed-width primary CTA slot: Verify <-> outline Cancel in-place (zero horizontal shift) */}
      <View style={{ width: 132, flexDirection: "row", justifyContent: "flex-end", alignItems: "center" }}>
        <If
          when={!running()}
          else={
            <Btn
              testID="build-cancel-btn"
              label={t("common.cancel")}
              icon="x"
              variant="outline"
              onPress={() => props.ctrl.cancelRun()}
              style={{ width: 132 }}
            />
          }
        >
          <View style={{ flexDirection: "row", alignItems: "center" }}>
            <Btn
              testID="build-verify-btn"
              label={
                filterCount() > 0
                  ? t("build.action.verifySelected", { count: filterCount() })
                  : t("build.action.verify")
              }
              icon="play"
              variant="primary"
              disabled={props.readOnly}
              tooltip={tip(t("build.action.verify"), "mod+b")}
              onPress={() => props.ctrl.startRun("verify")}
              style={{
                width: 106,
                borderTopRightRadius: 0,
                borderBottomRightRadius: 0,
              }}
            />
            <AppMenu
              align="end"
              minWidth={176}
              entries={[{ id: "debug", label: t("build.action.quickRun"), icon: "play" }]}
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
      </View>
    </View>
  );
}

type TreeSelection =
  | { kind: "results" }
  | { kind: "testGroup"; groupId: string; testIndex: number }
  | { kind: "selfTests"; relPath?: string }
  | { kind: "validator" }
  | { kind: "checker" }
  | { kind: "generator"; file: string }
  | { kind: "solution"; name: string };

type TestPaneTab = "input" | "sample_ans" | "std_out";

const COL_TEST_W = 200;
const COL_SOL_W = 112;

export function BuildConsole(props: {
  problemId: string;
  readOnly: boolean;
  inspectionResult: PackageInspectionResultJs | undefined;
  onInitPackage?: () => void;
  onRefreshPackage: () => void;
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
  const testEditing = signal(false);
  const testPreview = signal<TestCasePreviewJs | undefined>(undefined);
  const fileRefreshKey = signal(0);

  // Modals & transient feedback
  const createModalKind = signal<"solution" | "generator" | undefined>(undefined);
  const addGenModalOpen = signal(false);
  const deleteRuleIndex = signal<number | undefined>(undefined);
  const dirtyUndoNotice = signal<number | undefined>(undefined);
  const notice = signal<UiError | undefined>(undefined);

  // Live build runner polling state
  const buildStatus = signal<PackageBuildStatusJs>(preview.packageBuildStatus());
  const finishedReport = signal<BuildReportData | undefined>(undefined);
  const liveCells = signal<MatrixCellData[]>([]);

  // Reset per-problem state when switching problems
  let lastSlug = "";
  effect(() => {
    const slug = props.problemId;
    untracked(() => {
      if (slug !== lastSlug) {
        lastSlug = slug;
        selection.set({ kind: "results" });
        selectedCell.set(undefined);
        testEditing.set(false);
        testPaneTab.set("input");
        filterSolutions.set([]);
        filterTests.set([]);
        onlyAnomalyRows.set(false);
        checksExpanded.set(false);
        dirtyUndoNotice.set(undefined);
        notice.set(undefined);
        finishedReport.set(undefined);
        liveCells.set([]);
      }
    });
  });

  // Subscribe to external filesystem changes (Fix A-2: CLI rebuilds, external editor saves)
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
      props.onRefreshPackage();
    }
  });
  cleanup(unsubWatch);

  // Parse live inspection from disk
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
          // ignore malformed partial json
        }
      }
      if (!next.running) {
        stopPolling();
        if (next.seq !== lastSeenSeq || next.reportJson || next.error) {
          lastSeenSeq = next.seq;
          liveCells.set([]);
          if (next.reportJson && next.slug === untracked(() => props.problemId)) {
            try {
              finishedReport.set(JSON.parse(next.reportJson) as BuildReportData);
            } catch {
              // ignore
            }
          }
          if (next.error) {
            notice.set({ error: next.error, context: t("ctx.packageBuild") });
          } else {
            notice.set(undefined);
          }
          props.onRefreshPackage();
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

  // Logical test groups for the Left Object Tree (Samples / Rules / Self-tests)
  const testGroups = computed(() => groupTestRows(inspection()?.expanded_tests, testRows()));
  const selfTestFiles = computed(() => inspection()?.files?.self_test_files ?? []);
  const hasSelfTestFailure = computed(() =>
    (activeReport()?.self_tests ?? []).some((s) => !s.passed),
  );

  const matrixLookup = computed(() =>
    mergeLiveMatrixCells(activeReport()?.matrix, isRunningThis() ? liveCells() : undefined),
  );

  const summaryBySolution = computed(() => {
    const m = new Map<string, NonNullable<BuildReportData["solutions"]>[number]>();
    for (const s of activeReport()?.solutions ?? []) m.set(s.name, s);
    return m;
  });

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

  const selectedCellData = computed(() => {
    const sel = selectedCell();
    if (!sel) return undefined;
    return matrixLookup().get(matrixCellKey(sel.testIndex, sel.solution));
  });

  // Refresh test case preview whenever the selected test changes or disk refreshes
  effect(() => {
    const sel = selection();
    const slug = props.problemId;
    void props.inspectionResult;
    void fileRefreshKey();
    untracked(() => {
      if (sel.kind !== "testGroup") {
        testPreview.set(undefined);
        testEditing.set(false);
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
    });
  });

  // ── Actions ────────────────────────────────────────────────────────────────

  function selectTestByIndex(testIndex: number, startEditing = false) {
    const grp = testGroups().find((g) => g.tests.some((t) => t.index === testIndex));
    const groupId = grp?.id ?? "samples";
    testPaneTab.set("input");
    testEditing.set(startEditing);
    selection.set({ kind: "testGroup", groupId, testIndex });
  }

  function startRun(mode: "debug" | "verify") {
    if (props.readOnly || isRunningThis()) return;
    notice.set(undefined);
    const hasFilter = filterTests().length > 0 || filterSolutions().length > 0;
    const effectiveMode = hasFilter ? "debug" : mode;
    const testsArg = hasFilter && filterTests().length > 0 ? filterTests() : null;
    const solsArg = hasFilter && filterSolutions().length > 0 ? filterSolutions() : null;
    liveCells.set([]);
    const res = preview.startPackageBuild(props.problemId, effectiveMode, testsArg, solsArg);
    if (!res.ok) {
      notice.set({ code: res.code, error: res.error, context: t("ctx.packageBuild") });
      return;
    }
    const next = preview.packageBuildStatus();
    lastSeenSeq = next.seq - 1;
    buildStatus.set(next);
    startPolling();
  }

  function cancelRun() {
    preview.cancelPackageBuild();
    buildStatus.set(preview.packageBuildStatus());
  }

  function triggerPrimaryRun() {
    if (!hasPackage() || isRunningThis()) return;
    startRun("verify");
  }

  const activeFilterCount = computed(() => filterTests().length + filterSolutions().length);
  const progressRatio = computed(() => {
    if (!isRunningThis()) return 0;
    const tot = buildStatus().total;
    if (tot <= 0) return 0.18;
    return Math.max(0.06, Math.min(1, buildStatus().completed / tot));
  });

  const statusPillStyle = computed(() => {
    const st = singleStatus();
    const dark = colorScheme() === "dark";
    if (st.kind === "ready") {
      return {
        label: t("build.status.ready"),
        bg: dark ? "rgba(34, 197, 94, 0.12)" : "rgba(22, 163, 74, 0.08)",
        fg: dark ? "#4ade80" : "#15803d",
        expanded: checksExpanded(),
      };
    }
    if (st.kind === "issues") {
      return {
        label: t("build.status.issues", { count: st.issueCount }),
        bg: dark ? "rgba(239, 68, 68, 0.14)" : "rgba(220, 38, 38, 0.08)",
        fg: dark ? "#f87171" : "#b91c1c",
        expanded: checksExpanded(),
      };
    }
    if (st.kind === "needs_reverify") {
      return {
        label: t("build.status.needsReverify"),
        bg: dark ? "rgba(245, 158, 11, 0.14)" : "rgba(217, 119, 6, 0.09)",
        fg: dark ? "#fbbf24" : "#b45309",
        expanded: checksExpanded(),
      };
    }
    return {
      label: t("build.status.unverified"),
      bg: dark ? "rgba(255, 255, 255, 0.06)" : "rgba(24, 24, 27, 0.05)",
      fg: dark ? "#a1a1aa" : "#52525b",
      expanded: checksExpanded(),
    };
  });

  // Expose live controller for StudioView's TitleBar trailing slot
  props.controller?.({
    triggerPrimaryRun,
    statusPill: () => statusPillStyle(),
    toggleChecks: () => {
      if (selection().kind !== "results") selection.set({ kind: "results" });
      checksExpanded.set(!checksExpanded());
    },
    showFilter: () => selection().kind === "results",
    filterCount: () => activeFilterCount(),
    toggleFilter: () => filterOpen.set(!filterOpen()),
    running: () => isRunningThis(),
    startRun,
    cancelRun,
  });
  cleanup(() => props.controller?.(undefined));

  function openExternal(relPath: string) {
    void preview.openPackageFile(props.problemId, relPath).then((res) => {
      if (!res.ok) notice.set({ code: res.code, error: res.error, context: t("ctx.openPackageFile") });
    });
  }

  function openFileInTree(relPath: string) {
    if (relPath === "validator.cpp" || relPath === inspection()?.files?.validator) {
      selection.set({ kind: "validator" });
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

  // Fix A-1: Immediately select the newly created hand test and open its inline editor bound to its exact fileRel!
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
    testEditing.set(true);
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

  return (
    <View style={{ flex: 1, minHeight: 0 }} class="bg-card" testID="build-console">
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
              borderLeftWidth: 3,
              flexDirection: "row",
              alignItems: "center",
              justifyContent: "space-between",
              gap: 10,
            }}
            class="border-b-border border-l-destructive bg-muted/30"
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
            {/* 1. Results overview (no redundant count badge) */}
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
              />
              <TreeRow
                testID="tree-item-checker"
                icon="scale"
                label={t("build.tree.checker")}
                sub={inspection()?.checker || "wcmp"}
                active={selection().kind === "checker"}
                onPress={() => selection.set({ kind: "checker" })}
              />
            </View>

            {/* 3. Solutions (crown icon identifies main solution without redundant 标程 pill) */}
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
                  const sum = () => summaryBySolution().get(spec().name);
                  return (
                    <TreeRow
                      testID={`tree-item-solution-${spec().name}`}
                      icon={role() === "main" ? "crown" : "code"}
                      label={spec().name}
                      sub={
                        role() === "main"
                          ? undefined
                          : role() === "alt"
                            ? t("build.role.alt")
                            : spec().expected.join("/")
                      }
                      active={selection().kind === "solution" && (selection() as { kind: "solution"; name: string }).name === spec().name}
                      statusDot={sum() ? (sum()!.matched ? "ok" : "fail") : undefined}
                      onPress={() => selection.set({ kind: "solution", name: spec().name })}
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
                  />
                )}
              </Each>
            </View>

            {/* 5. Test Data — logically grouped: Samples / Rules (Batches) / Self-tests */}
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
                        else if (id === "gen") addGenModalOpen.set(true);
                        else if (id === "valid") handleAddSelfTest("validator_self_test_valid");
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
                        testEditing.set(false);
                        testPaneTab.set("input");
                        selection.set({ kind: "testGroup", groupId: grp().id, testIndex: firstIdx });
                      }}
                    />
                  );
                }}
              </Each>

              {/* Self-tests logical group */}
              <TreeRow
                testID="tree-item-self-tests"
                icon="shieldCheck"
                label={`${t("build.tree.selfTests")} (${selfTestFiles().length})`}
                active={selection().kind === "selfTests"}
                statusDot={hasSelfTestFailure() ? "fail" : undefined}
                onPress={() => selection.set({ kind: "selfTests", relPath: selfTestFiles()[0] })}
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
                {/* Expandable Borderless Check List (Failed -> Suggestions -> Passed collapsed to 1 row) */}
                <If when={checksExpanded() && gates().length > 0}>
                  <View
                    testID="build-checks-panel"
                    style={{
                      borderBottomWidth: 1,
                      paddingLeft: 16,
                      paddingRight: 16,
                      paddingTop: 10,
                      paddingBottom: 10,
                      gap: 6,
                    }}
                    class="border-border bg-background/20"
                  >
                    <Each of={partitionedGates().attention} key={(g) => g.id}>
                      {(gate) => {
                        const action = () => nextActionForCheck(gate(), activeReport(), inspection()?.files);
                        return (
                          <View
                            style={{
                              paddingLeft: 10,
                              paddingTop: 4,
                              paddingBottom: 4,
                              borderLeftWidth: 2,
                              flexDirection: "row",
                              alignItems: "center",
                              justifyContent: "space-between",
                              gap: 10,
                            }}
                            class={gate().status === "fail" ? "border-l-destructive" : "border-l-chart-4"}
                          >
                            <View style={{ flex: 1, minWidth: 0, flexDirection: "row", alignItems: "center", gap: 8 }}>
                              <Icon
                                name={gate().status === "fail" ? "circleAlert" : "triangleAlert"}
                                size={13}
                                tone={gate().status === "fail" ? "destructive" : "chart-4"}
                              />
                              <Text style={{ fontSize: 12, fontWeight: 500 }} class="text-foreground">
                                {humanizeCheckText(gate().title, isZh())}
                              </Text>
                              <Text
                                style={{ fontSize: 12, flex: 1, numberOfLines: 1, textOverflow: "ellipsis" }}
                                class="text-secondary-foreground"
                              >
                                {humanizeCheckText(gate().message, isZh())}
                              </Text>
                            </View>
                            <If when={action()}>
                              {(act) => (
                                <Btn
                                  label={t("build.check.fix")}
                                  variant="ghost"
                                  onPress={() => {
                                    const a = act();
                                    if (a.kind === "view_test") selectTestByIndex(a.testIndex);
                                    else if (a.kind === "select_cell") selectedCell.set({ testIndex: a.testIndex, solution: a.solution });
                                    else if (a.kind === "open_file") openFileInTree(a.relPath);
                                  }}
                                />
                              )}
                            </If>
                          </View>
                        );
                      }}
                    </Each>

                    {/* Passed checks collapsed to 1 row */}
                    <If when={partitionedGates().passed.length > 0}>
                      <View style={{ gap: 4 }}>
                        <View
                          onPress={() => passedChecksExpanded.set(!passedChecksExpanded())}
                          style={{ flexDirection: "row", alignItems: "center", gap: 6, paddingLeft: 2, paddingTop: 2 }}
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
                  </View>
                </If>

                {/* Dirty inputs borderless left-accented notice + Undo */}
                <If when={dirtyInputs().length > 0}>
                  <View
                    testID="dirty-inputs-banner"
                    style={{
                      paddingLeft: 16,
                      paddingRight: 14,
                      paddingTop: 7,
                      paddingBottom: 7,
                      borderBottomWidth: 1,
                      borderLeftWidth: 3,
                      flexDirection: "row",
                      alignItems: "center",
                      justifyContent: "space-between",
                      gap: 10,
                    }}
                    class="border-b-border border-l-chart-4 bg-background/30"
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
                      borderLeftWidth: 3,
                      flexDirection: "row",
                      alignItems: "center",
                      justifyContent: "space-between",
                      gap: 10,
                    }}
                    class="border-b-border border-l-primary bg-background/30"
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

                {/* Compile failures borderless left-accented rows */}
                <If when={compileFailures().length > 0}>
                  <View style={{ borderBottomWidth: 1 }} class="border-border bg-background/20">
                    <Each of={compileFailures()} key={(cf) => cf.target}>
                      {(cf) => (
                        <View
                          style={{
                            paddingLeft: 16,
                            paddingRight: 14,
                            paddingTop: 8,
                            paddingBottom: 8,
                            borderLeftWidth: 3,
                            gap: 4,
                          }}
                          class="border-l-destructive"
                        >
                          <View style={{ flexDirection: "row", alignItems: "center", justifyContent: "space-between", gap: 8 }}>
                            <Text style={{ fontSize: 12, fontWeight: 600 }} class="text-destructive">
                              {t("build.compileError.title", { target: cf().source_path })}
                            </Text>
                            <Btn
                              label={t("build.compileError.openSource")}
                              icon="code"
                              variant="ghost"
                              onPress={() => openFileInTree(cf().source_path)}
                            />
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

                {/* Subset filter bar (collapsible) */}
                <If when={filterOpen()}>
                  <View
                    style={{
                      paddingLeft: 16,
                      paddingRight: 16,
                      paddingTop: 8,
                      paddingBottom: 8,
                      borderBottomWidth: 1,
                      gap: 6,
                    }}
                    class="border-border bg-background/40"
                  >
                    <View style={{ flexDirection: "row", alignItems: "center", gap: 6, flexWrap: "wrap" }}>
                      <Text style={{ fontSize: 12, fontWeight: 500 }} class="text-secondary-foreground">
                        {t("build.tree.solutions")}
                      </Text>
                      <Each of={solutionSpecs()} key={(s) => s.name}>
                        {(spec) => {
                          const active = () => filterSolutions().includes(spec().name);
                          return (
                            <View
                              onPress={() => filterSolutions.set(toggleSubsetSelection(filterSolutions(), spec().name))}
                              class={active() ? "bg-primary text-primary-foreground" : "bg-muted hover:bg-accent"}
                              style={{ height: 22, paddingLeft: 8, paddingRight: 8, borderRadius: 5, justifyContent: "center" }}
                            >
                              <Text
                                style={{ fontSize: 11, fontWeight: 500 }}
                                class={active() ? "text-primary-foreground" : "text-foreground"}
                              >
                                {spec().name}
                              </Text>
                            </View>
                          );
                        }}
                      </Each>
                      <If when={activeFilterCount() > 0}>
                        <Btn
                          label={t("build.filter.clear")}
                          icon="filterX"
                          variant="ghost"
                          onPress={() => {
                            filterSolutions.set([]);
                            filterTests.set([]);
                          }}
                        />
                      </If>
                    </View>
                  </View>
                </If>

                {/* Borderless Full-Bleed Results Table (starts directly at top!) */}
                <Scroll horizontal style={{ flex: 1, minHeight: 0 }} contentStyle={{ flexGrow: 1 }}>
                  <View style={{ flex: 1, minHeight: 0, minWidth: COL_TEST_W + solutionSpecs().length * COL_SOL_W + 24 }}>
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
                            style={{ width: COL_SOL_W, flexShrink: 0, flexDirection: "row", alignItems: "center", gap: 4, paddingRight: 8 }}
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

                    {/* Sticky Summary Conclusion Row */}
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
                            const cells: MatrixCellData[] = [];
                            const lookup = matrixLookup();
                            for (const row of testRows()) {
                              const c = lookup.get(matrixCellKey(row.index, spec().name));
                              if (c) cells.push(c);
                            }
                            return formatSolutionConclusion(sum, cells, isZh());
                          };
                          return (
                            <View style={{ width: COL_SOL_W, flexShrink: 0, paddingRight: 8 }}>
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
                            </View>
                          );
                        }}
                      </Each>
                    </View>

                    {/* Scrollable Compact Single-Line Test Rows (34px height) */}
                    <Scroll style={{ flex: 1, minHeight: 0 }}>
                      <Each of={displayedTableRows()} key={(r) => String(r.index)}>
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
                            {/* Test case label cell -> jumps directly to that test in right pane */}
                            <View
                              onPress={() => selectTestByIndex(row().index)}
                              style={{
                                width: COL_TEST_W,
                                flexShrink: 0,
                                flexDirection: "row",
                                alignItems: "center",
                                gap: 6,
                                paddingRight: 10,
                              }}
                            >
                              <Text
                                style={{ fontSize: 12, fontVariant: ["tabular-nums"], width: 20, flexShrink: 0 }}
                                class="text-secondary-foreground"
                              >
                                {String(row().index).padStart(2, "0")}
                              </Text>
                              <Text
                                style={{ fontSize: 13, fontWeight: 500, numberOfLines: 1, textOverflow: "ellipsis", flex: 1 }}
                                class="text-foreground"
                              >
                                {row().name}
                              </Text>
                              <If when={row().validator_ok === false}>
                                <Icon name="triangleAlert" size={12} tone="destructive" />
                              </If>
                              <If when={row().size_bytes > 0}>
                                <Text style={{ fontSize: 11, fontVariant: ["tabular-nums"], flexShrink: 0 }} class="text-secondary-foreground">
                                  {formatBytesCompact(row().size_bytes)}
                                </Text>
                              </If>
                            </View>

                            {/* Solution verdict cells (fixed 112px width, P1-6 visual coding) */}
                            <Each of={solutionSpecs()} key={(s) => s.name}>
                              {(spec) => {
                                const cell = () => matrixLookup().get(matrixCellKey(row().index, spec().name));
                                const sum = () => summaryBySolution().get(spec().name);
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
                                        selectedCell.set({ testIndex: row().index, solution: spec().name });
                                      }
                                    }}
                                    style={{
                                      width: COL_SOL_W,
                                      flexShrink: 0,
                                      paddingRight: 8,
                                      justifyContent: "center",
                                    }}
                                  >
                                    <If
                                      when={cell()}
                                      else={
                                        <Text style={{ fontSize: 12 }} class="text-muted-foreground">
                                          {"-"}
                                        </Text>
                                      }
                                    >
                                      {(c) => {
                                        const st = () => cellVisualStyle(c().verdict, isAnom(), colorScheme() === "dark");
                                        return (
                                          <View
                                            style={{
                                              height: 24,
                                              paddingLeft: 7,
                                              paddingRight: 7,
                                              borderRadius: 5,
                                              borderWidth: 1,
                                              backgroundColor: st().bg,
                                              borderColor: isPicked() ? st().fg : st().border,
                                              opacity: c().stale ? 0.55 : 1,
                                              flexDirection: "row",
                                              alignItems: "center",
                                              justifyContent: "space-between",
                                              gap: 4,
                                            }}
                                          >
                                            <View style={{ flexDirection: "row", alignItems: "center", gap: 3 }}>
                                              <If when={isAnom()}>
                                                <Icon name="triangleAlert" size={11} tone="destructive" />
                                              </If>
                                              <Text
                                                style={{
                                                  fontSize: 11,
                                                  fontWeight: 600,
                                                  color: st().fg,
                                                  fontVariant: ["tabular-nums"],
                                                }}
                                              >
                                                {c().verdict}
                                              </Text>
                                            </View>
                                            <Text
                                              style={{
                                                fontSize: 11,
                                                color: st().fg,
                                                opacity: 0.85,
                                                fontVariant: ["tabular-nums"],
                                              }}
                                            >
                                              {`${c().cpu_time_ms}ms`}
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
                    </Scroll>
                  </View>
                </Scroll>

                {/* Quiet Footer Bar: Safety Margin metrics + Live progress detail + Only Anomalies toggle */}
                <If when={Boolean(activeReport()?.safety_margin) || anomalousRowIndices().size > 0 || isRunningThis()}>
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
                            style={{ fontSize: 11.5, numberOfLines: 1, textOverflow: "ellipsis", fontVariant: ["tabular-nums"] }}
                            class="text-secondary-foreground"
                          >
                            {buildStatus().detail || t("build.running.preparing")}
                          </Text>
                        }
                      >
                        <If when={activeReport()?.safety_margin}>
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
                  </View>
                </If>

                {/* P1-7 Right-Overlay 3-Column Diagnostic Drawer (absolute, NEVER squeezes table!) */}
                <If when={selectedCellData()}>
                  {(cell) => {
                    const spec = () => solutionSpecs().find((s) => s.name === cell().solution);
                    const sum = () => summaryBySolution().get(cell().solution);
                    const rowMeta = () => testRows().find((r) => r.index === cell().test_index);
                    const isAnom = () => {
                      const sp = spec();
                      return sp ? isCellVerdictAnomaly(cell().verdict, sp.expected, sp.is_main, sum()?.matched) : false;
                    };
                    return (
                      <CellDiagnosticDrawer
                        problemId={props.problemId}
                        cell={cell()}
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

            {/* B. Logical Test Group Detail View (Samples / Batch / Hand test) */}
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
                const editable = () => !props.readOnly && canInlineEditTest(prev());
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

                return (
                  <View style={{ flex: 1, minHeight: 0 }}>
                    {/* Header: Name + Target relative path + Fixed-slot controls */}
                    <View
                      style={{
                        height: 44,
                        paddingLeft: 16,
                        paddingRight: 14,
                        borderBottomWidth: 1,
                        flexDirection: "row",
                        alignItems: "center",
                        justifyContent: "space-between",
                        gap: 10,
                      }}
                      class="border-border"
                    >
                      <View style={{ flexDirection: "row", alignItems: "center", gap: 8, flex: 1, minWidth: 0 }}>
                        <Text style={{ fontSize: 13, fontWeight: 600, fontVariant: ["tabular-nums"] }} class="text-foreground">
                          {`#${String(activeIdx()).padStart(2, "0")} ${row()?.name ?? prev()?.name ?? ""}`}
                        </Text>
                        <If when={fileRel()}>
                          {(rel) => (
                            <Text
                              style={{ fontSize: 12, fontFamily: MONO, numberOfLines: 1, textOverflow: "ellipsis" }}
                              class="text-secondary-foreground"
                            >
                              {rel()}
                            </Text>
                          )}
                        </If>
                      </View>

                      <View style={{ flexDirection: "row", alignItems: "center", gap: 6, flexShrink: 0 }}>
                        <If when={paneItems().length > 1 && !testEditing()}>
                          <Segmented
                            items={paneItems()}
                            value={testPaneTab()}
                            onChange={(v) => testPaneTab.set(v)}
                            itemWidth={isZh() ? 86 : 98}
                          />
                        </If>
                        <If when={editable() && testPaneTab() === "input"}>
                          <Btn
                            testID="test-inline-edit-toggle"
                            label={testEditing() ? t("build.tests.doneEdit") : t("build.tests.inlineEdit")}
                            icon={testEditing() ? "check" : "pencil"}
                            variant={testEditing() ? "outline" : "ghost"}
                            onPress={() => testEditing.set(!testEditing())}
                          />
                        </If>
                        <If when={!props.readOnly && activeGroup()?.kind !== "samples" && ruleIdx() != null}>
                          <IconBtn
                            testID="test-delete-rule-btn"
                            icon="trash"
                            tone="destructive"
                            tooltip={tip(t("build.tests.deleteRule"))}
                            onPress={() => deleteRuleIndex.set(ruleIdx()!)}
                          />
                        </If>
                      </View>
                    </View>

                    {/* Compact batch member selector strip when group has > 1 test case */}
                    <If when={(activeGroup()?.tests.length ?? 0) > 1}>
                      <Scroll
                        horizontal
                        style={{ maxHeight: 38, borderBottomWidth: 1 }}
                        contentStyle={{
                          paddingLeft: 16,
                          paddingRight: 16,
                          paddingTop: 6,
                          paddingBottom: 6,
                          flexDirection: "row",
                          alignItems: "center",
                          gap: 6,
                        }}
                        class="border-border bg-background/30"
                      >
                        <Each of={activeGroup()?.tests ?? []} key={(item) => String(item.index)}>
                          {(item) => {
                            const selected = () => item().index === activeIdx();
                            return (
                              <View
                                onPress={() => {
                                  testEditing.set(false);
                                  selection.set({
                                    kind: "testGroup",
                                    groupId: activeGroup()!.id,
                                    testIndex: item().index,
                                  });
                                }}
                                class={selected() ? "bg-muted border border-border" : "border border-transparent hover:bg-muted/60"}
                                style={{
                                  height: 24,
                                  paddingLeft: 8,
                                  paddingRight: 8,
                                  borderRadius: 5,
                                  flexDirection: "row",
                                  alignItems: "center",
                                  gap: 4,
                                }}
                              >
                                <Text
                                  style={{ fontSize: 12, fontWeight: 500, fontVariant: ["tabular-nums"] }}
                                  class={selected() ? "text-foreground" : "text-secondary-foreground"}
                                >
                                  {`#${String(item().index).padStart(2, "0")}`}
                                </Text>
                                <If when={item().validator_ok === false}>
                                  <Icon name="triangleAlert" size={11} tone="destructive" />
                                </If>
                              </View>
                            );
                          }}
                        </Each>
                      </Scroll>
                    </If>

                    {/* Validator failure callout */}
                    <If when={row()?.validator_ok === false}>
                      <View
                        style={{
                          paddingLeft: 16,
                          paddingRight: 14,
                          paddingTop: 8,
                          paddingBottom: 8,
                          borderBottomWidth: 1,
                          borderLeftWidth: 3,
                          flexDirection: "row",
                          alignItems: "center",
                          justifyContent: "space-between",
                          gap: 10,
                        }}
                        class="border-b-border border-l-destructive bg-background/30"
                      >
                        <View style={{ flex: 1, minWidth: 0, gap: 2 }}>
                          <Text style={{ fontSize: 12, fontWeight: 600 }} class="text-destructive">
                            {t("build.tests.validatorFailed")}
                          </Text>
                          <If when={!!row()?.validator_message}>
                            <Text style={{ fontSize: 12, fontFamily: MONO }} class="text-foreground">
                              {row()!.validator_message!}
                            </Text>
                          </If>
                        </View>
                        <Btn
                          label={t("build.tests.openValidator")}
                          icon="shieldCheck"
                          variant="outline"
                          onPress={() => selection.set({ kind: "validator" })}
                        />
                      </View>
                    </If>

                    {/* Body: Either InlineFileEditor (Fix A-1) or Bounded Preview (Fix A-6) */}
                    <If
                      when={testEditing() && !!fileRel() && testPaneTab() === "input"}
                      else={
                        <Scroll style={{ flex: 1, minHeight: 0 }} contentStyle={{ padding: 16, gap: 12 }}>
                          <View style={{ flexDirection: "row", alignItems: "center", justifyContent: "space-between", gap: 8 }}>
                            <Text style={{ fontSize: 12, fontVariant: ["tabular-nums"] }} class="text-secondary-foreground">
                              {activeContent().linesExact != null
                                ? `${formatBytesCompact(activeContent().bytes)}   ${t("build.tests.linesCount", { count: activeContent().linesExact! })}`
                                : formatBytesCompact(activeContent().bytes)}
                            </Text>
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
                          </View>

                          <If when={activeContent().truncated}>
                            <View
                              testID="test-truncation-banner"
                              style={{
                                paddingLeft: 10,
                                paddingTop: 6,
                                paddingBottom: 6,
                                borderLeftWidth: 2,
                              }}
                              class="border-l-chart-4 bg-muted/30"
                            >
                              <Text style={{ fontSize: 12, fontVariant: ["tabular-nums"] }} class="text-secondary-foreground">
                                {activeContent().omitted != null
                                  ? t("build.tests.truncatedWithOmitted", { bytes: formatBytesCompact(activeContent().omitted!) })
                                  : t("build.tests.truncatedBanner")}
                              </Text>
                            </View>
                          </If>

                          <View style={{ padding: 12, borderRadius: 6 }} class="bg-muted/40">
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
                      }
                    >
                      <InlineFileEditor
                        problemId={props.problemId}
                        relPath={fileRel()!}
                        readOnly={props.readOnly}
                        refreshKey={fileRefreshKey()}
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

            {/* C. Self-Tests Logical Group View */}
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
                      when={selfTestFiles().length > 0}
                      else={
                        <EmptyState
                          icon="shieldCheck"
                          title={t("build.tests.selfTestsEmpty")}
                          hint={t("build.programs.validatorHint")}
                        />
                      }
                    >
                      {/* Compact self-test selector pills */}
                      <Scroll
                        horizontal
                        style={{ maxHeight: 40, borderBottomWidth: 1 }}
                        contentStyle={{
                          paddingLeft: 16,
                          paddingRight: 16,
                          paddingTop: 7,
                          paddingBottom: 7,
                          flexDirection: "row",
                          alignItems: "center",
                          gap: 6,
                        }}
                        class="border-border bg-background/30"
                      >
                        <Each of={selfTestFiles()} key={(p) => p}>
                          {(rel) => {
                            const selected = () => activeRel() === rel();
                            const res = () => matchSelfTestResult(rel(), activeReport()?.self_tests);
                            const shortName = () => rel().split("/").slice(-2).join("/");
                            return (
                              <View
                                onPress={() => selection.set({ kind: "selfTests", relPath: rel() })}
                                class={selected() ? "bg-muted border border-border" : "border border-transparent hover:bg-muted/60"}
                                style={{
                                  height: 24,
                                  paddingLeft: 8,
                                  paddingRight: 8,
                                  borderRadius: 5,
                                  flexDirection: "row",
                                  alignItems: "center",
                                  gap: 5,
                                }}
                              >
                                <Text
                                  style={{ fontSize: 12, fontWeight: 500 }}
                                  class={selected() ? "text-foreground" : "text-secondary-foreground"}
                                >
                                  {shortName()}
                                </Text>
                                <If when={res()?.passed === false}>
                                  <Icon name="triangleAlert" size={11} tone="destructive" />
                                </If>
                                <If when={res()?.passed === true}>
                                  <Icon name="check" size={11} tone="muted-foreground" />
                                </If>
                              </View>
                            );
                          }}
                        </Each>
                      </Scroll>

                      <If when={activeRel()}>
                        {(rel) => (
                          <InlineFileEditor
                            problemId={props.problemId}
                            relPath={rel()}
                            readOnly={props.readOnly}
                            refreshKey={fileRefreshKey()}
                            onSaved={() => {
                              finishedReport.set(undefined);
                              props.onRefreshPackage();
                            }}
                            onOpenExternal={openExternal}
                          />
                        )}
                      </If>
                    </If>
                  </View>
                );
              })()}
            </If>

            {/* D. Validator Detail View */}
            <If when={selection().kind === "validator"}>
              <View style={{ flex: 1, minHeight: 0 }}>
                <View
                  style={{
                    height: 44,
                    paddingLeft: 16,
                    paddingRight: 14,
                    borderBottomWidth: 1,
                    flexDirection: "row",
                    alignItems: "center",
                    justifyContent: "space-between",
                  }}
                  class="border-border"
                >
                  <View style={{ flexDirection: "row", alignItems: "center", gap: 8 }}>
                    <Icon name="shieldCheck" size={15} tone="secondary-foreground" />
                    <Text style={{ fontSize: 13, fontWeight: 600 }} class="text-foreground">
                      {t("build.tree.validator")}
                    </Text>
                  </View>
                </View>
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

            {/* E. Checker Detail View */}
            <If when={selection().kind === "checker"}>
              <View style={{ flex: 1, minHeight: 0 }}>
                <View
                  style={{
                    height: 44,
                    paddingLeft: 16,
                    paddingRight: 14,
                    borderBottomWidth: 1,
                    flexDirection: "row",
                    alignItems: "center",
                    justifyContent: "space-between",
                    gap: 10,
                  }}
                  class="border-border"
                >
                  <View style={{ flexDirection: "row", alignItems: "center", gap: 8 }}>
                    <Icon name="scale" size={15} tone="secondary-foreground" />
                    <Text style={{ fontSize: 13, fontWeight: 600 }} class="text-foreground">
                      {t("build.tree.checker")}
                    </Text>
                  </View>
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
                </View>

                <If
                  when={inspection()?.checker === "checker.cpp" ? inspection()?.files?.custom_checker : undefined}
                  else={
                    <View style={{ padding: 24, gap: 8 }}>
                      <Text style={{ fontSize: 13, fontWeight: 500 }} class="text-foreground">
                        {t("build.programs.builtinCheckerActive", { name: inspection()?.checker || "wcmp" })}
                      </Text>
                      <Text style={{ fontSize: 12, lineHeight: 18 }} class="text-secondary-foreground">
                        {t("build.programs.builtinCheckerHint")}
                      </Text>
                    </View>
                  }
                >
                  {(rel) => (
                    <InlineFileEditor
                      problemId={props.problemId}
                      relPath={rel()}
                      readOnly={props.readOnly}
                      refreshKey={fileRefreshKey()}
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

            {/* F. Generator Detail View */}
            <If when={selection().kind === "generator"}>
              {(() => {
                const file = () =>
                  selection().kind === "generator" ? (selection() as { kind: "generator"; file: string }).file : "";
                return (
                  <View style={{ flex: 1, minHeight: 0 }}>
                    <View
                      style={{
                        height: 44,
                        paddingLeft: 16,
                        paddingRight: 14,
                        borderBottomWidth: 1,
                        flexDirection: "row",
                        alignItems: "center",
                        gap: 8,
                      }}
                      class="border-border"
                    >
                      <Icon name="sparkles" size={15} tone="secondary-foreground" />
                      <Text style={{ fontSize: 13, fontWeight: 600 }} class="text-foreground">
                        {logicalSolutionName(file())}
                      </Text>
                    </View>
                    <InlineFileEditor
                      problemId={props.problemId}
                      relPath={`generators/${file()}`}
                      readOnly={props.readOnly}
                      refreshKey={fileRefreshKey()}
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

            {/* G. Solution Detail View */}
            <If when={selection().kind === "solution"}>
              {(() => {
                const solName = () =>
                  selection().kind === "solution" ? (selection() as { kind: "solution"; name: string }).name : "";
                const spec = () => solutionSpecs().find((s) => s.name === solName());
                const role = () => (spec() ? solutionRole(spec()!) : "alt");
                return (
                  <View style={{ flex: 1, minHeight: 0 }}>
                    <View
                      style={{
                        height: 44,
                        paddingLeft: 16,
                        paddingRight: 14,
                        borderBottomWidth: 1,
                        flexDirection: "row",
                        alignItems: "center",
                        justifyContent: "space-between",
                        gap: 10,
                      }}
                      class="border-border"
                    >
                      <View style={{ flexDirection: "row", alignItems: "center", gap: 8 }}>
                        <Icon name={role() === "main" ? "crown" : "code"} size={15} tone="secondary-foreground" />
                        <Text style={{ fontSize: 13, fontWeight: 600 }} class="text-foreground">
                          {solName()}
                        </Text>
                        <View
                          style={{ height: 20, paddingLeft: 7, paddingRight: 7, borderRadius: 4, borderWidth: 1, justifyContent: "center" }}
                          class="bg-muted border-border"
                        >
                          <Text style={{ fontSize: 11, fontWeight: 500 }} class="text-foreground">
                            {role() === "main"
                              ? t("build.role.main")
                              : role() === "alt"
                                ? t("build.role.alt")
                                : t("build.role.wrong")}
                          </Text>
                        </View>
                      </View>

                      <If when={!props.readOnly && !!spec()}>
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
                      </If>
                    </View>
                    <If when={spec()?.source_rel}>
                      {(rel) => (
                        <InlineFileEditor
                          problemId={props.problemId}
                          relPath={rel()}
                          readOnly={props.readOnly}
                          refreshKey={fileRefreshKey()}
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
          onClose={() => addGenModalOpen.set(false)}
          onAdded={() => {
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
      <Text style={{ fontSize: 11, fontWeight: 600, letterSpacing: 0.3 }} class="text-secondary-foreground">
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
  testID?: string;
}) {
  return (
    <View
      testID={props.testID}
      onPress={props.onPress}
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
      <If when={props.statusDot === "fail"}>
        <Icon name="triangleAlert" size={12} tone="destructive" />
      </If>
      <If when={props.statusDot === "ok"}>
        <Icon name="check" size={12} tone="muted-foreground" />
      </If>
    </View>
  );
}

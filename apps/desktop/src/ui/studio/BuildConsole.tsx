// Polygon-grade local Build Console (4th single-problem tab in Studio).
// Organizes the package workflow into 3 clean segmented sub-views:
// 1. `测试矩阵` (Matrix): compact single-line check summary bar (expandable with actionable
//    next-step buttons), compact metrics strip (post-build), tightly grouped Result Matrix
//    (zero Unicode status glyphs — Lucide SVG icons everywhere), and on-demand right diagnostic
//    panel when a cell is selected.
// 2. `测试数据` (Tests): left list of expanded test cases + right safe head/tail input preview
//    powered by `preview.inspectTestCase`, with generator/validator quick actions.
// 3. `程序与配置` (Programs & Config): 5 ordered Polygon cards (Validator, Checker, Generators,
//    Solutions, Config) with self-test summaries and one-click external open.

import { cleanup, Clipboard, colorScheme, computed, effect, Scroll, signal, Text, untracked, View } from "nsh";
import { Progress } from "nsh/base";
import type { PackageBuildStatusJs, PackageInspectionResultJs, TestCasePreviewJs } from "xcpc-workstation/native";
import { ErrorDisplay } from "../ErrorDisplay";
import { Icon } from "../Icon";
import { locale, t, type MessageKey } from "../i18n";
import { Btn, Chip, IconBtn, Segmented, tip } from "../kit";
import { AppMenu, type MenuEntry } from "../Menu";
import { colorTransition } from "../motion";
import { errorOf, goSettings, preview, refresh, type UiError } from "../store";
import { cardShadow, EASE_STANDARD, MONO, ms, token } from "../theme";
import {
  buildMatrixLookup,
  formatAcVsTleMargin,
  formatBytesCompact,
  formatMemoryKib,
  formatStdTimeRatio,
  gateBadgeStyle,
  humanizeCheckText,
  isCellVerdictAnomaly,
  isMissingCompilerError,
  isPackageIncomplete,
  matrixCellKey,
  nextActionForCheck,
  parseBuildReport,
  parsePackageInspection,
  partitionGates,
  resolveSolutionHeaderSpecs,
  selectMatrixCell,
  solutionRole,
  toggleSubsetSelection,
  verdictBadgeStyle,
  type BuildReportData,
  type CheckAction,
  type MatrixCellData,
  type PackageInspectionData,
  type SelectedCellKey,
  type SolutionSummaryData,
} from "./buildLogic";

const POLL_INTERVAL_MS = 120;
const DIAG_PANEL_WIDTH = 320;

type BuildSubTab = "matrix" | "tests" | "programs";

export interface BuildConsoleController {
  /** Triggered by Mod+B on the Build tab: runs Verify, or Run Selected when subset filters are active. */
  triggerPrimaryRun(): void;
}

function phaseLabel(phase: string | null | undefined): string {
  switch (phase) {
    case "compiling":
      return t("build.phase.compiling");
    case "self_testing":
      return t("build.phase.self_testing");
    case "generating":
      return t("build.phase.generating");
    case "validating":
      return t("build.phase.validating");
    case "running":
      return t("build.phase.running");
    default:
      return t("build.status.running");
  }
}

function freshnessMeta(status: string | undefined): { labelKey: MessageKey; tone: "pass" | "warn" | "fail" | "skip" } {
  switch (status) {
    case "verified_fresh":
      return { labelKey: "build.status.verifiedFresh", tone: "pass" };
    case "stale":
      return { labelKey: "build.status.stale", tone: "warn" };
    case "failed":
      return { labelKey: "build.status.failed", tone: "fail" };
    default:
      return { labelKey: "build.status.unverified", tone: "skip" };
  }
}

export function BuildConsole(props: {
  problemId: string;
  readOnly: boolean;
  inspectionResult: PackageInspectionResultJs | undefined;
  onRefreshPackage: () => void;
  /** Called when a matrix cell is selected so Studio collapses the right Inspector drawer if open. */
  onCollapseInspector?: () => void;
  controller?: (c: BuildConsoleController) => void;
}) {
  const subTab = signal<BuildSubTab>("matrix");
  const buildStatus = signal<PackageBuildStatusJs>(preview.packageBuildStatus());
  const selectedSolutions = signal<string[]>([]);
  const selectedTests = signal<string[]>([]);
  const filterOpen = signal(false);
  const selectedCell = signal<SelectedCellKey | undefined>(undefined);
  const selectedTestIndex = signal<number | undefined>(undefined);
  const checksExpanded = signal(false);
  const actionError = signal<UiError | undefined>(undefined);
  const copiedRepro = signal(false);
  const copiedGenCmd = signal(false);
  const copiedCompileTarget = signal<string | undefined>(undefined);

  let pollTimer: number | undefined;
  let copyTimer: number | undefined;
  let genCopyTimer: number | undefined;
  let compileCopyTimer: number | undefined;
  let lastSeenSeq = buildStatus().seq;

  const stopPolling = () => {
    if (pollTimer !== undefined) {
      clearInterval(pollTimer);
      pollTimer = undefined;
    }
  };

  const pollOnce = () => {
    const next = preview.packageBuildStatus();
    buildStatus.set(next);
    if (!next.running) {
      stopPolling();
      if (next.seq !== lastSeenSeq) {
        lastSeenSeq = next.seq;
        props.onRefreshPackage();
        refresh();
      }
    }
  };

  const startPolling = () => {
    stopPolling();
    pollTimer = setInterval(pollOnce, POLL_INTERVAL_MS);
  };

  // Reset per-problem UI state when switching problems, and resume polling if a build is active.
  effect(() => {
    void props.problemId;
    untracked(() => {
      subTab.set("matrix");
      selectedSolutions.set([]);
      selectedTests.set([]);
      filterOpen.set(false);
      selectedCell.set(undefined);
      selectedTestIndex.set(undefined);
      checksExpanded.set(false);
      actionError.set(undefined);
      const cur = preview.packageBuildStatus();
      buildStatus.set(cur);
      lastSeenSeq = cur.seq;
      if (cur.running) startPolling();
      else stopPolling();
    });
  });

  cleanup(() => {
    stopPolling();
    clearTimeout(copyTimer);
    clearTimeout(genCopyTimer);
    clearTimeout(compileCopyTimer);
  });

  const inspection = computed<PackageInspectionData | undefined>(() =>
    props.inspectionResult?.ok ? parsePackageInspection(props.inspectionResult.inspectionJson) : undefined,
  );

  const isRunningThis = computed(() => {
    const st = buildStatus();
    return st.running && st.slug === props.problemId;
  });

  const isRunningAny = computed(() => buildStatus().running);

  const activeReport = computed<BuildReportData | undefined>(() => {
    const st = buildStatus();
    if (!st.running && st.slug === props.problemId && st.reportJson) {
      const fromRun = parseBuildReport(st.reportJson);
      if (fromRun) return fromRun;
    }
    return inspection()?.last_report ?? undefined;
  });

  const allChecks = computed(() => activeReport()?.gates ?? inspection()?.gates ?? []);
  const splitChecks = computed(() => partitionGates(allChecks()));
  const files = computed(() => inspection()?.files);
  const incomplete = computed(() => isPackageIncomplete(files()));
  const hasSubset = computed(() => selectedSolutions().length > 0 || selectedTests().length > 0);
  const subsetCount = computed(() => selectedSolutions().length + selectedTests().length);

  const matrixCols = computed(() => resolveSolutionHeaderSpecs(inspection(), activeReport()));
  const availableSolutions = computed<string[]>(() => matrixCols().map((s) => s.name));

  const solutionSummaryByName = computed(() => {
    const map = new Map<string, SolutionSummaryData>();
    for (const s of activeReport()?.solutions ?? []) {
      map.set(s.name, s);
    }
    return map;
  });

  const availableTags = computed<string[]>(() => {
    const tags: string[] = [];
    for (const test of inspection()?.expanded_tests ?? []) {
      const tg = test.tag ?? (test.is_sample ? "sample" : undefined);
      if (tg && !tags.includes(tg)) tags.push(tg);
    }
    return tags;
  });

  const matrixLookup = computed(() => buildMatrixLookup(activeReport()?.matrix));

  const expandedTestByIndex = computed(() => {
    const map = new Map<number, NonNullable<PackageInspectionData["expanded_tests"]>[number]>();
    for (const tItem of inspection()?.expanded_tests ?? []) {
      map.set(tItem.index, tItem);
    }
    return map;
  });

  const matrixRows = computed(() => {
    const rep = activeReport();
    if (rep?.tests && rep.tests.length > 0) return rep.tests;
    return (inspection()?.expanded_tests ?? []).map((tItem) => ({
      index: tItem.index,
      name: tItem.name,
      tag: tItem.tag,
      is_sample: tItem.is_sample,
      size_bytes: 0,
      gen_cmd: tItem.gen_cmd,
      validator_ok: null,
      validator_message: null,
    }));
  });

  // Keep a valid test selected when switching to the `测试数据` sub-view.
  const activeTestIndex = computed<number | undefined>(() => {
    const rows = matrixRows();
    if (rows.length === 0) return undefined;
    const cur = selectedTestIndex();
    if (cur != null && rows.some((r) => r.index === cur)) return cur;
    return rows[0].index;
  });

  const activeTestRow = computed(() => {
    const idx = activeTestIndex();
    if (idx == null) return undefined;
    return matrixRows().find((r) => r.index === idx);
  });

  const activeTestPreview = computed<TestCasePreviewJs | undefined>(() => {
    if (subTab() !== "tests") return undefined;
    const idx = activeTestIndex();
    if (idx == null) return undefined;
    // Re-read when report or inspection refreshes.
    void activeReport();
    void props.inspectionResult;
    return preview.inspectTestCase(props.problemId, String(idx), 40, 20);
  });

  const inspectedCell = computed<MatrixCellData | undefined>(() =>
    selectMatrixCell(activeReport()?.matrix, selectedCell()),
  );

  const inspectedTest = computed(() => {
    const sel = selectedCell();
    if (!sel) return undefined;
    return matrixRows().find((r) => r.index === sel.testIndex);
  });

  const inspectedColSpec = computed(() => {
    const sel = selectedCell();
    if (!sel) return undefined;
    return matrixCols().find((c) => c.name === sel.solution);
  });

  const progressPct = computed(() => {
    const st = buildStatus();
    if (!st.running || st.total <= 0) return 0;
    return Math.min(100, Math.max(0, (st.completed / st.total) * 100));
  });

  function runBuild(mode: "debug" | "verify") {
    if (props.readOnly || isRunningAny()) return;
    actionError.set(undefined);
    const testsArg = mode === "debug" && selectedTests().length > 0 ? selectedTests() : null;
    const solsArg = mode === "debug" && selectedSolutions().length > 0 ? selectedSolutions() : null;
    const res = preview.startPackageBuild(props.problemId, mode, testsArg, solsArg);
    if (!res.ok) {
      actionError.set(errorOf(res, t("ctx.buildPackage")));
      return;
    }
    const st = preview.packageBuildStatus();
    buildStatus.set(st);
    startPolling();
  }

  function triggerPrimaryRun() {
    if (untracked(hasSubset)) runBuild("debug");
    else runBuild("verify");
  }

  props.controller?.({ triggerPrimaryRun });

  function cancelBuild() {
    preview.cancelPackageBuild();
    buildStatus.set(preview.packageBuildStatus());
  }

  function scaffoldPackage() {
    if (props.readOnly) return;
    const res = preview.initPackage(props.problemId);
    if (!res.ok) {
      actionError.set({ code: res.code, error: res.error, context: t("ctx.initPackage") });
      return;
    }
    actionError.set(undefined);
    props.onRefreshPackage();
    refresh();
  }

  function openSource(relPath: string) {
    void preview.openPackageFile(props.problemId, relPath).then((res) => {
      if (!res.ok) actionError.set(errorOf(res, t("ctx.openPackageFile")));
    });
  }

  function jumpToTest(index: number) {
    selectedTestIndex.set(index);
    subTab.set("tests");
  }

  function selectMatrixCellWithCollapse(key: SelectedCellKey | undefined) {
    selectedCell.set(key);
    if (key) props.onCollapseInspector?.();
  }

  function runCheckAction(act: CheckAction) {
    if (act.kind === "view_test") jumpToTest(act.testIndex);
    else if (act.kind === "select_cell") {
      checksExpanded.set(false);
      selectMatrixCellWithCollapse({ testIndex: act.testIndex, solution: act.solution });
    } else if (act.kind === "open_file") openSource(act.relPath);
    else subTab.set("programs");
  }

  function checkActionLabel(act: CheckAction): string {
    if (act.kind === "view_test") return t("build.checks.action.viewFailedTest", { index: act.testIndex });
    if (act.kind === "select_cell") {
      return t("build.checks.action.viewFailedCell", { index: act.testIndex, solution: act.solution });
    }
    if (act.kind === "open_programs") return t("build.checks.action.viewPrograms");
    if (act.relPath === "solutions") return t("build.checks.action.openSolutionsDir");
    if (act.relPath.includes("validator")) return t("build.checks.action.openValidator");
    if (act.relPath.includes("checker")) return t("build.checks.action.openChecker");
    return act.relPath;
  }

  function copyRepro(cmd: string) {
    void Clipboard.writeText(cmd).then(() => {
      copiedRepro.set(true);
      clearTimeout(copyTimer);
      copyTimer = setTimeout(() => copiedRepro.set(false), 1500);
    });
  }

  function copyGen(cmd: string) {
    void Clipboard.writeText(cmd).then(() => {
      copiedGenCmd.set(true);
      clearTimeout(genCopyTimer);
      genCopyTimer = setTimeout(() => copiedGenCmd.set(false), 1500);
    });
  }

  function copyCompileStderr(target: string, stderr: string) {
    void Clipboard.writeText(stderr).then(() => {
      copiedCompileTarget.set(target);
      clearTimeout(compileCopyTimer);
      compileCopyTimer = setTimeout(() => copiedCompileTarget.set(undefined), 1500);
    });
  }

  const dark = () => colorScheme() === "dark";
  const isZh = () => locale() === "zh-CN";
  const freshBadge = computed(() => {
    const meta = freshnessMeta(props.inspectionResult?.freshnessStatus);
    const colors = gateBadgeStyle(meta.tone, dark());
    return { label: t(meta.labelKey), ...colors };
  });

  const buildErrorMsg = computed(() => {
    const st = buildStatus();
    if (!st.running && st.slug === props.problemId && st.error) return st.error;
    return undefined;
  });

  const acTleMargin = computed(() => formatAcVsTleMargin(activeReport()?.safety_margin));
  const hasFiltersAvailable = computed(() => availableSolutions().length > 1 || availableTags().length > 0);
  const splitDisabled = () => props.readOnly || isRunningAny();
  const useCompactCols = computed(() => matrixCols().length <= 3);

  const subTabItems = computed(() => [
    { value: "matrix" as BuildSubTab, label: t("build.subtab.matrix") },
    { value: "tests" as BuildSubTab, label: t("build.subtab.tests") },
    { value: "programs" as BuildSubTab, label: t("build.subtab.programs") },
  ]);

  const splitMenuEntries = (): MenuEntry[] => [
    { id: "debug", label: t("build.action.quickRun"), icon: "code" },
  ];

  const splitChevronTrigger = (disabled: boolean) => (
    <View
      testID="build-verify-menu"
      disabled={disabled}
      class={
        disabled
          ? "bg-primary"
          : "bg-primary hover:bg-sidebar-primary active:opacity-90 focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-ring"
      }
      style={{
        width: 24,
        height: 28,
        alignItems: "center",
        justifyContent: "center",
        transitionProperty: "colors",
        transitionDuration: ms(120),
        transitionTimingFunction: EASE_STANDARD,
      }}
    >
      <Icon name="chevronDown" size={14} tone="primary-foreground" />
    </View>
  );

  const checkSummaryText = computed(() => {
    const { failed, warn, passed } = splitChecks();
    if (failed.length > 0 && warn.length > 0) {
      return t("build.checks.failAndWarn", { failed: failed.length, warn: warn.length });
    }
    if (failed.length > 0) return t("build.checks.failOnly", { failed: failed.length });
    if (warn.length > 0) return t("build.checks.warnOnly", { warn: warn.length });
    return t("build.checks.passedSummary", { count: passed.length });
  });

  return (
    <View testID="build-console" style={{ flex: 1, minHeight: 0 }} class="bg-card">
      {/* Shared compact top bar: left 3 sub-views, right split CTA + filter + status pill */}
      <View
        style={{
          paddingLeft: 14,
          paddingRight: 14,
          paddingTop: 8,
          paddingBottom: 8,
          gap: 8,
          borderBottomWidth: 1,
        }}
        class="border-border bg-background/50"
      >
        <View style={{ flexDirection: "row", alignItems: "center", flexWrap: "wrap", gap: 8 }}>
          <Segmented
            items={subTabItems()}
            value={subTab()}
            onChange={(next) => {
              subTab.set(next);
              if (next !== "matrix") selectedCell.set(undefined);
            }}
            itemWidth={isZh() ? 64 : 84}
          />

          <View style={{ flex: 1 }} />

          {/* Filter toggle (only relevant on Matrix sub-view when filters exist) */}
          <If when={subTab() === "matrix" && hasFiltersAvailable()}>
            <Btn
              testID="build-filter-toggle"
              label={subsetCount() > 0 ? `${t("build.action.filter")} (${subsetCount()})` : t("build.action.filter")}
              variant="outline"
              onPress={() => filterOpen.set(!filterOpen())}
              style={filterOpen() || hasSubset() ? { backgroundColor: token("accent") } : undefined}
            />
          </If>

          {/* Single split primary CTA: 验证 ▾ (or 运行所选 when subset filters are active) */}
          <If
            when={hasSubset()}
            else={
              <View
                style={{
                  flexDirection: "row",
                  height: 28,
                  borderRadius: 6,
                  overflow: "hidden",
                  opacity: splitDisabled() ? 0.5 : 1,
                  transitionProperty: "opacity",
                  transitionDuration: ms(120),
                  transitionTimingFunction: EASE_STANDARD,
                }}
                class="bg-primary"
              >
                <View
                  testID="build-run-verify"
                  disabled={splitDisabled()}
                  onPress={() => !splitDisabled() && runBuild("verify")}
                  tooltip={tip(t("build.action.verify"), "mod+b")}
                  class={
                    splitDisabled()
                      ? "bg-primary"
                      : "bg-primary hover:bg-sidebar-primary active:opacity-90 focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-ring"
                  }
                  style={{
                    paddingLeft: 10,
                    paddingRight: 11,
                    flexDirection: "row",
                    alignItems: "center",
                    gap: 6,
                    transitionProperty: "colors",
                    transitionDuration: ms(120),
                    transitionTimingFunction: EASE_STANDARD,
                  }}
                >
                  <Icon name="check" size={15} tone="primary-foreground" />
                  <Text style={{ fontSize: 13 }} class="text-primary-foreground">
                    {t("build.action.verify")}
                  </Text>
                </View>
                <View style={{ width: 1, alignSelf: "stretch", backgroundColor: "rgba(255,255,255,0.22)" }} />
                <If when={!splitDisabled()} else={splitChevronTrigger(true)}>
                  <AppMenu
                    entries={splitMenuEntries()}
                    onSelect={(id) => {
                      if (id === "debug") runBuild("debug");
                    }}
                    align="end"
                    minWidth={156}
                    trigger={splitChevronTrigger(false)}
                  />
                </If>
              </View>
            }
          >
            <Btn
              testID="build-run-selected"
              label={t("build.action.runSelected")}
              icon="code"
              variant="primary"
              disabled={splitDisabled()}
              tooltip={tip(t("build.action.runSelected"), "mod+b")}
              onPress={() => runBuild("debug")}
            />
          </If>

          <If when={isRunningThis()}>
            <Btn
              testID="build-cancel-btn"
              label={t("build.action.cancel")}
              icon="x"
              variant="danger"
              onPress={cancelBuild}
            />
          </If>

          <If when={activeReport()?.subset}>
            <View
              testID="build-partial-badge"
              class="bg-muted"
              style={{ height: 22, paddingLeft: 7, paddingRight: 7, borderRadius: 9999, justifyContent: "center" }}
            >
              <Text style={{ fontSize: 11.5 }} class="text-secondary-foreground">
                {t("build.badge.partialRun")}
              </Text>
            </View>
          </If>

          <View
            testID="build-freshness-badge"
            style={{
              height: 22,
              paddingLeft: 8,
              paddingRight: 8,
              borderRadius: 9999,
              justifyContent: "center",
              backgroundColor: freshBadge().bg,
            }}
          >
            <Text style={{ fontSize: 11.5, fontWeight: 500, color: freshBadge().fg }}>
              {freshBadge().label}
            </Text>
          </View>
        </View>

        {/* Collapsible subset filter chips */}
        <If when={subTab() === "matrix" && filterOpen() && hasFiltersAvailable()}>
          <View testID="build-filter-panel" style={{ paddingTop: 4, gap: 6 }}>
            <If when={availableSolutions().length > 1}>
              <View style={{ flexDirection: "row", alignItems: "center", flexWrap: "wrap", gap: 6 }}>
                <Text style={{ fontSize: 11.5, minWidth: 36 }} class="text-muted-foreground">
                  {t("build.filter.solutionsGroup")}
                </Text>
                <Each of={availableSolutions()} key={(s) => `sol:${s}`}>
                  {(sol) => (
                    <Chip
                      label={sol()}
                      active={selectedSolutions().includes(sol())}
                      onPress={() => selectedSolutions.set(toggleSubsetSelection(selectedSolutions(), sol()))}
                    />
                  )}
                </Each>
              </View>
            </If>
            <If when={availableTags().length > 0}>
              <View style={{ flexDirection: "row", alignItems: "center", flexWrap: "wrap", gap: 6 }}>
                <Text style={{ fontSize: 11.5, minWidth: 36 }} class="text-muted-foreground">
                  {t("build.filter.testsGroup")}
                </Text>
                <Each of={availableTags()} key={(tg) => `tag:${tg}`}>
                  {(tag) => (
                    <Chip
                      label={tag()}
                      active={selectedTests().includes(tag())}
                      onPress={() => selectedTests.set(toggleSubsetSelection(selectedTests(), tag()))}
                    />
                  )}
                </Each>
                <If when={hasSubset()}>
                  <Btn
                    label={t("build.filter.clear")}
                    icon="filterX"
                    variant="ghost"
                    onPress={() => {
                      selectedSolutions.set([]);
                      selectedTests.set([]);
                    }}
                  />
                </If>
              </View>
            </If>
          </View>
        </If>

        {/* Live progress bar while running */}
        <If when={isRunningThis()}>
          <View style={{ gap: 4, paddingTop: 2 }}>
            <View style={{ flexDirection: "row", alignItems: "center", justifyContent: "space-between", gap: 8 }}>
              <Text style={{ fontSize: 12, fontWeight: 500 }} class="text-foreground">
                {buildStatus().detail ? `${phaseLabel(buildStatus().phase)}  ${buildStatus().detail}` : phaseLabel(buildStatus().phase)}
              </Text>
              <If when={buildStatus().total > 0}>
                <Text style={{ fontSize: 11.5, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
                  {`${buildStatus().completed} / ${buildStatus().total}`}
                </Text>
              </If>
            </View>
            <Progress
              value={progressPct()}
              indeterminate={buildStatus().total <= 0}
              trackClass="h-1.5 rounded-full bg-muted overflow-hidden"
              indicatorClass="h-full rounded-full bg-primary transition-all duration-[120ms]"
            />
          </View>
        </If>
      </View>

      {/* ── Sub-view 1: 测试矩阵 (Matrix) ─────────────────────────────────────── */}
      <If when={subTab() === "matrix"}>
        <View style={{ flex: 1, flexDirection: "row", minHeight: 0 }}>
          <View style={{ flex: 1, minWidth: 0 }}>
            {/* Compact horizontal metrics strip (hidden before first build) */}
            <If when={activeReport()}>
              {(rep) => (
                <View
                  testID="build-metrics-strip"
                  style={{
                    paddingLeft: 14,
                    paddingRight: 14,
                    paddingTop: 7,
                    paddingBottom: 7,
                    flexDirection: "row",
                    alignItems: "center",
                    flexWrap: "wrap",
                    gap: 20,
                    borderBottomWidth: 1,
                  }}
                  class="border-border bg-muted/20"
                >
                  <CompactMetric
                    label={t("build.margin.maxInput")}
                    value={formatBytesCompact(rep().safety_margin.max_input_bytes)}
                  />
                  <CompactMetric
                    label={t("build.margin.stdSlowest")}
                    value={formatStdTimeRatio(rep().safety_margin)}
                    warn={rep().safety_margin.std_time_ratio > 0.5}
                  />
                  <If when={acTleMargin()}>
                    {(m) => <CompactMetric label={t("build.margin.acVsTle")} value={m()} />}
                  </If>
                  <CompactMetric label={t("build.margin.checker")} value={rep().checker} mono />
                </View>
              )}
            </If>

            <Scroll style={{ flex: 1 }}>
              <View style={{ padding: 14, gap: 12 }}>
                <If when={actionError()}>
                  <ErrorDisplay error={actionError()} onDismiss={() => actionError.set(undefined)} />
                </If>
                <If when={buildErrorMsg()}>
                  {(msg) => (
                    <ErrorDisplay
                      error={{ error: msg(), context: t("ctx.buildPackage") }}
                      action={
                        isMissingCompilerError(msg()) ? (
                          <Btn
                            testID="build-open-toolchain-btn"
                            label={t("build.error.openToolchain")}
                            icon="settings"
                            onPress={() => goSettings("toolchain")}
                          />
                        ) : undefined
                      }
                    />
                  )}
                </If>

                {/* Compact single-line check summary bar (concentric radii: outer 10 - padding 4 = inner 6) */}
                <If when={allChecks().length > 0}>
                  <View
                    testID="build-checks-bar"
                    style={{ borderRadius: 10, borderWidth: 1, overflow: "hidden" }}
                    class="border-border bg-muted/25"
                  >
                    <View
                      testID="build-checks-toggle"
                      onPress={() => checksExpanded.set(!checksExpanded())}
                      class="hover:bg-muted/60 active:bg-accent"
                      style={{
                        minHeight: 34,
                        paddingLeft: 10,
                        paddingRight: 10,
                        flexDirection: "row",
                        alignItems: "center",
                        gap: 8,
                        ...colorTransition(120),
                      }}
                    >
                      <Icon
                        name={
                          splitChecks().failed.length > 0
                            ? "circleAlert"
                            : splitChecks().warn.length > 0
                              ? "triangleAlert"
                              : "check"
                        }
                        size={14}
                        tone={
                          splitChecks().failed.length > 0
                            ? "destructive"
                            : splitChecks().warn.length > 0
                              ? "chart-4"
                              : "primary"
                        }
                      />
                      <Text
                        style={{ flex: 1, fontSize: 12.5, fontWeight: 500, fontVariant: ["tabular-nums"] }}
                        class={
                          splitChecks().failed.length > 0
                            ? "text-destructive"
                            : splitChecks().warn.length > 0
                              ? "text-chart-4"
                              : "text-secondary-foreground"
                        }
                      >
                        {checkSummaryText()}
                      </Text>
                      <Text style={{ fontSize: 11.5 }} class="text-muted-foreground">
                        {checksExpanded() ? t("build.checks.collapse") : t("build.checks.expand")}
                      </Text>
                      <Icon
                        name="chevronDown"
                        size={13}
                        tone="muted-foreground"
                        style={{ transform: checksExpanded() ? "rotate(180deg)" : "rotate(0deg)" }}
                      />
                    </View>

                    <If when={checksExpanded()}>
                      <View style={{ borderTopWidth: 1, padding: 4, gap: 4 }} class="border-border bg-card">
                        <Each of={allChecks()} key={(g) => g.id}>
                          {(gate) => {
                            const isFail = () => gate().status === "fail";
                            const isWarn = () => gate().status === "warn";
                            const act = () =>
                              isFail() || isWarn() ? nextActionForCheck(gate(), activeReport(), files()) : undefined;
                            return (
                              <View
                                testID={`build-check-item-${gate().id}`}
                                style={{
                                  paddingLeft: 8,
                                  paddingRight: 8,
                                  paddingTop: 6,
                                  paddingBottom: 6,
                                  borderRadius: 6,
                                  flexDirection: "row",
                                  alignItems: "center",
                                  gap: 8,
                                }}
                                class={isFail() || isWarn() ? "bg-muted/40" : undefined}
                              >
                                <Icon
                                  name={isFail() ? "circleAlert" : isWarn() ? "triangleAlert" : "check"}
                                  size={13.5}
                                  tone={isFail() ? "destructive" : isWarn() ? "chart-4" : "muted-foreground"}
                                />
                                <View style={{ flex: 1, minWidth: 0, gap: 1 }}>
                                  <Text
                                    style={{ fontSize: 12, fontWeight: 600 }}
                                    class={isFail() ? "text-destructive" : isWarn() ? "text-chart-4" : "text-foreground"}
                                  >
                                    {humanizeCheckText(gate().title, isZh())}
                                  </Text>
                                  <Text style={{ fontSize: 12, lineHeight: 16.5 }} class="text-secondary-foreground">
                                    {humanizeCheckText(gate().message, isZh())}
                                  </Text>
                                </View>
                                <If when={act()}>
                                  {(a) => (
                                    <Btn
                                      label={checkActionLabel(a())}
                                      variant="outline"
                                      onPress={() => runCheckAction(a())}
                                    />
                                  )}
                                </If>
                              </View>
                            );
                          }}
                        </Each>
                      </View>
                    </If>
                  </View>
                </If>

                {/* Compile errors: full copyable stderr + click-to-open source file */}
                <If when={(activeReport()?.compile_failures?.length ?? 0) > 0}>
                  <View
                    style={{ borderRadius: 10, borderWidth: 1, padding: 12, gap: 10 }}
                    class="border-border bg-muted/40"
                  >
                    <View style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>
                      <Icon name="circleAlert" size={14} tone="destructive" />
                      <Text style={{ fontSize: 12.5, fontWeight: 600 }} class="text-destructive">
                        {t("build.section.compileErrors")}
                      </Text>
                    </View>
                    <Each of={activeReport()?.compile_failures ?? []} key={(f) => `${f.target}:${f.source_path}`}>
                      {(cf) => {
                        const canOpen = () => !cf().source_path.startsWith("<");
                        const isCopied = () => copiedCompileTarget() === cf().target;
                        return (
                          <View style={{ gap: 6 }}>
                            <View style={{ flexDirection: "row", alignItems: "center", gap: 8 }}>
                              <Text style={{ flex: 1, fontSize: 12, fontWeight: 600, fontFamily: MONO }} class="text-foreground">
                                {`${cf().target} (${cf().source_path})`}
                              </Text>
                              <If when={canOpen()}>
                                <Btn
                                  label={t("build.compile.openFile")}
                                  icon="code"
                                  variant="ghost"
                                  onPress={() => openSource(cf().source_path)}
                                />
                              </If>
                              <Btn
                                label={isCopied() ? t("common.copied") : t("build.compile.copyStderr")}
                                icon={isCopied() ? "check" : "copy"}
                                variant="ghost"
                                onPress={() => copyCompileStderr(cf().target, cf().stderr)}
                              />
                            </View>
                            <Scroll style={{ maxHeight: 220 }}>
                              <View style={{ borderRadius: 6, borderWidth: 1, padding: 8 }} class="border-border bg-card">
                                <Text style={{ fontSize: 11.5, lineHeight: 16, fontFamily: MONO }} class="text-secondary-foreground">
                                  {cf().stderr.trim()}
                                </Text>
                              </View>
                            </Scroll>
                          </View>
                        );
                      }}
                    </Each>
                  </View>
                </If>

                {/* Dirty inputs warning */}
                <If when={(inspection()?.dirty_inputs?.length ?? 0) > 0}>
                  <View
                    style={{ borderRadius: 10, borderWidth: 1, padding: 10, gap: 4 }}
                    class="border-border bg-muted/40"
                  >
                    <View style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>
                      <Icon name="triangleAlert" size={14} tone="chart-4" />
                      <Text style={{ fontSize: 12, fontWeight: 600 }} class="text-chart-4">
                        {t("build.section.dirtyInputs")}
                      </Text>
                    </View>
                    <Each of={inspection()?.dirty_inputs ?? []} key={(d) => d.path}>
                      {(d) => (
                        <Text style={{ fontSize: 11.5, fontFamily: MONO }} class="text-secondary-foreground">
                          {`${d().path}: ${[
                            d().has_crlf && "CRLF",
                            d().has_bom && "BOM",
                            d().missing_trailing_newline && t("build.dirty.missingNewline"),
                          ]
                            .filter(Boolean)
                            .join(", ")}`}
                        </Text>
                      )}
                    </Each>
                  </View>
                </If>

                {/* Clean Result Matrix: Lucide SVG icons, tightly grouped cells, sensible column widths */}
                <View
                  testID="build-result-matrix"
                  style={{
                    borderRadius: 10,
                    borderWidth: 1,
                    overflow: "hidden",
                    boxShadow: cardShadow(),
                  }}
                  class="border-border bg-card"
                >
                  <If
                    when={matrixCols().length > 0}
                    else={
                      <View style={{ padding: 20, alignItems: "center" }}>
                        <Text style={{ fontSize: 12.5 }} class="text-muted-foreground">
                          {t("build.matrix.noSolutions")}
                        </Text>
                      </View>
                    }
                  >
                    <Scroll horizontal style={{ maxWidth: "100%" }}>
                      <View style={{ minWidth: "100%", flexGrow: 1 }}>
                        {/* Solution header row */}
                        <View
                          style={{
                            flexDirection: "row",
                            alignItems: "center",
                            borderBottomWidth: 1,
                            minHeight: 42,
                          }}
                          class="border-border bg-muted/30"
                        >
                          <View style={{ width: 184, flexShrink: 0, paddingLeft: 12, paddingRight: 8, paddingTop: 6, paddingBottom: 6 }}>
                            <Text style={{ fontSize: 11.5, fontWeight: 600 }} class="text-secondary-foreground">
                              {t("build.matrix.testHeader")}
                            </Text>
                          </View>
                          <Each of={matrixCols()} key={(col) => col.name}>
                            {(col) => (
                              <View
                                style={{
                                  flex: 1,
                                  minWidth: 112,
                                  maxWidth: useCompactCols() ? 220 : undefined,
                                  paddingLeft: 10,
                                  paddingRight: 10,
                                  paddingTop: 6,
                                  paddingBottom: 6,
                                  gap: 2,
                                }}
                              >
                                <View style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>
                                  <Text
                                    style={{ numberOfLines: 1, textOverflow: "ellipsis", flexShrink: 1, fontSize: 12, fontWeight: 600, fontFamily: MONO }}
                                    class="text-foreground"
                                  >
                                    {col().name}
                                  </Text>
                                  <If when={col().is_main}>
                                    <View
                                      class="bg-muted border border-border"
                                      style={{ height: 18, paddingLeft: 5, paddingRight: 5, borderRadius: 4, justifyContent: "center" }}
                                    >
                                      <Text style={{ fontSize: 10, fontWeight: 500 }} class="text-foreground">
                                        {t("build.matrix.mainBadge")}
                                      </Text>
                                    </View>
                                  </If>
                                </View>
                                <Text style={{ fontSize: 10.5, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
                                  {t("build.matrix.expectPrefix", {
                                    verdict: col().is_main ? "AC" : col().expected.join(" / "),
                                  })}
                                </Text>
                              </View>
                            )}
                          </Each>
                        </View>

                        {/* Per-solution summary row using Lucide SVG icons */}
                        <If when={activeReport() && (activeReport()!.solutions?.length ?? 0) > 0}>
                          <View
                            testID="build-matrix-summary-row"
                            style={{
                              flexDirection: "row",
                              alignItems: "center",
                              borderBottomWidth: 1,
                              minHeight: 42,
                            }}
                            class="border-border bg-muted/15"
                          >
                            <View style={{ width: 184, flexShrink: 0, paddingLeft: 12, paddingRight: 8 }}>
                              <Text style={{ fontSize: 11.5, fontWeight: 500 }} class="text-muted-foreground">
                                {t("build.summary.label")}
                              </Text>
                            </View>
                            <Each of={matrixCols()} key={(col) => `sum:${col.name}`}>
                              {(col) => {
                                const sum = () => solutionSummaryByName().get(col().name);
                                return (
                                  <View
                                    style={{
                                      flex: 1,
                                      minWidth: 112,
                                      maxWidth: useCompactCols() ? 220 : undefined,
                                      paddingLeft: 10,
                                      paddingRight: 10,
                                      paddingTop: 6,
                                      paddingBottom: 6,
                                      gap: 2,
                                    }}
                                  >
                                    <If
                                      when={sum()}
                                      else={
                                        <Text style={{ fontSize: 11.5 }} class="text-muted-foreground">
                                          –
                                        </Text>
                                      }
                                    >
                                      {(s) => {
                                        const badge = () => verdictBadgeStyle(s().matched ? "AC" : "WA", dark());
                                        return (
                                          <>
                                            <View style={{ flexDirection: "row", alignItems: "center", gap: 5 }}>
                                              <Icon
                                                name={s().matched ? "check" : "x"}
                                                size={13}
                                                tone={s().matched ? "primary" : "destructive"}
                                              />
                                              <Text
                                                style={{
                                                  fontSize: 11.5,
                                                  fontWeight: 600,
                                                  fontVariant: ["tabular-nums"],
                                                  color: s().matched ? undefined : badge().fg,
                                                }}
                                                class={s().matched ? "text-foreground" : undefined}
                                              >
                                                {`${s().passed_tests}/${s().total_tests}`}
                                              </Text>
                                            </View>
                                            <Text style={{ fontSize: 10.5, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
                                              {`${s().max_time_ms} ms  ${formatMemoryKib(s().max_memory_kib)}`}
                                            </Text>
                                          </>
                                        );
                                      }}
                                    </If>
                                  </View>
                                );
                              }}
                            </Each>
                          </View>
                        </If>

                        {/* Test rows */}
                        <Each of={matrixRows()} key={(row) => String(row.index)}>
                          {(row, rowIdx) => {
                            const isLast = () => rowIdx() === matrixRows().length - 1;
                            const valFailed = () => row().validator_ok === false;
                            return (
                              <View
                                style={{
                                  flexDirection: "row",
                                  alignItems: "center",
                                  borderBottomWidth: isLast() ? 0 : 1,
                                  minHeight: 38,
                                }}
                                class="border-border"
                              >
                                <View
                                  onPress={() => jumpToTest(row().index)}
                                  class="hover:bg-muted/40"
                                  style={{
                                    width: 184,
                                    flexShrink: 0,
                                    paddingLeft: 12,
                                    paddingRight: 8,
                                    paddingTop: 5,
                                    paddingBottom: 5,
                                    gap: 1,
                                    ...colorTransition(120),
                                  }}
                                >
                                  <View style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>
                                    <Text style={{ fontSize: 11, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
                                      {String(row().index).padStart(2, "0")}
                                    </Text>
                                    <Text
                                      style={{ numberOfLines: 1, textOverflow: "ellipsis", flex: 1, fontSize: 12, fontFamily: MONO }}
                                      class={valFailed() ? "text-destructive" : "text-foreground"}
                                    >
                                      {row().name}
                                    </Text>
                                    <If when={valFailed()}>
                                      <View tooltip={tip(row().validator_message ?? t("build.detail.validatorFailed"))}>
                                        <Icon name="triangleAlert" size={13} tone="destructive" />
                                      </View>
                                    </If>
                                  </View>
                                  <If when={row().tag || row().size_bytes > 0}>
                                    <View style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>
                                      <If when={row().tag || row().is_sample}>
                                        <Text style={{ fontSize: 10.5 }} class="text-muted-foreground">
                                          {row().tag ?? t("build.matrix.sampleTag")}
                                        </Text>
                                      </If>
                                      <If when={row().size_bytes > 0}>
                                        <Text style={{ fontSize: 10.5, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
                                          {formatBytesCompact(row().size_bytes)}
                                        </Text>
                                      </If>
                                    </View>
                                  </If>
                                </View>

                                <Each of={matrixCols()} key={(col) => `${row().index}:${col.name}`}>
                                  {(col) => {
                                    const cell = () => matrixLookup().get(matrixCellKey(row().index, col().name));
                                    const solSum = () => solutionSummaryByName().get(col().name);
                                    const isSelected = () =>
                                      selectedCell()?.testIndex === row().index && selectedCell()?.solution === col().name;
                                    const anomaly = () => {
                                      const c = cell();
                                      if (!c) return false;
                                      return isCellVerdictAnomaly(c.verdict, col().expected, col().is_main, solSum()?.matched);
                                    };
                                    return (
                                      <View
                                        testID={`matrix-cell-${row().index}-${col().name}`}
                                        onPress={() => {
                                          if (!cell()) return;
                                          if (isSelected()) selectMatrixCellWithCollapse(undefined);
                                          else selectMatrixCellWithCollapse({ testIndex: row().index, solution: col().name });
                                        }}
                                        class={
                                          isSelected()
                                            ? "bg-accent"
                                            : cell()
                                              ? "hover:bg-muted/60 active:bg-accent"
                                              : undefined
                                        }
                                        style={{
                                          flex: 1,
                                          minWidth: 112,
                                          maxWidth: useCompactCols() ? 220 : undefined,
                                          minHeight: 38,
                                          paddingLeft: 10,
                                          paddingRight: 10,
                                          paddingTop: 5,
                                          paddingBottom: 5,
                                          flexDirection: "row",
                                          alignItems: "center",
                                          gap: 8,
                                          ...colorTransition(120),
                                        }}
                                      >
                                        <If
                                          when={cell()}
                                          else={
                                            <Text style={{ fontSize: 11.5 }} class="text-muted-foreground">
                                              –
                                            </Text>
                                          }
                                        >
                                          {(c) => {
                                            const badge = () => verdictBadgeStyle(c().verdict, dark());
                                            const errColor = () => verdictBadgeStyle("WA", dark()).fg;
                                            return (
                                              <>
                                                <View
                                                  style={{
                                                    height: 20,
                                                    minWidth: 32,
                                                    paddingLeft: 6,
                                                    paddingRight: 6,
                                                    borderRadius: 4,
                                                    borderWidth: anomaly() ? 1.5 : 0,
                                                    borderColor: anomaly() ? errColor() : "transparent",
                                                    alignItems: "center",
                                                    justifyContent: "center",
                                                    backgroundColor: badge().bg,
                                                  }}
                                                >
                                                  <Text
                                                    style={{
                                                      fontSize: 10.5,
                                                      fontWeight: 600,
                                                      fontVariant: ["tabular-nums"],
                                                      color: badge().fg,
                                                    }}
                                                  >
                                                    {c().verdict}
                                                  </Text>
                                                </View>
                                                <Text
                                                  style={{ fontSize: 11.5, fontVariant: ["tabular-nums"] }}
                                                  class="text-secondary-foreground"
                                                >
                                                  {`${c().cpu_time_ms} ms`}
                                                </Text>
                                              </>
                                            );
                                          }}
                                        </If>
                                      </View>
                                    );
                                  }}
                                </Each>
                              </View>
                            );
                          }}
                        </Each>
                      </View>
                    </Scroll>

                    <If when={!activeReport()}>
                      <View style={{ padding: 12, borderTopWidth: 1, alignItems: "center" }} class="border-border bg-muted/20">
                        <Text style={{ fontSize: 12 }} class="text-muted-foreground">
                          {t("build.matrix.emptyHint")}
                        </Text>
                      </View>
                    </If>
                  </If>
                </View>
              </View>
            </Scroll>
          </View>

          {/* On-demand right diagnostic panel: rendered ONLY when a matrix cell is selected */}
          <If when={inspectedCell()}>
            {(cell) => {
              const badge = () => verdictBadgeStyle(cell().verdict, dark());
              const colSpec = () => inspectedColSpec();
              const solSum = () => solutionSummaryByName().get(cell().solution);
              const isAnomaly = () =>
                colSpec()
                  ? isCellVerdictAnomaly(cell().verdict, colSpec()!.expected, colSpec()!.is_main, solSum()?.matched)
                  : false;
              const expectedLabel = () =>
                colSpec()?.is_main ? "AC" : (colSpec()?.expected.join(" / ") ?? "AC");
              const genCmd = () => inspectedTest()?.gen_cmd;
              const valFailed = () => inspectedTest()?.validator_ok === false;
              const valMsg = () => inspectedTest()?.validator_message ?? "";
              const checkerOut = () => cell().checker_message?.trim() ?? "";
              const stderrOut = () => cell().stderr?.trim() ?? "";
              const reproCmd = () => cell().repro_cmd?.trim() ?? "";
              return (
                <View
                  testID="build-cell-diagnostic"
                  style={{
                    width: DIAG_PANEL_WIDTH,
                    flexShrink: 0,
                    borderLeftWidth: 1,
                  }}
                  class="border-border bg-background/40"
                >
                  <Scroll style={{ flex: 1 }}>
                    <View style={{ padding: 14, gap: 10 }}>
                      <View style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>
                        <View
                          style={{
                            height: 20,
                            paddingLeft: 6,
                            paddingRight: 6,
                            borderRadius: 4,
                            justifyContent: "center",
                            backgroundColor: badge().bg,
                          }}
                        >
                          <Text style={{ fontSize: 11, fontWeight: 600, color: badge().fg }}>
                            {cell().verdict}
                          </Text>
                        </View>
                        <Text
                          style={{ flex: 1, numberOfLines: 1, textOverflow: "ellipsis", fontSize: 12.5, fontWeight: 600, fontFamily: MONO }}
                          class="text-foreground"
                        >
                          {`${cell().solution}  #${cell().test_index}`}
                        </Text>
                        <IconBtn icon="x" size={22} iconSize={13} onPress={() => selectMatrixCellWithCollapse(undefined)} />
                      </View>

                      <View style={{ flexDirection: "row", alignItems: "center", justifyContent: "space-between", gap: 8 }}>
                        <Text style={{ fontSize: 11.5, fontFamily: MONO }} class="text-secondary-foreground">
                          {cell().test_name}
                        </Text>
                        <Text style={{ fontSize: 11.5, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
                          {`${cell().cpu_time_ms} ms  ${formatMemoryKib(cell().peak_memory_kib)}`}
                        </Text>
                      </View>

                      <View style={{ flexDirection: "row", alignItems: "center", gap: 5 }}>
                        <Icon
                          name={isAnomaly() ? "circleAlert" : "check"}
                          size={13}
                          tone={isAnomaly() ? "destructive" : "primary"}
                        />
                        <Text
                          style={{ fontSize: 11.5, fontWeight: 500 }}
                          class={isAnomaly() ? "text-destructive" : "text-secondary-foreground"}
                        >
                          {isAnomaly()
                            ? t("build.detail.mismatch", { expected: expectedLabel() })
                            : t("build.detail.matched")}
                        </Text>
                      </View>

                      <If when={valFailed()}>
                        <DiagBlock label={t("build.detail.validatorFailed")} text={valMsg() || t("build.detail.validatorFailed")} />
                      </If>
                      <If when={genCmd()}>
                        <DiagBlock label={t("build.detail.genCmd")} text={genCmd()!} />
                      </If>
                      <If when={checkerOut().length > 0}>
                        <DiagBlock label={t("build.detail.checkerOut")} text={checkerOut()} />
                      </If>
                      <If when={stderrOut().length > 0}>
                        <DiagBlock label={t("build.detail.stderr")} text={stderrOut()} />
                      </If>
                      <If when={reproCmd().length > 0}>
                        <View style={{ gap: 4 }}>
                          <View style={{ flexDirection: "row", alignItems: "center", justifyContent: "space-between" }}>
                            <Text style={{ fontSize: 11.5, fontWeight: 500 }} class="text-secondary-foreground">
                              {t("build.detail.repro")}
                            </Text>
                            <Btn
                              label={copiedRepro() ? t("common.copied") : t("build.detail.copyRepro")}
                              icon={copiedRepro() ? "check" : "copy"}
                              variant="ghost"
                              onPress={() => copyRepro(reproCmd())}
                            />
                          </View>
                          <View style={{ borderRadius: 6, borderWidth: 1, padding: 8 }} class="border-border bg-card">
                            <Text style={{ fontSize: 11, fontFamily: MONO, lineHeight: 16 }} class="text-foreground">
                              {reproCmd()}
                            </Text>
                          </View>
                        </View>
                      </If>
                      <If when={!valFailed() && !genCmd() && checkerOut().length === 0 && stderrOut().length === 0 && reproCmd().length === 0}>
                        <Text style={{ fontSize: 12 }} class="text-muted-foreground">
                          {t("build.detail.noExtra")}
                        </Text>
                      </If>
                    </View>
                  </Scroll>
                </View>
              );
            }}
          </If>
        </View>
      </If>

      {/* ── Sub-view 2: 测试数据 (Tests & Safe Preview) ───────────────────────── */}
      <If when={subTab() === "tests"}>
        <View testID="build-tests-view" style={{ flex: 1, flexDirection: "row", minHeight: 0 }}>
          {/* Left test case list */}
          <View style={{ width: 264, flexShrink: 0, borderRightWidth: 1 }} class="border-border bg-background/40">
            <Scroll style={{ flex: 1 }}>
              <View style={{ padding: 6, gap: 2 }}>
                <If
                  when={matrixRows().length > 0}
                  else={
                    <View style={{ padding: 16, alignItems: "center" }}>
                      <Text style={{ fontSize: 12 }} class="text-muted-foreground">
                        {t("build.tests.empty")}
                      </Text>
                    </View>
                  }
                >
                  <Each of={matrixRows()} key={(r) => String(r.index)}>
                    {(row) => {
                      const selected = () => activeTestIndex() === row().index;
                      const valFailed = () => row().validator_ok === false;
                      const valPassed = () => row().validator_ok === true;
                      return (
                        <View
                          testID={`build-test-row-${row().index}`}
                          onPress={() => selectedTestIndex.set(row().index)}
                          class={selected() ? "bg-accent" : "hover:bg-muted active:bg-accent"}
                          style={{
                            paddingLeft: 10,
                            paddingRight: 10,
                            paddingTop: 7,
                            paddingBottom: 7,
                            borderRadius: 7,
                            gap: 2,
                            ...colorTransition(120),
                          }}
                        >
                          <View style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>
                            <Text style={{ fontSize: 11, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
                              {String(row().index).padStart(2, "0")}
                            </Text>
                            <Text
                              style={{ flex: 1, numberOfLines: 1, textOverflow: "ellipsis", fontSize: 12, fontWeight: 500, fontFamily: MONO }}
                              class={valFailed() ? "text-destructive" : "text-foreground"}
                            >
                              {row().name}
                            </Text>
                            <If when={valFailed()}>
                              <Icon name="triangleAlert" size={13} tone="destructive" />
                            </If>
                            <If when={valPassed()}>
                              <Icon name="check" size={13} tone="primary" />
                            </If>
                          </View>
                          <View style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>
                            <If when={row().tag || row().is_sample}>
                              <Text style={{ fontSize: 10.5 }} class="text-muted-foreground">
                                {row().tag ?? t("build.matrix.sampleTag")}
                              </Text>
                            </If>
                            <If when={row().size_bytes > 0}>
                              <Text style={{ fontSize: 10.5, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
                                {formatBytesCompact(row().size_bytes)}
                              </Text>
                            </If>
                          </View>
                        </View>
                      );
                    }}
                  </Each>
                </If>
              </View>
            </Scroll>
          </View>

          {/* Right safe head/tail preview pane */}
          <Scroll style={{ flex: 1 }}>
            <If when={activeTestRow()}>
              {(row) => {
                const prev = () => activeTestPreview();
                const expanded = () => expandedTestByIndex().get(row().index);
                const genName = () => {
                  const src = expanded()?.source;
                  return src && src.kind === "generated" ? src.gen_name : undefined;
                };
                const sourceFileRel = () => {
                  const src = expanded()?.source;
                  if (!src) return undefined;
                  if (src.kind === "sample") return src.input_rel;
                  if (src.kind === "file") return src.file_rel;
                  return undefined;
                };
                const genFileRel = () => {
                  const g = genName();
                  if (!g) return undefined;
                  const match = (files()?.generators ?? []).find((f) => f === g || f.startsWith(`${g}.`));
                  return match ? `generators/${match}` : undefined;
                };
                return (
                  <View testID="build-test-preview" style={{ padding: 16, gap: 14 }}>
                    <View style={{ flexDirection: "row", alignItems: "center", flexWrap: "wrap", gap: 8 }}>
                      <Text style={{ fontSize: 14, fontWeight: 600, fontFamily: MONO }} class="text-foreground">
                        {`#${String(row().index).padStart(2, "0")}  ${row().name}`}
                      </Text>
                      <If when={row().tag || row().is_sample}>
                        <Chip label={row().tag ?? t("build.matrix.sampleTag")} />
                      </If>
                      <If when={prev()?.ok && prev()!.availableOnDisk}>
                        <Text style={{ fontSize: 12, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
                          {`${formatBytesCompact(prev()!.sizeBytes)}  ${t("build.tests.linesInfo", { lines: prev()!.totalLines })}`}
                        </Text>
                      </If>
                      <View style={{ flex: 1 }} />
                      <If when={genFileRel()}>
                        {(rel) => (
                          <Btn
                            label={t("build.tests.openGenerator")}
                            icon="code"
                            onPress={() => openSource(rel())}
                          />
                        )}
                      </If>
                      <If when={sourceFileRel()}>
                        {(rel) => (
                          <Btn
                            label={t("build.tests.openSource")}
                            icon="fileText"
                            onPress={() => openSource(rel())}
                          />
                        )}
                      </If>
                    </View>

                    {/* Validator status callout when failed */}
                    <If when={row().validator_ok === false}>
                      <View
                        style={{ borderRadius: 8, borderWidth: 1, padding: 12, gap: 8 }}
                        class="border-border bg-muted/40"
                      >
                        <View style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>
                          <Icon name="circleAlert" size={14} tone="destructive" />
                          <Text style={{ flex: 1, fontSize: 12.5, fontWeight: 600 }} class="text-destructive">
                            {t("build.detail.validatorFailed")}
                          </Text>
                          <If when={files()?.validator}>
                            {(valRel) => (
                              <Btn
                                label={t("build.checks.action.openValidator")}
                                icon="code"
                                onPress={() => openSource(valRel())}
                              />
                            )}
                          </If>
                        </View>
                        <If when={row().validator_message}>
                          <Text style={{ fontSize: 12, fontFamily: MONO, lineHeight: 17 }} class="text-foreground">
                            {row().validator_message!}
                          </Text>
                        </If>
                      </View>
                    </If>

                    {/* Generation command */}
                    <If when={row().gen_cmd}>
                      <View style={{ gap: 4 }}>
                        <View style={{ flexDirection: "row", alignItems: "center", justifyContent: "space-between" }}>
                          <Text style={{ fontSize: 12, fontWeight: 500 }} class="text-secondary-foreground">
                            {t("build.detail.genCmd")}
                          </Text>
                          <Btn
                            label={copiedGenCmd() ? t("common.copied") : t("build.tests.copyGenCmd")}
                            icon={copiedGenCmd() ? "check" : "copy"}
                            variant="ghost"
                            onPress={() => copyGen(row().gen_cmd!)}
                          />
                        </View>
                        <View style={{ borderRadius: 8, borderWidth: 1, padding: 10 }} class="border-border bg-muted/30">
                          <Text style={{ fontSize: 12, fontFamily: MONO }} class="text-foreground">
                            {row().gen_cmd!}
                          </Text>
                        </View>
                      </View>
                    </If>

                    {/* Input head/tail monospace preview */}
                    <If
                      when={prev()?.ok && prev()!.availableOnDisk}
                      else={
                        <View style={{ borderRadius: 8, borderWidth: 1, padding: 16 }} class="border-border bg-muted/20">
                          <Text style={{ fontSize: 12.5 }} class="text-muted-foreground">
                            {t("build.tests.notGenerated")}
                          </Text>
                        </View>
                      }
                    >
                      <View style={{ borderRadius: 10, borderWidth: 1, overflow: "hidden" }} class="border-border bg-card">
                        <View style={{ padding: 12 }}>
                          <Text style={{ fontSize: 12, lineHeight: 18, fontFamily: MONO }} class="text-foreground">
                            {prev()!.head}
                          </Text>
                        </View>
                        <If when={prev()!.truncated && prev()!.tail.length > 0}>
                          <View
                            style={{ paddingLeft: 12, paddingRight: 12, paddingTop: 4, paddingBottom: 4, borderTopWidth: 1, borderBottomWidth: 1 }}
                            class="border-border bg-muted/30"
                          >
                            <Text style={{ fontSize: 11 }} class="text-muted-foreground">
                              {t("build.tests.truncatedHint")}
                            </Text>
                          </View>
                          <View style={{ padding: 12 }}>
                            <Text style={{ fontSize: 12, lineHeight: 18, fontFamily: MONO }} class="text-foreground">
                              {prev()!.tail}
                            </Text>
                          </View>
                        </If>
                      </View>
                    </If>
                  </View>
                );
              }}
            </If>
          </Scroll>
        </View>
      </If>

      {/* ── Sub-view 3: 程序与配置 (5 Ordered Polygon Cards) ──────────────────── */}
      <If when={subTab() === "programs"}>
        <Scroll style={{ flex: 1 }}>
          <View testID="build-programs-view" style={{ maxWidth: 820, width: "100%", alignSelf: "center", padding: 16, gap: 14 }}>
            {/* 1. 校验器 (Validator) */}
            <ProgramCard
              title={t("build.programs.validatorTitle")}
              trailing={
                files()?.validator ? (
                  <Btn
                    label={t("build.programs.openFile")}
                    icon="code"
                    onPress={() => openSource(files()!.validator!)}
                  />
                ) : undefined
              }
            >
              <If
                when={files()?.validator}
                else={
                  <Text style={{ fontSize: 12.5 }} class="text-muted-foreground">
                    {t("build.programs.validatorMissing")}
                  </Text>
                }
              >
                <View style={{ gap: 4 }}>
                  <Text style={{ fontSize: 13, fontWeight: 500, fontFamily: MONO }} class="text-foreground">
                    {files()!.validator!}
                  </Text>
                  <Text style={{ fontSize: 12, fontVariant: ["tabular-nums"] }} class="text-secondary-foreground">
                    {t("build.programs.validatorSelfTests", {
                      valid: files()?.validator_self_tests_valid?.length ?? 0,
                      invalid: files()?.validator_self_tests_invalid?.length ?? 0,
                    })}
                  </Text>
                </View>
              </If>
            </ProgramCard>

            {/* 2. 评测器 (Checker) */}
            <ProgramCard
              title={t("build.programs.checkerTitle")}
              trailing={
                files()?.custom_checker ? (
                  <Btn
                    label={t("build.programs.openFile")}
                    icon="code"
                    onPress={() => openSource(files()!.custom_checker!)}
                  />
                ) : undefined
              }
            >
              <View style={{ gap: 4 }}>
                <Text style={{ fontSize: 13, fontWeight: 500 }} class="text-foreground">
                  {files()?.custom_checker
                    ? t("build.programs.checkerCustom", { name: files()!.custom_checker! })
                    : t("build.programs.checkerBuiltin", { name: inspection()?.checker ?? "wcmp" })}
                </Text>
                <If when={files()?.custom_checker}>
                  <Text style={{ fontSize: 12, fontVariant: ["tabular-nums"] }} class="text-secondary-foreground">
                    {t("build.programs.checkerSelfTests", {
                      count: files()?.checker_self_tests?.length ?? 0,
                    })}
                  </Text>
                </If>
              </View>
            </ProgramCard>

            {/* 3. 数据生成器 (Generators) */}
            <ProgramCard
              title={t("build.programs.generatorsTitle")}
              trailing={
                (files()?.generators?.length ?? 0) > 0 ? (
                  <Btn
                    label={t("build.programs.openFolder")}
                    icon="folderOpen"
                    variant="ghost"
                    onPress={() => openSource("generators")}
                  />
                ) : undefined
              }
            >
              <If
                when={(files()?.generators?.length ?? 0) > 0}
                else={
                  <Text style={{ fontSize: 12.5 }} class="text-muted-foreground">
                    {t("build.programs.generatorsEmpty")}
                  </Text>
                }
              >
                <View style={{ gap: 4 }}>
                  <Each of={files()?.generators ?? []} key={(g) => g}>
                    {(gen) => (
                      <ProgramFileRow
                        name={gen()}
                        onOpen={() => openSource(`generators/${gen()}`)}
                      />
                    )}
                  </Each>
                </View>
              </If>
            </ProgramCard>

            {/* 4. 解法 (Solutions) */}
            <ProgramCard
              title={t("build.programs.solutionsTitle")}
              trailing={
                matrixCols().length > 0 ? (
                  <Btn
                    label={t("build.programs.openFolder")}
                    icon="folderOpen"
                    variant="ghost"
                    onPress={() => openSource("solutions")}
                  />
                ) : undefined
              }
            >
              <If
                when={matrixCols().length > 0}
                else={
                  <Text style={{ fontSize: 12.5 }} class="text-muted-foreground">
                    {t("build.programs.solutionsEmpty")}
                  </Text>
                }
              >
                <View style={{ gap: 4 }}>
                  <Each of={matrixCols()} key={(s) => s.name}>
                    {(sol) => {
                      const role = () => solutionRole(sol());
                      const sum = () => solutionSummaryByName().get(sol().name);
                      const roleLabel = () =>
                        role() === "main"
                          ? t("build.matrix.mainBadge")
                          : role() === "alt"
                            ? t("build.matrix.altBadge")
                            : t("build.matrix.wrongBadge");
                      const relPath = () => sol().source_rel ?? `solutions/${sol().name}.cpp`;
                      return (
                        <View
                          onPress={() => openSource(relPath())}
                          class="hover:bg-muted active:bg-accent"
                          style={{
                            minHeight: 34,
                            paddingLeft: 8,
                            paddingRight: 8,
                            borderRadius: 6,
                            flexDirection: "row",
                            alignItems: "center",
                            gap: 8,
                            ...colorTransition(120),
                          }}
                        >
                          <Icon name="code" size={14} tone="muted-foreground" />
                          <Text style={{ fontSize: 12.5, fontWeight: 500, fontFamily: MONO }} class="text-foreground">
                            {sol().name}
                          </Text>
                          <View
                            class={role() === "main" ? "bg-muted border border-border" : "bg-muted"}
                            style={{ height: 18, paddingLeft: 6, paddingRight: 6, borderRadius: 4, justifyContent: "center" }}
                          >
                            <Text
                              style={{ fontSize: 10.5, fontWeight: 500 }}
                              class={role() === "main" ? "text-foreground" : "text-secondary-foreground"}
                            >
                              {roleLabel()}
                            </Text>
                          </View>
                          <Text style={{ fontSize: 11.5 }} class="text-muted-foreground">
                            {t("build.matrix.expectPrefix", {
                              verdict: sol().is_main ? "AC" : sol().expected.join(" / "),
                            })}
                          </Text>
                          <View style={{ flex: 1 }} />
                          <If when={sum()}>
                            {(s) => (
                              <View style={{ flexDirection: "row", alignItems: "center", gap: 5 }}>
                                <Icon
                                  name={s().matched ? "check" : "x"}
                                  size={13}
                                  tone={s().matched ? "primary" : "destructive"}
                                />
                                <Text style={{ fontSize: 11.5, fontVariant: ["tabular-nums"] }} class="text-secondary-foreground">
                                  {`${s().passed_tests}/${s().total_tests}`}
                                </Text>
                              </View>
                            )}
                          </If>
                        </View>
                      );
                    }}
                  </Each>
                </View>
              </If>
            </ProgramCard>

            {/* 5. 配置 (Config) */}
            <ProgramCard
              title={t("build.programs.configTitle")}
              trailing={
                <View style={{ flexDirection: "row", alignItems: "center", gap: 8 }}>
                  <If when={incomplete() && !props.readOnly}>
                    <Btn
                      testID="build-scaffold-btn"
                      label={t("build.action.scaffold")}
                      icon="plus"
                      variant="outline"
                      disabled={isRunningAny()}
                      onPress={scaffoldPackage}
                    />
                  </If>
                  <Btn
                    testID="pkg-file-problem.toml"
                    label={t("build.programs.openFile")}
                    icon="code"
                    onPress={() => openSource("problem.toml")}
                  />
                </View>
              }
            >
              <View style={{ gap: 2 }}>
                <Text style={{ fontSize: 13, fontWeight: 500, fontFamily: MONO }} class="text-foreground">
                  problem.toml
                </Text>
                <Text style={{ fontSize: 12 }} class="text-muted-foreground">
                  {t("build.programs.configDesc")}
                </Text>
              </View>
            </ProgramCard>
          </View>
        </Scroll>
      </If>
    </View>
  );
}

function CompactMetric(props: { label: string; value: string; warn?: boolean; mono?: boolean }) {
  return (
    <View style={{ flexDirection: "row", alignItems: "baseline", gap: 6 }}>
      <Text style={{ fontSize: 11.5 }} class="text-muted-foreground">
        {props.label}
      </Text>
      <Text
        style={{
          fontSize: 12,
          fontWeight: 600,
          fontVariant: ["tabular-nums"],
          fontFamily: props.mono ? MONO : undefined,
        }}
        class={props.warn ? "text-chart-4" : "text-foreground"}
      >
        {props.value}
      </Text>
    </View>
  );
}

function DiagBlock(props: { label: string; text: string }) {
  return (
    <View style={{ gap: 4 }}>
      <Text style={{ fontSize: 11.5, fontWeight: 500 }} class="text-secondary-foreground">
        {props.label}
      </Text>
      <View style={{ borderRadius: 6, borderWidth: 1, padding: 8 }} class="border-border bg-card">
        <Text style={{ fontSize: 11, fontFamily: MONO, lineHeight: 16 }} class="text-secondary-foreground">
          {props.text}
        </Text>
      </View>
    </View>
  );
}

function ProgramCard(props: { title: string; trailing?: any; children?: any }) {
  return (
    <View
      style={{
        borderRadius: 10,
        borderWidth: 1,
        padding: 14,
        gap: 10,
        boxShadow: cardShadow(),
      }}
      class="border-border bg-card"
    >
      <View style={{ flexDirection: "row", alignItems: "center", justifyContent: "space-between", gap: 8 }}>
        <Text style={{ fontSize: 13, fontWeight: 600 }} class="text-foreground">
          {props.title}
        </Text>
        {props.trailing}
      </View>
      {props.children}
    </View>
  );
}

function ProgramFileRow(props: { name: string; onOpen: () => void }) {
  return (
    <View
      onPress={props.onOpen}
      class="hover:bg-muted active:bg-accent"
      style={{
        height: 32,
        paddingLeft: 8,
        paddingRight: 8,
        borderRadius: 6,
        flexDirection: "row",
        alignItems: "center",
        gap: 8,
        ...colorTransition(120),
      }}
    >
      <Icon name="code" size={14} tone="muted-foreground" />
      <Text style={{ flex: 1, numberOfLines: 1, textOverflow: "ellipsis", fontSize: 12.5, fontFamily: MONO }} class="text-foreground">
        {props.name}
      </Text>
      <Icon name="chevronRight" size={13} tone="muted-foreground" />
    </View>
  );
}

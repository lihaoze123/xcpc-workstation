// Polygon-grade local Build Console (4th single-problem tab in Studio).
// Organizes the package workflow into 3 clean segmented sub-views (`结果` | `数据` | `程序`):
// 1. `结果` (Results): compact single-line check bar with actionable next-steps, `仅看异常`
//    toggle, pinned header + summary row over scrollable test rows, dirty-input one-click fix,
//    and on-demand right diagnostic panel with `查看测试点 #N` jump.
// 2. `数据` (Tests): left list with `全部 / 异常 / 自测` filter pills & `+ 添加 ▾` menu
//    (`手写测试`, `生成批次`, `有效/无效校验自测`); right bounded preview with `输入 | 标程输出`
//    toggle, inline `<Editor>` for small hand-written tests (<= 64 KiB), and rule deletion.
// 3. `程序` (Programs): 5 ordered Polygon cards with structured quick config (checker dropdown,
//    solution expect/main menu, `+ 新建` solution/generator) and lightweight single-file
//    quick-edit modal (`PackageQuickEditModal`) with `外部打开` escape hatch.

import { cleanup, Clipboard, colorScheme, computed, effect, Scroll, signal, Text, untracked, View } from "nsh";
import { Editor, Progress } from "nsh/base";
import type { PackageBuildStatusJs, PackageInspectionResultJs, TestCasePreviewJs } from "xcpc-workstation/native";
import { ErrorDisplay } from "../ErrorDisplay";
import { Icon } from "../Icon";
import { locale, t, type MessageKey } from "../i18n";
import { Btn, Chip, IconBtn, Segmented, tip } from "../kit";
import { AppMenu, type MenuEntry } from "../Menu";
import { colorTransition } from "../motion";
import { errorOf, goSettings, preview, refresh, type UiError } from "../store";
import { cardShadow, EASE_STANDARD, editorFontSize, editorLineHeight, editorMono, MONO, ms, token } from "../theme";
import {
  buildMatrixLookup,
  BUILTIN_CHECKERS,
  canInlineEditTest,
  EXPECT_PRESETS,
  formatAcVsTleMargin,
  formatBytesCompact,
  formatMemoryKib,
  formatStdTimeRatio,
  gateBadgeStyle,
  humanizeCheckText,
  isCellVerdictAnomaly,
  isMissingCompilerError,
  isPackageIncomplete,
  isRowAnomalous,
  matchSelfTestResult,
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
import { AddGeneratorBatchModal, ConfirmDeleteRuleModal, CreateSourceModal } from "./PackageModals";
import { PackageQuickEditModal } from "./PackageQuickEditModal";

const POLL_INTERVAL_MS = 120;
const DIAG_PANEL_WIDTH = 320;

type BuildSubTab = "matrix" | "tests" | "programs";
type TestsFilterMode = "all" | "anomalies" | "self_tests";
type PreviewStreamTab = "input" | "ans";

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
  const anomaliesOnly = signal(false);
  const selectedCell = signal<SelectedCellKey | undefined>(undefined);
  const selectedTestIndex = signal<number | undefined>(undefined);
  const testsFilter = signal<TestsFilterMode>("all");
  const previewStream = signal<PreviewStreamTab>("input");
  const inlineEditing = signal(false);
  const inlineInitialText = signal("");
  const inlineDirty = signal(false);
  let inlineDraft = "";
  const checksExpanded = signal(false);
  const actionError = signal<UiError | undefined>(undefined);
  const copiedRepro = signal(false);
  const copiedGenCmd = signal(false);
  const copiedCompileTarget = signal<string | undefined>(undefined);
  const testPreviewRev = signal(0);

  // Modals
  const quickEditPath = signal<string | undefined>(undefined);
  const createModalKind = signal<"solution" | "generator" | undefined>(undefined);
  const genBatchOpen = signal(false);
  const deleteRuleIndex = signal<number | undefined>(undefined);

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
      anomaliesOnly.set(false);
      selectedCell.set(undefined);
      selectedTestIndex.set(undefined);
      testsFilter.set("all");
      previewStream.set("input");
      inlineEditing.set(false);
      inlineDirty.set(false);
      checksExpanded.set(false);
      actionError.set(undefined);
      quickEditPath.set(undefined);
      createModalKind.set(undefined);
      genBatchOpen.set(false);
      deleteRuleIndex.set(undefined);
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

  const anomalousRowSet = computed(() => {
    const set = new Set<number>();
    const cols = matrixCols();
    const lookup = matrixLookup();
    const sumMap = solutionSummaryByName();
    for (const r of matrixRows()) {
      if (isRowAnomalous(r, cols, lookup, sumMap)) set.add(r.index);
    }
    return set;
  });

  const visibleResultRows = computed(() => {
    const rows = matrixRows();
    if (!anomaliesOnly()) return rows;
    const set = anomalousRowSet();
    return rows.filter((r) => set.has(r.index));
  });

  const visibleTestsList = computed(() => {
    const rows = matrixRows();
    if (testsFilter() === "anomalies") {
      const set = anomalousRowSet();
      return rows.filter((r) => set.has(r.index));
    }
    return rows;
  });

  // Keep a valid test selected when switching to the `数据` sub-view.
  const activeTestIndex = computed<number | undefined>(() => {
    const rows = visibleTestsList();
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
    if (subTab() !== "tests" || testsFilter() === "self_tests") return undefined;
    const idx = activeTestIndex();
    if (idx == null) return undefined;
    void activeReport();
    void props.inspectionResult;
    void testPreviewRev();
    return preview.inspectTestCase(props.problemId, String(idx), 40, 20);
  });

  // Reset inline edit & stream toggle when active test changes.
  effect(() => {
    void activeTestIndex();
    untracked(() => {
      inlineEditing.set(false);
      inlineDirty.set(false);
      previewStream.set("input");
    });
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

  function refreshAll() {
    testPreviewRev.set(testPreviewRev() + 1);
    props.onRefreshPackage();
    refresh();
  }

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
    refreshAll();
  }

  function openExternal(relPath: string) {
    void preview.openPackageFile(props.problemId, relPath).then((res) => {
      if (!res.ok) actionError.set(errorOf(res, t("ctx.openPackageFile")));
    });
  }

  function openQuickEdit(relPath: string) {
    quickEditPath.set(relPath);
  }

  function jumpToTest(index: number) {
    testsFilter.set("all");
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
    } else if (act.kind === "open_file") {
      if (act.relPath === "solutions" || act.relPath === "generators") openExternal(act.relPath);
      else openQuickEdit(act.relPath);
    } else {
      subTab.set("programs");
    }
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

  function fixDirtyInputs() {
    if (props.readOnly) return;
    const res = preview.normalizeDirtyInputs(props.problemId);
    if (!res.ok) {
      actionError.set({ code: res.code, error: res.error, context: t("ctx.updatePackage") });
      return;
    }
    actionError.set(undefined);
    refreshAll();
  }

  function switchChecker(checker: string) {
    if (props.readOnly) return;
    if (checker === "checker.cpp" && !files()?.custom_checker) {
      const created = preview.createPackageSource(props.problemId, "custom_checker", "checker");
      if (!created.ok) {
        actionError.set({ code: created.code, error: created.error, context: t("ctx.updatePackage") });
        return;
      }
      const setRes = preview.setPackageChecker(props.problemId, "checker.cpp");
      if (!setRes.ok) {
        actionError.set(errorOf(setRes, t("ctx.updatePackage")));
        return;
      }
      refreshAll();
      openQuickEdit(created.relPath);
      return;
    }
    const res = preview.setPackageChecker(props.problemId, checker);
    if (!res.ok) {
      actionError.set(errorOf(res, t("ctx.updatePackage")));
      return;
    }
    refreshAll();
  }

  function updateSolutionExpect(name: string, expect: readonly string[]) {
    if (props.readOnly) return;
    const res = preview.setSolutionConfig(props.problemId, name, null, [...expect], null);
    if (!res.ok) {
      actionError.set(errorOf(res, t("ctx.updatePackage")));
      return;
    }
    refreshAll();
  }

  function promoteSolutionToMain(name: string) {
    if (props.readOnly) return;
    const res = preview.setSolutionConfig(props.problemId, name, true, ["AC"], null);
    if (!res.ok) {
      actionError.set(errorOf(res, t("ctx.updatePackage")));
      return;
    }
    refreshAll();
  }

  function handleAddTestAction(id: string) {
    if (props.readOnly) return;
    if (id === "hand") {
      const res = preview.addHandTest(props.problemId, null, "hand", "");
      if (!res.ok) {
        actionError.set({ code: res.code, error: res.error, context: t("ctx.updatePackage") });
        return;
      }
      testsFilter.set("all");
      refreshAll();
      // Select the newly appended test and open inline editor.
      const latest = preview.inspectPackage(props.problemId);
      const parsed = latest.ok ? parsePackageInspection(latest.inspectionJson) : undefined;
      const lastTest = parsed?.expanded_tests?.[parsed.expanded_tests.length - 1];
      if (lastTest) {
        selectedTestIndex.set(lastTest.index);
        if (res.fileRel) {
          startInlineEdit(res.fileRel);
        }
      }
    } else if (id === "gen_batch") {
      genBatchOpen.set(true);
    } else if (id === "self_valid" || id === "self_invalid") {
      const existingCount =
        id === "self_valid"
          ? (files()?.validator_self_tests_valid?.length ?? 0)
          : (files()?.validator_self_tests_invalid?.length ?? 0);
      const stem = String(existingCount + 1).padStart(2, "0");
      const kind = id === "self_valid" ? "validator_self_test_valid" : "validator_self_test_invalid";
      const res = preview.createPackageSource(props.problemId, kind, stem);
      if (!res.ok) {
        actionError.set({ code: res.code, error: res.error, context: t("ctx.updatePackage") });
        return;
      }
      refreshAll();
      openQuickEdit(res.relPath);
    }
  }

  function startInlineEdit(fileRel: string) {
    const res = preview.readPackageFile(props.problemId, fileRel);
    if (!res.ok || res.tooLarge) {
      openExternal(fileRel);
      return;
    }
    inlineInitialText.set(res.content);
    inlineDraft = res.content;
    inlineDirty.set(false);
    inlineEditing.set(true);
  }

  function saveInlineEdit(fileRel: string) {
    const res = preview.savePackageFile(props.problemId, fileRel, inlineDraft);
    if (!res.ok) {
      actionError.set(errorOf(res, t("ctx.savePackageFile")));
      return;
    }
    inlineInitialText.set(inlineDraft);
    inlineDirty.set(false);
    inlineEditing.set(false);
    refreshAll();
  }

  function confirmDeleteRule() {
    const idx = deleteRuleIndex();
    deleteRuleIndex.set(undefined);
    if (idx == null || props.readOnly) return;
    const res = preview.removeTestRule(props.problemId, idx);
    if (!res.ok) {
      actionError.set(errorOf(res, t("ctx.updatePackage")));
      return;
    }
    selectedTestIndex.set(undefined);
    refreshAll();
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

  const selfTestFiles = computed(() => files()?.self_test_files ?? []);

  return (
    <View testID="build-console" style={{ flex: 1, minHeight: 0 }} class="bg-card">
      {/* Shared compact top bar: left 3 sub-views, right `[状态] [局部运行?] [筛选?] [取消?] [验证 ▾]` */}
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

          {/* Status pill first */}
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

          {/* Filter toggle (only relevant on Results sub-view when filters exist) */}
          <If when={subTab() === "matrix" && hasFiltersAvailable()}>
            <Btn
              testID="build-filter-toggle"
              label={subsetCount() > 0 ? `${t("build.action.filter")} (${subsetCount()})` : t("build.action.filter")}
              variant="outline"
              onPress={() => filterOpen.set(!filterOpen())}
              style={filterOpen() || hasSubset() ? { backgroundColor: token("accent") } : undefined}
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

          {/* Primary split CTA permanently anchored at the far right edge */}
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

      {/* ── Sub-view 1: 结果 (Results) ────────────────────────────────────────── */}
      <If when={subTab() === "matrix"}>
        <View style={{ flex: 1, flexDirection: "row", minHeight: 0 }}>
          <View style={{ flex: 1, minWidth: 0 }}>
            {/* Compact horizontal metrics strip (hidden before first build) + subtle 仅看异常 toggle */}
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
                    gap: 18,
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
                  <View style={{ flex: 1 }} />
                  <If when={anomalousRowSet().size > 0}>
                    <Chip
                      label={`${t("build.results.anomaliesOnly")} (${anomalousRowSet().size})`}
                      active={anomaliesOnly()}
                      onPress={() => anomaliesOnly.set(!anomaliesOnly())}
                    />
                  </If>
                </View>
              )}
            </If>

            {/* Fixed top notices (errors, compact check bar, compile errors, dirty inputs) + flex-1 sticky-header table */}
            <View style={{ flex: 1, minHeight: 0, padding: 14, gap: 10 }}>
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
                  style={{ borderRadius: 10, borderWidth: 1, overflow: "hidden", flexShrink: 0 }}
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
                    <Scroll style={{ maxHeight: 220 }}>
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
                    </Scroll>
                  </If>
                </View>
              </If>

              {/* Compile errors */}
              <If when={(activeReport()?.compile_failures?.length ?? 0) > 0}>
                <View
                  style={{ borderRadius: 10, borderWidth: 1, padding: 12, gap: 10, flexShrink: 0 }}
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
                                onPress={() => openQuickEdit(cf().source_path)}
                              />
                            </If>
                            <Btn
                              label={isCopied() ? t("common.copied") : t("build.compile.copyStderr")}
                              icon={isCopied() ? "check" : "copy"}
                              variant="ghost"
                              onPress={() => copyCompileStderr(cf().target, cf().stderr)}
                            />
                          </View>
                          <Scroll style={{ maxHeight: 160 }}>
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

              {/* Dirty inputs warning with one-click fix */}
              <If when={(inspection()?.dirty_inputs?.length ?? 0) > 0}>
                <View
                  style={{ borderRadius: 10, borderWidth: 1, padding: 10, gap: 6, flexShrink: 0 }}
                  class="border-border bg-muted/40"
                >
                  <View style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>
                    <Icon name="triangleAlert" size={14} tone="chart-4" />
                    <Text style={{ flex: 1, fontSize: 12, fontWeight: 600 }} class="text-chart-4">
                      {t("build.section.dirtyInputs")}
                    </Text>
                    <If when={!props.readOnly}>
                      <Btn
                        testID="build-fix-dirty-btn"
                        label={t("build.dirty.fixAll")}
                        icon="check"
                        onPress={fixDirtyInputs}
                      />
                    </If>
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

              {/* Sticky-header Result Table: header + summary pinned above scrollable test rows */}
              <View
                testID="build-result-matrix"
                style={{
                  flex: 1,
                  minHeight: 160,
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
                  <Scroll horizontal style={{ flex: 1, maxWidth: "100%" }}>
                    <View style={{ flex: 1, minWidth: "100%", flexGrow: 1, minHeight: 0 }}>
                      {/* Pinned solution header row */}
                      <View
                        style={{
                          flexDirection: "row",
                          alignItems: "center",
                          borderBottomWidth: 1,
                          minHeight: 42,
                          flexShrink: 0,
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

                      {/* Pinned per-solution summary row */}
                      <If when={activeReport() && (activeReport()!.solutions?.length ?? 0) > 0}>
                        <View
                          testID="build-matrix-summary-row"
                          style={{
                            flexDirection: "row",
                            alignItems: "center",
                            borderBottomWidth: 1,
                            minHeight: 42,
                            flexShrink: 0,
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

                      {/* Vertically scrollable test rows */}
                      <Scroll style={{ flex: 1 }}>
                        <If
                          when={visibleResultRows().length > 0}
                          else={
                            <View style={{ padding: 20, alignItems: "center" }}>
                              <Text style={{ fontSize: 12.5 }} class="text-muted-foreground">
                                {anomaliesOnly() ? t("build.results.noAnomalies") : t("build.tests.empty")}
                              </Text>
                            </View>
                          }
                        >
                          <Each of={visibleResultRows()} key={(row) => String(row.index)}>
                            {(row, rowIdx) => {
                              const isLast = () => rowIdx() === visibleResultRows().length - 1;
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
                        </If>
                      </Scroll>

                      <If when={!activeReport()}>
                        <View style={{ padding: 12, borderTopWidth: 1, alignItems: "center", flexShrink: 0 }} class="border-border bg-muted/20">
                          <Text style={{ fontSize: 12 }} class="text-muted-foreground">
                            {t("build.matrix.emptyHint")}
                          </Text>
                        </View>
                      </If>
                    </View>
                  </Scroll>
                </If>
              </View>
            </View>
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

                      <View style={{ flexDirection: "row", alignItems: "center", justifyContent: "space-between", gap: 8 }}>
                        <View style={{ flexDirection: "row", alignItems: "center", gap: 5, flex: 1 }}>
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
                        <Btn
                          testID="diag-view-test-btn"
                          label={t("build.detail.viewTest", { index: cell().test_index })}
                          variant="outline"
                          onPress={() => jumpToTest(cell().test_index)}
                        />
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

      {/* ── Sub-view 2: 数据 (Tests, .ans Preview, Inline Edit, Self-Tests) ───── */}
      <If when={subTab() === "tests"}>
        <View testID="build-tests-view" style={{ flex: 1, flexDirection: "row", minHeight: 0 }}>
          {/* Left test / self-test list */}
          <View style={{ width: 276, flexShrink: 0, borderRightWidth: 1 }} class="border-border bg-background/40">
            {/* List header: filter pills + `+ 添加 ▾` menu */}
            <View
              style={{
                paddingLeft: 8,
                paddingRight: 8,
                paddingTop: 8,
                paddingBottom: 8,
                borderBottomWidth: 1,
                flexDirection: "row",
                alignItems: "center",
                gap: 4,
              }}
              class="border-border"
            >
              <Chip
                label={`${t("build.tests.filterAll")} (${matrixRows().length})`}
                active={testsFilter() === "all"}
                onPress={() => testsFilter.set("all")}
              />
              <If when={anomalousRowSet().size > 0}>
                <Chip
                  label={`${t("build.tests.filterAnomalies")} (${anomalousRowSet().size})`}
                  active={testsFilter() === "anomalies"}
                  onPress={() => testsFilter.set("anomalies")}
                />
              </If>
              <Chip
                label={t("build.tests.filterSelfTests")}
                active={testsFilter() === "self_tests"}
                onPress={() => testsFilter.set("self_tests")}
              />
              <View style={{ flex: 1 }} />
              <If when={!props.readOnly}>
                <AppMenu
                  align="end"
                  minWidth={172}
                  entries={[
                    { id: "hand", label: t("build.tests.addHand"), icon: "fileText" },
                    {
                      id: "gen_batch",
                      label: t("build.tests.addGenBatch"),
                      icon: "code",
                      disabled: (files()?.generators?.length ?? 0) === 0,
                    },
                    { separator: true, id: "sep" },
                    { id: "self_valid", label: t("build.tests.addSelfValid"), icon: "check" },
                    { id: "self_invalid", label: t("build.tests.addSelfInvalid"), icon: "triangleAlert" },
                  ]}
                  onSelect={handleAddTestAction}
                  trigger={
                    <IconBtn
                      testID="build-add-test-menu"
                      icon="plus"
                      size={24}
                      iconSize={14}
                      tooltip={tip(t("build.tests.add"))}
                    />
                  }
                />
              </If>
            </View>

            <Scroll style={{ flex: 1 }}>
              <If
                when={testsFilter() === "self_tests"}
                else={
                  <View style={{ padding: 6, gap: 2 }}>
                    <If
                      when={visibleTestsList().length > 0}
                      else={
                        <View style={{ padding: 16, alignItems: "center" }}>
                          <Text style={{ fontSize: 12 }} class="text-muted-foreground">
                            {t("build.tests.empty")}
                          </Text>
                        </View>
                      }
                    >
                      <Each of={visibleTestsList()} key={(r) => String(r.index)}>
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
                }
              >
                {/* Self-tests list */}
                <View style={{ padding: 6, gap: 2 }}>
                  <If
                    when={selfTestFiles().length > 0}
                    else={
                      <View style={{ padding: 16, alignItems: "center" }}>
                        <Text style={{ fontSize: 12 }} class="text-muted-foreground">
                          {t("build.tests.selfTestsEmpty")}
                        </Text>
                      </View>
                    }
                  >
                    <Each of={selfTestFiles()} key={(f) => f}>
                      {(rel) => {
                        const res = () => matchSelfTestResult(rel(), activeReport()?.self_tests);
                        return (
                          <View
                            onPress={() => openQuickEdit(rel())}
                            class="hover:bg-muted active:bg-accent"
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
                              <Text
                                style={{ flex: 1, numberOfLines: 1, textOverflow: "ellipsis", fontSize: 12, fontWeight: 500, fontFamily: MONO }}
                                class="text-foreground"
                              >
                                {rel().replace(/^tests\//, "")}
                              </Text>
                              <If when={res()}>
                                {(r) => (
                                  <Icon
                                    name={r().passed ? "check" : "circleAlert"}
                                    size={13}
                                    tone={r().passed ? "primary" : "destructive"}
                                  />
                                )}
                              </If>
                            </View>
                            <If when={res()?.message}>
                              <Text style={{ fontSize: 11, numberOfLines: 1, textOverflow: "ellipsis" }} class="text-muted-foreground">
                                {res()!.message!}
                              </Text>
                            </If>
                          </View>
                        );
                      }}
                    </Each>
                  </If>
                </View>
              </If>
            </Scroll>
          </View>

          {/* Right safe head/tail preview & inline editor pane */}
          <Scroll style={{ flex: 1 }}>
            <If when={testsFilter() !== "self_tests" && activeTestRow()}>
              {() => {
                const row = () => activeTestRow()!;
                const prev = () => activeTestPreview();
                const expanded = () => expandedTestByIndex().get(row().index);
                const genName = () => {
                  const src = expanded()?.source;
                  return src && src.kind === "generated" ? src.gen_name : undefined;
                };
                const genFileRel = () => {
                  const g = genName();
                  if (!g) return undefined;
                  const match = (files()?.generators ?? []).find((f) => f === g || f.startsWith(`${g}.`));
                  return match ? `generators/${match}` : undefined;
                };
                const fileRel = () => prev()?.fileRel ?? undefined;
                const ruleIdx = () => prev()?.ruleIndex ?? expanded()?.rule_index ?? undefined;
                const canEditInline = () => !props.readOnly && canInlineEditTest(prev());
                const showAns = () => Boolean(prev()?.ansAvailable);
                const streamHead = () => (previewStream() === "ans" ? (prev()?.ansHead ?? "") : (prev()?.head ?? ""));
                const streamTail = () => (previewStream() === "ans" ? (prev()?.ansTail ?? "") : (prev()?.tail ?? ""));
                const streamTruncated = () =>
                  previewStream() === "ans" ? Boolean(prev()?.ansTruncated) : Boolean(prev()?.truncated);
                const streamSize = () =>
                  previewStream() === "ans" ? (prev()?.ansSizeBytes ?? 0) : (prev()?.sizeBytes ?? 0);
                const streamLines = () =>
                  previewStream() === "ans" ? (prev()?.ansTotalLines ?? 0) : (prev()?.totalLines ?? 0);

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
                          {`${formatBytesCompact(streamSize())}  ${t("build.tests.linesInfo", { lines: streamLines() })}`}
                        </Text>
                      </If>
                      <View style={{ flex: 1 }} />
                      <If when={showAns() && !inlineEditing()}>
                        <Segmented
                          items={[
                            { value: "input" as PreviewStreamTab, label: t("build.tests.tabInput") },
                            { value: "ans" as PreviewStreamTab, label: t("build.tests.tabAns") },
                          ]}
                          value={previewStream()}
                          onChange={(v) => previewStream.set(v)}
                          itemWidth={isZh() ? 74 : 84}
                        />
                      </If>
                      <If when={canEditInline() && previewStream() === "input"}>
                        <If
                          when={inlineEditing()}
                          else={
                            <Btn
                              testID="test-inline-edit-btn"
                              label={t("build.tests.editInline")}
                              icon="pencil"
                              onPress={() => fileRel() && startInlineEdit(fileRel()!)}
                            />
                          }
                        >
                          <Btn
                            testID="test-inline-save-btn"
                            label={t("build.quickEdit.save")}
                            icon="check"
                            variant="primary"
                            disabled={!inlineDirty()}
                            onPress={() => fileRel() && saveInlineEdit(fileRel()!)}
                          />
                          <Btn
                            label={t("common.cancel")}
                            variant="ghost"
                            onPress={() => inlineEditing.set(false)}
                          />
                        </If>
                      </If>
                      <If when={genFileRel()}>
                        {(rel) => (
                          <Btn
                            label={t("build.tests.openGenerator")}
                            icon="code"
                            onPress={() => openQuickEdit(rel())}
                          />
                        )}
                      </If>
                      <If when={!inlineEditing() ? fileRel() : undefined}>
                        {(rel) => (
                          <Btn
                            label={t("build.quickEdit.openExternal")}
                            icon="folderOpen"
                            variant="ghost"
                            onPress={() => openExternal(rel())}
                          />
                        )}
                      </If>
                      <If when={!props.readOnly && !row().is_sample && ruleIdx() != null}>
                        <IconBtn
                          testID="test-delete-rule-btn"
                          icon="trash"
                          tone="destructive"
                          tooltip={tip(t("build.tests.deleteRule"))}
                          onPress={() => deleteRuleIndex.set(ruleIdx()!)}
                        />
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
                                onPress={() => openQuickEdit(valRel())}
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

                    {/* Inline editor for small hand-written tests OR bounded head/tail preview */}
                    <If
                      when={inlineEditing() && fileRel()}
                      else={
                        <If
                          when={prev()?.ok && (previewStream() === "ans" ? prev()!.ansAvailable : prev()!.availableOnDisk)}
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
                                {streamHead()}
                              </Text>
                            </View>
                            {/* Always show truncation banner whenever `truncated` is true, even if `tail` is empty */}
                            <If when={streamTruncated()}>
                              <View
                                style={{
                                  paddingLeft: 12,
                                  paddingRight: 12,
                                  paddingTop: 5,
                                  paddingBottom: 5,
                                  borderTopWidth: 1,
                                  borderBottomWidth: streamTail().length > 0 ? 1 : 0,
                                }}
                                class="border-border bg-muted/30"
                              >
                                <Text style={{ fontSize: 11 }} class="text-muted-foreground">
                                  {t("build.tests.truncatedHint")}
                                </Text>
                              </View>
                            </If>
                            <If when={streamTruncated() && streamTail().length > 0}>
                              <View style={{ padding: 12 }}>
                                <Text style={{ fontSize: 12, lineHeight: 18, fontFamily: MONO }} class="text-foreground">
                                  {streamTail()}
                                </Text>
                              </View>
                            </If>
                          </View>
                        </If>
                      }
                    >
                      <View style={{ height: 360, borderRadius: 10, borderWidth: 1, overflow: "hidden" }} class="border-border bg-card">
                        <Editor
                          style={{ flex: 1 }}
                          defaultValue={inlineInitialText()}
                          highlighting={false}
                          folding={false}
                          tabSize={2}
                          softWrap={false}
                          fontFamily={editorMono()}
                          fontSize={editorFontSize()}
                          lineHeight={Math.round(editorFontSize() * editorLineHeight())}
                          activeLine
                          class="bg-card"
                          classes={{ content: "pl-3 pr-4 pt-3 pb-4" }}
                          onChange={(e) => {
                            inlineDraft = e.value;
                            inlineDirty.set(e.value !== inlineInitialText());
                          }}
                        />
                      </View>
                    </If>
                  </View>
                );
              }}
            </If>
          </Scroll>
        </View>
      </If>

      {/* ── Sub-view 3: 程序 (5 Ordered Polygon Cards + Structured Quick Config) ─ */}
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
                    onPress={() => openQuickEdit(files()!.validator!)}
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

            {/* 2. 评测器 (Checker — inline selector for all 9 built-ins + custom checker.cpp) */}
            <ProgramCard
              title={t("build.programs.checkerTitle")}
              trailing={
                <View style={{ flexDirection: "row", alignItems: "center", gap: 8 }}>
                  <If when={!props.readOnly}>
                    <AppMenu
                      align="end"
                      minWidth={190}
                      entries={[
                        ...BUILTIN_CHECKERS.map((name) => ({
                          id: name,
                          label: name,
                          checked: !files()?.custom_checker && (inspection()?.checker ?? "wcmp") === name,
                        })),
                        { separator: true, id: "sep" },
                        {
                          id: "checker.cpp",
                          label: t("build.programs.customChecker"),
                          icon: "code",
                          checked: Boolean(files()?.custom_checker),
                        },
                      ]}
                      onSelect={switchChecker}
                      trigger={
                        <View
                          testID="program-checker-select"
                          class="border border-border bg-card hover:bg-muted active:bg-accent"
                          style={{
                            height: 28,
                            paddingLeft: 10,
                            paddingRight: 8,
                            borderRadius: 6,
                            flexDirection: "row",
                            alignItems: "center",
                            gap: 6,
                            ...colorTransition(120),
                          }}
                        >
                          <Text style={{ fontSize: 12.5, fontFamily: MONO }} class="text-foreground">
                            {files()?.custom_checker ?? inspection()?.checker ?? "wcmp"}
                          </Text>
                          <Icon name="chevronDown" size={13} tone="muted-foreground" />
                        </View>
                      }
                    />
                  </If>
                  <If when={files()?.custom_checker}>
                    <Btn
                      label={t("build.programs.openFile")}
                      icon="code"
                      onPress={() => openQuickEdit(files()!.custom_checker!)}
                    />
                  </If>
                </View>
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

            {/* 3. 数据生成器 (Generators — clean basenames + New button + Open folder) */}
            <ProgramCard
              title={t("build.programs.generatorsTitle")}
              trailing={
                <View style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>
                  <If when={!props.readOnly}>
                    <Btn
                      testID="program-new-generator-btn"
                      label={t("build.programs.newItem")}
                      icon="plus"
                      onPress={() => createModalKind.set("generator")}
                    />
                  </If>
                  <If when={(files()?.generators?.length ?? 0) > 0}>
                    <Btn
                      label={t("build.programs.openFolder")}
                      icon="folderOpen"
                      variant="ghost"
                      onPress={() => openExternal("generators")}
                    />
                  </If>
                </View>
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
                        onOpen={() => openQuickEdit(`generators/${gen()}`)}
                      />
                    )}
                  </Each>
                </View>
              </If>
            </ProgramCard>

            {/* 4. 解法 (Solutions — New button + inline expect/main menu + quick-edit) */}
            <ProgramCard
              title={t("build.programs.solutionsTitle")}
              trailing={
                <View style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>
                  <If when={!props.readOnly}>
                    <Btn
                      testID="program-new-solution-btn"
                      label={t("build.programs.newItem")}
                      icon="plus"
                      onPress={() => createModalKind.set("solution")}
                    />
                  </If>
                  <If when={matrixCols().length > 0}>
                    <Btn
                      label={t("build.programs.openFolder")}
                      icon="folderOpen"
                      variant="ghost"
                      onPress={() => openExternal("solutions")}
                    />
                  </If>
                </View>
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
                      const curExpectKey = () => (sol().is_main ? "AC" : sol().expected.join("/"));
                      return (
                        <View
                          style={{
                            minHeight: 36,
                            paddingLeft: 8,
                            paddingRight: 6,
                            borderRadius: 6,
                            flexDirection: "row",
                            alignItems: "center",
                            gap: 8,
                          }}
                          class="hover:bg-muted/50"
                        >
                          <View
                            onPress={() => openQuickEdit(relPath())}
                            style={{ flex: 1, minWidth: 0, flexDirection: "row", alignItems: "center", gap: 8 }}
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
                          </View>

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

                          <If
                            when={!props.readOnly}
                            else={
                              <Text style={{ fontSize: 11.5 }} class="text-muted-foreground">
                                {t("build.matrix.expectPrefix", {
                                  verdict: sol().is_main ? "AC" : sol().expected.join(" / "),
                                })}
                              </Text>
                            }
                          >
                            <AppMenu
                              align="end"
                              minWidth={168}
                              entries={[
                                ...EXPECT_PRESETS.map((preset) => {
                                  const key = preset.join("/");
                                  return {
                                    id: `exp:${key}`,
                                    label: t("build.matrix.expectPrefix", { verdict: preset.join(" / ") }),
                                    checked: curExpectKey() === key,
                                    disabled: sol().is_main && key !== "AC",
                                  };
                                }),
                                ...(!sol().is_main
                                  ? [
                                      { separator: true as const, id: "sep" },
                                      { id: "set_main", label: t("build.programs.setMain"), icon: "check" as const },
                                    ]
                                  : []),
                              ]}
                              onSelect={(id) => {
                                if (id === "set_main") promoteSolutionToMain(sol().name);
                                else if (id.startsWith("exp:")) {
                                  updateSolutionExpect(sol().name, id.slice(4).split("/"));
                                }
                              }}
                              trigger={
                                <View
                                  testID={`sol-expect-menu-${sol().name}`}
                                  class="border border-border bg-card hover:bg-muted active:bg-accent"
                                  style={{
                                    height: 24,
                                    paddingLeft: 7,
                                    paddingRight: 6,
                                    borderRadius: 5,
                                    flexDirection: "row",
                                    alignItems: "center",
                                    gap: 4,
                                    ...colorTransition(120),
                                  }}
                                >
                                  <Text style={{ fontSize: 11, fontVariant: ["tabular-nums"] }} class="text-secondary-foreground">
                                    {t("build.matrix.expectPrefix", {
                                      verdict: sol().is_main ? "AC" : sol().expected.join(" / "),
                                    })}
                                  </Text>
                                  <Icon name="chevronDown" size={11} tone="muted-foreground" />
                                </View>
                              }
                            />
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
                    onPress={() => openQuickEdit("problem.toml")}
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

      {/* Lightweight single-file quick-editor modal */}
      <PackageQuickEditModal
        open={Boolean(quickEditPath())}
        problemId={props.problemId}
        relPath={quickEditPath()}
        readOnly={props.readOnly}
        onClose={() => quickEditPath.set(undefined)}
        onSaved={refreshAll}
        onOpenExternal={(rel) => openExternal(rel)}
      />

      {/* New solution / generator modal */}
      <CreateSourceModal
        open={Boolean(createModalKind())}
        kind={createModalKind() ?? "solution"}
        problemId={props.problemId}
        onClose={() => createModalKind.set(undefined)}
        onCreated={(relPath) => {
          refreshAll();
          openQuickEdit(relPath);
        }}
      />

      {/* Add generator batch modal */}
      <AddGeneratorBatchModal
        open={genBatchOpen()}
        problemId={props.problemId}
        generators={files()?.generators ?? []}
        onClose={() => genBatchOpen.set(false)}
        onAdded={refreshAll}
      />

      {/* Confirm delete test rule modal */}
      <ConfirmDeleteRuleModal
        open={deleteRuleIndex() != null}
        onClose={() => deleteRuleIndex.set(undefined)}
        onConfirm={confirmDeleteRule}
      />
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

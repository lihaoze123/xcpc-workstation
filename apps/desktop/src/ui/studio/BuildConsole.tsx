// Full-width local Polygon Build Console (4th single-problem tab in Studio).
// 2-column layout:
// - Left main pane (flex: 1): Single split primary CTA (`验证 ▾` / `运行所选`), collapsible
//   subset filter (`筛选`), compact safety margin strip (hidden before first build),
//   inline failed/warn gates + collapsed passing gates pill, and fluid full-width
//   Test × Solution Result Matrix with per-solution summary row & expectation anomaly highlight.
// - Right detail sidebar (320pt): Selected cell diagnostic card (when clicked) + Package Files
//   list (starting with `problem.toml`), opening in the system default external editor.

import { cleanup, Clipboard, colorScheme, computed, effect, Scroll, signal, Text, untracked, View } from "nsh";
import { Progress } from "nsh/base";
import type { PackageBuildStatusJs, PackageInspectionResultJs } from "xcpc-workstation/native";
import { ErrorDisplay } from "../ErrorDisplay";
import { Icon } from "../Icon";
import { t, type MessageKey } from "../i18n";
import { Btn, Chip, IconBtn, tip } from "../kit";
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
  isCellVerdictAnomaly,
  isMissingCompilerError,
  isPackageIncomplete,
  matrixCellKey,
  parseBuildReport,
  parsePackageInspection,
  partitionGates,
  resolveSolutionHeaderSpecs,
  selectMatrixCell,
  toggleSubsetSelection,
  verdictBadgeStyle,
  type BuildReportData,
  type MatrixCellData,
  type PackageInspectionData,
  type SelectedCellKey,
  type SolutionSummaryData,
} from "./buildLogic";

const POLL_INTERVAL_MS = 120;
const SIDEBAR_WIDTH = 320;

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
  controller?: (c: BuildConsoleController) => void;
}) {
  const buildStatus = signal<PackageBuildStatusJs>(preview.packageBuildStatus());
  const selectedSolutions = signal<string[]>([]);
  const selectedTests = signal<string[]>([]);
  const filterOpen = signal(false);
  const selectedCell = signal<SelectedCellKey | undefined>(undefined);
  const gatesExpanded = signal(false);
  const actionError = signal<UiError | undefined>(undefined);
  const copiedRepro = signal(false);
  const copiedCompileTarget = signal<string | undefined>(undefined);

  let pollTimer: number | undefined;
  let copyTimer: number | undefined;
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

  // Reset per-problem selection when switching problems, and resume polling if a build is active.
  effect(() => {
    void props.problemId;
    untracked(() => {
      selectedSolutions.set([]);
      selectedTests.set([]);
      filterOpen.set(false);
      selectedCell.set(undefined);
      gatesExpanded.set(false);
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

  const allGates = computed(() => activeReport()?.gates ?? inspection()?.gates ?? []);
  const splitGates = computed(() => partitionGates(allGates()));
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

  function copyRepro(cmd: string) {
    void Clipboard.writeText(cmd).then(() => {
      copiedRepro.set(true);
      clearTimeout(copyTimer);
      copyTimer = setTimeout(() => copiedRepro.set(false), 1500);
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

  const splitMenuEntries = (): MenuEntry[] => [
    { id: "debug", label: t("build.action.runWithoutGates"), icon: "code" },
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

  return (
    <View testID="build-console" style={{ flex: 1, flexDirection: "row", minHeight: 0 }} class="bg-card">
      {/* Left main pane: streamlined action bar + metrics (post-build) + gates + result matrix */}
      <View style={{ flex: 1, minWidth: 0 }}>
        {/* Top action bar */}
        <View
          style={{
            paddingLeft: 14,
            paddingRight: 14,
            paddingTop: 10,
            paddingBottom: 10,
            gap: 8,
            borderBottomWidth: 1,
          }}
          class="border-border bg-background/50"
        >
          <View style={{ flexDirection: "row", alignItems: "center", flexWrap: "wrap", gap: 8 }}>
            {/* Single primary split CTA: 验证 ▾ (or 运行所选 when subset filters are active) */}
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
                      align="start"
                      minWidth={184}
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

            {/* Collapsible Filter toggle */}
            <If when={hasFiltersAvailable()}>
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

            <If when={incomplete() && !props.readOnly}>
              <Btn
                testID="build-scaffold-btn"
                label={t("build.action.scaffold")}
                icon="plus"
                variant="ghost"
                disabled={isRunningAny()}
                onPress={scaffoldPackage}
              />
            </If>

            <View style={{ flex: 1 }} />

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

          {/* Collapsible subset filter chips with clear group labels */}
          <If when={filterOpen() && hasFiltersAvailable()}>
            <View
              testID="build-filter-panel"
              style={{ paddingTop: 4, gap: 6 }}
            >
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

        {/* Compact horizontal metrics strip: hidden completely before first build */}
        <If when={activeReport()}>
          {(rep) => (
            <View
              testID="build-metrics-strip"
              style={{
                paddingLeft: 14,
                paddingRight: 14,
                paddingTop: 8,
                paddingBottom: 8,
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

        {/* Scrollable main content: inline fail/warn gates + errors + result matrix */}
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

            {/* Gates: fail/warn show message inline; passing gates collapse into a single toggle pill */}
            <If when={allGates().length > 0}>
              <View style={{ gap: 6 }}>
                <If when={splitGates().attention.length > 0}>
                  <View style={{ gap: 6 }}>
                    <Each of={splitGates().attention} key={(g) => g.id}>
                      {(gate) => {
                        const colors = () => gateBadgeStyle(gate().status, dark());
                        return (
                          <View
                            testID={`build-gate-${gate().id}`}
                            style={{
                              paddingLeft: 10,
                              paddingRight: 10,
                              paddingTop: 7,
                              paddingBottom: 7,
                              borderRadius: 7,
                              flexDirection: "row",
                              alignItems: "flex-start",
                              gap: 8,
                              backgroundColor: colors().bg,
                            }}
                          >
                            <View style={{ width: 6, height: 6, borderRadius: 3, marginTop: 6, backgroundColor: colors().fg }} />
                            <View style={{ flex: 1, minWidth: 0, gap: 1 }}>
                              <Text style={{ fontSize: 12, fontWeight: 600, color: colors().fg }}>
                                {gate().title}
                              </Text>
                              <Text style={{ fontSize: 12, lineHeight: 17 }} class="text-foreground">
                                {gate().message}
                              </Text>
                            </View>
                          </View>
                        );
                      }}
                    </Each>
                  </View>
                </If>

                <If when={splitGates().passed.length > 0}>
                  <View style={{ flexDirection: "row", alignItems: "center", flexWrap: "wrap", gap: 6 }}>
                    <View
                      testID="build-gates-toggle"
                      onPress={() => gatesExpanded.set(!gatesExpanded())}
                      class="border border-border bg-muted/40 hover:bg-muted active:bg-accent"
                      style={{
                        height: 24,
                        paddingLeft: 8,
                        paddingRight: 7,
                        borderRadius: 6,
                        flexDirection: "row",
                        alignItems: "center",
                        gap: 4,
                        ...colorTransition(120),
                      }}
                    >
                      <Text style={{ fontSize: 11.5, fontVariant: ["tabular-nums"] }} class="text-secondary-foreground">
                        {gatesExpanded()
                          ? t("build.gates.collapse")
                          : t("build.gates.passedSummary", { count: splitGates().passed.length })}
                      </Text>
                      <Icon
                        name="chevronDown"
                        size={12}
                        tone="muted-foreground"
                        style={{ transform: gatesExpanded() ? "rotate(180deg)" : "rotate(0deg)" }}
                      />
                    </View>
                    <If when={gatesExpanded()}>
                      <Each of={splitGates().passed} key={(g) => g.id}>
                        {(gate) => {
                          const colors = () => gateBadgeStyle(gate().status, dark());
                          return (
                            <View
                              tooltip={tip(gate().message)}
                              style={{
                                height: 24,
                                paddingLeft: 8,
                                paddingRight: 8,
                                borderRadius: 6,
                                flexDirection: "row",
                                alignItems: "center",
                                gap: 5,
                                backgroundColor: colors().bg,
                              }}
                            >
                              <View style={{ width: 6, height: 6, borderRadius: 3, backgroundColor: colors().fg }} />
                              <Text style={{ fontSize: 11.5, fontWeight: 500, color: colors().fg }}>
                                {gate().title}
                              </Text>
                            </View>
                          );
                        }}
                      </Each>
                    </If>
                  </View>
                </If>
              </View>
            </If>

            {/* Compile errors: full copyable stderr + click-to-open failing source file */}
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

            {/* Fluid full-width Result Matrix (no redundant header, no vertical borders) */}
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
                        minHeight: 40,
                      }}
                      class="border-border bg-muted/30"
                    >
                      <View style={{ width: 176, flexShrink: 0, paddingLeft: 12, paddingRight: 8, paddingTop: 6, paddingBottom: 6 }}>
                        <Text style={{ fontSize: 11.5, fontWeight: 600 }} class="text-secondary-foreground">
                          {t("build.matrix.testHeader")}
                        </Text>
                      </View>
                      <Each of={matrixCols()} key={(col) => col.name}>
                        {(col) => (
                          <View
                            style={{
                              flex: 1,
                              minWidth: 108,
                              paddingLeft: 10,
                              paddingRight: 10,
                              paddingTop: 6,
                              paddingBottom: 6,
                              gap: 1,
                            }}
                          >
                            <Text
                              style={{ numberOfLines: 1, textOverflow: "ellipsis", fontSize: 12, fontWeight: 600, fontFamily: MONO }}
                              class="text-foreground"
                            >
                              {col().is_main ? `★ ${col().name}` : col().name}
                            </Text>
                            <Text style={{ fontSize: 10.5, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
                              {col().is_main ? "AC" : col().expected.join("/")}
                            </Text>
                          </View>
                        )}
                      </Each>
                    </View>

                    {/* Per-solution summary row (pinned right below header when report exists) */}
                    <If when={activeReport() && (activeReport()!.solutions?.length ?? 0) > 0}>
                      <View
                        testID="build-matrix-summary-row"
                        style={{
                          flexDirection: "row",
                          alignItems: "center",
                          borderBottomWidth: 1,
                          minHeight: 40,
                        }}
                        class="border-border bg-muted/15"
                      >
                        <View style={{ width: 176, flexShrink: 0, paddingLeft: 12, paddingRight: 8 }}>
                          <Text style={{ fontSize: 11.5, fontWeight: 500 }} class="text-muted-foreground">
                            {t("build.summary.label")}
                          </Text>
                        </View>
                        <Each of={matrixCols()} key={(col) => `sum:${col.name}`}>
                          {(col) => {
                            const sum = () => solutionSummaryByName().get(col().name);
                            return (
                              <View style={{ flex: 1, minWidth: 108, paddingLeft: 10, paddingRight: 10, paddingTop: 6, paddingBottom: 6, gap: 1 }}>
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
                                          <Text
                                            style={{ fontSize: 12, fontWeight: 700, color: badge().fg }}
                                          >
                                            {s().matched ? "✓" : "✗"}
                                          </Text>
                                          <Text
                                            style={{ fontSize: 11.5, fontWeight: 600, fontVariant: ["tabular-nums"], color: s().matched ? undefined : badge().fg }}
                                            class={s().matched ? "text-foreground" : undefined}
                                          >
                                            {`${s().passed_tests}/${s().total_tests}`}
                                          </Text>
                                        </View>
                                        <Text style={{ fontSize: 10.5, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
                                          {`${s().max_time_ms}ms  ${formatMemoryKib(s().max_memory_kib)}`}
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
                            <View style={{ width: 176, flexShrink: 0, paddingLeft: 12, paddingRight: 8, paddingTop: 5, paddingBottom: 5, gap: 1 }}>
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
                                      if (isSelected()) selectedCell.set(undefined);
                                      else selectedCell.set({ testIndex: row().index, solution: col().name });
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
                                      minWidth: 108,
                                      minHeight: 38,
                                      paddingLeft: 10,
                                      paddingRight: 10,
                                      paddingTop: 5,
                                      paddingBottom: 5,
                                      flexDirection: "row",
                                      alignItems: "center",
                                      justifyContent: "space-between",
                                      gap: 6,
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
                                                paddingLeft: 6,
                                                paddingRight: 6,
                                                borderRadius: 4,
                                                borderWidth: anomaly() ? 1.5 : 0,
                                                borderColor: anomaly() ? errColor() : "transparent",
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
                                              {`${c().cpu_time_ms}ms`}
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

      {/* Right fixed detail sidebar (~320pt): cell diagnostics (when selected) + Package Files */}
      <View
        testID="build-detail-sidebar"
        style={{
          width: SIDEBAR_WIDTH,
          flexShrink: 0,
          borderLeftWidth: 1,
        }}
        class="border-border bg-background/40"
      >
        <Scroll style={{ flex: 1 }}>
          <View style={{ padding: 14, gap: 14 }}>
            {/* Cell diagnostic card (shown only when a cell is clicked; no empty placeholder) */}
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
                  colSpec()?.is_main ? "AC" : (colSpec()?.expected.join("/") ?? "AC");
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
                      borderRadius: 10,
                      borderWidth: 1,
                      padding: 12,
                      gap: 10,
                      boxShadow: cardShadow(),
                    }}
                    class="border-border bg-card"
                  >
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
                      <IconBtn icon="x" size={22} iconSize={13} onPress={() => selectedCell.set(undefined)} />
                    </View>

                    <View style={{ flexDirection: "row", alignItems: "center", justifyContent: "space-between", gap: 8 }}>
                      <Text style={{ fontSize: 11.5, fontFamily: MONO }} class="text-secondary-foreground">
                        {cell().test_name}
                      </Text>
                      <Text style={{ fontSize: 11.5, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
                        {`${cell().cpu_time_ms} ms  ${formatMemoryKib(cell().peak_memory_kib)}`}
                      </Text>
                    </View>

                    <Text
                      style={{ fontSize: 11.5, fontWeight: 500 }}
                      class={isAnomaly() ? "text-destructive" : "text-secondary-foreground"}
                    >
                      {isAnomaly()
                        ? t("build.detail.mismatch", { expected: expectedLabel() })
                        : t("build.detail.matched")}
                    </Text>

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
                        <View style={{ borderRadius: 6, borderWidth: 1, padding: 8 }} class="border-border bg-muted/30">
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
                );
              }}
            </If>

            {/* Package Files list (always includes `problem.toml` at top) */}
            <If when={files()}>
              {(f) => (
                <View style={{ gap: 8 }}>
                  <View style={{ flexDirection: "row", alignItems: "center", justifyContent: "space-between" }}>
                    <Text style={{ fontSize: 12, fontWeight: 600 }} class="text-foreground">
                      {t("build.section.files")}
                    </Text>
                    <Text style={{ fontSize: 11 }} class="text-muted-foreground">
                      {t("build.files.openHint")}
                    </Text>
                  </View>
                  <View style={{ borderRadius: 10, borderWidth: 1, padding: 4, gap: 2 }} class="border-border bg-card">
                    <SourceFileRow relPath="problem.toml" role={t("build.files.config")} onOpen={openSource} />
                    <If when={f().validator}>
                      <SourceFileRow relPath={f().validator!} role={t("build.files.validator")} onOpen={openSource} />
                    </If>
                    <If when={f().custom_checker}>
                      <SourceFileRow relPath={f().custom_checker!} role={t("build.files.checker")} onOpen={openSource} />
                    </If>
                    <Each of={f().generators ?? []} key={(g) => `gen:${g}`}>
                      {(g) => <SourceFileRow relPath={`generators/${g()}`} role={t("build.files.generators")} onOpen={openSource} />}
                    </Each>
                    <Each of={f().solutions ?? []} key={(s) => `solfile:${s}`}>
                      {(s) => <SourceFileRow relPath={`solutions/${s()}`} role={t("build.files.solutions")} onOpen={openSource} />}
                    </Each>
                  </View>
                </View>
              )}
            </If>
          </View>
        </Scroll>
      </View>
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
      <View style={{ borderRadius: 6, borderWidth: 1, padding: 8 }} class="border-border bg-muted/30">
        <Text style={{ fontSize: 11, fontFamily: MONO, lineHeight: 16 }} class="text-secondary-foreground">
          {props.text}
        </Text>
      </View>
    </View>
  );
}

function SourceFileRow(props: { relPath: string; role: string; onOpen: (relPath: string) => void }) {
  return (
    <View
      testID={`pkg-file-${props.relPath}`}
      onPress={() => props.onOpen(props.relPath)}
      class="hover:bg-muted active:bg-accent"
      style={{
        height: 30,
        paddingLeft: 8,
        paddingRight: 8,
        borderRadius: 6,
        flexDirection: "row",
        alignItems: "center",
        gap: 7,
        ...colorTransition(120),
      }}
    >
      <Icon name="code" size={13} tone="muted-foreground" />
      <Text
        style={{ flex: 1, minWidth: 0, numberOfLines: 1, textOverflow: "ellipsis", fontSize: 12, fontFamily: MONO }}
        class="text-foreground"
      >
        {props.relPath}
      </Text>
      <Text style={{ fontSize: 10.5 }} class="text-muted-foreground">
        {props.role}
      </Text>
    </View>
  );
}

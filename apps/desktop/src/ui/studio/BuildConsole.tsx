// Full-width local Polygon Build Console (4th single-problem tab in Studio).
// 2-column layout:
// - Left main pane (flex: 1): Action bar (Build / Verify / Cancel / Scaffold + subset filters),
//   compact horizontal safety margin strip, restrained gates (only fail/warn expanded by default),
//   and prominent Test × Solution Matrix.
// - Right detail sidebar (320pt): Clicked cell diagnostic card on top + clickable Package Sources
//   file list opening in the system default external editor.

import { cleanup, Clipboard, colorScheme, computed, effect, Scroll, signal, Text, untracked, View } from "nsh";
import { Progress } from "nsh/base";
import type { PackageBuildStatusJs, PackageInspectionResultJs } from "xcpc-workstation/native";
import { ErrorDisplay } from "../ErrorDisplay";
import { Icon } from "../Icon";
import { t, type MessageKey } from "../i18n";
import { Btn, Chip, IconBtn, tip } from "../kit";
import { colorTransition } from "../motion";
import { errorOf, preview, refresh, type UiError } from "../store";
import { cardShadow, MONO } from "../theme";
import {
  buildMatrixLookup,
  formatAcVsTleMargin,
  formatBytesCompact,
  formatMemoryKib,
  formatStdTimeRatio,
  gateBadgeStyle,
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
} from "./buildLogic";

const POLL_INTERVAL_MS = 120;
const SIDEBAR_WIDTH = 320;

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
}) {
  const buildStatus = signal<PackageBuildStatusJs>(preview.packageBuildStatus());
  const selectedSolutions = signal<string[]>([]);
  const selectedTests = signal<string[]>([]);
  const selectedCell = signal<SelectedCellKey | undefined>(undefined);
  const gatesExpanded = signal(false);
  const actionError = signal<UiError | undefined>(undefined);
  const copiedRepro = signal(false);

  let pollTimer: number | undefined;
  let copyTimer: number | undefined;
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
  const visibleGates = computed(() =>
    gatesExpanded() ? allGates() : splitGates().attention,
  );

  const files = computed(() => inspection()?.files);
  const incomplete = computed(() => isPackageIncomplete(files()));
  const hasSubset = computed(() => selectedSolutions().length > 0 || selectedTests().length > 0);

  const matrixCols = computed(() => resolveSolutionHeaderSpecs(inspection(), activeReport()));
  const availableSolutions = computed<string[]>(() => matrixCols().map((s) => s.name));

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

  return (
    <View testID="build-console" style={{ flex: 1, flexDirection: "row", minHeight: 0 }} class="bg-card">
      {/* Left main pane: Action bar + compact metrics + restrained gates + Test × Solution Matrix */}
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
            <Btn
              testID="build-run-debug"
              label={t("build.action.debug")}
              icon="code"
              disabled={props.readOnly || isRunningAny()}
              onPress={() => runBuild("debug")}
            />
            <Btn
              testID="build-run-verify"
              label={t("build.action.verify")}
              icon="check"
              variant="primary"
              disabled={props.readOnly || isRunningAny()}
              onPress={() => runBuild("verify")}
            />
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

          {/* Compact subset filter bar for local debug runs */}
          <If when={availableSolutions().length > 1 || availableTags().length > 0}>
            <View style={{ flexDirection: "row", alignItems: "center", flexWrap: "wrap", gap: 6 }}>
              <Each of={availableSolutions()} key={(s) => `sol:${s}`}>
                {(sol) => (
                  <Chip
                    label={sol()}
                    active={selectedSolutions().includes(sol())}
                    onPress={() => selectedSolutions.set(toggleSubsetSelection(selectedSolutions(), sol()))}
                  />
                )}
              </Each>
              <If when={availableSolutions().length > 0 && availableTags().length > 0}>
                <View style={{ width: 1, height: 12 }} class="bg-border" />
              </If>
              <Each of={availableTags()} key={(tg) => `tag:${tg}`}>
                {(tag) => (
                  <Chip
                    label={`#${tag()}`}
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

          {/* Live progress bar while running */}
          <If when={isRunningThis()}>
            <View style={{ gap: 4, paddingTop: 2 }}>
              <View style={{ flexDirection: "row", alignItems: "center", justifyContent: "space-between", gap: 8 }}>
                <Text style={{ fontSize: 12, fontWeight: 500 }} class="text-foreground">
                  {phaseLabel(buildStatus().phase)}
                  {buildStatus().detail ? ` · ${buildStatus().detail}` : ""}
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

        {/* Compact single-row horizontal safety margin strip */}
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
            gap: 14,
            borderBottomWidth: 1,
          }}
          class="border-border bg-muted/25"
        >
          <CompactMetric
            label={t("build.margin.maxInput")}
            value={activeReport() ? formatBytesCompact(activeReport()!.safety_margin.max_input_bytes) : "–"}
          />
          <View style={{ width: 1, height: 14 }} class="bg-border" />
          <CompactMetric
            label={t("build.margin.stdSlowest")}
            value={formatStdTimeRatio(activeReport()?.safety_margin)}
            warn={(activeReport()?.safety_margin.std_time_ratio ?? 0) > 0.5}
          />
          <View style={{ width: 1, height: 14 }} class="bg-border" />
          <CompactMetric
            label={t("build.margin.acVsTle")}
            value={formatAcVsTleMargin(activeReport()?.safety_margin)}
          />
          <View style={{ width: 1, height: 14 }} class="bg-border" />
          <CompactMetric
            label={t("build.margin.checker")}
            value={activeReport()?.checker ?? inspection()?.checker ?? "wcmp"}
            mono
          />
        </View>

        {/* Scrollable main content: restrained gates + errors + Test × Solution Matrix */}
        <Scroll style={{ flex: 1 }}>
          <View style={{ padding: 14, gap: 12 }}>
            <If when={actionError()}>
              <ErrorDisplay error={actionError()} onDismiss={() => actionError.set(undefined)} />
            </If>
            <If when={buildErrorMsg()}>
              <ErrorDisplay error={{ error: buildErrorMsg(), context: t("ctx.buildPackage") }} />
            </If>

            {/* Restrained gates: fail/warn shown prominently; passing gates collapsed into a single toggle pill */}
            <If when={allGates().length > 0}>
              <View style={{ flexDirection: "row", alignItems: "center", flexWrap: "wrap", gap: 6 }}>
                <Each of={visibleGates()} key={(g) => g.id}>
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
                <If when={splitGates().passed.length > 0}>
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
                </If>
              </View>
            </If>

            {/* Compile errors */}
            <If when={(activeReport()?.compile_failures?.length ?? 0) > 0}>
              <View
                style={{ borderRadius: 10, borderWidth: 1, padding: 12, gap: 8 }}
                class="border-border bg-muted/40"
              >
                <View style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>
                  <Icon name="circleAlert" size={14} tone="destructive" />
                  <Text style={{ fontSize: 12.5, fontWeight: 600 }} class="text-destructive">
                    {t("build.section.compileErrors")}
                  </Text>
                </View>
                <Each of={activeReport()?.compile_failures ?? []} key={(f) => `${f.target}:${f.source_path}`}>
                  {(cf) => (
                    <View style={{ gap: 4 }}>
                      <Text style={{ fontSize: 12, fontWeight: 600, fontFamily: MONO }} class="text-foreground">
                        {`${cf().target} (${cf().source_path})`}
                      </Text>
                      <Text style={{ fontSize: 11.5, lineHeight: 16, fontFamily: MONO }} class="text-secondary-foreground">
                        {cf().stderr.trim().slice(0, 600)}
                      </Text>
                    </View>
                  )}
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
                      {`${d().path}: ${[d().has_crlf && "CRLF", d().has_bom && "BOM", d().missing_trailing_newline && "EOF"].filter(Boolean).join(", ")}`}
                    </Text>
                  )}
                </Each>
              </View>
            </If>

            {/* Prominent Test × Solution Matrix */}
            <View
              style={{
                borderRadius: 10,
                borderWidth: 1,
                overflow: "hidden",
                boxShadow: cardShadow(),
              }}
              class="border-border bg-card"
            >
              <View
                style={{
                  height: 34,
                  paddingLeft: 12,
                  paddingRight: 12,
                  flexDirection: "row",
                  alignItems: "center",
                  justifyContent: "space-between",
                  borderBottomWidth: 1,
                }}
                class="border-border bg-muted/40"
              >
                <Text style={{ fontSize: 12, fontWeight: 600 }} class="text-foreground">
                  {t("build.section.matrix")}
                </Text>
                <Text style={{ fontSize: 11.5, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
                  {`${matrixRows().length} × ${matrixCols().length}`}
                </Text>
              </View>

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
                  <View>
                    {/* Header row */}
                    <View
                      style={{
                        flexDirection: "row",
                        alignItems: "center",
                        borderBottomWidth: 1,
                        minHeight: 36,
                      }}
                      class="border-border bg-muted/25"
                    >
                      <View style={{ width: 168, paddingLeft: 12, paddingRight: 8, paddingTop: 6, paddingBottom: 6 }}>
                        <Text style={{ fontSize: 11.5, fontWeight: 600 }} class="text-secondary-foreground">
                          {t("build.matrix.testHeader")}
                        </Text>
                      </View>
                      <Each of={matrixCols()} key={(col) => col.name}>
                        {(col) => (
                          <View
                            style={{
                              width: 124,
                              paddingLeft: 10,
                              paddingRight: 10,
                              paddingTop: 6,
                              paddingBottom: 6,
                              borderLeftWidth: 1,
                              gap: 1,
                            }}
                            class="border-border"
                          >
                            <Text
                              style={{ numberOfLines: 1, textOverflow: "ellipsis", fontSize: 11.5, fontWeight: 600, fontFamily: MONO }}
                              class="text-foreground"
                            >
                              {col().name}
                            </Text>
                            <Text style={{ fontSize: 10.5, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
                              {col().is_main ? "std" : col().expected.join("/")}
                            </Text>
                          </View>
                        )}
                      </Each>
                    </View>

                    {/* Body rows */}
                    <Each of={matrixRows()} key={(row) => String(row.index)}>
                      {(row) => (
                        <View
                          style={{
                            flexDirection: "row",
                            alignItems: "center",
                            borderBottomWidth: 1,
                            minHeight: 38,
                          }}
                          class="border-border"
                        >
                          <View style={{ width: 168, paddingLeft: 12, paddingRight: 8, paddingTop: 5, paddingBottom: 5, gap: 1 }}>
                            <View style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>
                              <Text style={{ fontSize: 11, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
                                {String(row().index).padStart(2, "0")}
                              </Text>
                              <Text
                                style={{ numberOfLines: 1, textOverflow: "ellipsis", flex: 1, fontSize: 12, fontFamily: MONO }}
                                class="text-foreground"
                              >
                                {row().name}
                              </Text>
                            </View>
                            <If when={row().tag || row().size_bytes > 0}>
                              <Text style={{ fontSize: 10.5, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
                                {[
                                  row().tag ?? (row().is_sample ? t("build.matrix.sampleTag") : ""),
                                  row().size_bytes > 0 ? formatBytesCompact(row().size_bytes) : "",
                                ]
                                  .filter(Boolean)
                                  .join(" · ")}
                              </Text>
                            </If>
                          </View>
                          <Each of={matrixCols()} key={(col) => `${row().index}:${col.name}`}>
                            {(col) => {
                              const cell = () => matrixLookup().get(matrixCellKey(row().index, col().name));
                              const isSelected = () =>
                                selectedCell()?.testIndex === row().index && selectedCell()?.solution === col().name;
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
                                      ? "border-border bg-accent"
                                      : cell()
                                        ? "border-border hover:bg-muted/60 active:bg-accent"
                                        : "border-border"
                                  }
                                  style={{
                                    width: 124,
                                    minHeight: 38,
                                    paddingLeft: 10,
                                    paddingRight: 10,
                                    paddingTop: 5,
                                    paddingBottom: 5,
                                    borderLeftWidth: 1,
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
                                      return (
                                        <>
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
                      )}
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

      {/* Right fixed detail sidebar (~320pt): cell diagnostics + Package Sources */}
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
            {/* Cell diagnostic card when a cell is selected */}
            <If
              when={inspectedCell()}
              else={
                <If when={activeReport()}>
                  <View
                    style={{ borderRadius: 8, borderWidth: 1, padding: 10 }}
                    class="border-border bg-muted/30"
                  >
                    <Text style={{ fontSize: 12, lineHeight: 17 }} class="text-muted-foreground">
                      {t("build.detail.emptyHint")}
                    </Text>
                  </View>
                </If>
              }
            >
              {(cell) => {
                const badge = () => verdictBadgeStyle(cell().verdict, dark());
                const genCmd = () => inspectedTest()?.gen_cmd;
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
                        {`${cell().solution} · #${cell().test_index}`}
                      </Text>
                      <IconBtn icon="x" size={22} iconSize={13} onPress={() => selectedCell.set(undefined)} />
                    </View>

                    <View style={{ flexDirection: "row", alignItems: "center", gap: 10 }}>
                      <Text style={{ fontSize: 11.5, fontFamily: MONO }} class="text-secondary-foreground">
                        {cell().test_name}
                      </Text>
                      <View style={{ flex: 1 }} />
                      <Text style={{ fontSize: 11.5, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
                        {`${cell().cpu_time_ms} ms · ${formatMemoryKib(cell().peak_memory_kib)}`}
                      </Text>
                    </View>

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
                    <If when={!genCmd() && checkerOut().length === 0 && stderrOut().length === 0 && reproCmd().length === 0}>
                      <Text style={{ fontSize: 12 }} class="text-muted-foreground">
                        {t("build.detail.noExtra")}
                      </Text>
                    </If>
                  </View>
                );
              }}
            </If>

            {/* Package Sources list */}
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

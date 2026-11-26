// Independent NSH 0.7 developer diagnostics window built on `nsh-o11y`.
//
// Keeps the main workspace 100% free of technical HUDs while offering:
// 1. Unified 44pt TitleBar (with window controls & live locale window title) + concise tabs
// 2. Metrics: 4 synchronized `TimeSeriesChart` panels via `createChartSync()`, flex-filled height,
//    idle-filtered 50ms-capped frame chart, discrete pulses with last/P95 readouts
// 3. Traces: stable list (queued new-records banner), `TraceWaterfall`, inline named baseline
//    creation, and same-scenario-guarded `TraceComparison`
// 4. Logs: `LogView` with volume histogram, virtualized list, subsystem filter & copy

import {
  Clipboard,
  colorScheme,
  computed,
  effect,
  Scroll,
  signal,
  Text,
  View,
} from "nsh";
import { Input } from "nsh/base";
import {
  createChartSync,
  LogView,
  TimeSeriesChart,
  TraceComparison,
  TraceWaterfall,
  withAlpha,
  type ChartColors,
  type LogEntry,
  type SeriesOptions,
  type Span,
  type Threshold,
} from "nsh-o11y";
import { t } from "../i18n";
import { Btn, createTransientFlag, Segmented, tip } from "../kit";
import { Icon } from "../Icon";
import { MONO, token } from "../theme";
import { TitleBar, useWindowTitle } from "../TitleBar";
import {
  cacheBudgetMb,
  cacheSeries,
  capturedTraces,
  clearAllDiagnostics,
  diagnosticsActiveTab,
  diagnosticsAnnotations,
  diagnosticsPaused,
  diagnosticsWindowMs,
  exportDiagnosticsTrace,
  flushPendingTraces,
  FRAME_CHART_CAP_MS,
  frameSeries,
  latencySeries,
  logStore,
  metricSummaries,
  namedBaselines,
  O11Y_SUBSYSTEMS,
  pendingTraces,
  pipelineSeries,
  removeBaseline,
  saveTraceAsBaseline,
  toggleDiagnosticsPaused,
  type CapturedTrace,
  type MetricStat,
  type NamedBaseline,
  type TimeWindowPreset,
} from "./telemetry";

import { CONTROL, FONT_SIZE, FONT_WEIGHT, RADIUS, SPACE } from "../tokens";
type DiagTab = "metrics" | "traces" | "logs";
type TraceFilter = "all" | "slow" | "inp" | "error";
type TracePaneMode = "waterfall" | "compare";

const WINDOW_OPTIONS: { value: TimeWindowPreset; label: string }[] = [
  { value: 60_000, label: "1m" },
  { value: 300_000, label: "5m" },
  { value: 900_000, label: "15m" },
];

function useChartColors(): () => ChartColors {
  return computed<ChartColors>(() => {
    void colorScheme();
    const fg = token("foreground");
    const muted = token("muted-foreground");
    const border = token("border");
    const primary = token("primary");
    const destructive = token("destructive");
    return {
      text: fg,
      muted,
      grid: withAlpha(border, 0.55),
      cursor: withAlpha(fg, 0.5),
      selection: withAlpha(primary, 0.18),
      error: destructive,
    };
  });
}

function formatStatReadout(label: string, stat: MetricStat): string {
  if (stat.last === null) return `${label} —`;
  return `${label} ${stat.last.toFixed(1)} / P95 ${stat.p95?.toFixed(1) ?? "—"} ms`;
}

function formatSpanCount(count: number): string {
  return count === 1 ? "1 span" : `${count} spans`;
}

// ── Tab 1: Metrics (4 synced TimeSeriesChart panels, flex height) ────────────

function MetricsTab() {
  const sync = createChartSync({ zoom: true });
  const colors = useChartColors();
  const winMs = () => diagnosticsWindowMs();
  const anns = () => diagnosticsAnnotations();

  const frameSeriesOpts = computed<SeriesOptions[]>(() => {
    void colorScheme();
    return [
      { label: t("o11y.series.frameMs"), color: token("primary"), width: 1.5, fill: 0.08 },
      {
        label: t("o11y.series.missedFrames"),
        color: token("destructive"),
        width: 1.5,
        step: true,
        axis: "right",
      },
    ];
  });

  const frameThresholds = computed<Threshold[]>(() => {
    void colorScheme();
    return [
      { value: 16.67, color: withAlpha(token("destructive"), 0.7), label: "60Hz", dash: [4, 3] },
      { value: 8.33, color: withAlpha(token("muted-foreground"), 0.55), label: "120Hz", dash: [2, 3] },
    ];
  });

  // Discrete events: `spanGaps: false` + no area fill so isolated events render as crisp pulses.
  const pipelineSeriesOpts = computed<SeriesOptions[]>(() => {
    void colorScheme();
    return [
      { label: "build_document", color: "#2a78d6", width: 2, spanGaps: false },
      { label: "rasterize_cached", color: "#1baf7a", width: 2, spanGaps: false },
    ];
  });

  const latencySeriesOpts = computed<SeriesOptions[]>(() => {
    void colorScheme();
    return [
      { label: "IPC", color: "#8b5cf6", width: 2, spanGaps: false },
      { label: "ux.inp.*", color: "#eda100", width: 2, spanGaps: false },
    ];
  });

  const slowThreshold = computed<Threshold[]>(() => {
    void colorScheme();
    return [
      { value: 50, color: withAlpha("#eda100", 0.7), label: "50ms", dash: [4, 3] },
    ];
  });

  const cacheSeriesOpts = computed<SeriesOptions[]>(() => {
    void colorScheme();
    return [
      { label: t("o11y.series.cacheMb"), color: "#0ea5e9", width: 1.5, fill: 0.1 },
      {
        label: t("o11y.series.hitRate"),
        color: "#10b981",
        width: 1.5,
        axis: "right",
        unit: "percent",
        spanGaps: true,
      },
    ];
  });

  const cacheThresholds = computed<Threshold[]>(() => {
    void colorScheme();
    const budget = cacheBudgetMb();
    return [
      {
        value: budget,
        color: withAlpha(token("destructive"), 0.65),
        label: `${budget}MB`,
        dash: [4, 3],
      },
    ];
  });

  const cardClass = "bg-card border border-border";
  const cardStyle = {
    flex: 1,
    minWidth: 320,
    minHeight: 220,
    borderRadius: RADIUS.lg,
    padding: SPACE[12],
    flexDirection: "column" as const,
    gap: SPACE[6],
  };

  return (
    <View style={{ flex: 1, padding: SPACE[12], gap: SPACE[8], flexDirection: "column", minHeight: 0 }}>
      {/* Row 1 */}
      <View style={{ flex: 1, flexDirection: "row", gap: SPACE[8], minHeight: 220 }}>
        <View class={cardClass} style={cardStyle} tooltip={tip(t("o11y.metrics.hint"))}>
          <View style={{ flexDirection: "row", alignItems: "center", justifyContent: "space-between", gap: SPACE[8] }}>
            <Text style={{ fontSize: FONT_SIZE.caption, fontWeight: FONT_WEIGHT.semibold }} class="text-foreground">
              {t("o11y.chart.frame")}
            </Text>
            <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[8] }}>
              <If when={metricSummaries().framePeakMs !== null}>
                <Text
                  style={{ fontSize: FONT_SIZE.micro, fontFamily: MONO, fontVariant: ["tabular-nums"] }}
                  class="text-destructive"
                >
                  {`↑ ${t("o11y.metrics.peak")} ${metricSummaries().framePeakMs?.toFixed(1)} ms`}
                </Text>
              </If>
              <Btn
                label={t("o11y.metrics.resetZoom")}
                size="sm"
                variant="ghost"
                tooltip={tip(t("o11y.metrics.hint"))}
                onPress={() => sync.reset()}
              />
            </View>
          </View>
          <TimeSeriesChart
            testID="o11y-chart-frame"
            data={frameSeries}
            series={frameSeriesOpts()}
            unit="ms"
            yMin={0}
            yMax={FRAME_CHART_CAP_MS}
            y2={{ unit: "short", softMin: 0, softMax: 4 }}
            window={winMs()}
            thresholds={frameThresholds()}
            annotations={anns()}
            sync={sync}
            colors={colors()}
            style={{ flex: 1, minHeight: 170 }}
          />
        </View>

        <View class={cardClass} style={cardStyle} tooltip={tip(t("o11y.metrics.hint"))}>
          <View style={{ flexDirection: "row", alignItems: "center", justifyContent: "space-between", gap: SPACE[8] }}>
            <Text style={{ fontSize: FONT_SIZE.caption, fontWeight: FONT_WEIGHT.semibold }} class="text-foreground">
              {t("o11y.chart.pipeline")}
            </Text>
            <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[8] }}>
              <Text style={{ fontSize: FONT_SIZE.micro, fontFamily: MONO, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
                {formatStatReadout("compile", metricSummaries().buildDoc)}
              </Text>
              <Text style={{ fontSize: FONT_SIZE.micro, fontFamily: MONO, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
                {formatStatReadout("raster", metricSummaries().rasterize)}
              </Text>
            </View>
          </View>
          <TimeSeriesChart
            testID="o11y-chart-pipeline"
            data={pipelineSeries}
            series={pipelineSeriesOpts()}
            unit="ms"
            softMin={0}
            softMax={60}
            window={winMs()}
            thresholds={slowThreshold()}
            annotations={anns()}
            sync={sync}
            colors={colors()}
            style={{ flex: 1, minHeight: 170 }}
          />
        </View>
      </View>

      {/* Row 2 */}
      <View style={{ flex: 1, flexDirection: "row", gap: SPACE[8], minHeight: 220 }}>
        <View class={cardClass} style={cardStyle} tooltip={tip(t("o11y.metrics.hint"))}>
          <View style={{ flexDirection: "row", alignItems: "center", justifyContent: "space-between", gap: SPACE[8] }}>
            <Text style={{ fontSize: FONT_SIZE.caption, fontWeight: FONT_WEIGHT.semibold }} class="text-foreground">
              {t("o11y.chart.latency")}
            </Text>
            <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[8] }}>
              <Text style={{ fontSize: FONT_SIZE.micro, fontFamily: MONO, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
                {formatStatReadout("IPC", metricSummaries().ipc)}
              </Text>
              <Text style={{ fontSize: FONT_SIZE.micro, fontFamily: MONO, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
                {formatStatReadout("INP", metricSummaries().inp)}
              </Text>
            </View>
          </View>
          <TimeSeriesChart
            testID="o11y-chart-latency"
            data={latencySeries}
            series={latencySeriesOpts()}
            unit="ms"
            softMin={0}
            softMax={60}
            window={winMs()}
            thresholds={slowThreshold()}
            annotations={anns()}
            sync={sync}
            colors={colors()}
            style={{ flex: 1, minHeight: 170 }}
          />
        </View>

        <View class={cardClass} style={cardStyle} tooltip={tip(t("o11y.metrics.hint"))}>
          <View style={{ flexDirection: "row", alignItems: "center", justifyContent: "space-between", gap: SPACE[8] }}>
            <Text style={{ fontSize: FONT_SIZE.caption, fontWeight: FONT_WEIGHT.semibold }} class="text-foreground">
              {t("o11y.chart.cache")}
            </Text>
          </View>
          <TimeSeriesChart
            testID="o11y-chart-cache"
            data={cacheSeries}
            series={cacheSeriesOpts()}
            unit="short"
            softMin={0}
            softMax={cacheBudgetMb()}
            y2={{ unit: "percent", min: 0, max: 100 }}
            window={winMs()}
            thresholds={cacheThresholds()}
            annotations={anns()}
            sync={sync}
            colors={colors()}
            style={{ flex: 1, minHeight: 170 }}
          />
        </View>
      </View>
    </View>
  );
}

// ── Tab 2: Traces (Stable list + Waterfall + Named Baselines + Guarded Diff) ─

function TracesTab(props: {
  selectedTraceId: () => string | null;
  onSelectTrace: (id: string | null) => void;
}) {
  const filter = signal<TraceFilter>("all");
  const mode = signal<TracePaneMode>("waterfall");
  const selectedSpan = signal<Span | null>(null);
  const activeBaselineId = signal<string | null>(null);
  const namingBaseline = signal(false);
  const baselineDraft = signal("");
  const colors = useChartColors();
  const savedFlag = createTransientFlag(1500);

  const filteredTraces = computed<CapturedTrace[]>(() => {
    const f = filter();
    const all = capturedTraces();
    if (f === "all") return all;
    if (f === "slow") return all.filter((tr) => tr.durationMs >= 50);
    if (f === "inp") return all.filter((tr) => tr.inpMs !== undefined);
    return all.filter((tr) => tr.hasError);
  });

  const selectedTrace = computed<CapturedTrace | null>(() => {
    const list = filteredTraces();
    const id = props.selectedTraceId();
    return list.find((tr) => tr.id === id) ?? list[0] ?? null;
  });

  const activeBaseline = computed<NamedBaseline | null>(() => {
    const list = namedBaselines();
    const id = activeBaselineId();
    return list.find((b) => b.id === id) ?? list[0] ?? null;
  });

  const compareMismatch = computed(() => {
    const b = activeBaseline();
    const tr = selectedTrace();
    if (!b || !tr) return false;
    return b.scenarioKey !== tr.scenarioKey;
  });

  effect(() => {
    const tr = selectedTrace();
    if (tr && props.selectedTraceId() !== tr.id) {
      props.onSelectTrace(tr.id);
    }
    selectedSpan.set(null);
  });

  const openBaselineInput = () => {
    const tr = selectedTrace();
    if (!tr) return;
    baselineDraft.set(`${tr.name} (${tr.durationMs.toFixed(1)} ms)`);
    namingBaseline.set(true);
  };

  const confirmSaveBaseline = () => {
    const tr = selectedTrace();
    if (!tr) {
      namingBaseline.set(false);
      return;
    }
    const base = saveTraceAsBaseline(tr, baselineDraft());
    activeBaselineId.set(base.id);
    namingBaseline.set(false);
    savedFlag.trigger();
  };

  return (
    <View style={{ flex: 1, flexDirection: "row", minHeight: 0 }}>
      {/* Left sidebar: stable trace list + pending banner + saved baselines */}
      <View
        class="border-r border-border bg-card"
        style={{ width: 292, flexDirection: "column", minHeight: 0 }}
      >
        <View class="border-b border-border" style={{ padding: SPACE[12], gap: SPACE[8] }}>
          <Segmented
            value={filter()}
            onChange={(v) => filter.set(v as TraceFilter)}
            items={[
              { value: "all", label: t("o11y.traces.filterAll") },
              { value: "slow", label: ">50ms" },
              { value: "inp", label: "INP" },
              { value: "error", label: t("o11y.traces.filterError") },
            ]}
          />
          <If when={pendingTraces().length > 0}>
            <View
              testID="o11y-flush-pending"
              onPress={flushPendingTraces}
              class="bg-primary/10 hover:bg-primary/20 border border-primary/30"
              style={{
                borderRadius: RADIUS.sm,
                paddingHorizontal: SPACE[12],
                paddingVertical: SPACE[4],
                alignItems: "center",
                justifyContent: "center",
              }}
            >
              <Text style={{ fontSize: FONT_SIZE.caption, fontWeight: FONT_WEIGHT.semibold, fontVariant: ["tabular-nums"] }} class="text-primary">
                {t("o11y.traces.newRecords", { count: pendingTraces().length })}
              </Text>
            </View>
          </If>
        </View>

        <Scroll style={{ flex: 1 }}>
          <View style={{ padding: SPACE[6], gap: SPACE[2] }}>
            <If when={filteredTraces().length === 0}>
              <View style={{ padding: SPACE[16], alignItems: "center" }}>
                <Text style={{ fontSize: FONT_SIZE.caption }} class="text-muted-foreground">
                  {t("o11y.traces.empty")}
                </Text>
              </View>
            </If>
            <Each of={filteredTraces()} key={(tr) => tr.id}>
              {(tr) => {
                const active = () => selectedTrace()?.id === tr().id;
                return (
                  <View
                    testID={`o11y-trace-row-${tr().id}`}
                    onPress={() => props.onSelectTrace(tr().id)}
                    class={active() ? "bg-accent" : "hover:bg-muted"}
                    style={{
                      paddingHorizontal: SPACE[8],
                      paddingVertical: SPACE[8],
                      borderRadius: RADIUS.sm,
                      gap: SPACE[2],
                    }}
                  >
                    <View style={{ flexDirection: "row", alignItems: "center", justifyContent: "space-between", gap: SPACE[6] }}>
                      <Text
                        style={{ fontSize: FONT_SIZE.caption, fontWeight: FONT_WEIGHT.semibold, flex: 1, numberOfLines: 1, textOverflow: "ellipsis" }}
                        class={tr().hasError ? "text-destructive" : "text-foreground"}
                      >
                        {tr().name}
                      </Text>
                      <Text
                        style={{ fontSize: FONT_SIZE.micro, fontFamily: MONO, fontVariant: ["tabular-nums"] }}
                        class={tr().durationMs >= 50 ? "text-destructive" : "text-muted-foreground"}
                      >
                        {tr().durationMs.toFixed(1)} ms
                      </Text>
                    </View>
                    <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[8] }}>
                      <Text style={{ fontSize: FONT_SIZE.micro }} class="text-muted-foreground">
                        {formatSpanCount(tr().spans.length)}
                      </Text>
                      <If when={tr().inpMs !== undefined}>
                        <Text style={{ fontSize: FONT_SIZE.micro, fontFamily: MONO, fontVariant: ["tabular-nums"] }} class="text-primary">
                          INP {tr().inpMs?.toFixed(1)} ms
                        </Text>
                      </If>
                    </View>
                  </View>
                );
              }}
            </Each>
          </View>
        </Scroll>

        {/* Named baselines section */}
        <View class="border-t border-border" style={{ maxHeight: 190, padding: SPACE[8], gap: SPACE[6] }}>
          <View style={{ flexDirection: "row", alignItems: "center", justifyContent: "space-between" }}>
            <Text style={{ fontSize: FONT_SIZE.micro, fontWeight: FONT_WEIGHT.semibold }} class="text-muted-foreground">
              {t("o11y.traces.baselines")} ({namedBaselines().length})
            </Text>
          </View>
          <Scroll style={{ maxHeight: 140 }}>
            <View style={{ gap: SPACE[2] }}>
              <If when={namedBaselines().length === 0}>
                <Text style={{ fontSize: FONT_SIZE.micro }} class="text-muted-foreground">
                  {t("o11y.traces.noBaselines")}
                </Text>
              </If>
              <Each of={namedBaselines()} key={(b) => b.id}>
                {(b) => {
                  const isSelected = () => activeBaseline()?.id === b().id;
                  return (
                    <View
                      onPress={() => {
                        activeBaselineId.set(b().id);
                        mode.set("compare");
                      }}
                      class={isSelected() ? "bg-accent" : "hover:bg-muted"}
                      style={{
                        flexDirection: "row",
                        alignItems: "center",
                        justifyContent: "space-between",
                        paddingHorizontal: SPACE[8],
                        paddingVertical: SPACE[4],
                        borderRadius: RADIUS.xs,
                        gap: SPACE[6],
                      }}
                    >
                      <Text
                        style={{ fontSize: FONT_SIZE.caption, flex: 1, numberOfLines: 1, textOverflow: "ellipsis" }}
                        class="text-foreground"
                      >
                        {b().label}
                      </Text>
                      <View
                        onPress={() => removeBaseline(b().id)}
                        class="hover:bg-background"
                        style={{ width: 18, height: 18, borderRadius: RADIUS.xs, alignItems: "center", justifyContent: "center" }} // token-exempt: compact inline icon box
                      >
                        <Icon name="x" size="sm" tone="muted-foreground" />
                      </View>
                    </View>
                  );
                }}
              </Each>
            </View>
          </Scroll>
        </View>
      </View>

      {/* Right pane: Waterfall or Comparison */}
      <View style={{ flex: 1, flexDirection: "column", minWidth: 0, minHeight: 0 }}>
        <View
          class="border-b border-border bg-card"
          style={{
            paddingHorizontal: SPACE[12],
            paddingVertical: SPACE[8],
            flexDirection: "row",
            alignItems: "center",
            justifyContent: "space-between",
            gap: SPACE[8],
          }}
        >
          <Segmented
            value={mode()}
            onChange={(v) => mode.set(v as TracePaneMode)}
            items={[
              { value: "waterfall", label: t("o11y.traces.waterfall") },
              { value: "compare", label: t("o11y.traces.compare") },
            ]}
          />
          <If
            when={namingBaseline()}
            else={
              <Btn
                testID="o11y-save-baseline"
                size="sm"
                variant="ghost"
                icon={savedFlag.active() ? "check" : "flag"}
                label={t("o11y.traces.saveBaseline")}
                disabled={!selectedTrace()}
                onPress={openBaselineInput}
                style={{ minWidth: 96 }}
              />
            }
          >
            <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[6] }}>
              <Input
                testID="o11y-baseline-name-input"
                value={baselineDraft()}
                onChange={(e) => baselineDraft.set(e.value)}
                onSubmit={confirmSaveBaseline}
                placeholder={t("o11y.traces.baselinePlaceholder")}
                autoFocus
                style={{ width: 210, height: CONTROL.md }}
              />
              <Btn
                testID="o11y-baseline-confirm"
                size="sm"
                variant="primary"
                label={t("common.save")}
                onPress={confirmSaveBaseline}
              />
              <Btn
                size="sm"
                variant="ghost"
                label={t("common.cancel")}
                onPress={() => namingBaseline.set(false)}
              />
            </View>
          </If>
        </View>

        <If when={mode() === "waterfall"}>
          <View style={{ flex: 1, flexDirection: "column", minHeight: 0, padding: SPACE[12], gap: SPACE[8] }}>
            <If
              when={selectedTrace() !== null}
              else={
                <View style={{ flex: 1, alignItems: "center", justifyContent: "center" }}>
                  <Text style={{ fontSize: FONT_SIZE.caption }} class="text-muted-foreground">
                    {t("o11y.traces.selectHint")}
                  </Text>
                </View>
              }
            >
              <TraceWaterfall
                testID="o11y-trace-waterfall"
                spans={selectedTrace()!.spans}
                minimap
                criticalPath
                colors={colors()}
                onSelect={(sp) => selectedSpan.set(sp)}
                style={{ flex: 1 }}
              />
            </If>
            <If when={selectedSpan() !== null}>
              <View
                class="bg-card border border-border"
                style={{ borderRadius: RADIUS.md, padding: SPACE[12], gap: SPACE[4] }}
              >
                <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[8] }}>
                  <Text style={{ fontSize: FONT_SIZE.caption, fontWeight: FONT_WEIGHT.semibold }} class="text-foreground">
                    {selectedSpan()!.name}
                  </Text>
                  <Text style={{ fontSize: FONT_SIZE.caption, fontFamily: MONO }} class="text-primary">
                    {selectedSpan()!.service}
                  </Text>
                  <Text style={{ fontSize: FONT_SIZE.caption, fontFamily: MONO, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
                    {selectedSpan()!.duration.toFixed(2)} ms
                  </Text>
                </View>
                <If when={selectedSpan()!.attributes !== undefined}>
                  <Text style={{ fontSize: FONT_SIZE.micro, fontFamily: MONO }} class="text-secondary-foreground">
                    {JSON.stringify(selectedSpan()!.attributes)}
                  </Text>
                </If>
              </View>
            </If>
          </View>
        </If>

        <If when={mode() === "compare"}>
          <View style={{ flex: 1, padding: SPACE[12], minHeight: 0 }}>
            <If
              when={activeBaseline() !== null && selectedTrace() !== null && !compareMismatch()}
              else={
                <View style={{ flex: 1, alignItems: "center", justifyContent: "center", gap: SPACE[6], padding: SPACE[24] }}>
                  <Text style={{ fontSize: FONT_SIZE.body, fontWeight: FONT_WEIGHT.semibold }} class="text-foreground">
                    {compareMismatch()
                      ? t("o11y.traces.compareMismatchTitle")
                      : t("o11y.traces.compareHint")}
                  </Text>
                  <If when={compareMismatch()}>
                    <Text style={{ fontSize: FONT_SIZE.caption }} class="text-muted-foreground">
                      {t("o11y.traces.compareMismatchDesc", {
                        baseline: activeBaseline()!.scenarioKey,
                        current: selectedTrace()!.scenarioKey,
                      })}
                    </Text>
                  </If>
                </View>
              }
            >
              <TraceComparison
                testID="o11y-trace-comparison"
                a={activeBaseline()!.spans}
                b={selectedTrace()!.spans}
                aLabel={activeBaseline()!.label}
                bLabel={`${selectedTrace()!.name} (${selectedTrace()!.durationMs.toFixed(1)} ms)`}
                colors={colors()}
                style={{ flex: 1 }}
              />
            </If>
          </View>
        </If>
      </View>
    </View>
  );
}

// ── Tab 3: Logs (LogView + subsystem filter + copy) ──────────────────────────

function LogsTab() {
  const subsystem = signal<string>("all");
  const selectedLog = signal<LogEntry | null>(null);
  const storeTick = signal(0);
  const copiedFlag = createTransientFlag(1400);
  const colors = useChartColors();

  effect(() => {
    const off = logStore.subscribe(() => storeTick.update((n) => n + 1));
    return off;
  });

  const logData = computed<typeof logStore | LogEntry[]>(() => {
    const sub = subsystem();
    if (sub === "all") return logStore;
    void storeTick();
    const out: LogEntry[] = [];
    const n = logStore.length();
    for (let i = 0; i < n; i += 1) {
      const entry = logStore.get(i);
      if (entry.service === sub) out.push(entry);
    }
    return out;
  });

  const copySelectedLog = () => {
    const entry = selectedLog();
    if (!entry) return;
    const payload = JSON.stringify(
      {
        time: new Date(entry.time).toISOString(),
        level: entry.level,
        subsystem: entry.service,
        message: entry.message,
        attributes: entry.attributes,
      },
      null,
      2,
    );
    void Clipboard.writeText(payload).then(() => copiedFlag.trigger());
  };

  return (
    <View style={{ flex: 1, flexDirection: "column", minHeight: 0 }}>
      <View
        class="border-b border-border bg-card"
        style={{
          paddingHorizontal: SPACE[12],
          paddingVertical: SPACE[8],
          flexDirection: "row",
          alignItems: "center",
          justifyContent: "space-between",
          gap: SPACE[8],
          flexWrap: "wrap",
        }}
      >
        <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[4], flexWrap: "wrap" }}>
          {(["all", ...O11Y_SUBSYSTEMS] as const).map((sub) => {
            const active = () => subsystem() === sub;
            return (
              <View
                testID={`o11y-log-sub-${sub}`}
                onPress={() => subsystem.set(sub)}
                class={active() ? "bg-primary" : "hover:bg-muted border border-border"}
                style={{
                  paddingHorizontal: SPACE[8],
                  paddingVertical: SPACE[4],
                  borderRadius: RADIUS.xs,
                }}
              >
                <Text
                  style={{ fontSize: FONT_SIZE.caption, fontWeight: FONT_WEIGHT.semibold }}
                  class={active() ? "text-primary-foreground" : "text-secondary-foreground"}
                >
                  {sub === "all" ? t("o11y.logs.allSubsystems") : sub}
                </Text>
              </View>
            );
          })}
        </View>
        <Btn
          testID="o11y-copy-log"
          size="sm"
          variant="ghost"
          icon={copiedFlag.active() ? "check" : "copy"}
          label={t("o11y.logs.copyEntry")}
          disabled={!selectedLog()}
          onPress={copySelectedLog}
          style={{ minWidth: 96 }}
        />
      </View>

      <View style={{ flex: 1, padding: SPACE[12], minHeight: 0 }}>
        <LogView
          testID="o11y-log-view"
          data={logData()}
          histogram
          toolbar
          window={diagnosticsWindowMs()}
          colors={colors()}
          onSelect={(entry) => selectedLog.set(entry)}
          style={{ flex: 1 }}
        />
      </View>
    </View>
  );
}

// ── Diagnostics Window Root ──────────────────────────────────────────────────

export function DiagnosticsWindowContent() {
  useWindowTitle(() => t("o11y.windowTitle"));
  const activeTab = diagnosticsActiveTab;
  const selectedTraceId = signal<string | null>(null);
  const exporting = signal(false);
  const exportedFlag = createTransientFlag(1800);

  const handleExportPerfetto = async () => {
    if (exporting()) return;
    exporting.set(true);
    try {
      const res = await exportDiagnosticsTrace(
        t("o11y.export.title"),
        t("o11y.export.btn"),
      );
      if (res.ok) exportedFlag.trigger();
    } finally {
      exporting.set(false);
    }
  };

  return (
    <View
      testID="diagnostics-window-root"
      class="bg-background"
      style={{ flex: 1, flexDirection: "column", minHeight: 0 }}
    >
      {/* Unified 44pt TitleBar with native window controls and live locale title */}
      <TitleBar
        leading={
          <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[8] }}>
            <View
              style={{
                width: 8,
                height: 8,
                borderRadius: RADIUS.xs,
                backgroundColor: diagnosticsPaused() ? "#eda100" : "#1baf7a",
              }}
            />
            <Text style={{ fontSize: FONT_SIZE.body, fontWeight: FONT_WEIGHT.semibold }} class="text-foreground">
              {t("o11y.windowTitle")}
            </Text>
          </View>
        }
        center={
          <Segmented
            value={activeTab()}
            onChange={(v) => activeTab.set(v as DiagTab)}
            items={[
              { value: "metrics", label: t("o11y.tab.metrics") },
              { value: "traces", label: t("o11y.tab.traces") },
              { value: "logs", label: t("o11y.tab.logs") },
            ]}
          />
        }
        trailing={
          <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[6] }}>
            <Segmented
              value={String(diagnosticsWindowMs())}
              onChange={(v) => diagnosticsWindowMs.set(Number(v) as TimeWindowPreset)}
              items={WINDOW_OPTIONS.map((o) => ({ value: String(o.value), label: o.label }))}
              itemWidth={42}
            />
            <Btn
              testID="o11y-toggle-pause"
              size="sm"
              variant={diagnosticsPaused() ? "primary" : "ghost"}
              icon={diagnosticsPaused() ? "play" : "pause"}
              label={diagnosticsPaused() ? t("o11y.action.resume") : t("o11y.action.pause")}
              onPress={toggleDiagnosticsPaused}
              style={{ minWidth: 68 }}
            />
            <Btn
              testID="o11y-clear"
              size="sm"
              variant="ghost"
              icon="trash"
              label={t("o11y.action.clear")}
              onPress={clearAllDiagnostics}
            />
            <Btn
              testID="o11y-export-perfetto"
              size="sm"
              variant="ghost"
              icon={exportedFlag.active() ? "check" : "fileDown"}
              label={t("o11y.action.exportPerfetto")}
              disabled={exporting()}
              onPress={() => void handleExportPerfetto()}
              style={{ minWidth: 138 }}
            />
          </View>
        }
      />

      {/* Active Tab Content */}
      <View style={{ flex: 1, minHeight: 0 }}>
        <If when={activeTab() === "metrics"}>
          <MetricsTab />
        </If>
        <If when={activeTab() === "traces"}>
          <TracesTab
            selectedTraceId={() => selectedTraceId()}
            onSelectTrace={(id) => selectedTraceId.set(id)}
          />
        </If>
        <If when={activeTab() === "logs"}>
          <LogsTab />
        </If>
      </View>
    </View>
  );
}

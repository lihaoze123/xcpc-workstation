// Independent NSH 0.7 developer diagnostics window built on `nsh-o11y`.
//
// Keeps the main workspace 100% free of technical HUDs while offering:
// 1. Metrics: 4 synchronized `TimeSeriesChart` panels via `createChartSync()`
// 2. Traces: `TraceWaterfall` + named baselines + `TraceComparison` diff
// 3. Logs: `LogView` with volume histogram, virtualized list, subsystem filter & copy

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
import { Btn, createTransientFlag, Segmented } from "../kit";
import { Icon } from "../Icon";
import { MONO, token } from "../theme";
import {
  cacheBudgetMb,
  cacheSeries,
  capturedTraces,
  clearAllDiagnostics,
  diagnosticsAnnotations,
  diagnosticsPaused,
  diagnosticsWindowMs,
  exportDiagnosticsTrace,
  frameSeries,
  latencySeries,
  latestCacheStats,
  logStore,
  namedBaselines,
  O11Y_SUBSYSTEMS,
  pipelineSeries,
  removeBaseline,
  saveTraceAsBaseline,
  toggleDiagnosticsPaused,
  type CapturedTrace,
  type NamedBaseline,
  type TimeWindowPreset,
} from "./telemetry";

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

// ── Tab 1: Metrics (4 synced TimeSeriesChart panels) ─────────────────────────

function MetricsTab() {
  const sync = createChartSync({ zoom: true });
  const colors = useChartColors();
  const winMs = () => diagnosticsWindowMs();
  const anns = () => diagnosticsAnnotations();

  const frameSeriesOpts = computed<SeriesOptions[]>(() => {
    void colorScheme();
    return [
      { label: t("o11y.series.frameMs"), color: token("primary"), width: 1.5, fill: 0.1 },
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
      { value: 16.67, color: withAlpha(token("destructive"), 0.75), label: "16.67ms (60Hz)", dash: [4, 3] },
      { value: 8.33, color: withAlpha(token("muted-foreground"), 0.55), label: "8.33ms (120Hz)", dash: [2, 3] },
    ];
  });

  const pipelineSeriesOpts = computed<SeriesOptions[]>(() => {
    void colorScheme();
    return [
      { label: "build_document", color: "#2a78d6", width: 1.5, fill: 0.08, spanGaps: true },
      { label: "rasterize_cached", color: "#1baf7a", width: 1.5, fill: 0.08, spanGaps: true },
    ];
  });

  const latencySeriesOpts = computed<SeriesOptions[]>(() => {
    void colorScheme();
    return [
      { label: "IPC", color: "#8b5cf6", width: 1.5, fill: 0.08, spanGaps: true },
      { label: "ux.inp.*", color: "#eda100", width: 1.75, fill: 0.1, spanGaps: true },
    ];
  });

  const slowThreshold = computed<Threshold[]>(() => {
    void colorScheme();
    return [
      { value: 50, color: withAlpha("#eda100", 0.75), label: "50ms", dash: [4, 3] },
    ];
  });

  const cacheSeriesOpts = computed<SeriesOptions[]>(() => {
    void colorScheme();
    return [
      { label: t("o11y.series.cacheMb"), color: "#0ea5e9", width: 1.5, fill: 0.12 },
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
        label: `LRU ${budget} MB`,
        dash: [4, 3],
      },
    ];
  });

  const cacheSummary = computed(() => {
    const cs = latestCacheStats();
    const mb = (cs.totalBytes / (1024 * 1024)).toFixed(1);
    const total = cs.hits + cs.misses;
    const hitPct = total > 0 ? ((cs.hits / total) * 100).toFixed(1) : "—";
    return `${mb} / ${cacheBudgetMb()} MB · ${cs.entryCount} entries · ${hitPct}% hit`;
  });

  const cardClass = "bg-card border border-border";
  const cardStyle = {
    flex: 1,
    minWidth: 320,
    borderRadius: 10,
    padding: 12,
    flexDirection: "column" as const,
    gap: 6,
  };

  return (
    <Scroll style={{ flex: 1 }}>
      <View style={{ padding: 14, gap: 12 }}>
      <View style={{ flexDirection: "row", alignItems: "center", justifyContent: "space-between" }}>
        <Text style={{ fontSize: 12 }} class="text-muted-foreground">
          {t("o11y.metrics.hint")}
        </Text>
        <View style={{ flexDirection: "row", alignItems: "center", gap: 10 }}>
          <Text style={{ fontSize: 11.5, fontFamily: MONO, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
            {cacheSummary()}
          </Text>
          <Btn
            label={t("o11y.metrics.resetZoom")}
            size="sm"
            variant="ghost"
            onPress={() => sync.reset()}
          />
        </View>
      </View>

      <View style={{ flexDirection: "row", gap: 12, flexWrap: "wrap" }}>
        <View class={cardClass} style={cardStyle}>
          <TimeSeriesChart
            testID="o11y-chart-frame"
            title={t("o11y.chart.frame")}
            data={frameSeries}
            series={frameSeriesOpts()}
            unit="ms"
            softMin={0}
            softMax={20}
            y2={{ unit: "short", softMin: 0, softMax: 4 }}
            window={winMs()}
            thresholds={frameThresholds()}
            annotations={anns()}
            sync={sync}
            colors={colors()}
            style={{ height: 210 }}
          />
        </View>
        <View class={cardClass} style={cardStyle}>
          <TimeSeriesChart
            testID="o11y-chart-pipeline"
            title={t("o11y.chart.pipeline")}
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
            style={{ height: 210 }}
          />
        </View>
      </View>

      <View style={{ flexDirection: "row", gap: 12, flexWrap: "wrap" }}>
        <View class={cardClass} style={cardStyle}>
          <TimeSeriesChart
            testID="o11y-chart-latency"
            title={t("o11y.chart.latency")}
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
            style={{ height: 210 }}
          />
        </View>
        <View class={cardClass} style={cardStyle}>
          <TimeSeriesChart
            testID="o11y-chart-cache"
            title={t("o11y.chart.cache")}
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
            style={{ height: 210 }}
          />
        </View>
      </View>
      </View>
    </Scroll>
  );
}

// ── Tab 2: Traces (TraceWaterfall + Named Baselines + TraceComparison) ───────

function TracesTab(props: {
  selectedTraceId: () => string | null;
  onSelectTrace: (id: string | null) => void;
}) {
  const filter = signal<TraceFilter>("all");
  const mode = signal<TracePaneMode>("waterfall");
  const selectedSpan = signal<Span | null>(null);
  const activeBaselineId = signal<string | null>(null);
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

  effect(() => {
    const tr = selectedTrace();
    if (tr && props.selectedTraceId() !== tr.id) {
      props.onSelectTrace(tr.id);
    }
    selectedSpan.set(null);
  });

  const handleSaveBaseline = () => {
    const tr = selectedTrace();
    if (!tr) return;
    const base = saveTraceAsBaseline(tr);
    activeBaselineId.set(base.id);
    savedFlag.trigger();
  };

  return (
    <View style={{ flex: 1, flexDirection: "row", minHeight: 0 }}>
      {/* Left sidebar: captured traces + saved baselines */}
      <View
        class="border-r border-border bg-card"
        style={{ width: 280, flexDirection: "column", minHeight: 0 }}
      >
        <View class="border-b border-border" style={{ padding: 10, gap: 8 }}>
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
        </View>

        <Scroll style={{ flex: 1 }}>
          <View style={{ padding: 6, gap: 3 }}>
          <If when={filteredTraces().length === 0}>
            <View style={{ padding: 16, alignItems: "center" }}>
              <Text style={{ fontSize: 12 }} class="text-muted-foreground">
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
                    paddingHorizontal: 9,
                    paddingVertical: 7,
                    borderRadius: 6,
                    gap: 2,
                  }}
                >
                  <View style={{ flexDirection: "row", alignItems: "center", justifyContent: "space-between", gap: 6 }}>
                    <Text
                      style={{ fontSize: 12, fontWeight: 500, flex: 1, numberOfLines: 1, textOverflow: "ellipsis" }}
                      class={tr().hasError ? "text-destructive" : "text-foreground"}
                    >
                      {tr().name}
                    </Text>
                    <Text
                      style={{ fontSize: 11, fontFamily: MONO, fontVariant: ["tabular-nums"] }}
                      class={tr().durationMs >= 50 ? "text-destructive" : "text-muted-foreground"}
                    >
                      {tr().durationMs.toFixed(1)} ms
                    </Text>
                  </View>
                  <View style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>
                    <Text style={{ fontSize: 10.5 }} class="text-muted-foreground">
                      {tr().spans.length} spans
                    </Text>
                    <If when={tr().inpMs !== undefined}>
                      <Text style={{ fontSize: 10.5, fontFamily: MONO, fontVariant: ["tabular-nums"] }} class="text-primary">
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
        <View class="border-t border-border" style={{ maxHeight: 190, padding: 8, gap: 6 }}>
          <View style={{ flexDirection: "row", alignItems: "center", justifyContent: "space-between" }}>
            <Text style={{ fontSize: 11, fontWeight: 600 }} class="text-muted-foreground">
              {t("o11y.traces.baselines")} ({namedBaselines().length})
            </Text>
          </View>
          <Scroll style={{ maxHeight: 140 }}>
            <View style={{ gap: 3 }}>
            <If when={namedBaselines().length === 0}>
              <Text style={{ fontSize: 11 }} class="text-muted-foreground">
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
                      paddingHorizontal: 8,
                      paddingVertical: 5,
                      borderRadius: 5,
                      gap: 6,
                    }}
                  >
                    <Text
                      style={{ fontSize: 11.5, flex: 1, numberOfLines: 1, textOverflow: "ellipsis" }}
                      class="text-foreground"
                    >
                      {b().label}
                    </Text>
                    <View
                      onPress={() => removeBaseline(b().id)}
                      class="hover:bg-background"
                      style={{ width: 18, height: 18, borderRadius: 4, alignItems: "center", justifyContent: "center" }}
                    >
                      <Icon name="x" size={11} tone="muted-foreground" />
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
            paddingHorizontal: 12,
            paddingVertical: 8,
            flexDirection: "row",
            alignItems: "center",
            justifyContent: "space-between",
            gap: 8,
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
          <Btn
            testID="o11y-save-baseline"
            size="sm"
            variant="ghost"
            icon={savedFlag.active() ? "check" : "flag"}
            label={savedFlag.active() ? t("o11y.traces.baselineSaved") : t("o11y.traces.saveBaseline")}
            disabled={!selectedTrace()}
            onPress={handleSaveBaseline}
          />
        </View>

        <If when={mode() === "waterfall"}>
          <View style={{ flex: 1, flexDirection: "column", minHeight: 0, padding: 10, gap: 8 }}>
            <If
              when={selectedTrace() !== null}
              else={
                <View style={{ flex: 1, alignItems: "center", justifyContent: "center" }}>
                  <Text style={{ fontSize: 12.5 }} class="text-muted-foreground">
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
                style={{ borderRadius: 8, padding: 10, gap: 4 }}
              >
                <View style={{ flexDirection: "row", alignItems: "center", gap: 10 }}>
                  <Text style={{ fontSize: 12, fontWeight: 600 }} class="text-foreground">
                    {selectedSpan()!.name}
                  </Text>
                  <Text style={{ fontSize: 11.5, fontFamily: MONO }} class="text-primary">
                    {selectedSpan()!.service}
                  </Text>
                  <Text style={{ fontSize: 11.5, fontFamily: MONO, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
                    {selectedSpan()!.duration.toFixed(2)} ms
                  </Text>
                </View>
                <If when={selectedSpan()!.attributes !== undefined}>
                  <Text style={{ fontSize: 11, fontFamily: MONO }} class="text-secondary-foreground">
                    {JSON.stringify(selectedSpan()!.attributes)}
                  </Text>
                </If>
              </View>
            </If>
          </View>
        </If>

        <If when={mode() === "compare"}>
          <View style={{ flex: 1, padding: 10, minHeight: 0 }}>
            <If
              when={activeBaseline() !== null && selectedTrace() !== null}
              else={
                <View style={{ flex: 1, alignItems: "center", justifyContent: "center" }}>
                  <Text style={{ fontSize: 12.5 }} class="text-muted-foreground">
                    {t("o11y.traces.compareHint")}
                  </Text>
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
          paddingHorizontal: 12,
          paddingVertical: 8,
          flexDirection: "row",
          alignItems: "center",
          justifyContent: "space-between",
          gap: 8,
          flexWrap: "wrap",
        }}
      >
        <View style={{ flexDirection: "row", alignItems: "center", gap: 4, flexWrap: "wrap" }}>
          {(["all", ...O11Y_SUBSYSTEMS] as const).map((sub) => {
            const active = () => subsystem() === sub;
            return (
              <View
                testID={`o11y-log-sub-${sub}`}
                onPress={() => subsystem.set(sub)}
                class={active() ? "bg-primary" : "hover:bg-muted border border-border"}
                style={{
                  paddingHorizontal: 8,
                  paddingVertical: 3,
                  borderRadius: 5,
                }}
              >
                <Text
                  style={{ fontSize: 11.5, fontWeight: 500 }}
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
          label={copiedFlag.active() ? t("common.copied") : t("o11y.logs.copyEntry")}
          disabled={!selectedLog()}
          onPress={copySelectedLog}
        />
      </View>

      <View style={{ flex: 1, padding: 10, minHeight: 0 }}>
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
  const activeTab = signal<DiagTab>("metrics");
  const selectedTraceId = signal<string | null>(null);
  const exporting = signal(false);
  const exportedFlag = createTransientFlag(1800);
  const baselineFlag = createTransientFlag(1500);

  const handleCaptureBaseline = () => {
    const list = capturedTraces();
    const target = list.find((tr) => tr.id === selectedTraceId()) ?? list[0];
    if (!target) return;
    saveTraceAsBaseline(target);
    baselineFlag.trigger();
  };

  const handleExportPerfetto = async () => {
    if (exporting()) return;
    exporting.set(true);
    try {
      const res = await exportDiagnosticsTrace(
        t("settings.about.traceExportTitle"),
        t("settings.about.traceExportBtn"),
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
      {/* Unified Top Control Bar */}
      <View
        class="border-b border-border bg-card"
        style={{
          paddingHorizontal: 14,
          paddingVertical: 9,
          flexDirection: "row",
          alignItems: "center",
          justifyContent: "space-between",
          gap: 10,
          flexWrap: "wrap",
        }}
      >
        <View style={{ flexDirection: "row", alignItems: "center", gap: 10 }}>
          <View style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>
            <View
              style={{
                width: 8,
                height: 8,
                borderRadius: 4,
                backgroundColor: diagnosticsPaused() ? "#eda100" : "#1baf7a",
              }}
            />
            <Text style={{ fontSize: 13, fontWeight: 600 }} class="text-foreground">
              {t("o11y.windowTitle")}
            </Text>
          </View>
          <Segmented
            value={activeTab()}
            onChange={(v) => activeTab.set(v as DiagTab)}
            items={[
              { value: "metrics", label: t("o11y.tab.metrics") },
              { value: "traces", label: t("o11y.tab.traces") },
              { value: "logs", label: t("o11y.tab.logs") },
            ]}
          />
        </View>

        <View style={{ flexDirection: "row", alignItems: "center", gap: 6, flexWrap: "wrap" }}>
          <Segmented
            value={String(diagnosticsWindowMs())}
            onChange={(v) => diagnosticsWindowMs.set(Number(v) as TimeWindowPreset)}
            items={WINDOW_OPTIONS.map((o) => ({ value: String(o.value), label: o.label }))}
          />
          <Btn
            testID="o11y-toggle-pause"
            size="sm"
            variant={diagnosticsPaused() ? "primary" : "ghost"}
            icon={diagnosticsPaused() ? "play" : "pause"}
            label={diagnosticsPaused() ? t("o11y.action.resume") : t("o11y.action.pause")}
            onPress={toggleDiagnosticsPaused}
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
            testID="o11y-capture-baseline"
            size="sm"
            variant="ghost"
            icon={baselineFlag.active() ? "check" : "flag"}
            label={baselineFlag.active() ? t("o11y.traces.baselineSaved") : t("o11y.action.captureBaseline")}
            disabled={capturedTraces().length === 0}
            onPress={handleCaptureBaseline}
          />
          <Btn
            testID="o11y-export-perfetto"
            size="sm"
            variant="ghost"
            icon={exportedFlag.active() ? "check" : "fileDown"}
            label={exportedFlag.active() ? t("settings.about.traceExported") : t("o11y.action.exportPerfetto")}
            disabled={exporting()}
            onPress={() => void handleExportPerfetto()}
          />
        </View>
      </View>

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

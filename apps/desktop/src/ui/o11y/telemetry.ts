// Tiered observability collector for the independent NSH 0.7 diagnostics window.
//
// Two tiers:
// 1. Always-On Ring (default, zero rAF, zero disk writes):
//    - In-memory `LogStore` (max 5,000 entries) fed by structured logs, slow IPC (>50ms),
//      uncaught errors, and Rust always-on slow/error logs pulled when the window opens.
// 2. Live Diagnostics (active only while the diagnostics window is open or XCPC_TRACE/XCPC_O11Y=1):
//    - 200ms batched push into 4 `nsh-o11y` `TimeSeries` stores:
//      - Frame chart filters out idle gaps (>120ms) and caps active spikes at 50ms while tracking peak.
//      - Discrete event charts (compile/rasterize, IPC/INP) emit isolated pulses (never sloped lines
//        across idle minutes) and maintain window last + P95 summaries.
//    - Merged Rust/JS cross-thread traces queue into `pendingTraces` once the list is non-empty so
//      new arrivals never shift rows under the user's cursor.

import { signal } from "nsh";
import {
  LogStore,
  TimeSeries,
  type Annotation,
  type LogEntry,
  type LogLevel,
  type Span,
} from "nsh-o11y";
import { Preview } from "xcpc-workstation/native";
import {
  addTraceObserver,
  exportTraceDialog,
  getClockOffsetUs,
  isTraceRecording,
  pauseFrameWatch,
  resumeFrameWatch,
  startTraceRecording,
  stopTraceRecording,
  updateClockOffsetUs,
  type JsTraceEvent,
} from "../trace";

export type O11ySubsystem =
  | "engine"
  | "convert"
  | "judge"
  | "plugin"
  | "sync"
  | "ipc"
  | "ui"
  | "desktop";

export const O11Y_SUBSYSTEMS: readonly O11ySubsystem[] = [
  "engine",
  "convert",
  "judge",
  "plugin",
  "sync",
  "ipc",
  "ui",
  "desktop",
];

export type TimeWindowPreset = 60_000 | 300_000 | 900_000;

export interface MetricStat {
  last: number | null;
  p95: number | null;
  count: number;
}

export interface O11yMetricSummaries {
  framePeakMs: number | null;
  buildDoc: MetricStat;
  rasterize: MetricStat;
  ipc: MetricStat;
  inp: MetricStat;
}

export interface CapturedTrace {
  id: string;
  /** Normalized scenario identifier used to guard against cross-scenario comparisons. */
  scenarioKey: string;
  name: string;
  category: string;
  startedAtUnixMs: number;
  durationMs: number;
  inpMs?: number;
  hasError: boolean;
  spans: Span[];
}

export interface NamedBaseline {
  id: string;
  scenarioKey: string;
  label: string;
  capturedAtUnixMs: number;
  durationMs: number;
  spans: Span[];
}

export interface RustO11yEvent {
  seq: number;
  name: string;
  cat: string;
  ph: string;
  tsUs: number;
  durUs?: number;
  pid: number;
  tid: number;
  threadName: string;
  args?: Record<string, unknown>;
}

export interface RustLogRecord {
  seq: number;
  timeUnixMs: number;
  tsUs: number;
  level: LogLevel;
  subsystem: string;
  message: string;
  traceId?: string;
  spanId?: string;
  attrs?: Record<string, unknown>;
}

export interface PixelCacheStatsPayload {
  totalBytes: number;
  budgetBytes: number;
  entryCount: number;
  hits: number;
  misses: number;
}

interface RustPollPayload {
  delta: {
    enabled: boolean;
    o11yEnv: boolean;
    rustNowUs: number;
    nowUnixMs: number;
    eventSeq: number;
    logSeq: number;
    slowCount: number;
    droppedEvents: number;
    droppedLogs: number;
    events: RustO11yEvent[];
    logs: RustLogRecord[];
  };
  cacheStats: PixelCacheStatsPayload;
}

const POLL_INTERVAL_MS = 1000;
const MAX_SERIES_POINTS = 12_000;
const MAX_ANNOTATIONS = 200;
const MAX_CAPTURED_TRACES = 80;
const TRACE_GROUP_GAP_MS = 80;
/** Frame deltas above 120ms indicate an idle window (no rAF tick), not a jank stall. */
export const IDLE_FRAME_THRESHOLD_MS = 120;
/** Upper Y bound for the frame time chart so 60Hz/120Hz lines remain clearly legible. */
export const FRAME_CHART_CAP_MS = 50;

const EMPTY_STAT: MetricStat = { last: null, p95: null, count: 0 };

function emptySummaries(): O11yMetricSummaries {
  return {
    framePeakMs: null,
    buildDoc: { ...EMPTY_STAT },
    rasterize: { ...EMPTY_STAT },
    ipc: { ...EMPTY_STAT },
    inp: { ...EMPTY_STAT },
  };
}

// ── Public reactive state ────────────────────────────────────────────────────

export const diagnosticsOpen = signal(false);
export const diagnosticsPaused = signal(false);
export const diagnosticsActiveTab = signal<"metrics" | "traces" | "logs">("metrics");
export const diagnosticsWindowMs = signal<TimeWindowPreset>(300_000);
export const diagnosticsAnnotations = signal<Annotation[]>([]);
export const capturedTraces = signal<CapturedTrace[]>([]);
/** Newly arrived traces queued when the list already has entries so rows never shift under click. */
export const pendingTraces = signal<CapturedTrace[]>([]);
export const namedBaselines = signal<NamedBaseline[]>([]);
export const cacheBudgetMb = signal(48);
export const latestCacheStats = signal<PixelCacheStatsPayload>({
  totalBytes: 0,
  budgetBytes: 48 * 1024 * 1024,
  entryCount: 0,
  hits: 0,
  misses: 0,
});
export const slowEventCount = signal(0);
export const metricSummaries = signal<O11yMetricSummaries>(emptySummaries());

// ── nsh-o11y live data stores ────────────────────────────────────────────────

/** Panel 1: [activeFrameMaxMs (capped at 50), activeMissedFrames] */
export const frameSeries = new TimeSeries({ series: 2, maxPoints: MAX_SERIES_POINTS });
/** Panel 2: [buildDocMs pulses, rasterizeMs pulses] */
export const pipelineSeries = new TimeSeries({ series: 2, maxPoints: MAX_SERIES_POINTS });
/** Panel 3: [ipcMaxMs pulses, inpMaxMs pulses] */
export const latencySeries = new TimeSeries({ series: 2, maxPoints: MAX_SERIES_POINTS });
/** Panel 4: [cacheMb, hitRatePct] */
export const cacheSeries = new TimeSeries({ series: 2, maxPoints: MAX_SERIES_POINTS });

/** Always-on structured log ring buffer (5,000 entries, zero disk I/O). */
export const logStore = new LogStore({ maxEntries: 5_000 });

// ── Internal collector state ─────────────────────────────────────────────────

let pollTimer: ReturnType<typeof setInterval> | null = null;
let lastRustEventSeq = 0;
let lastRustLogSeq = 0;
let startedTraceByWindow = false;
let nextTraceSeq = 1;
let nextBaselineSeq = 1;

// Accumulators flushed every 200ms while live diagnostics is running.
let bucketFrameMaxMs = NaN;
let bucketMissedFrames = 0;
let bucketIpcMaxMs = NaN;
let bucketInpMaxMs = NaN;
let pendingJsSpans: RawSpan[] = [];

// Sliding sample rings for window P95 / last summary readouts.
interface TimedSample {
  time: number;
  value: number;
}
let buildDocSamples: TimedSample[] = [];
let rasterizeSamples: TimedSample[] = [];
let ipcSamples: TimedSample[] = [];
let inpSamples: TimedSample[] = [];
let activeFramePeakSamples: TimedSample[] = [];

export interface RawSpan {
  id: string;
  name: string;
  service: string;
  cat: string;
  startMs: number;
  durationMs: number;
  hasError: boolean;
  attributes?: Record<string, string | number | boolean>;
}

function nowPerfMs(): number {
  return typeof performance !== "undefined" ? performance.now() : Date.now();
}

/** Normalizes JS/Rust span names to consistent `snake_case` (P5). */
export function normalizeSpanName(name: string): string {
  if (name === "preview.compile") return "compile_preview";
  if (name.startsWith("ux.inp.") || name.includes("_") || name.includes(".")) {
    return name;
  }
  // Convert camelCase IPC method names (e.g. searchProblems -> search_problems).
  return name.replace(/([a-z0-9])([A-Z])/g, "$1_$2").toLowerCase();
}

function formatTraceDisplayName(span: RawSpan): string {
  const attrs = span.attributes;
  if (span.name === "ux.inp.nav.space" && attrs && typeof attrs.space === "string") {
    return `nav → ${attrs.space}`;
  }
  if (span.name === "ux.inp.outline.select" && attrs) {
    const target = attrs.label ?? attrs.id ?? attrs.problemId;
    if (typeof target === "string" && target.length > 0) {
      return `outline → ${target}`;
    }
  }
  if (span.name === "ux.inp.palette.toggle") {
    return "palette.toggle";
  }
  if (span.name.startsWith("ux.inp.")) {
    return span.name.slice("ux.inp.".length);
  }
  return span.name;
}

function flattenAttrs(
  raw?: Record<string, unknown>,
): Record<string, string | number | boolean> | undefined {
  if (!raw) return undefined;
  const out: Record<string, string | number | boolean> = {};
  let count = 0;
  for (const [k, v] of Object.entries(raw)) {
    if (typeof v === "string" || typeof v === "number" || typeof v === "boolean") {
      out[k] = v;
      count += 1;
    } else if (v !== null && v !== undefined) {
      out[k] = JSON.stringify(v);
      count += 1;
    }
  }
  return count > 0 ? out : undefined;
}

function serviceFromCategory(cat: string, pid: number, name: string): string {
  const c = cat.toLowerCase();
  if (c === "engine" || c === "typst") return "engine";
  if (c === "convert") return "convert";
  if (c === "judge") return "judge";
  if (c === "plugin") return "plugin";
  if (c === "sync") return "sync";
  if (c === "ipc") return "ipc";
  if (c === "desktop" || pid === 1) {
    if (name.includes("rasterize") || name.includes("compile")) return "engine";
    return "desktop";
  }
  return "ui";
}

function pushSample(list: TimedSample[], time: number, value: number, cutoff: number): TimedSample[] {
  if (!Number.isNaN(value)) {
    list.push({ time, value });
  }
  let drop = 0;
  while (drop < list.length && list[drop]!.time < cutoff) drop += 1;
  return drop > 0 ? list.slice(drop) : list;
}

function summarizeSamples(list: readonly TimedSample[]): MetricStat {
  if (list.length === 0) return { ...EMPTY_STAT };
  const last = Math.round(list[list.length - 1]!.value * 10) / 10;
  const sorted = list.map((s) => s.value).sort((a, b) => a - b);
  const idx = Math.min(sorted.length - 1, Math.floor(sorted.length * 0.95));
  const p95 = Math.round(sorted[idx]! * 10) / 10;
  return { last, p95, count: list.length };
}

/** Appends a discrete event pulse to a 2-series store without interpolating across idle gaps. */
function appendDiscretePulse(store: TimeSeries, now: number, v0: number, v1: number): void {
  const has0 = !Number.isNaN(v0);
  const has1 = !Number.isNaN(v1);
  if (!has0 && !has1) {
    store.append(now, [NaN, NaN]);
    return;
  }
  // Emit a crisp vertical impulse [0 -> val -> NaN] spanning 2ms so discrete operations never
  // connect with sloped lines across idle minutes when `spanGaps: false`.
  store.append(
    [now - 2, now - 1, now],
    [
      [has0 ? 0 : NaN, has0 ? v0 : NaN, NaN],
      [has1 ? 0 : NaN, has1 ? v1 : NaN, NaN],
    ],
  );
}

// ── Always-on logging & annotations ──────────────────────────────────────────

/** Appends a structured log entry into the always-on in-memory ring buffer. */
export function o11yLog(
  level: LogLevel,
  subsystem: O11ySubsystem | string,
  message: string,
  attrs?: Record<string, unknown>,
  traceId?: string,
): void {
  const entry: LogEntry = {
    time: Date.now(),
    level,
    message,
    service: subsystem,
    traceId,
    attributes: flattenAttrs(attrs),
  };
  logStore.append(entry);
}

/** Marks a notable user/system event on the Metrics timeline. */
export function o11yAnnotate(label: string, color?: string): void {
  const next: Annotation = { time: Date.now(), label, color };
  diagnosticsAnnotations.update((list) => {
    const appended = [...list, next];
    return appended.length > MAX_ANNOTATIONS
      ? appended.slice(appended.length - MAX_ANNOTATIONS)
      : appended;
  });
}

// ── Pure trace grouping & tree construction (exported for unit tests) ────────

/**
 * Merges a batch of normalized cross-thread spans (Rust + JS, already on the same
 * millisecond timeline) into grouped `CapturedTrace` objects with inferred `parentId`
 * trees compatible with `nsh-o11y`'s `<TraceWaterfall>` and `<TraceComparison>`.
 */
export function groupRawSpansIntoTraces(
  rawSpans: readonly RawSpan[],
  baseUnixMs: number,
  traceIdPrefix = "tr",
): CapturedTrace[] {
  if (rawSpans.length === 0) return [];
  const sorted = [...rawSpans].sort(
    (a, b) => a.startMs - b.startMs || b.durationMs - a.durationMs,
  );

  // Partition into contiguous groups separated by > TRACE_GROUP_GAP_MS.
  const groups: RawSpan[][] = [];
  let cur: RawSpan[] = [sorted[0]!];
  let curEnd = sorted[0]!.startMs + sorted[0]!.durationMs;
  for (let i = 1; i < sorted.length; i += 1) {
    const s = sorted[i]!;
    if (s.startMs <= curEnd + TRACE_GROUP_GAP_MS) {
      cur.push(s);
      curEnd = Math.max(curEnd, s.startMs + s.durationMs);
    } else {
      groups.push(cur);
      cur = [s];
      curEnd = s.startMs + s.durationMs;
    }
  }
  groups.push(cur);

  const firstStart = sorted[0]!.startMs;
  const traces: CapturedTrace[] = [];

  for (let g = 0; g < groups.length; g += 1) {
    const items = groups[g]!;
    const groupStart = items[0]!.startMs;
    let groupEnd = groupStart;
    let hasError = false;
    let inpSpan: RawSpan | undefined;
    let primarySpan: RawSpan = items[0]!;
    for (const s of items) {
      groupEnd = Math.max(groupEnd, s.startMs + s.durationMs);
      if (s.hasError) hasError = true;
      if (s.cat === "ux.inp" && (!inpSpan || s.durationMs > inpSpan.durationMs)) {
        inpSpan = s;
      }
      if (s.durationMs > primarySpan.durationMs) {
        primarySpan = s;
      }
    }
    const totalDuration = Math.max(0.01, groupEnd - groupStart);
    if (items.length === 1 && totalDuration < 0.5 && !hasError && !inpSpan) {
      continue;
    }

    // Build parent pointers: smallest enclosing span that started <= child.startMs.
    const spans: Span[] = items.map((s, idx) => {
      let parentId: string | undefined;
      let parentDur = Infinity;
      const sEnd = s.startMs + s.durationMs;
      for (let p = idx - 1; p >= 0; p -= 1) {
        const cand = items[p]!;
        const cEnd = cand.startMs + cand.durationMs;
        // Allow 0.5ms clock skew tolerance between Rust & JS lanes.
        if (cand.startMs <= s.startMs + 0.5 && cEnd + 0.5 >= sEnd && cand.durationMs < parentDur) {
          parentId = cand.id;
          parentDur = cand.durationMs;
        }
      }
      return {
        spanId: s.id,
        parentId,
        name: s.name,
        service: s.service,
        start: Math.max(0, s.startMs - groupStart),
        duration: Math.max(0.01, s.durationMs),
        status: s.hasError ? "error" : "ok",
        attributes: s.attributes,
      };
    });

    const chosen = inpSpan ?? primarySpan;
    const displayName = formatTraceDisplayName(chosen);
    const traceId = `${traceIdPrefix}-${nextTraceSeq++}`;
    traces.push({
      id: traceId,
      scenarioKey: chosen.name,
      name: displayName,
      category: chosen.cat,
      startedAtUnixMs: Math.round(baseUnixMs + (groupStart - firstStart)),
      durationMs: Math.round(totalDuration * 100) / 100,
      inpMs: inpSpan ? Math.round(inpSpan.durationMs * 100) / 100 : undefined,
      hasError,
      spans,
    });
  }

  return traces;
}

// ── Always-on trace observer registration ────────────────────────────────────

addTraceObserver({
  onFrame(dtMs, missed) {
    if (!diagnosticsOpen() || diagnosticsPaused()) return;
    // Ignore idle gaps when the host was not rendering frames (prevents 1.5s false jank spikes).
    if (dtMs > IDLE_FRAME_THRESHOLD_MS) return;
    if (Number.isNaN(bucketFrameMaxMs) || dtMs > bucketFrameMaxMs) {
      bucketFrameMaxMs = dtMs;
    }
    if (missed) {
      bucketMissedFrames += 1;
    }
  },
  onEvent(ev: JsTraceEvent) {
    if (!diagnosticsOpen() || diagnosticsPaused()) return;
    if (ev.ph !== "X" || ev.durMs === undefined) return;
    if (ev.cat === "ipc") {
      if (Number.isNaN(bucketIpcMaxMs) || ev.durMs > bucketIpcMaxMs) {
        bucketIpcMaxMs = ev.durMs;
      }
    } else if (ev.cat === "ux.inp") {
      if (Number.isNaN(bucketInpMaxMs) || ev.durMs > bucketInpMaxMs) {
        bucketInpMaxMs = ev.durMs;
      }
    }
    if (ev.cat === "frame") return;
    const rustUs = ev.tsMs * 1000.0 + getClockOffsetUs();
    const normName = normalizeSpanName(ev.name);
    pendingJsSpans.push({
      id: `js-${Math.round(rustUs)}-${pendingJsSpans.length}`,
      name: normName,
      service: serviceFromCategory(ev.cat, 2, normName),
      cat: ev.cat,
      startMs: rustUs / 1000.0,
      durationMs: ev.durMs,
      hasError: Boolean(ev.args && ev.args.error),
      attributes: flattenAttrs(ev.args),
    });
  },
  onSlowIpc(name, durMs, error) {
    const norm = normalizeSpanName(name);
    slowEventCount.update((n) => n + 1);
    if (error !== undefined) {
      o11yLog("error", "ipc", `IPC ${norm} failed (${durMs.toFixed(1)} ms)`, {
        method: norm,
        durMs: Math.round(durMs * 100) / 100,
        error: error instanceof Error ? error.message : String(error),
      });
    } else {
      o11yLog("warn", "ipc", `Slow IPC ${norm} (${durMs.toFixed(1)} ms)`, {
        method: norm,
        durMs: Math.round(durMs * 100) / 100,
      });
    }
  },
});

// ── 200ms batched poll & push ────────────────────────────────────────────────

function pollOnce(): void {
  if (diagnosticsPaused()) return;
  let payload: RustPollPayload | null = null;
  try {
    if (typeof Preview.o11yPollDelta === "function") {
      const t0 = nowPerfMs();
      const raw = Preview.o11yPollDelta(lastRustEventSeq, lastRustLogSeq);
      const t1 = nowPerfMs();
      payload = JSON.parse(raw) as RustPollPayload;
      if (payload?.delta) {
        updateClockOffsetUs(payload.delta.rustNowUs, (t0 + t1) / 2);
      }
    }
  } catch {
    // Headless test mocks may not implement o11yPollDelta.
  }

  const now = payload?.delta?.nowUnixMs ?? Date.now();
  const cutoff = now - diagnosticsWindowMs();

  // 1. Ingest Rust structured logs.
  if (payload?.delta?.logs && payload.delta.logs.length > 0) {
    const entries: LogEntry[] = payload.delta.logs.map((rec) => ({
      time: rec.timeUnixMs || now,
      level: rec.level,
      message: rec.message,
      service: rec.subsystem,
      traceId: rec.traceId,
      spanId: rec.spanId,
      attributes: flattenAttrs(rec.attrs),
    }));
    logStore.append(entries);
    lastRustLogSeq = Math.max(lastRustLogSeq, payload.delta.logSeq);
  }
  if (payload?.delta && slowEventCount() !== payload.delta.slowCount) {
    slowEventCount.set(payload.delta.slowCount);
  }

  // 2. Extract pipeline durations & convert Rust trace events to RawSpans.
  let buildDocMaxMs = NaN;
  let rasterizeMaxMs = NaN;
  const rustRawSpans: RawSpan[] = [];
  if (payload?.delta?.events && payload.delta.events.length > 0) {
    lastRustEventSeq = Math.max(lastRustEventSeq, payload.delta.eventSeq);
    for (const ev of payload.delta.events) {
      if (ev.ph !== "X" || ev.durUs === undefined) continue;
      const durMs = ev.durUs / 1000.0;
      const normName = normalizeSpanName(ev.name);
      if (normName === "build_document" || normName === "compile" || normName === "compile_preview") {
        if (Number.isNaN(buildDocMaxMs) || durMs > buildDocMaxMs) buildDocMaxMs = durMs;
      } else if (normName === "rasterize_cached") {
        if (Number.isNaN(rasterizeMaxMs) || durMs > rasterizeMaxMs) rasterizeMaxMs = durMs;
      }
      rustRawSpans.push({
        id: `rs-${ev.seq}`,
        name: normName,
        service: serviceFromCategory(ev.cat, ev.pid, normName),
        cat: ev.cat,
        startMs: ev.tsUs / 1000.0,
        durationMs: durMs,
        hasError: Boolean(ev.args && ev.args.error),
        attributes: flattenAttrs({
          ...ev.args,
          thread: ev.threadName,
        }),
      });
    }
  }

  // 3. Push samples into the 4 TimeSeries stores only while the Metrics tab is visible
  //    so Traces/Logs views incur zero chart redraw overhead.
  const metricsVisible = diagnosticsActiveTab() === "metrics";
  if (metricsVisible) {
    const cappedFrame = Number.isNaN(bucketFrameMaxMs)
      ? NaN
      : Math.min(bucketFrameMaxMs, FRAME_CHART_CAP_MS);
    frameSeries.append(now, [cappedFrame, bucketMissedFrames]);
    appendDiscretePulse(pipelineSeries, now, buildDocMaxMs, rasterizeMaxMs);
    appendDiscretePulse(latencySeries, now, bucketIpcMaxMs, bucketInpMaxMs);
  }

  // Update sliding window summaries (last + P95 + frame peak over cap).
  buildDocSamples = pushSample(buildDocSamples, now, buildDocMaxMs, cutoff);
  rasterizeSamples = pushSample(rasterizeSamples, now, rasterizeMaxMs, cutoff);
  ipcSamples = pushSample(ipcSamples, now, bucketIpcMaxMs, cutoff);
  inpSamples = pushSample(inpSamples, now, bucketInpMaxMs, cutoff);
  activeFramePeakSamples = pushSample(
    activeFramePeakSamples,
    now,
    !Number.isNaN(bucketFrameMaxMs) && bucketFrameMaxMs > FRAME_CHART_CAP_MS ? bucketFrameMaxMs : NaN,
    cutoff,
  );
  const framePeakMs =
    activeFramePeakSamples.length > 0
      ? Math.round(Math.max(...activeFramePeakSamples.map((s) => s.value)) * 10) / 10
      : null;
  const nextSummaries: O11yMetricSummaries = {
    framePeakMs,
    buildDoc: summarizeSamples(buildDocSamples),
    rasterize: summarizeSamples(rasterizeSamples),
    ipc: summarizeSamples(ipcSamples),
    inp: summarizeSamples(inpSamples),
  };
  const prevSummaries = metricSummaries();
  const statEq = (a: MetricStat, b: MetricStat) =>
    a.last === b.last && a.p95 === b.p95 && a.count === b.count;
  if (
    prevSummaries.framePeakMs !== nextSummaries.framePeakMs ||
    !statEq(prevSummaries.buildDoc, nextSummaries.buildDoc) ||
    !statEq(prevSummaries.rasterize, nextSummaries.rasterize) ||
    !statEq(prevSummaries.ipc, nextSummaries.ipc) ||
    !statEq(prevSummaries.inp, nextSummaries.inp)
  ) {
    metricSummaries.set(nextSummaries);
  }

  if (payload?.cacheStats) {
    const cs = payload.cacheStats;
    const prevCs = latestCacheStats();
    if (
      prevCs.totalBytes !== cs.totalBytes ||
      prevCs.budgetBytes !== cs.budgetBytes ||
      prevCs.entryCount !== cs.entryCount ||
      prevCs.hits !== cs.hits ||
      prevCs.misses !== cs.misses
    ) {
      latestCacheStats.set(cs);
    }
    const mb = Math.round((cs.totalBytes / (1024 * 1024)) * 100) / 100;
    const budgetMb = Math.max(1, Math.round(cs.budgetBytes / (1024 * 1024)));
    if (cacheBudgetMb() !== budgetMb) {
      cacheBudgetMb.set(budgetMb);
    }
    const totalLookups = cs.hits + cs.misses;
    const hitRate = totalLookups > 0 ? Math.round((cs.hits / totalLookups) * 1000) / 10 : NaN;
    if (metricsVisible) {
      cacheSeries.append(now, [mb, hitRate]);
    }
  }

  // Reset per-tick JS accumulators.
  bucketFrameMaxMs = NaN;
  bucketMissedFrames = 0;
  bucketIpcMaxMs = NaN;
  bucketInpMaxMs = NaN;

  // 4. Merge Rust + JS spans into grouped Traces without shifting an already-populated list.
  const batchSpans =
    pendingJsSpans.length > 0 ? [...rustRawSpans, ...pendingJsSpans] : rustRawSpans;
  pendingJsSpans = [];

  if (batchSpans.length > 0) {
    const newTraces = groupRawSpansIntoTraces(batchSpans, now, "tr");
    if (newTraces.length > 0) {
      const incoming = newTraces.reverse();
      if (capturedTraces().length === 0) {
        capturedTraces.set(incoming.slice(0, MAX_CAPTURED_TRACES));
      } else {
        pendingTraces.update((prev) => {
          const merged = [...incoming, ...prev];
          return merged.length > MAX_CAPTURED_TRACES
            ? merged.slice(0, MAX_CAPTURED_TRACES)
            : merged;
        });
      }
    }
  }
}

/** Merges queued new traces into the visible trace list on explicit user click (A4). */
export function flushPendingTraces(): void {
  const queued = pendingTraces();
  if (queued.length === 0) return;
  capturedTraces.update((prev) => {
    const merged = [...queued, ...prev];
    return merged.length > MAX_CAPTURED_TRACES
      ? merged.slice(0, MAX_CAPTURED_TRACES)
      : merged;
  });
  pendingTraces.set([]);
}

// ── Public lifecycle controls ────────────────────────────────────────────────

/** Activates Live Diagnostics (200ms poll + rAF frame watch + Rust full spans). */
export function startLiveDiagnostics(): void {
  diagnosticsOpen.set(true);
  diagnosticsPaused.set(false);
  pollOnce();
  if (!isTraceRecording()) {
    startedTraceByWindow = true;
    startTraceRecording();
  } else {
    startedTraceByWindow = false;
  }
  if (pollTimer === null) {
    pollTimer = setInterval(pollOnce, POLL_INTERVAL_MS);
  }
}

/** Deactivates Live Diagnostics and returns cleanly to Always-On Ring mode. */
export function stopLiveDiagnostics(): void {
  diagnosticsOpen.set(false);
  if (pollTimer !== null) {
    clearInterval(pollTimer);
    pollTimer = null;
  }
  pendingJsSpans = [];
  if (startedTraceByWindow) {
    startedTraceByWindow = false;
    stopTraceRecording();
  }
}

export function toggleDiagnosticsPaused(): void {
  const next = !diagnosticsPaused();
  diagnosticsPaused.set(next);
  if (next) {
    pauseFrameWatch();
  } else if (diagnosticsOpen()) {
    resumeFrameWatch();
  }
}

export function clearAllDiagnostics(): void {
  frameSeries.clear();
  pipelineSeries.clear();
  latencySeries.clear();
  cacheSeries.clear();
  logStore.clear();
  capturedTraces.set([]);
  pendingTraces.set([]);
  diagnosticsAnnotations.set([]);
  slowEventCount.set(0);
  buildDocSamples = [];
  rasterizeSamples = [];
  ipcSamples = [];
  inpSamples = [];
  activeFramePeakSamples = [];
  metricSummaries.set(emptySummaries());
  pendingJsSpans = [];
  lastRustEventSeq = 0;
  lastRustLogSeq = 0;
  try {
    if (typeof Preview.o11yClear === "function") {
      Preview.o11yClear();
    }
  } catch {
    // Ignore in headless tests.
  }
}

/** Saves a captured trace as a named baseline for `TraceComparison`. */
export function saveTraceAsBaseline(trace: CapturedTrace, label?: string): NamedBaseline {
  const baseline: NamedBaseline = {
    id: `base-${nextBaselineSeq++}`,
    scenarioKey: trace.scenarioKey,
    label: label?.trim() || `${trace.name} (${trace.durationMs.toFixed(1)} ms)`,
    capturedAtUnixMs: Date.now(),
    durationMs: trace.durationMs,
    spans: trace.spans,
  };
  namedBaselines.update((prev) => [baseline, ...prev]);
  return baseline;
}

export function removeBaseline(id: string): void {
  namedBaselines.update((prev) => prev.filter((b) => b.id !== id));
}

/** Checks whether `XCPC_O11Y=1` requested launching with the diagnostics window open. */
export function isO11yEnvEnabled(): boolean {
  try {
    return typeof Preview.o11yEnvEnabled === "function" && Preview.o11yEnvEnabled();
  } catch {
    return false;
  }
}

/** Exports the current merged Rust + JS trace as a Perfetto-compatible `.json` file. */
export async function exportDiagnosticsTrace(title: string, button: string) {
  return exportTraceDialog(title, button);
}

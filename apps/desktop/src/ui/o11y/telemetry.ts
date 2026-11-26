// Tiered observability collector for the independent NSH 0.7 diagnostics window.
//
// Two tiers:
// 1. Always-On Ring (default, zero rAF, zero disk writes):
//    - In-memory `LogStore` (max 5,000 entries) fed by structured logs, slow IPC (>50ms),
//      uncaught errors, and Rust always-on slow/error logs pulled when the window opens.
// 2. Live Diagnostics (active only while the diagnostics window is open or XCPC_TRACE/XCPC_O11Y=1):
//    - 200ms batched push into 4 `nsh-o11y` `TimeSeries` stores (frame/jank, compile/rasterize,
//      IPC/INP, pixel cache MB & hit rate) + merged Rust/JS cross-thread `TraceModel` traces.

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

export interface CapturedTrace {
  id: string;
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

const POLL_INTERVAL_MS = 200;
const MAX_SERIES_POINTS = 6_000;
const MAX_ANNOTATIONS = 200;
const MAX_CAPTURED_TRACES = 80;
const TRACE_GROUP_GAP_MS = 80;

// ── Public reactive state ────────────────────────────────────────────────────

export const diagnosticsOpen = signal(false);
export const diagnosticsPaused = signal(false);
export const diagnosticsWindowMs = signal<TimeWindowPreset>(300_000);
export const diagnosticsAnnotations = signal<Annotation[]>([]);
export const capturedTraces = signal<CapturedTrace[]>([]);
export const namedBaselines = signal<NamedBaseline[]>([]);
export const cacheBudgetMb = signal(96);
export const latestCacheStats = signal<PixelCacheStatsPayload>({
  totalBytes: 0,
  budgetBytes: 96 * 1024 * 1024,
  entryCount: 0,
  hits: 0,
  misses: 0,
});
export const slowEventCount = signal(0);

// ── nsh-o11y live data stores ────────────────────────────────────────────────

/** Panel 1: [frameMaxMs, missedFrames] */
export const frameSeries = new TimeSeries({ series: 2, maxPoints: MAX_SERIES_POINTS });
/** Panel 2: [buildDocMs, rasterizeMs] */
export const pipelineSeries = new TimeSeries({ series: 2, maxPoints: MAX_SERIES_POINTS });
/** Panel 3: [ipcMaxMs, inpMaxMs] */
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

interface RawSpan {
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
    // Ignore tiny single-span noise (< 0.5ms) unless it is an error or interaction.
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
    const traceId = `${traceIdPrefix}-${nextTraceSeq++}`;
    traces.push({
      id: traceId,
      name: chosen.name,
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
    // Skip raw missed-frame spans from the waterfall list (they belong on the frame chart).
    if (ev.cat === "frame") return;
    const rustUs = ev.tsMs * 1000.0 + getClockOffsetUs();
    pendingJsSpans.push({
      id: `js-${Math.round(rustUs)}-${pendingJsSpans.length}`,
      name: ev.name,
      service: serviceFromCategory(ev.cat, 2, ev.name),
      cat: ev.cat,
      startMs: rustUs / 1000.0,
      durationMs: ev.durMs,
      hasError: Boolean(ev.args && ev.args.error),
      attributes: flattenAttrs(ev.args),
    });
  },
  onSlowIpc(name, durMs, error) {
    slowEventCount.update((n) => n + 1);
    if (error !== undefined) {
      o11yLog("error", "ipc", `IPC ${name} failed (${durMs.toFixed(1)} ms)`, {
        method: name,
        durMs: Math.round(durMs * 100) / 100,
        error: error instanceof Error ? error.message : String(error),
      });
    } else {
      o11yLog("warn", "ipc", `Slow IPC ${name} (${durMs.toFixed(1)} ms)`, {
        method: name,
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
  if (payload?.delta) {
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
      if (ev.name === "build_document" || ev.name === "compile" || ev.name === "compile_preview") {
        if (Number.isNaN(buildDocMaxMs) || durMs > buildDocMaxMs) buildDocMaxMs = durMs;
      } else if (ev.name === "rasterize_cached") {
        if (Number.isNaN(rasterizeMaxMs) || durMs > rasterizeMaxMs) rasterizeMaxMs = durMs;
      }
      rustRawSpans.push({
        id: `rs-${ev.seq}`,
        name: ev.name,
        service: serviceFromCategory(ev.cat, ev.pid, ev.name),
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

  // 3. Push one batched point into each of the 4 TimeSeries stores.
  frameSeries.append(now, [bucketFrameMaxMs, bucketMissedFrames]);
  pipelineSeries.append(now, [buildDocMaxMs, rasterizeMaxMs]);
  latencySeries.append(now, [bucketIpcMaxMs, bucketInpMaxMs]);

  if (payload?.cacheStats) {
    const cs = payload.cacheStats;
    latestCacheStats.set(cs);
    const mb = Math.round((cs.totalBytes / (1024 * 1024)) * 100) / 100;
    const budgetMb = Math.max(1, Math.round(cs.budgetBytes / (1024 * 1024)));
    cacheBudgetMb.set(budgetMb);
    const totalLookups = cs.hits + cs.misses;
    const hitRate = totalLookups > 0 ? Math.round((cs.hits / totalLookups) * 1000) / 10 : NaN;
    cacheSeries.append(now, [mb, hitRate]);
  }

  // Reset per-tick JS accumulators.
  bucketFrameMaxMs = NaN;
  bucketMissedFrames = 0;
  bucketIpcMaxMs = NaN;
  bucketInpMaxMs = NaN;

  // 4. Merge Rust + JS spans into grouped Traces.
  const batchSpans =
    pendingJsSpans.length > 0 ? [...rustRawSpans, ...pendingJsSpans] : rustRawSpans;
  pendingJsSpans = [];

  if (batchSpans.length > 0) {
    const newTraces = groupRawSpansIntoTraces(batchSpans, now, "tr");
    if (newTraces.length > 0) {
      capturedTraces.update((prev) => {
        const merged = [...newTraces.reverse(), ...prev];
        return merged.length > MAX_CAPTURED_TRACES
          ? merged.slice(0, MAX_CAPTURED_TRACES)
          : merged;
      });
    }
  }
}

// ── Public lifecycle controls ────────────────────────────────────────────────

/** Activates Live Diagnostics (200ms poll + rAF frame watch + Rust full spans). */
export function startLiveDiagnostics(): void {
  diagnosticsOpen.set(true);
  diagnosticsPaused.set(false);
  // First pull any always-on logs & slow events accumulated before opening.
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
  diagnosticsPaused.update((p) => !p);
}

export function clearAllDiagnostics(): void {
  frameSeries.clear();
  pipelineSeries.clear();
  latencySeries.clear();
  cacheSeries.clear();
  logStore.clear();
  capturedTraces.set([]);
  diagnosticsAnnotations.set([]);
  slowEventCount.set(0);
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

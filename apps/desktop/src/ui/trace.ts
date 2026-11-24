// Offline performance trace collector (JS/UI lane) merged with Rust `xcpc-trace`
// into a single Chrome Trace / Perfetto `.json` timeline.
// - Zero overhead when disabled (single boolean check, no rAF or allocations).
// - O(1) fixed-capacity circular ring buffer (never calls O(N) `Array.shift()`).
// - Records only frames that missed a refresh (> 1.5x the display's measured
//   vsync period) plus a summary counter, so normal vsync jitter (8.3-8.5ms at
//   120Hz) never floods the buffer.
// - Note: the rAF loop keeps NSH's engine thread ticking every refresh while
//   recording (idle CPU rises); jank rates therefore include idle frames, which
//   is why the analyzer also reports absolute stall time and per-interaction INP.

import { Preview } from "xcpc-workstation/native";
import { signal } from "nsh";

export interface JsTraceEvent {
  name: string;
  cat: string;
  ph: "X" | "i" | "C";
  tsMs: number;
  durMs?: number;
  tid?: number;
  args?: Record<string, unknown>;
}

const MAX_JS_EVENTS = 100_000;
const TID_UI_MAIN = 1;
const TID_IPC = 2;

let recording = false;
let clockOffsetUs = 0;
let rafId: number | null = null;
let lastFrameMs = 0;

// O(1) circular ring buffer.
let ring: (JsTraceEvent | undefined)[] = [];
let ringHead = 0;
let ringLen = 0;
let ringDropped = 0;

// Frame counters for accurate jank rate without storing 7,200 idle events/min.
let frameTotal = 0;
let frameJank60 = 0;
let frameMissed = 0;
let frameStallMs = 0;
let frameWorstMs = 0;

// Display refresh period, calibrated from the median of the first frames.
const VSYNC_CALIBRATION_FRAMES = 60;
const FALLBACK_VSYNC_MS = 1000 / 60;
let vsyncMs = 0;
let calibration: number[] = [];

/** A frame "missed" a refresh when it took more than 1.5 refresh periods. */
export function isMissedFrame(dtMs: number, vsync: number): boolean {
  return dtMs > vsync * 1.5;
}

/** A frame would have dropped at 60Hz when it took more than 1.5 x 16.67ms. */
export function isJank60(dtMs: number): boolean {
  return dtMs > (1000 / 60) * 1.5;
}

function median(values: number[]): number {
  const s = [...values].sort((a, b) => a - b);
  return s[Math.floor(s.length / 2)] ?? FALLBACK_VSYNC_MS;
}

export const traceRecording = signal(false);

function nowMs(): number {
  return typeof performance !== "undefined" ? performance.now() : Date.now();
}

function resetRing(): void {
  ring = [];
  ringHead = 0;
  ringLen = 0;
  ringDropped = 0;
  frameTotal = 0;
  frameJank60 = 0;
  frameMissed = 0;
  frameStallMs = 0;
  frameWorstMs = 0;
  vsyncMs = 0;
  calibration = [];
}

function pushJsEvent(ev: JsTraceEvent): void {
  if (ringLen < MAX_JS_EVENTS) {
    ring[(ringHead + ringLen) % MAX_JS_EVENTS] = ev;
    ringLen += 1;
  } else {
    ring[ringHead] = ev;
    ringHead = (ringHead + 1) % MAX_JS_EVENTS;
    ringDropped += 1;
  }
}

function snapshotRing(): JsTraceEvent[] {
  const out: JsTraceEvent[] = new Array(ringLen);
  for (let i = 0; i < ringLen; i += 1) {
    out[i] = ring[(ringHead + i) % MAX_JS_EVENTS]!;
  }
  return out;
}

function stopFrameWatch(): void {
  if (rafId !== null && typeof cancelAnimationFrame === "function") {
    cancelAnimationFrame(rafId);
  }
  rafId = null;
  lastFrameMs = 0;
}

function startFrameWatch(): void {
  stopFrameWatch();
  if (typeof requestAnimationFrame !== "function") return;
  lastFrameMs = nowMs();
  const tick = () => {
    if (!recording) {
      rafId = null;
      return;
    }
    const now = nowMs();
    const dt = now - lastFrameMs;
    if (vsyncMs === 0) {
      calibration.push(dt);
      if (calibration.length >= VSYNC_CALIBRATION_FRAMES) {
        vsyncMs = median(calibration);
        calibration = [];
      }
    }
    const vsync = vsyncMs || FALLBACK_VSYNC_MS;
    frameTotal += 1;
    if (dt > frameWorstMs) frameWorstMs = dt;
    if (isMissedFrame(dt, vsync)) {
      frameMissed += 1;
      frameStallMs += dt - vsync;
      const jank60 = isJank60(dt);
      if (jank60) frameJank60 += 1;
      pushJsEvent({
        name: jank60 ? "ui.long_frame" : "ui.missed_frame",
        cat: "frame",
        ph: "X",
        tsMs: lastFrameMs,
        durMs: dt,
        tid: TID_UI_MAIN,
        args: {
          dtMs: Math.round(dt * 100) / 100,
          missedRefreshes: Math.max(1, Math.round(dt / vsync) - 1),
          jank60,
        },
      });
    }
    lastFrameMs = now;
    rafId = requestAnimationFrame(tick);
  };
  rafId = requestAnimationFrame(tick);
}

/** Returns a snapshot of buffered JS trace events (primarily for diagnostics & tests). */
export function getJsTraceEvents(): readonly JsTraceEvent[] {
  return snapshotRing();
}

/** Syncs JS recording state with Rust (e.g. when launched with `XCPC_TRACE=1`). */
export function syncTraceStatus(): { enabled: boolean; rustEvents: number; jsEvents: number } {
  try {
    const t0 = nowMs();
    const st = Preview.traceStatus();
    const t1 = nowMs();
    clockOffsetUs = st.rustNowUs - ((t0 + t1) / 2) * 1000.0;
    recording = st.enabled;
    traceRecording.set(st.enabled);
    if (st.enabled && rafId === null) {
      startFrameWatch();
    } else if (!st.enabled) {
      stopFrameWatch();
    }
    return { enabled: st.enabled, rustEvents: st.eventCount, jsEvents: ringLen };
  } catch {
    return { enabled: recording, rustEvents: 0, jsEvents: ringLen };
  }
}

export function startTraceRecording(): void {
  resetRing();
  try {
    const t0 = nowMs();
    const st = Preview.traceStart();
    const t1 = nowMs();
    clockOffsetUs = st.rustNowUs - ((t0 + t1) / 2) * 1000.0;
  } catch {
    // Headless test environments may mock Preview without traceStart.
  }
  recording = true;
  traceRecording.set(true);
  startFrameWatch();
}

export function stopTraceRecording(): void {
  if (recording && frameTotal > 0) {
    pushJsEvent({
      name: "ui.frame_summary",
      cat: "frame.summary",
      ph: "i",
      tsMs: nowMs(),
      tid: TID_UI_MAIN,
      args: {
        totalFrames: frameTotal,
        vsyncMs: Math.round((vsyncMs || FALLBACK_VSYNC_MS) * 100) / 100,
        missed: frameMissed,
        jank60: frameJank60,
        stallMs: Math.round(frameStallMs * 100) / 100,
        worstMs: Math.round(frameWorstMs * 100) / 100,
        droppedJsEvents: ringDropped,
      },
    });
  }
  recording = false;
  traceRecording.set(false);
  stopFrameWatch();
  try {
    const t0 = nowMs();
    const st = Preview.traceStop();
    const t1 = nowMs();
    clockOffsetUs = st.rustNowUs - ((t0 + t1) / 2) * 1000.0;
  } catch {
    // Ignore in headless mocks.
  }
}

export async function exportTraceDialog(title: string, button: string) {
  if (recording) {
    stopTraceRecording();
  }
  const payload = JSON.stringify(snapshotRing());
  return Preview.traceExportDialog(payload, clockOffsetUs, title, button);
}

/** Records a synchronous JS span; zero-cost when recording is off. */
export function traceSpan<T>(
  cat: string,
  name: string,
  fn: () => T,
  args?: Record<string, unknown>,
  isIpc = false,
): T {
  if (!recording) return fn();
  const start = nowMs();
  try {
    return fn();
  } finally {
    pushJsEvent({
      name,
      cat,
      ph: "X",
      tsMs: start,
      durMs: Math.max(0, nowMs() - start),
      tid: isIpc ? TID_IPC : TID_UI_MAIN,
      args,
    });
  }
}

/** Records an async JS/IPC span; zero-cost when recording is off. */
export async function traceAsyncSpan<T>(
  cat: string,
  name: string,
  fn: () => Promise<T>,
  args?: Record<string, unknown>,
  isIpc = true,
): Promise<T> {
  if (!recording) return fn();
  const start = nowMs();
  try {
    return await fn();
  } finally {
    pushJsEvent({
      name,
      cat,
      ph: "X",
      tsMs: start,
      durMs: Math.max(0, nowMs() - start),
      tid: isIpc ? TID_IPC : TID_UI_MAIN,
      args,
    });
  }
}

/** Records an instant UI anchor event; zero-cost when recording is off. */
export function traceInstant(cat: string, name: string, args?: Record<string, unknown>): void {
  if (!recording) return;
  pushJsEvent({
    name,
    cat,
    ph: "i",
    tsMs: nowMs(),
    tid: TID_UI_MAIN,
    args,
  });
}

/**
 * Measures Interaction-to-Next-Paint (INP) for a user action (`ux.inp` span).
 * Runs `fn()` immediately; when recording, the span closes when the frame
 * *after* the one that applies the update starts (double rAF), i.e. once the
 * commit, layout and paint triggered by `fn()` are done. A single rAF fires
 * before NSH applies the update and would only time the handler itself.
 */
export function traceInteraction<T>(
  name: string,
  fn: () => T,
  args?: Record<string, unknown>,
): T {
  if (!recording) return fn();
  const start = nowMs();
  const result = fn();
  let done = false;
  let fallback: ReturnType<typeof setTimeout> | undefined;
  const finish = (timedOut: boolean) => {
    if (done) return;
    done = true;
    if (fallback !== undefined) clearTimeout(fallback);
    pushJsEvent({
      name: `ux.inp.${name}`,
      cat: "ux.inp",
      ph: "X",
      tsMs: start,
      durMs: Math.max(0, nowMs() - start),
      tid: TID_UI_MAIN,
      args: timedOut ? { ...args, timedOut: true } : args,
    });
  };
  if (typeof requestAnimationFrame === "function") {
    requestAnimationFrame(() => requestAnimationFrame(() => finish(false)));
    // Only for hosts without a running frame clock (headless tests): a stalled
    // engine thread delays this timer just as much as the frames, so it never
    // cuts a real long interaction short.
    fallback = setTimeout(() => finish(true), 1000);
  } else {
    finish(false);
  }
  return result;
}

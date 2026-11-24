// Deterministic UI/UX quantitative regression gates (Phase 1):
// - Signal isolation & commit latency (re-render count = 0 for unrelated subscribers)
// - Spring physics compliance (ζ in [0.85, 0.92], low-overshoot warm feel)
// - Offline trace zero-overhead when disabled & INP span capture when enabled
// - Single-slot toast & non-stacking feedback compliance

import { batch, computed, signal } from "nsh";
import { describe, expect, test } from "nsh/test";
import { Preview } from "xcpc-workstation/native";
import { adaptiveCompileDelayMs, neighborPagesToPrewarm } from "./logic";
import { matrixCellKey, type MatrixCellData } from "./studio/buildLogic";
import { createMatrixCellStore, stepMatrixSelection } from "./studio/matrixStore";
import {
  pageTransform,
  SPRING_GENTLE,
  SPRING_INDICATOR,
  SPRING_OVERLAY,
  SPRING_PAGE,
  SPRING_PANEL,
  SPRING_PRESS,
} from "./motion";
import {
  getJsTraceEvents,
  isJank60,
  isMissedFrame,
  startTraceRecording,
  stopTraceRecording,
  traceInteraction,
  traceSpan,
} from "./trace";

function dampingRatio(s: { stiffness?: number; damping?: number; mass?: number }): number {
  const k = s.stiffness ?? 100;
  const c = s.damping ?? 10;
  const m = s.mass ?? 1;
  return c / (2 * Math.sqrt(k * m));
}

describe("UI/UX quantitative gates (Phase 1)", () => {
  test("all motion spring presets stay within warm low-overshoot damping band (0.85 <= ζ <= 0.92)", () => {
    const presets = [
      ["SPRING_OVERLAY", SPRING_OVERLAY],
      ["SPRING_INDICATOR", SPRING_INDICATOR],
      ["SPRING_PANEL", SPRING_PANEL],
      ["SPRING_PAGE", SPRING_PAGE],
      ["SPRING_PRESS", SPRING_PRESS],
      ["SPRING_GENTLE", SPRING_GENTLE],
    ] as const;

    for (const [, preset] of presets) {
      const zeta = dampingRatio(preset);
      expect(zeta).toBeGreaterThanOrEqual(0.85);
      expect(zeta).toBeLessThanOrEqual(0.92);
    }

    // Spatial continuity uses pure GPU pixel translation (no scale FBO resampling).
    expect(pageTransform(false, "space")).toBe("translateY(6px)");
    expect(pageTransform(true, "space")).toBe("translateY(0px)");
    expect(pageTransform(false, "studio")).toBe("translateX(14px)");
    expect(pageTransform(true, "studio")).toBe("translateX(0px)");
  });

  test("fine-grained signal updates keep unrelated subscribers at 0 recomputes and commit < 2ms", () => {
    const activeCaseStatus = signal("pending");
    const unrelatedTree = signal(["a", "b", "c"]);
    let unrelatedRecomputes = 0;
    let activeRecomputes = 0;

    const treeSummary = computed(() => {
      unrelatedRecomputes += 1;
      return unrelatedTree().length;
    });
    const caseBadge = computed(() => {
      activeRecomputes += 1;
      return activeCaseStatus();
    });

    // Prime both computeds once.
    expect(treeSummary()).toBe(3);
    expect(caseBadge()).toBe("pending");
    unrelatedRecomputes = 0;
    activeRecomputes = 0;

    const t0 = performance.now();
    for (let i = 0; i < 100; i += 1) {
      batch(() => {
        activeCaseStatus.set(i % 2 === 0 ? "running" : "ac");
      });
      // Read both to verify dirty tracking.
      expect(treeSummary()).toBe(3);
      expect(caseBadge()).toBe(i % 2 === 0 ? "running" : "ac");
    }
    const elapsedMs = performance.now() - t0;
    const perCommitMs = elapsedMs / 100;

    expect(unrelatedRecomputes).toBe(0);
    expect(activeRecomputes).toBe(100);
    expect(perCommitMs).toBeLessThan(2.0);
  });

  test("offline trace records 0 events when disabled and captures INP + spans when enabled", async () => {
    stopTraceRecording();
    const beforeLen = getJsTraceEvents().length;
    const val = traceSpan("ui", "disabled_span", () => 42);
    const val2 = traceInteraction("disabled_click", () => 7);
    expect(val).toBe(42);
    expect(val2).toBe(7);
    expect(getJsTraceEvents().length).toBe(beforeLen);

    startTraceRecording();
    expect(traceSpan("ui", "enabled_span", () => "ok")).toBe("ok");
    expect(traceInteraction("tab_switch", () => "done")).toBe("done");
    // INP closes on the second frame after the interaction (or the headless fallback).
    const hasInp = () => getJsTraceEvents().some((e) => e.name === "ux.inp.tab_switch");
    for (let waited = 0; !hasInp() && waited < 1200; waited += 20) {
      await new Promise<void>((resolve) => setTimeout(resolve, 20));
    }
    stopTraceRecording();

    const events = getJsTraceEvents();
    expect(events.some((e) => e.cat === "ui" && e.name === "enabled_span")).toBe(true);
    expect(events.some((e) => e.cat === "ux.inp" && e.name === "ux.inp.tab_switch")).toBe(true);
  });

  test("frame watch flags missed refreshes relative to the measured vsync, not a fixed 8.33ms", () => {
    // 120Hz jitter (8.3-8.6ms) is a normal frame, not jank.
    expect(isMissedFrame(8.46, 8.33)).toBe(false);
    expect(isMissedFrame(10.9, 8.33)).toBe(false);
    // One refresh missed at 120Hz.
    expect(isMissedFrame(16.8, 8.33)).toBe(true);
    // At 60Hz a 20ms frame is jitter, a 33ms frame missed one refresh.
    expect(isMissedFrame(20, 16.67)).toBe(false);
    expect(isMissedFrame(33.4, 16.67)).toBe(true);
    // 60Hz jank is display independent: > 1.5 x 16.67ms.
    expect(isJank60(16.8)).toBe(false);
    expect(isJank60(25.1)).toBe(true);
  });

  test("adaptive typing cadence fires fast on pause and coalesces cleanly during bursts", () => {
    // Pause / single edit fires rapidly (35ms problem, 65ms contest).
    expect(adaptiveCompileDelayMs(false, 260, 8)).toBe(35);
    expect(adaptiveCompileDelayMs(true, 260, 15)).toBe(65);

    // Rapid burst coalesces proportionally to last compile cost within bounds.
    expect(adaptiveCompileDelayMs(false, 50, 10)).toBe(55);
    expect(adaptiveCompileDelayMs(false, 50, 60)).toBe(90);
    expect(adaptiveCompileDelayMs(false, 50, 200)).toBe(110);
    expect(adaptiveCompileDelayMs(true, 50, 10)).toBe(90);
    expect(adaptiveCompileDelayMs(true, 50, 90)).toBe(135);
    expect(adaptiveCompileDelayMs(true, 50, 200)).toBe(160);
  });

  test("neighbor pre-warm picks [max+1, min-1, max+2] outside viewport within page bounds", () => {
    expect(neighborPagesToPrewarm([0], 5)).toEqual([1, 2]);
    expect(neighborPagesToPrewarm([2, 3], 6)).toEqual([4, 1, 5]);
    expect(neighborPagesToPrewarm([4], 5)).toEqual([3]);
    expect(neighborPagesToPrewarm([0], 1)).toEqual([]);
  });

  test("viewport-first compile + speculative prewarmNeighbors warms adjacent page for 0-render scroll hit", async () => {
    const p = new Preview(null);
    const info = p.openWorkspace("../../tests/fixtures/olymp-ref/contest-zh/workspace", true);
    expect(info.error).toBeUndefined();
    const req = {
      target: "contest",
      id: "contest-zh",
      book: "statements",
      path: "",
      text: "",
    };
    // Use normal fit-to-width scale (<= BASE_SCALE = 1.5) where pages render as single base layers.
    const scale = 1.25;
    // Viewport only sees page 0.
    const first = await p.compile(req, scale, [{ page: 0, x: 0, y: 0, width: 595, height: 842 }]);
    expect(first.pages.length).toBeGreaterThanOrEqual(3);
    expect(first.pages[0].layers.length).toBeGreaterThan(0);

    // Pre-warm pages 1 and 2 in background.
    const warmed = await p.prewarmNeighbors(scale, [1, 2]);
    expect(warmed).toBeGreaterThanOrEqual(1);

    // Scrolling to page 1 now hits the warm PIXELS cache with 0 newly rasterized pages!
    const scrolled = await p.compile(req, scale, [{ page: 1, x: 0, y: 0, width: 595, height: 842 }]);
    expect(scrolled.rendered).toBe(0);
    expect(scrolled.pages[1].layers.length).toBeGreaterThan(0);
  });

  test("MatrixCellStore isolates 300-cell matrix updates: 0 recomputes on untouched cells, < 2ms per tick", () => {
    const store = createMatrixCellStore(50);
    const baseMap = new Map<string, MatrixCellData>();
    const sols = ["std", "bruteforce", "wa", "tle", "sub1", "sub2"];
    for (let r = 1; r <= 50; r += 1) {
      for (const s of sols) {
        baseMap.set(matrixCellKey(r, s), {
          test_index: r,
          test_name: String(r).padStart(2, "0"),
          solution: s,
          verdict: "AC",
          cpu_time_ms: 10,
          wall_time_ms: 12,
          peak_memory_kib: 1024,
          cache_hit: false,
        });
      }
    }
    store.reconcile(baseMap, false);

    let untouchedRecomputes = 0;
    let targetRecomputes = 0;
    const untouchedView = computed(() => {
      untouchedRecomputes += 1;
      return store.cell(1, "std")()?.verdict;
    });
    const targetView = computed(() => {
      targetRecomputes += 1;
      return store.cell(25, "wa")()?.verdict;
    });
    expect(untouchedView()).toBe("AC");
    expect(targetView()).toBe("AC");
    untouchedRecomputes = 0;
    targetRecomputes = 0;

    // Stream 50 ticks changing ONLY (25, "wa") while 299 other cells stay identical.
    const t0 = performance.now();
    for (let i = 0; i < 50; i += 1) {
      const next = new Map(baseMap);
      next.set(matrixCellKey(25, "wa"), {
        test_index: 25,
        test_name: "25",
        solution: "wa",
        verdict: i % 2 === 0 ? "WA" : "TLE",
        cpu_time_ms: 20 + i,
        wall_time_ms: 22 + i,
        peak_memory_kib: 1024,
        cache_hit: false,
      });
      const changed = store.reconcile(next, true);
      expect(changed).toBe(1);
      expect(untouchedView()).toBe("AC");
      expect(targetView()).toBe(i % 2 === 0 ? "WA" : "TLE");
    }
    const perTickMs = (performance.now() - t0) / 50;
    expect(untouchedRecomputes).toBe(0);
    expect(targetRecomputes).toBe(50);
    expect(perTickMs).toBeLessThan(2.0);
    expect(store.size()).toBe(300);
    store.dispose();
    expect(store.size()).toBe(0);
  });

  test("stepMatrixSelection navigates rows and solution columns cleanly with bounds clamping and empty-cell skip", () => {
    const rows = [1, 2, 5];
    const sols = ["std", "wa"];
    expect(stepMatrixSelection(undefined, rows, sols, "down")).toEqual({ testIndex: 1, solution: "std" });
    expect(stepMatrixSelection({ testIndex: 1, solution: "std" }, rows, sols, "down")).toEqual({
      testIndex: 2,
      solution: "std",
    });
    expect(stepMatrixSelection({ testIndex: 5, solution: "std" }, rows, sols, "down")).toEqual({
      testIndex: 5,
      solution: "std",
    });
    expect(stepMatrixSelection({ testIndex: 2, solution: "std" }, rows, sols, "right")).toEqual({
      testIndex: 2,
      solution: "wa",
    });
    expect(stepMatrixSelection({ testIndex: 2, solution: "wa" }, rows, sols, "up")).toEqual({
      testIndex: 1,
      solution: "wa",
    });
    // Skips empty cell (2, "std") to land on (5, "std").
    expect(
      stepMatrixSelection(
        { testIndex: 1, solution: "std" },
        rows,
        sols,
        "down",
        (ti, sol) => !(ti === 2 && sol === "std"),
      ),
    ).toEqual({ testIndex: 5, solution: "std" });
  });
});

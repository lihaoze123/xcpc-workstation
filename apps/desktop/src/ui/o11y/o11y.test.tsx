import { describe, expect, test } from "nsh/test";
import { DiagnosticsWindowContent } from "./DiagnosticsWindow";
import {
  closeDiagnosticsWindow,
  openDiagnosticsWindow,
  toggleDiagnosticsWindow,
} from "./windowManager";
import {
  capturedTraces,
  clearAllDiagnostics,
  diagnosticsAnnotations,
  diagnosticsOpen,
  diagnosticsPaused,
  groupRawSpansIntoTraces,
  logStore,
  namedBaselines,
  o11yAnnotate,
  o11yLog,
  removeBaseline,
  saveTraceAsBaseline,
  startLiveDiagnostics,
  stopLiveDiagnostics,
  toggleDiagnosticsPaused,
} from "./telemetry";
import { isTraceRecording } from "../trace";

describe("Phase 6B o11y telemetry and diagnostics window", () => {
  test("groups cross-thread Rust + JS spans into parent/child waterfall trees", () => {
    const traces = groupRawSpansIntoTraces(
      [
        {
          id: "js-1",
          name: "ux.inp.outline.select",
          service: "ui",
          cat: "ux.inp",
          startMs: 100,
          durationMs: 42,
          hasError: false,
        },
        {
          id: "js-2",
          name: "preview",
          service: "ipc",
          cat: "ipc",
          startMs: 102,
          durationMs: 36,
          hasError: false,
        },
        {
          id: "rs-1",
          name: "preview_request",
          service: "desktop",
          cat: "desktop",
          startMs: 103,
          durationMs: 33,
          hasError: false,
        },
        {
          id: "rs-2",
          name: "build_document",
          service: "convert",
          cat: "convert",
          startMs: 104,
          durationMs: 12,
          hasError: false,
        },
        {
          id: "rs-3",
          name: "rasterize_cached",
          service: "engine",
          cat: "engine",
          startMs: 117,
          durationMs: 18,
          hasError: false,
        },
        // Separate group after >80ms gap with an error span
        {
          id: "rs-4",
          name: "compile_preview",
          service: "engine",
          cat: "engine",
          startMs: 400,
          durationMs: 65,
          hasError: true,
        },
      ],
      1_700_000_000_000,
      "test",
    );

    expect(traces.length).toBe(2);
    const first = traces[0]!;
    expect(first.name).toBe("ux.inp.outline.select");
    expect(first.inpMs).toBe(42);
    expect(first.hasError).toBe(false);
    expect(first.spans.length).toBe(5);

    const byId = new Map(first.spans.map((s) => [s.spanId, s]));
    expect(byId.get("js-1")?.parentId).toBeUndefined();
    expect(byId.get("js-2")?.parentId).toBe("js-1");
    expect(byId.get("rs-1")?.parentId).toBe("js-2");
    expect(byId.get("rs-2")?.parentId).toBe("rs-1");
    expect(byId.get("rs-3")?.parentId).toBe("rs-1");

    const second = traces[1]!;
    expect(second.name).toBe("compile_preview");
    expect(second.hasError).toBe(true);
  });

  test("always-on ring records logs and annotations without activating live trace recording", () => {
    stopLiveDiagnostics();
    clearAllDiagnostics();

    expect(diagnosticsOpen()).toBe(false);
    expect(isTraceRecording()).toBe(false);

    o11yLog("warn", "engine", "Slow compile (62.4 ms)", { durMs: 62.4 });
    o11yAnnotate("Switch Problem");

    expect(logStore.length()).toBe(1);
    expect(logStore.get(0).service).toBe("engine");
    expect(logStore.get(0).level).toBe("warn");
    expect(diagnosticsAnnotations().length).toBe(1);
    expect(isTraceRecording()).toBe(false);
  });

  test("live diagnostics lifecycle activates and reverts cleanly, and baselines round-trip", () => {
    stopLiveDiagnostics();
    clearAllDiagnostics();

    startLiveDiagnostics();
    expect(diagnosticsOpen()).toBe(true);
    expect(isTraceRecording()).toBe(true);

    toggleDiagnosticsPaused();
    expect(diagnosticsPaused()).toBe(true);
    toggleDiagnosticsPaused();
    expect(diagnosticsPaused()).toBe(false);

    const sampleTrace = groupRawSpansIntoTraces(
      [
        {
          id: "s1",
          name: "preview_request",
          service: "desktop",
          cat: "desktop",
          startMs: 10,
          durationMs: 25,
          hasError: false,
        },
      ],
      Date.now(),
      "base",
    )[0]!;
    capturedTraces.set([sampleTrace]);

    const base = saveTraceAsBaseline(sampleTrace, "Before opt");
    expect(namedBaselines().length).toBe(1);
    expect(namedBaselines()[0]?.label).toBe("Before opt");

    removeBaseline(base.id);
    expect(namedBaselines().length).toBe(0);

    stopLiveDiagnostics();
    expect(diagnosticsOpen()).toBe(false);
    expect(isTraceRecording()).toBe(false);
  });

  test("windowManager open/toggle/close degrades safely in headless test host", () => {
    closeDiagnosticsWindow();
    expect(diagnosticsOpen()).toBe(false);

    openDiagnosticsWindow();
    expect(diagnosticsOpen()).toBe(true);
    expect(isTraceRecording()).toBe(true);

    toggleDiagnosticsWindow();
    expect(diagnosticsOpen()).toBe(false);
    expect(isTraceRecording()).toBe(false);

    // Ensure DiagnosticsWindowContent symbol is valid and callable as an NSH component.
    expect(typeof DiagnosticsWindowContent).toBe("function");
  });
});

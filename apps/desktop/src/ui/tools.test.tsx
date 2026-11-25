import { describe, expect, open, test } from "nsh/test";
import { Preview } from "xcpc-workstation/native";
import { spaceName, windowTitle } from "./logic";
import { goSpace, info, openToolPlugin, activeToolPluginId, activeToolPayload, space, preview } from "./store";
import { computeTableColumnSpecs, isMonoTableCell } from "./tools/OutputRenderer";
import { replayCanvasOps } from "./tools/PluginUiRenderer";
import { createPluginUiSdk, createPluginUiSdkWithCleanup, fitBounds, mountPluginUi } from "./tools/pluginSdk";
import {
  coerceToolInputs,
  defaultInputsForTool,
  parseJsonReport,
  type PluginToolGroup,
  type ToolExecutionResult,
  type ToolSpec,
} from "./tools/types";

describe("Phase 4C/4D Pure Tools space & Plugin SDK", () => {
  test("tools space is always navigable even without a workspace or with training disabled", () => {
    const prevInfo = info();
    const prevSpace = space();
    try {
      info.set({ ...prevInfo, root: "", trainingEnabled: false });
      goSpace("tools");
      expect(space()).toBe("tools");
      expect(windowTitle({ workspace: false, space: "tools" })).toContain(spaceName("tools"));

      openToolPlugin("graph-editor", { payload: { nodes: [{ id: "1" }], edges: [] } });
      expect(space()).toBe("tools");
      expect(activeToolPluginId()).toBe("graph-editor");
      expect(activeToolPayload()).toEqual({ nodes: [{ id: "1" }], edges: [] });
    } finally {
      info.set(prevInfo);
      space.set(prevSpace);
      activeToolPluginId.set(undefined);
      activeToolPayload.set(undefined);
    }
  });

  test("form helpers build defaults and coerce typed tool inputs", () => {
    const spec: ToolSpec = {
      id: "complexity",
      name: "Complexity",
      inputs: [
        { name: "n", label: "n", kind: "number", default: 100000, required: true },
        { name: "flag", label: "flag", kind: "boolean", default: false },
        { name: "mode", label: "mode", kind: "select", options: ["fast", "slow"] },
      ],
    };
    const defaults = defaultInputsForTool(spec);
    expect(defaults.n).toBe(100000);
    expect(defaults.flag).toBe(false);
    expect(defaults.mode).toBe("fast");

    const coerced = coerceToolInputs(spec, { ...defaults, n: "500000", flag: 1 });
    expect(coerced.n).toBe(500000);
    expect(coerced.flag).toBe(true);
  });

  test("main repository ships zero built-in tool plugins (pure plugin container)", () => {
    const groups = parseJsonReport<PluginToolGroup[]>(preview.pluginListTools(), []);
    expect(groups.filter((g) => g.is_builtin).length).toBe(0);
  });

  test("Plugin UI SDK catches malformed ui.js errors inside boundary", () => {
    const sdk = createPluginUiSdk({ pluginId: "broken-ui" });
    const badSyntax = mountPluginUi("function (", sdk, { pluginId: "broken-ui" });
    expect(badSyntax.ok).toBe(false);

    const noRender = mountPluginUi("module.exports = {};", sdk, { pluginId: "broken-ui" });
    expect(noRender.ok).toBe(false);
  });

  test("SVG->PNG rasterization bridge works end to end", () => {
    const png = Preview.pluginRenderSvgPng(
      '<svg xmlns="http://www.w3.org/2000/svg" width="32" height="32"><rect width="32" height="32" fill="#0b6989"/></svg>',
      2.0,
    );
    expect(png.ok).toBe(true);
    expect((png.pngBase64 ?? "").length).toBeGreaterThan(20);
  });

  test("A3: HeadlessToolRunner boolean switch toggles repeatedly (true -> false -> true)", async () => {
    const app = await open("./tools/testHarness.tsx", {
      width: 960,
      height: 820,
      platform: "linux",
    });

    await app.waitFor("id:harness-switch-bool");
    await app.press("id:harness-switch-bool");
    await app.waitFor("id:tool-field-directed");
    expect(app.texts().includes("开")).toBe(true);

    // First click: true -> false
    await app.press("id:tool-field-directed");
    await app.waitUntil(() => app.texts().includes("关") && !app.texts().includes("开"));
    expect(app.texts().includes("关")).toBe(true);

    // Second click: false -> true
    await app.press("id:tool-field-directed");
    await app.waitUntil(() => app.texts().includes("开") && !app.texts().includes("关"));
    expect(app.texts().includes("开")).toBe(true);
  });

  test("#5/#6: table column specs are content-aware and Chinese prose never uses monospace", () => {
    expect(isMonoTableCell("998244353")).toBe(true);
    expect(isMonoTableCell("0x000000003B800001")).toBe(true);
    expect(isMonoTableCell("O(n log n)")).toBe(true);
    expect(isMonoTableCell("119 × 2^23 + 1")).toBe(true);
    expect(isMonoTableCell("最常用 NTT 素数模数")).toBe(false);
    expect(isMonoTableCell("高风险 TLE")).toBe(false);
    expect(isMonoTableCell("充裕通过")).toBe(false);

    const modSpecs = computeTableColumnSpecs(
      ["模数 p", "p - 1 分解", "最小原根 g", "NTT 最大 2^k", "说明"],
      [
        ["998244353", "119 × 2^23 + 1", "3", "2^23 = 8,388,608", "最常用 NTT 素数模数"],
        ["1000000007", "2 × 500000003 + 1", "5", "2^1（非 NTT）", "经典竞赛素数模数"],
      ],
    );
    expect(modSpecs.length).toBe(5);
    expect(modSpecs[4].flexWeight).toBeGreaterThan(modSpecs[2].flexWeight);
    expect(modSpecs[2].fixedWidth).toBeGreaterThan(0);

    const kmpSpecs = computeTableColumnSpecs(
      ["下标", "0", "1", "2", "3", "4", "5", "6"],
      [
        ["S[i]", "a", "b", "a", "c", "a", "b", "a"],
        ["KMP π", "0", "0", "1", "0", "1", "2", "3"],
      ],
    );
    expect(kmpSpecs.length).toBe(8);
    expect(kmpSpecs[0].flexWeight).toBeGreaterThan(0);
  });

  test("Phase 4D SDK primitives: fitBounds, onCleanup reverse order, storage, and useTool race drop", async () => {
    const emptyFit = fitBounds([], { width: 400, height: 300 });
    expect(emptyFit.scale).toBe(1);
    expect(emptyFit.offsetX).toBe(200);
    expect(emptyFit.offsetY).toBe(150);

    const fit = fitBounds(
      [
        { x: 0, y: 0 },
        { x: 200, y: 100 },
      ],
      { width: 400, height: 300 },
      20,
    );
    expect(fit.centerX).toBe(100);
    expect(fit.centerY).toBe(50);
    expect(fit.scale).toBeGreaterThan(1);

    const order: string[] = [];
    const mem = new Map<string, unknown>();
    let resolveFirst: ((r: ToolExecutionResult) => void) | undefined;
    let resolveSecond: ((r: ToolExecutionResult) => void) | undefined;
    let calls = 0;

    const { sdk, cleanup } = createPluginUiSdkWithCleanup({
      pluginId: "sdk-test-plugin",
      storageOverride: {
        get: <T,>(k: string) => mem.get(k) as T | undefined,
        set: (k: string, v: unknown) => {
          mem.set(k, v);
        },
        delete: (k: string) => mem.delete(k),
      },
      runToolOverride: (_toolId, inputs) =>
        new Promise<ToolExecutionResult>((resolve) => {
          calls += 1;
          const idx = calls;
          const handler = (res: ToolExecutionResult) => resolve(res);
          if (idx === 1) resolveFirst = handler;
          else resolveSecond = handler;
          void inputs;
        }),
    });

    expect(typeof sdk.ui.Each).toBe("function");
    expect(typeof sdk.ui.Row).toBe("function");
    expect(typeof sdk.ui.Col).toBe("function");
    expect(typeof sdk.ui.Card).toBe("function");
    expect(typeof sdk.ui.Switch).toBe("function");
    expect(typeof sdk.ui.Select).toBe("function");
    expect(typeof sdk.ui.Badge).toBe("function");
    expect(typeof sdk.ui.Menu).toBe("function");
    expect(typeof sdk.ui.Modal).toBe("function");
    expect(typeof sdk.ui.Image).toBe("function");
    expect(typeof sdk.ui.CodeEditor).toBe("function");
    expect(typeof sdk.canvas.fitBounds).toBe("function");
    expect(typeof sdk.pickFile).toBe("function");
    expect(typeof sdk.readClipboard).toBe("function");
    expect(typeof sdk.openExternal).toBe("function");
    expect(typeof sdk.openTool).toBe("function");
    expect(typeof sdk.onShortcut).toBe("function");
    expect(typeof sdk.workspace.listProblems).toBe("function");
    expect(typeof sdk.workspace.getProblem).toBe("function");

    // openExternal must reject non-http(s) URLs
    expect(() => sdk.openExternal("javascript:alert(1)")).toThrow();
    expect(() => sdk.openExternal("file:///etc/passwd")).toThrow();

    // onShortcut registers and cleans up cleanly
    const unbind = sdk.onShortcut("mod+shift+9", () => {});
    expect(typeof unbind).toBe("function");

    expect(sdk.storage.get("k")).toBeUndefined();
    sdk.storage.set("k", { n: 42 });
    expect(sdk.storage.get<{ n: number }>("k")).toEqual({ n: 42 });
    expect(sdk.storage.delete("k")).toBe(true);
    expect(sdk.storage.get("k")).toBeUndefined();

    const q = sdk.signal("first");
    const handle = sdk.useTool("echo", () => ({ q: q() }), { debounceMs: 0, immediate: true });
    expect(calls).toBe(1);
    q.set("second");
    expect(calls).toBe(2);
    resolveSecond?.({ blocks: [{ type: "text", content: "second-result" }] });
    await new Promise((r) => setTimeout(r, 10));
    expect(handle.data()?.blocks[0]).toEqual({ type: "text", content: "second-result" });
    resolveFirst?.({ blocks: [{ type: "text", content: "stale-first" }] });
    await new Promise((r) => setTimeout(r, 10));
    expect(handle.data()?.blocks[0]).toEqual({ type: "text", content: "second-result" });

    sdk.onCleanup(() => order.push("first"));
    sdk.onCleanup(() => order.push("second"));
    cleanup();
    cleanup();
    expect(order).toEqual(["second", "first"]);
  });

  test("Core QuickJS declarative UI protocol: replayCanvasOps & native bridge safety", () => {
    const calls: string[] = [];
    const mockCx = {
      fillStyle: "",
      strokeStyle: "",
      lineWidth: 1,
      font: "",
      textAlign: "left",
      textBaseline: "alphabetic",
      save: () => calls.push("save"),
      restore: () => calls.push("restore"),
      beginPath: () => calls.push("beginPath"),
      moveTo: (x: number, y: number) => calls.push(`moveTo(${x},${y})`),
      lineTo: (x: number, y: number) => calls.push(`lineTo(${x},${y})`),
      arc: (x: number, y: number, r: number) => calls.push(`arc(${x},${y},${r})`),
      fill: () => calls.push("fill"),
      stroke: () => calls.push("stroke"),
      fillText: (text: string, x: number, y: number) => calls.push(`fillText(${text},${x},${y})`),
      setLineDash: (segs: number[]) => calls.push(`setLineDash(${segs.join(",")})`),
    };

    replayCanvasOps(mockCx, [
      ["save"],
      ["fillStyle", "#0b6989"],
      ["strokeStyle", "#333333"],
      ["lineWidth", 2],
      ["setLineDash", [4, 2]],
      ["beginPath"],
      ["moveTo", 10, 20],
      ["lineTo", 50, 60],
      ["stroke"],
      ["beginPath"],
      ["arc", 30, 40, 12, 0, Math.PI * 2],
      ["fill"],
      ["fillText", "A", 30, 40],
      ["restore"],
    ]);

    expect(mockCx.fillStyle).toBe("#0b6989");
    expect(mockCx.strokeStyle).toBe("#333333");
    expect(mockCx.lineWidth).toBe(2);
    expect(calls).toEqual([
      "save",
      "setLineDash(4,2)",
      "beginPath",
      "moveTo(10,20)",
      "lineTo(50,60)",
      "stroke",
      "beginPath",
      "arc(30,40,12)",
      "fill",
      "fillText(A,30,40)",
      "restore",
    ]);

    // Mounting a non-existent plugin fails gracefully without throwing or needing `new Function`.
    const missing = preview.pluginUiMount("does-not-exist", "light", "{}", "");
    expect(missing.ok).toBe(false);

    // Built-in plugins cannot be uninstalled.
    const uninstBuiltin = preview.uninstallPlugin("codeforces");
    expect(uninstBuiltin.ok).toBe(false);
  });
});

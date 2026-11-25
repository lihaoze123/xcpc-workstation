import { describe, expect, open, test } from "nsh/test";
import { Preview } from "xcpc-workstation/native";
import { spaceName, windowTitle } from "./logic";
import { goSpace, info, openToolPlugin, activeToolPluginId, activeToolPayload, space, preview } from "./store";
import { computeTableColumnSpecs, isMonoTableCell } from "./tools/OutputRenderer";
import { createPluginUiSdk, mountPluginUi } from "./tools/pluginSdk";
import {
  coerceToolInputs,
  defaultInputsForTool,
  parseJsonReport,
  type PluginToolGroup,
  type RegisteredPluginCommand,
  type ToolExecutionResult,
} from "./tools/types";

describe("Phase 4C Tools space, Plugin SDK, and headless tool pipeline", () => {
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

  test("dynamic tool list includes all 4 built-in tool plugins and form helpers coerce inputs", () => {
    const groups = parseJsonReport<PluginToolGroup[]>(preview.pluginListTools(), []);
    const ids = new Set(groups.map((g) => g.plugin_id));
    expect(ids.has("graph-editor")).toBe(true);
    expect(ids.has("math-bitwise")).toBe(true);
    expect(ids.has("complexity-mem")).toBe(true);
    expect(ids.has("string-lab")).toBe(true);

    const comp = groups.find((g) => g.plugin_id === "complexity-mem")!;
    const tool = comp.tools[0];
    const defaults = defaultInputsForTool(tool);
    expect(defaults.n).toBe(100000);
    const coerced = coerceToolInputs(tool, { ...defaults, n: "500000" });
    expect(coerced.n).toBe(500000);
  });

  test("headless tool execution returns structured output blocks", async () => {
    const res = await preview.pluginRunTool(
      "string-lab",
      "analyze",
      JSON.stringify({ text: "abacaba", pattern: "aba" }),
    );
    expect(res.ok).toBe(true);
    const exec = parseJsonReport<ToolExecutionResult>(res, { blocks: [] });
    expect(exec.blocks.length).toBeGreaterThan(0);
    expect(exec.blocks.some((b) => b.type === "table")).toBe(true);
  });

  test("Plugin UI SDK mounts real graph-editor/ui.js and math-bitwise/ui.js cleanly", () => {
    for (const pluginId of ["graph-editor", "math-bitwise"]) {
      const uiRes = preview.pluginUiSource(pluginId);
      expect(uiRes.ok).toBe(true);
      expect(typeof uiRes.source).toBe("string");
      expect((uiRes.source ?? "").length).toBeGreaterThan(100);

      const sdk = createPluginUiSdk({
        pluginId,
        getInitialPayload: () => undefined,
      });
      const mounted = mountPluginUi(uiRes.source!, sdk, { builtinPluginId: pluginId });
      if (!mounted.ok) {
        throw new Error(`mountPluginUi(${pluginId}) failed: ${mounted.error}`);
      }
      expect(mounted.ok).toBe(true);
    }
  });

  test("Plugin UI SDK catches malformed ui.js errors inside boundary", () => {
    const sdk = createPluginUiSdk({ pluginId: "broken-ui" });
    const badSyntax = mountPluginUi("function (", sdk);
    expect(badSyntax.ok).toBe(false);

    const noRender = mountPluginUi("module.exports = {};", sdk);
    expect(noRender.ok).toBe(false);
  });

  test("plugin commands and SVG->PNG rasterization bridge work end to end", () => {
    const cmds = parseJsonReport<RegisteredPluginCommand[]>(
      preview.pluginListCommands("global"),
      [],
    );
    expect(cmds.some((c) => c.plugin_id === "graph-editor")).toBe(true);
    expect(cmds.some((c) => c.plugin_id === "math-bitwise")).toBe(true);

    const png = Preview.pluginRenderSvgPng(
      '<svg xmlns="http://www.w3.org/2000/svg" width="32" height="32"><rect width="32" height="32" fill="#0b6989"/></svg>',
      2.0,
    );
    expect(png.ok).toBe(true);
    expect((png.pngBase64 ?? "").length).toBeGreaterThan(20);
  });
});

describe("Phase 4 UX interactive click regressions (A1-A4, 0.4, 0.6)", () => {
  test("A1/A2: math-bitwise tabs switch content on click (位运算 -> 数论计算 -> 常用模数)", async () => {
    const app = await open("./tools/testHarness.tsx", {
      width: 960,
      height: 820,
      platform: "linux",
    });

    await app.waitFor("id:tool-item-math-bitwise");
    await app.press("id:tool-item-math-bitwise");
    await app.waitFor("id:math-panel-bits");
    expect(app.texts().some((s) => s.includes("位 63–56"))).toBe(true);

    // Click "数论计算" tab
    await app.press("id:math-tab-nt");
    await app.waitFor("id:math-panel-nt");
    await app.waitUntil(() => app.texts().some((s) => s.includes("同余方程组")));
    expect(app.texts().some((s) => s.includes("同余方程组"))).toBe(true);

    // Click "常用模数" tab
    await app.press("id:math-tab-mods");
    await app.waitFor("id:math-panel-mods");
    await app.waitUntil(() => app.texts().some((s) => s.includes("1000000007")));
    expect(app.texts().some((s) => s.includes("1000000007"))).toBe(true);
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

  test("A4/0.6: complexity-mem switching to 数组内存估算 replaces stale time complexity table", async () => {
    const app = await open("./tools/testHarness.tsx", {
      width: 960,
      height: 820,
      platform: "linux",
    });

    await app.waitFor("id:tool-item-complexity-mem");
    await app.press("id:tool-item-complexity-mem");
    await app.waitUntil(() => app.texts().some((s) => s.includes("时间复杂度可行性评估")));
    expect(app.texts().some((s) => s.includes("1.00e5"))).toBe(true);

    // Switch to "数组内存估算"
    await app.press("id:tool-tab-memory");
    await app.waitUntil(() => app.texts().some((s) => s.includes("数组内存精确计算")));
    expect(app.texts().some((s) => s.includes("数组内存精确计算"))).toBe(true);
    expect(app.texts().some((s) => s.includes("时间复杂度可行性评估"))).toBe(false);
    expect(app.texts().some((s) => s.includes("MiB"))).toBe(true);
  });

  test("0.4: graph-editor layout buttons update node coordinates and keep nodes within canvas bounds", async () => {
    const app = await open("./tools/testHarness.tsx", {
      width: 960,
      height: 820,
      platform: "linux",
    });

    await app.waitFor("id:tool-item-graph-editor");
    await app.press("id:tool-item-graph-editor");
    await app.waitFor("id:graph-canvas");

    // Load geometry sample (which uses coords format so node coordinates sync to editor text)
    await app.press("id:graph-sample-geometry");
    await app.waitUntil(() => app.texts().some((s) => s.includes("节点 4")));

    const parseCoords = (raw: string | null): { id: string; x: number; y: number }[] => {
      const coords: { id: string; x: number; y: number }[] = [];
      for (const line of String(raw ?? "").split(/\r?\n/)) {
        const m = line.trim().match(/^(\d+)\s+(-?\d+)\s+(-?\d+)$/);
        if (m) coords.push({ id: m[1], x: Number(m[2]), y: Number(m[3]) });
      }
      return coords;
    };

    // Apply circle layout
    await app.press("id:graph-layout-circle");
    await app.wait(50);
    const circleCoords = parseCoords(app.value("id:graph-editor-input"));
    expect(circleCoords.length).toBeGreaterThanOrEqual(4);
    for (const pt of circleCoords) {
      expect(pt.x).toBeGreaterThanOrEqual(18);
      expect(pt.x).toBeLessThanOrEqual(600);
      expect(pt.y).toBeGreaterThanOrEqual(18);
      expect(pt.y).toBeLessThanOrEqual(800);
    }

    // Apply tree layout and assert coordinates actually changed and stay in bounds
    await app.press("id:graph-layout-tree");
    await app.wait(50);
    const treeCoords = parseCoords(app.value("id:graph-editor-input"));
    expect(treeCoords.length).toBeGreaterThanOrEqual(4);
    const moved = treeCoords.some((pt, i) => {
      const prev = circleCoords[i];
      return prev && (Math.abs(pt.x - prev.x) > 1 || Math.abs(pt.y - prev.y) > 1);
    });
    expect(moved).toBe(true);
    for (const pt of treeCoords) {
      expect(pt.x).toBeGreaterThanOrEqual(18);
      expect(pt.x).toBeLessThanOrEqual(600);
      expect(pt.y).toBeGreaterThanOrEqual(18);
      expect(pt.y).toBeLessThanOrEqual(800);
    }
  });
});

describe("Round-2 UX polish regressions (#5-#8)", () => {
  test("#5/#6: table column specs are content-aware and Chinese prose never uses monospace", () => {
    // #6 Font classification
    expect(isMonoTableCell("最常用 NTT 素数模数")).toBe(false);
    expect(isMonoTableCell("经典竞赛素数模数")).toBe(false);
    expect(isMonoTableCell("充裕通过")).toBe(false);
    expect(isMonoTableCell("2^1（非 NTT）")).toBe(false);
    expect(isMonoTableCell("998244353")).toBe(true);
    expect(isMonoTableCell("2 × 500000003 + 1")).toBe(true);
    expect(isMonoTableCell("2^26 = 67,108,864")).toBe(true);
    expect(isMonoTableCell("abacaba")).toBe(true);

    // #5 Content-aware widths & right-to-left column breathing room
    const modSpecs = computeTableColumnSpecs(
      ["模数 p", "p - 1 分解", "最小原根 g", "NTT 最大 2^k", "说明"],
      [
        ["998244353", "119 × 2^23 + 1", "3", "2^23 = 8,388,608", "最常用 NTT 素数模数"],
        ["1000000007", "2 × 500000003 + 1", "5", "2^1（非 NTT）", "经典竞赛素数模数"],
        ["469762049", "7 × 2^26 + 1", "3", "2^26 = 67,108,864", "超大长度 NTT 模数"],
      ],
      ["right", "left", "center", "left", "left"],
    );
    expect(modSpecs[0].align).toBe("right");
    expect(modSpecs[0].fixedWidth).toBeGreaterThan(0);
    expect(modSpecs[0].extraRightPad).toBeGreaterThan(0);
    expect(modSpecs[1].fixedWidth).toBeUndefined();
    expect(modSpecs[1].flexWeight).toBeGreaterThan(modSpecs[0].fixedWidth!);
    expect(modSpecs[2].fixedWidth).toBeGreaterThan(0);

    const saSpecs = computeTableColumnSpecs(
      ["排名 RK", "起点 SA", "LCP", "后缀"],
      [
        ["0", "6", "0", "a"],
        ["1", "4", "1", "aba"],
        ["2", "0", "3", "abacaba"],
      ],
      ["right", "right", "right", "left"],
    );
    expect(saSpecs[2].align).toBe("right");
    expect(saSpecs[2].extraRightPad).toBeGreaterThan(0);
    expect(saSpecs[3].fixedWidth).toBeUndefined();
    expect(saSpecs[3].flexWeight).toBeGreaterThan(0);
  });

  test("#7: math-bitwise bit grid stretches evenly and overflow menu runs << 1, >> 1, 全 1", async () => {
    const app = await open("./tools/testHarness.tsx", {
      width: 960,
      height: 820,
      platform: "linux",
    });

    await app.waitFor("id:tool-item-math-bitwise");
    await app.press("id:tool-item-math-bitwise");
    await app.waitFor("id:math-bit-grid");

    // Overflow menu is initially closed, then opens as a floating popover and auto-closes on item click
    expect(app.texts().includes("全 1")).toBe(false);
    await app.press("id:math-bit-more");
    await app.waitFor("id:math-bit-more-menu");
    expect(app.texts().includes("全 1")).toBe(true);

    // 998244353 << 1 = 1996488706 (and menu auto-closes)
    await app.press("id:math-bit-shl");
    await app.waitUntil(() => app.value("id:math-bit-input") === "1996488706");
    expect(app.value("id:math-bit-input")).toBe("1996488706");
    await app.waitUntil(() => !app.texts().includes("全 1"));

    // Re-open menu -> 1996488706 >> 1 = 998244353 (and menu auto-closes)
    await app.press("id:math-bit-more");
    await app.waitFor("id:math-bit-more-menu");
    await app.press("id:math-bit-shr");
    await app.waitUntil(() => app.value("id:math-bit-input") === "998244353");
    expect(app.value("id:math-bit-input")).toBe("998244353");
    await app.waitUntil(() => !app.texts().includes("全 1"));

    // Re-open menu -> 全 1 -> 18446744073709551615
    await app.press("id:math-bit-more");
    await app.waitFor("id:math-bit-more-menu");
    await app.press("id:math-bit-all-ones");
    await app.waitUntil(() => app.value("id:math-bit-input") === "18446744073709551615");
    expect(app.value("id:math-bit-input")).toBe("18446744073709551615");
    await app.waitUntil(() => !app.texts().includes("全 1"));
  });

  test("#8: graph-editor connect mode toggle keeps constant label width without layout shift", async () => {
    const app = await open("./tools/testHarness.tsx", {
      width: 960,
      height: 820,
      platform: "linux",
    });

    await app.waitFor("id:tool-item-graph-editor");
    await app.press("id:tool-item-graph-editor");
    await app.waitFor("id:graph-connect-toggle");

    expect(app.texts().includes("连边模式")).toBe(true);
    expect(app.texts().includes("连边模式：开")).toBe(false);

    // Toggling on/off keeps the exact same label to prevent 1px/text layout shift
    await app.press("id:graph-connect-toggle");
    await app.wait(30);
    expect(app.texts().includes("连边模式")).toBe(true);
    expect(app.texts().includes("连边模式：开")).toBe(false);

    await app.press("id:graph-connect-toggle");
    await app.wait(30);
    expect(app.texts().includes("连边模式")).toBe(true);
  });

  test("UX #6: switching directly between two headless plugins (complexity-mem <-> string-lab) selects valid tool", async () => {
    const app = await open("./tools/testHarness.tsx", {
      width: 960,
      height: 820,
      platform: "linux",
    });

    await app.waitFor("id:tool-item-complexity-mem");
    await app.press("id:tool-item-complexity-mem");
    await app.waitUntil(() => app.texts().some((s) => s.includes("时间复杂度可行性评估")));

    // Switch directly to string-lab
    await app.press("id:tool-item-string-lab");
    await app.waitUntil(() => app.texts().some((s) => s.includes("abacaba")));
    expect(app.texts().some((s) => s.includes("时间复杂度可行性评估"))).toBe(false);

    // Switch directly back to complexity-mem
    await app.press("id:tool-item-complexity-mem");
    await app.waitUntil(() => app.texts().some((s) => s.includes("时间复杂度可行性评估")));
    expect(app.texts().some((s) => s.includes("时间复杂度可行性评估"))).toBe(true);
  });
});

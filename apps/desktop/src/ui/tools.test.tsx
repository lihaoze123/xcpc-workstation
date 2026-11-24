import { describe, expect, test } from "nsh/test";
import { Preview } from "xcpc-workstation/native";
import { spaceName, windowTitle } from "./logic";
import { goSpace, info, openToolPlugin, activeToolPluginId, activeToolPayload, space, preview } from "./store";
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

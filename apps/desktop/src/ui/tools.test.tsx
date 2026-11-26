import { describe, expect, open, test } from "nsh/test";
import { Preview } from "xcpc-workstation/native";
import { languagePref } from "./i18n";
import { spaceName, windowTitle } from "./logic";
import {
  activeToolPayload,
  activeToolPluginId,
  goSpace,
  info,
  notifyPluginsChanged,
  openToolPlugin,
  pluginRevisionFor,
  preview,
  space,
} from "./store";
import { computeTableColumnSpecs, isMonoTableCell } from "./tools/OutputRenderer";
import { classifyPluginUiError, formatPluginError } from "./tools/pluginErrors";
import {
  buildThemeCssVars,
  injectPluginHtml,
  isTrustedPluginUrl,
  normalizeClipboardImageSrc,
  PLUGIN_WEBVIEW_CSP,
} from "./tools/PluginWebView";
import {
  coerceToolInputs,
  defaultInputsForTool,
  parseJsonReport,
  type PluginToolGroup,
  type ToolSpec,
} from "./tools/types";

describe("Phase 4C/4D Pure Tools space & WebView Plugin Bridge", () => {
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

  test("PluginWebView injects strict CSP, theme CSS vars, and window.xcpc bridge safely", () => {
    const lightVars = buildThemeCssVars("light");
    const darkVars = buildThemeCssVars("dark");
    expect(lightVars["--xcpc-bg"]).not.toBe(darkVars["--xcpc-bg"]);
    expect(lightVars["--xcpc-primary"]).toBeTruthy();
    expect(darkVars["--xcpc-border"]).toBeTruthy();

    const raw = `<!doctype html><html><head><title>Bitwise</title></head><body><div id="app"></div></body></html>`;
    const injected = injectPluginHtml(raw, {
      pluginId: "math-bitwise",
      theme: "dark",
      cssVars: darkVars,
      initialPayload: { note: "test </script> <!-- safe" },
      bridgeToken: "tok-fixed-1234",
    });

    // Outer host shell enforces CSP before byte 0 of plugin HTML and isolates plugin inside sandboxed iframe.
    expect(injected).toContain(`<meta http-equiv="Content-Security-Policy" content="${PLUGIN_WEBVIEW_CSP}">`);
    expect(injected).toContain("connect-src 'none'");
    expect(injected).toContain("form-action 'none'");
    expect(injected).toContain('<iframe id="xcpc-plugin-frame" sandbox="allow-scripts"></iframe>');
    expect(injected).not.toContain("allow-top-navigation");
    expect(injected).not.toContain("allow-same-origin");
    expect(injected).toContain("tok-fixed-1234");
    expect(injected).toContain("window.nsh.postMessage");
    expect(injected).toContain("window.chrome.webview.postMessage");
    expect(injected).toContain("margin:0;padding:0;");
    expect(injected).not.toContain("SPACE[0]");
    expect(isTrustedPluginUrl("")).toBe(true);
    expect(isTrustedPluginUrl("about:blank")).toBe(true);
    expect(isTrustedPluginUrl("about:blank#anchor")).toBe(true);
    expect(isTrustedPluginUrl("about:srcdoc")).toBe(true);
    expect(isTrustedPluginUrl("data:text/html;charset=utf-8;base64,PGh0bWw+")).toBe(true);
    expect(isTrustedPluginUrl("data:image/png;base64,AAAA")).toBe(false);
    expect(isTrustedPluginUrl("javascript:alert(1)")).toBe(false);
    expect(isTrustedPluginUrl("https://evil.example")).toBe(false);
    // S4: clipboard.readText, clipboard.readImage, and openExternal are not exposed to untrusted plugins.
    expect(injected).toContain("clipboard.writeImage");
    expect(injected).toContain("oj.listAccounts");
    expect(injected).not.toContain("clipboard.readText");
    expect(injected).not.toContain("clipboard.readImage");
    expect(injected).not.toContain("openExternal");
    // Payload with `</script>` must be safely escaped so it cannot break out of the injected script tag.
    expect(injected).toContain("<\\/script>");
    expect(injected).not.toContain('test </script>');

    // S1 & S2: Commented <head> and pre-<head> <script> cannot bypass outer CSP or break out of srcdoc.
    const attackS1 = injectPluginHtml(`<!--<head>--><script>fetch("http://evil")</script>`, {
      pluginId: "evil-s1",
      theme: "light",
      cssVars: lightVars,
    });
    expect(attackS1.indexOf(PLUGIN_WEBVIEW_CSP)).toBeLessThan(attackS1.indexOf("evil"));
    expect(attackS1).toContain('<iframe id="xcpc-plugin-frame" sandbox="allow-scripts"></iframe>');

    const attackS2 = injectPluginHtml(
      `<!doctype html><html><script>new Image().src="http://evil"</script><head></head></html>`,
      {
        pluginId: "evil-s2",
        theme: "light",
        cssVars: lightVars,
      },
    );
    expect(attackS2.indexOf(PLUGIN_WEBVIEW_CSP)).toBeLessThan(attackS2.indexOf("http://evil"));
  });

  test("normalizeClipboardImageSrc accepts raw PNG base64 and data:image URLs, and rejects unsafe schemes", () => {
    const sampleB64 = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=";
    expect(normalizeClipboardImageSrc(sampleB64)).toBe(`data:image/png;base64,${sampleB64}`);
    expect(normalizeClipboardImageSrc(`data:image/png;base64,${sampleB64}`)).toBe(
      `data:image/png;base64,${sampleB64}`,
    );
    expect(() => normalizeClipboardImageSrc("")).toThrow();
    expect(() => normalizeClipboardImageSrc("file:///etc/passwd")).toThrow();
    expect(() => normalizeClipboardImageSrc("https://example.com/a.png")).toThrow();
    expect(() => normalizeClipboardImageSrc("data:text/html;base64,PHNjcmlwdD4=")).toThrow();
  });

  test("New Issue C: per-plugin revision isolates unrelated plugin changes", () => {
    const bitBefore = pluginRevisionFor("math-bitwise");
    const graphBefore = pluginRevisionFor("graph-editor");

    // Modifying or installing an unrelated plugin must NOT bump `math-bitwise` or `graph-editor`.
    notifyPluginsChanged(["some-other-plugin"]);
    expect(pluginRevisionFor("math-bitwise")).toBe(bitBefore);
    expect(pluginRevisionFor("graph-editor")).toBe(graphBefore);

    // Modifying `math-bitwise` bumps ONLY `math-bitwise`.
    notifyPluginsChanged(["math-bitwise"]);
    expect(pluginRevisionFor("math-bitwise")).toBe(bitBefore + 1);
    expect(pluginRevisionFor("graph-editor")).toBe(graphBefore);

    // Full rescan (`undefined`) bumps all tracked plugins.
    notifyPluginsChanged(undefined);
    expect(pluginRevisionFor("math-bitwise")).toBe(bitBefore + 2);
    expect(pluginRevisionFor("graph-editor")).toBe(graphBefore + 1);
  });

  test("SVG->PNG rasterization bridge works end to end", () => {
    const png = Preview.pluginRenderSvgPng(
      '<svg xmlns="http://www.w3.org/2000/svg" width="32" height="32"><rect width="32" height="32" fill="#0b6989"/></svg>',
      2.0,
    );
    expect(png.ok).toBe(true);
    expect((png.pngBase64 ?? "").length).toBeGreaterThan(20);
  });

  test("Built-in plugins cannot be uninstalled", () => {
    const uninstBuiltin = preview.uninstallPlugin("codeforces");
    expect(uninstBuiltin.ok).toBe(false);
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

  test("Awkward 7, 10 & New Issue D: plugin error mapper formats raw codes, Chinese locale network errors, and ureq errors", () => {
    const prevLang = languagePref();
    languagePref.set("zh-CN");
    try {
      expect(formatPluginError("CHECKSUM_MISMATCH: expected sha256 '000'")).toContain("校验失败");
      expect(formatPluginError("SYMLINK_ESCAPE: /etc/passwd")).toContain("符号链接");
      expect(formatPluginError("ZIP_SLIP: ../evil")).toContain("非法文件路径");
      expect(formatPluginError("PLUGIN_UNTRUSTED: demo")).toContain("确认信任");
      expect(formatPluginError("HTTP_NOT_ALLOWED: http://example.com")).toContain("HTTPS");
      expect(formatPluginError("JSON_ERROR: expected value at line 1 column 1")).toContain("索引格式无效");

      // New Issue D: Chinese OS locale errors & ureq transport errors
      expect(
        formatPluginError(
          "HTTP_ERROR: http://127.0.0.1:18767/index.json: Connection Failed: Connect error: 连接被拒绝 (os error 111)",
        ),
      ).toContain("网络连接失败");
      expect(formatPluginError("无法连接到远程主机")).toContain("网络连接失败");
      expect(formatPluginError("连接超时 (os error 110)")).toContain("网络连接失败");
      expect(formatPluginError("名称解析失败")).toContain("网络连接失败");
      expect(formatPluginError("HTTP_ERROR: dns failed for example.invalid")).toContain("网络连接失败");
      expect(formatPluginError("HTTP_ERROR: unexpected status 503")).toBe("unexpected status 503");

      const mountErr = classifyPluginUiError("SyntaxError: bad token\n    at xcpc-plugin://bad/ui.html:1:5", "mount");
      expect(mountErr.title).toBe("插件界面加载失败");
      expect(mountErr.details).toContain("xcpc-plugin://bad/ui.html:1:5");

      const timeoutErr = classifyPluginUiError("TIMEOUT: script interrupted after 2000ms", "action");
      expect(timeoutErr.title).toBe("插件操作超时");
      expect(timeoutErr.summary).toContain("超时");

      const untrustedErr = classifyPluginUiError("PLUGIN_UNTRUSTED: net-plugin", "mount");
      expect(untrustedErr.untrusted).toBe(true);
      expect(untrustedErr.title).toBe("插件尚未信任");
    } finally {
      languagePref.set(prevLang);
    }
  });

  test("Blocker 1: typing into a plugin tool input works cleanly with plugin watching active", async () => {
    const app = await open("./tools/testHarness.tsx", {
      width: 960,
      height: 820,
      platform: "linux",
    });

    await app.waitFor("id:harness-switch-watch");
    await app.press("id:harness-switch-watch");
    await app.waitFor("id:tool-field-n");

    await app.type("id:tool-field-n", "7");
    await new Promise((r) => setTimeout(r, 250));
    expect(app.value("id:tool-field-n") ?? "").toContain("7");
  });
});

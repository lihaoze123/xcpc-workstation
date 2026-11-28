import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import {
  parseManifestEntries,
  checkQuickJsForbiddenSource,
  inlineHtmlBundle,
  xcpcPlugin,
} from "../vite.mjs";
import { getXcpc } from "../ui.js";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const sdkRoot = path.resolve(__dirname, "..");

test("manifest.schema.json is valid JSON Schema covering all plugin capabilities", () => {
  const schema = JSON.parse(
    fs.readFileSync(path.join(sdkRoot, "manifest.schema.json"), "utf8"),
  );
  assert.equal(schema.type, "object");
  assert.deepEqual(schema.required, ["id", "name", "version"]);
  assert.deepEqual(schema.properties.capabilities.items.enum, [
    "oj_source",
    "tool",
    "commands",
    "importer",
    "exporter",
    "credentials",
    "template",
  ]);
  assert.equal(schema.properties.permissions.properties.workspace_write.type, "boolean");
  assert.equal(schema.properties.templates.type, "array");
  assert.equal(schema.properties.font_dirs.type, "array");
});

test("parseManifestEntries extracts id, entry, and ui_entry while ignoring section keys", () => {
  const parsed = parseManifestEntries(`
# comment
id = "graph-editor"
name = "Graph Editor"
entry = "index.js"
ui_entry = "ui.html"

[permissions]
entry = "should-be-ignored"
`);
  assert.equal(parsed.id, "graph-editor");
  assert.equal(parsed.entry, "index.js");
  assert.equal(parsed.uiEntry, "ui.html");
});

test("QuickJS sandbox guard blocks forbidden Node built-ins and DOM globals", () => {
  const [guard] = xcpcPlugin();
  const ctx = {
    error(msg) {
      throw new Error(msg);
    },
  };

  assert.throws(
    () => guard.resolveId.call(ctx, "node:fs", "src/index.ts"),
    /Forbidden Node\.js module 'node:fs'/,
  );
  assert.throws(
    () => guard.resolveId.call(ctx, "child_process", "src/index.ts"),
    /Forbidden Node\.js module 'child_process'/,
  );
  assert.equal(guard.resolveId.call(ctx, "./helper", "src/index.ts"), null);

  assert.match(
    checkQuickJsForbiddenSource("const x = window.location.href;", "src/index.ts") ?? "",
    /window\/document/,
  );
  assert.match(
    checkQuickJsForbiddenSource("localStorage.setItem('a', 'b');", "src/index.ts") ?? "",
    /localStorage/,
  );
  // Comments and string literals must not trigger false positives.
  assert.equal(
    checkQuickJsForbiddenSource(
      '// window.location is mentioned in comment\nconst s = "document.body";\nexport function runTool() { return { blocks: [] }; }',
      "src/index.ts",
    ),
    null,
  );
});

test("inlineHtmlBundle inlines CSS and IIFE JS, strips modulepreload, and escapes </script>", () => {
  const html = `<!doctype html>
<html>
  <head>
    <link rel="modulepreload" crossorigin href="./assets/vendor.js">
    <link rel="stylesheet" crossorigin href="./assets/main.css">
  </head>
  <body>
    <div id="app"></div>
    <script type="module" crossorigin src="./assets/main.js"></script>
  </body>
</html>`;

  const result = inlineHtmlBundle(html, {
    cssAssets: [{ fileName: "assets/main.css", source: "body { color: red; }" }],
    jsChunks: [
      {
        fileName: "assets/main.js",
        code: '(function(){ const s = "</script><SCRIPT>"; console.log(s); })();',
      },
    ],
  });

  assert.ok(!result.includes("modulepreload"));
  assert.ok(!result.includes('href="./assets/main.css"'));
  assert.ok(!result.includes('src="./assets/main.js"'));
  assert.ok(result.includes("<style>\nbody { color: red; }\n</style>"));
  assert.ok(result.includes('<\\/script><SCRIPT>'));
  // Ensure there is only one closing </script> tag (the real one).
  assert.equal((result.match(/<\/script>/gi) ?? []).length, 1);
});

test("getXcpc throws outside WebView and returns window.xcpc when present", () => {
  assert.throws(() => getXcpc(), /window\.xcpc is not available/);
  const mockBridge = { toast() {} };
  globalThis.window = { xcpc: mockBridge };
  try {
    assert.equal(getXcpc(), mockBridge);
  } finally {
    delete globalThis.window;
  }
});

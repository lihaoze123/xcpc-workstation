import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import {
  parseManifestEntries,
  checkQuickJsForbiddenSource,
  xcpcPlugin,
} from "../vite.mjs";

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
  ]);
});

test("parseManifestEntries extracts id, entry, and ui_entry while ignoring section keys", () => {
  const parsed = parseManifestEntries(`
# comment
id = "graph-editor"
name = "Graph Editor"
entry = "index.js"
ui_entry = "ui.js"

[permissions]
entry = "should-be-ignored"
`);
  assert.equal(parsed.id, "graph-editor");
  assert.equal(parsed.entry, "index.js");
  assert.equal(parsed.uiEntry, "ui.js");
});

test("QuickJS sandbox guard blocks forbidden Node built-ins and DOM globals", () => {
  const [guard] = xcpcPlugin();
  const errors = [];
  const ctx = {
    error(msg) {
      errors.push(msg);
      throw new Error(msg);
    },
  };

  assert.throws(() => guard.resolveId.call(ctx, "node:fs", "src/index.ts"), /Forbidden Node\.js module 'node:fs'/);
  assert.throws(() => guard.resolveId.call(ctx, "child_process", "src/index.ts"), /Forbidden Node\.js module 'child_process'/);
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

---
title: Plugin Development Guide
---

# Plugin Development Guide

This guide covers developing, locally debugging, validating, packaging, and publishing XCPC Workstation plugins with `@xcpc-workstation/plugin-sdk` (current version `v0.3.0`) and the `xcpc plugin` CLI.

> **Authoritative API contracts** (TypeScript definitions and JSON Schema in the repository are the single source of truth):
> - QuickJS host global `host` and plugin export contracts: [`packages/plugin-sdk/host.d.ts`](https://github.com/lihaoze123/xcpc-workstation/blob/main/packages/plugin-sdk/host.d.ts)
> - WebView bridge `window.xcpc` and `getXcpc()`: [`packages/plugin-sdk/ui.d.ts`](https://github.com/lihaoze123/xcpc-workstation/blob/main/packages/plugin-sdk/ui.d.ts)
> - Single-file Vite bundler plugin `xcpcPlugin(options?)`: [`packages/plugin-sdk/vite.d.ts`](https://github.com/lihaoze123/xcpc-workstation/blob/main/packages/plugin-sdk/vite.d.ts)
> - `manifest.toml` JSON Schema: [`packages/plugin-sdk/manifest.schema.json`](https://github.com/lihaoze123/xcpc-workstation/blob/main/packages/plugin-sdk/manifest.schema.json)

---

## 1. Installing `@xcpc-workstation/plugin-sdk`

`@xcpc-workstation/plugin-sdk` is published on npm and can be installed directly as a dev dependency:

```bash
npm install -D @xcpc-workstation/plugin-sdk
```

When scaffolding a project with `xcpc plugin init <dir> --vite`, this dependency is added to `package.json` automatically.

`@xcpc-workstation/plugin-sdk` provides four subpath exports:

| Subpath | Purpose |
| :--- | :--- |
| `@xcpc-workstation/plugin-sdk/host` | QuickJS sandbox `host` global types and capability module contracts (`OjSourcePlugin`, `ToolPlugin`, `CommandPlugin`, `ImporterPlugin`, `ExporterPlugin`, etc.). |
| `@xcpc-workstation/plugin-sdk/ui` | Typed `window.xcpc` bridge interface (`XcpcBridge`) and the `getXcpc()` helper. |
| `@xcpc-workstation/plugin-sdk/vite` | Zero-config `xcpcPlugin(options?)` Vite plugin: bundles the WebView UI into a self-contained single-file IIFE `dist/ui.html`, compiles `src/index.ts` to `dist/index.js` with compile-time guardrails against forbidden QuickJS APIs, and copies `manifest.toml`. |
| `@xcpc-workstation/plugin-sdk/manifest.schema.json` | JSON Schema for `manifest.toml` autocompletion and static validation in editors (such as Taplo or YAML Language Server). |

---

## 2. Dual-Track Scaffolding (`xcpc plugin init`)

`xcpc plugin init` supports both a **zero-build (no Node.js)** track and a **Vite + TypeScript** track:

```bash
# Track 1: Zero-build tool plugin (default --kind tool; generates manifest.toml + index.js + ui.html)
xcpc plugin init ./my-tool

# Zero-build OJ source plugin (generates manifest.toml + index.js)
xcpc plugin init ./my-oj --kind oj

# Zero-build typesetting template plugin (generates manifest.toml + templates/sample.typ + assets/badge.svg)
xcpc plugin init ./my-template --kind template

# Track 2: Vite + TypeScript tool plugin (pre-configured with @xcpc-workstation/plugin-sdk/vite)
xcpc plugin init ./my-tool --kind tool --vite
cd ./my-tool
npm install
npm run build
```

> Keep `# yaml-language-server: $schema=./node_modules/@xcpc-workstation/plugin-sdk/manifest.schema.json` on the first line of `manifest.toml` (or configure Taplo) to enable editor autocompletion and static checks.

---

## 3. Local Development, Per-Plugin Hot Reload & Unit Testing

The desktop app includes **per-plugin ID file watching**: editing or rebuilding a plugin reloads only that plugin without resetting other open tool views.

### Zero-Build Plugins

Link the source directory directly; saving `manifest.toml`, `index.js`, or `ui.html` takes effect immediately:

```bash
xcpc plugin link ./my-tool
```

### Vite Plugins

Run an initial build, link the `dist/` output directory, and start watch mode:

```bash
npm run build
xcpc plugin link ./dist
npm run dev   # equivalent to vite build --watch; updates dist/ on save and triggers desktop hot reload
```

### Approving Sensitive Permissions

If the plugin declares `domains`, `credentials`, `workspace_read = true`, or `workspace_write = true`, trust it once after linking (pure `template` plugins do not require a trust prompt; see [Security Boundaries & Trust Gate](./security.md)):

```bash
xcpc plugin trust my-tool
```

### Headless Quick Testing (`xcpc plugin test`)

Invoke plugin exports directly inside the QuickJS sandbox without installing the plugin or launching the desktop app:

```bash
# Tool plugins default to invoking runTool with arguments [toolId, inputs]
xcpc plugin test ./dist runTool --args '["hello", {"text":"XCPC"}]'

# OJ source plugins default to invoking verify
xcpc plugin test ./my-oj verify --args '{"handle":"tourist"}'
```

### Removing a Development Link

Unlink the plugin when finished (this does not delete your local source directory):

```bash
xcpc plugin unlink my-tool
```

---

## 4. Validation & Packaging (`check` / `pack`)

Before distribution, run `xcpc plugin check` and `xcpc plugin pack` to verify integrity and security rules:

```bash
# Validate manifest.toml, path sandbox, QuickJS export contracts, and ui_entry (supports --json)
# Note: For Vite projects, check the built dist/ directory rather than the source root
xcpc plugin check ./dist

# Validate and package into a clean distribution .zip (outputs <id>-<version>.zip in the parent
# directory by default to avoid triggering hot reload) and print its SHA-256 checksum
xcpc plugin pack ./dist

# Or specify an explicit output path
xcpc plugin pack ./dist -o ./release/my-tool-0.1.0.zip
```

---

## 5. Minimal Working Examples

### Example A: Headless Form Tool (Zero-Build)

Directory layout: `my-calc/{manifest.toml, index.js}`.

`manifest.toml`:

```toml
id = "my-calc"
name = "Modular Arithmetic"
version = "0.1.0"
description = "Fast computation of (a^b) mod m"
entry = "index.js"
capabilities = ["tool"]

[permissions]
domains = []
workspace_read = false

[[tools]]
id = "powmod"
name = "Modular Exponentiation"

[[tools.inputs]]
name = "a"
label = "Base a"
kind = "number"
default = 2
required = true

[[tools.inputs]]
name = "b"
label = "Exponent b"
kind = "number"
default = 10
required = true

[[tools.inputs]]
name = "m"
label = "Modulus m"
kind = "number"
default = 998244353
required = true
```

`index.js` (zero-build plugins are loaded by QuickJS as CommonJS modules via `module.exports`):

```js
function runTool(toolId, inputs) {
  if (toolId !== "powmod") {
    throw new Error(`Unknown tool: ${toolId}`);
  }
  let base = BigInt(inputs.a ?? 0);
  let exp = BigInt(inputs.b ?? 0);
  const mod = BigInt(inputs.m ?? 1);
  if (mod <= 0n || exp < 0n) {
    throw new Error("Requires m > 0 and b >= 0");
  }
  let ans = 1n % mod;
  base = ((base % mod) + mod) % mod;
  while (exp > 0n) {
    if (exp & 1n) ans = (ans * base) % mod;
    base = (base * base) % mod;
    exp >>= 1n;
  }
  return {
    summary: `Result = ${ans}`,
    blocks: [
      { type: "text", title: "Modular Power", content: ans.toString(), monospace: true },
    ],
  };
}

module.exports = { runTool };
```

Validate and test:

```bash
xcpc plugin check ./my-calc
xcpc plugin test ./my-calc runTool --args '["powmod", {"a":2,"b":10,"m":998244353}]'
```

### Example B: Pure-Web `<WebView>` Tool (Vite + `@xcpc-workstation/plugin-sdk`)

This example demonstrates plugin-scoped persistent storage (`storage`), automatic host theme synchronization (`--xcpc-*` CSS variables and `onThemeChange`), file export (`files.saveText`), and toast notifications (`toast`) via `getXcpc()`.

`vite.config.ts`:

```ts
import { defineConfig } from "vite";
import { xcpcPlugin } from "@xcpc-workstation/plugin-sdk/vite";

export default defineConfig({
  plugins: [xcpcPlugin()],
});
```

`manifest.toml`:

```toml
id = "scratchpad"
name = "Scratchpad"
version = "0.1.0"
entry = "index.js"
ui_entry = "ui.html"
capabilities = ["tool"]

[permissions]
domains = []
workspace_read = false

[[tools]]
id = "stats"
name = "Character Count"
```

`src/index.ts` (required when declaring `capabilities = ["tool"]`; compiled to `dist/index.js` by `@xcpc-workstation/plugin-sdk/vite`):

```ts
import type { ToolExecutionResult } from "@xcpc-workstation/plugin-sdk/host";

export function runTool(
  toolId: string,
  inputs: Record<string, unknown>,
): ToolExecutionResult {
  if (toolId !== "stats") {
    throw new Error(`Unknown tool: ${toolId}`);
  }
  const text = String(inputs?.text ?? "");
  return {
    summary: `${text.length} characters`,
    blocks: [{ type: "text", content: `Character count: ${text.length}` }],
  };
}
```

`index.html`:

```html
<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <title>Scratchpad</title>
  </head>
  <body>
    <div id="app"></div>
    <script type="module" src="/src/main.ts"></script>
  </body>
</html>
```

`src/main.ts`:

```ts
import { getXcpc } from "@xcpc-workstation/plugin-sdk/ui";

const xcpc = getXcpc();
const root = document.getElementById("app")!;

root.innerHTML = `
  <main style="padding:16px;font-family:var(--xcpc-font-sans);color:var(--xcpc-fg);background:var(--xcpc-bg);min-height:100vh;">
    <textarea id="note" rows="8" style="width:100%;background:var(--xcpc-card);color:var(--xcpc-card-fg);border:1px solid var(--xcpc-border);border-radius:8px;padding:8px;"></textarea>
    <div style="margin-top:8px;display:flex;gap:8px;align-items:center;">
      <button id="export">Export Text</button>
      <span id="theme-badge"></span>
    </div>
  </main>
`;

const textarea = document.getElementById("note") as HTMLTextAreaElement;
const badge = document.getElementById("theme-badge")!;

// 1. Load and save plugin-scoped persistent draft
void xcpc.storage.get<string>("draft").then((saved) => {
  if (saved) textarea.value = saved;
});
textarea.addEventListener("input", () => {
  void xcpc.storage.set("draft", textarea.value);
});

// 2. React to host light/dark theme changes (--xcpc-* CSS variables sync automatically)
const renderTheme = (scheme: string) => {
  badge.textContent = `Theme: ${scheme}`;
};
renderTheme(xcpc.theme.scheme);
xcpc.onThemeChange((next) => renderTheme(next.scheme));

// 3. Export text via the host file dialog and show a toast notification
document.getElementById("export")!.addEventListener("click", async () => {
  const savedPath = await xcpc.files.saveText("note.txt", textarea.value);
  if (savedPath) {
    xcpc.toast("Draft exported", "success");
  }
});
```

> If you do not need Vite, you can author a standalone `ui.html` directly and access `window.xcpc` inside a `<script>` tag.

### Pure-Web `<WebView>` Host Environment Conventions

When running a `ui_entry = "ui.html"` tool, the desktop host automatically provides the following guarantees so plugins behave consistently across Linux (WebKitGTK) and Windows (WebView2):

- **Embedded fonts & theme variables**: Automatically injects `--xcpc-*` theme variables (`data-theme="light|dark"`) along with inline `Inter` and `JetBrains Mono` `@font-face` declarations (`font-src data:`). If your plugin renders host fonts in Canvas2D or SVG, trigger a redraw after `document.fonts.ready`.
- **Low-specificity base CSS (`:where(...)`)**: Injects zero-specificity `:where(...)` rules for thin `6px` scrollbars, light/dark-aware `<select>` dropdown arrows, and button font inheritance / `white-space: nowrap`, preventing jarring native controls on Windows WebView2 while remaining effortless to override with any custom CSS selector.
- **Full-height canvas layout**: No redundant host header bar is stacked above pure-Web plugins—the plugin's own toolbar acts directly as the primary top bar, and users can collapse the left plugin rail for a wider canvas.
- **Cross-platform clipboard & idle reclamation**: `xcpc.clipboard.writeImage(pngBase64)` writes both native `CF_DIB` bitmap and `"PNG"` formats on Windows so pasting works across editors, chat apps, and drawing tools. To keep resident memory low, idle `<WebView>` instances are automatically unmounted ~15 seconds after leaving the **Tools** space—always persist state you want to keep via `xcpc.storage`.

---

## 6. Publishing to `xcpc-plugins`

1. Run `xcpc plugin pack ./dist` and record the generated `.zip` archive and the SHA-256 checksum printed to the terminal;
2. Upload the `.zip` archive to a GitHub Release (or any static host served over HTTPS);
3. Users can install it directly from the local archive or via `xcpc plugin install <path-or-id>`;
4. To list your plugin in the default official index, open a pull request to [lihaoze123/xcpc-plugins](https://github.com/lihaoze123/xcpc-plugins) adding your plugin's metadata, HTTPS `download_url`, and `sha256` to `registry.toml`. Repository CI automatically fetches the archive, verifies its SHA-256 checksum and security rules, and updates `index.json` upon merge.

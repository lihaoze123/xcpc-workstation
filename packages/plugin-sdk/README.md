# `@xcpc-workstation/plugin-sdk`

TypeScript SDK (QuickJS host contracts + pure-Web `<WebView>` bridge) and zero-config Vite single-file HTML build plugin for [XCPC Workstation](https://github.com/lihaoze123/xcpc-workstation) plugins.

## Installation

```bash
npm install -D @xcpc-workstation/plugin-sdk
```

If you scaffold a project with `xcpc plugin init <dir> --vite`, this dependency is added to `package.json` automatically.

## Subpath Exports

| Subpath | Purpose |
| :--- | :--- |
| `@xcpc-workstation/plugin-sdk/host` | QuickJS sandbox `host` global types (`http`, `credentials`, `html`, `kv`, unified `workspace` read/write, `render.{svgToPng,svgToRgba,renderTypst,renderMath}`, `archive.{listZip,readZip,createZip}`, `crypto.sha256`, `highlight`) and plugin module contracts (`OjSourcePlugin`, `ToolPlugin`, `CommandPlugin`, `ImporterPlugin`, `ExporterPlugin`). |
| `@xcpc-workstation/plugin-sdk/ui` | Typed `window.xcpc` WebView capability bridge (`XcpcBridge`, including `workspace` read/write, `render.{renderTypst,renderMath}`, `files.{saveBinary,pickFiles}`) and `getXcpc()` helper. |
| `@xcpc-workstation/plugin-sdk/vite` | Zero-config `xcpcPlugin(options?)` Vite plugin: bundles WebView UI into a single self-contained IIFE `dist/ui.html`, compiles `src/index.ts` to `dist/index.js` with QuickJS sandbox guardrails, and copies `manifest.toml`. |
| `@xcpc-workstation/plugin-sdk/manifest.schema.json` | JSON Schema for `manifest.toml` editor validation and completion (including `template` plugins, `[[templates]]`, `font_dirs`, and `permissions.workspace_write`). |

## Minimal Vite Configuration

```ts
import { defineConfig } from "vite";
import { xcpcPlugin } from "@xcpc-workstation/plugin-sdk/vite";

export default defineConfig({
  plugins: [xcpcPlugin()],
});
```

## WebView Host Environment

When running a pure-Web `<WebView>` tool (`ui_entry = "ui.html"`), the desktop host automatically injects:

- **Theme tokens & fonts**: `--xcpc-*` CSS custom properties (`data-theme="light|dark"`), plus bundled `Inter` and `JetBrains Mono` `@font-face` declarations via `data:` URIs (for Canvas2D/SVG rendering, trigger a redraw on `document.fonts.ready`).
- **Low-specificity base CSS**: `:where(...)` rules for thin 6px scrollbars, light/dark-aware `<select>` arrows, and button font inheritance / `white-space: nowrap`, keeping Linux WebKitGTK and Windows WebView2 consistent while remaining effortless to override.
- **Cross-platform capabilities & lifecycle**: `xcpc.clipboard.writeImage` writes both native bitmap (`CF_DIB` on Windows) and `PNG`; idle `<WebView>` instances are unmounted ~15s after leaving the Tools view to reclaim memory, so persist user state with `xcpc.storage`.

## Full Documentation

See the complete plugin authoring guide (architecture, sandbox boundaries, CLI workflow, and runnable examples):

- In-repo: [`docs/plugin-development.md`](../../docs/plugin-development.md)
- GitHub: <https://github.com/lihaoze123/xcpc-workstation/blob/main/docs/plugin-development.md>

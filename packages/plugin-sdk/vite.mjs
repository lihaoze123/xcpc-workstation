import fs from "node:fs";
import { createRequire } from "node:module";
import path from "node:path";
import { pathToFileURL } from "node:url";

const VIRTUAL_UI_ID = "\0xcpc-plugin-sdk-ui";

const NODE_BUILTINS = new Set([
  "assert",
  "async_hooks",
  "buffer",
  "child_process",
  "cluster",
  "console",
  "constants",
  "crypto",
  "dgram",
  "diagnostics_channel",
  "dns",
  "domain",
  "events",
  "fs",
  "fs/promises",
  "http",
  "http2",
  "https",
  "inspector",
  "module",
  "net",
  "os",
  "path",
  "perf_hooks",
  "process",
  "punycode",
  "querystring",
  "readline",
  "repl",
  "stream",
  "string_decoder",
  "sys",
  "timers",
  "tls",
  "trace_events",
  "tty",
  "url",
  "util",
  "v8",
  "vm",
  "wasi",
  "worker_threads",
  "zlib",
]);

const NODE_REPLACEMENT_HINTS = {
  fs: "Use `host.kv` for plugin state or `host.workspace` (with `workspace_read = true`) for workspace reads.",
  "fs/promises": "Use `host.kv` for plugin state or `host.workspace` for workspace reads.",
  http: "Use `host.http.fetch(...)` with domains declared in `manifest.toml` `[permissions].domains`.",
  https: "Use `host.http.fetch(...)` with domains declared in `manifest.toml` `[permissions].domains`.",
  child_process: "Subprocess execution is forbidden inside the QuickJS plugin sandbox.",
  net: "Raw sockets are unavailable in QuickJS; use `host.http.fetch(...)`.",
  os: "OS APIs are unavailable inside the QuickJS plugin sandbox.",
};

const FORBIDDEN_DOM_PATTERNS = [
  {
    re: /\b(?:window|document)\s*\./,
    name: "window/document",
    hint: "QuickJS sandbox has no DOM. Move interactive UI to `src/ui.tsx` or use `host.html.select(html, selector)`.",
  },
  {
    re: /\b(?:localStorage|sessionStorage)\b/,
    name: "localStorage/sessionStorage",
    hint: "Use `host.kv.get/set/delete` in `index.js` (or `storage` in `ui.tsx`).",
  },
  {
    re: /\bXMLHttpRequest\b/,
    name: "XMLHttpRequest",
    hint: "Use `host.http.fetch(...)` in `index.js`.",
  },
];

/** Lightweight parser for top-level `id`, `entry`, and `ui_entry` fields in `manifest.toml`. */
export function parseManifestEntries(manifestToml) {
  const result = { id: undefined, entry: "index.js", uiEntry: undefined };
  let inSection = false;
  for (const rawLine of String(manifestToml ?? "").split(/\r?\n/)) {
    const line = rawLine.replace(/#.*$/, "").trim();
    if (!line) continue;
    if (line.startsWith("[")) {
      inSection = true;
      continue;
    }
    if (inSection) continue;
    const m = line.match(/^([a-zA-Z0-9_-]+)\s*=\s*"([^"]*)"\s*$/);
    if (!m) continue;
    const [, key, val] = m;
    if (key === "id") result.id = val;
    else if (key === "entry" && val) result.entry = val;
    else if (key === "ui_entry" && val) result.uiEntry = val;
  }
  return result;
}

function stripCommentsAndStrings(code) {
  return code
    .replace(/\/\*[\s\S]*?\*\//g, " ")
    .replace(/\/\/[^\n]*/g, " ")
    .replace(/(["'`])(?:\\.|(?!\1)[^\\\n])*\1/g, '""');
}

/** Checks a QuickJS sandbox source file for forbidden browser DOM globals. */
export function checkQuickJsForbiddenSource(code, id) {
  const cleaned = stripCommentsAndStrings(code);
  for (const rule of FORBIDDEN_DOM_PATTERNS) {
    if (rule.re.test(cleaned)) {
      return `[xcpc-plugin-sdk] Forbidden QuickJS sandbox global '${rule.name}' used in ${id}. ${rule.hint}`;
    }
  }
  return null;
}

function isForbiddenNodeImport(specifier) {
  if (specifier.startsWith("node:")) return specifier.slice(5);
  const base = specifier.split("/")[0];
  if (NODE_BUILTINS.has(specifier)) return specifier;
  if (NODE_BUILTINS.has(base)) return base;
  return null;
}

function findFirstExisting(root, candidates) {
  for (const rel of candidates) {
    const full = path.resolve(root, rel);
    if (fs.existsSync(full) && fs.statSync(full).isFile()) {
      return full;
    }
  }
  return null;
}

function createVirtualUiModuleCode() {
  return `
function __getSdk() {
  if (typeof sdk !== "undefined" && sdk) return sdk;
  if (globalThis.__xcpc_current_sdk) return globalThis.__xcpc_current_sdk;
  throw new Error("@xcpc/plugin-sdk/ui must run inside XCPC Workstation ui.js");
}
export const getSdk = () => __getSdk();
export const h = (...args) => __getSdk().h(...args);
export const Fragment = (props) => (props ? props.children : null);
export const signal = (...args) => __getSdk().signal(...args);
export const computed = (...args) => __getSdk().computed(...args);
export const effect = (...args) => __getSdk().effect(...args);
export const untracked = (...args) => __getSdk().untracked(...args);
export const token = (...args) => __getSdk().token(...args);
export const colorScheme = (...args) => __getSdk().colorScheme(...args);
export const runTool = (...args) => __getSdk().runTool(...args);
export const useTool = (...args) => __getSdk().useTool(...args);
export const onCleanup = (...args) => __getSdk().onCleanup(...args);
export const onShortcut = (...args) => __getSdk().onShortcut(...args);
export const openTool = (...args) => __getSdk().openTool(...args);
export const pickFile = (...args) => __getSdk().pickFile(...args);
export const readClipboard = (...args) => __getSdk().readClipboard(...args);
export const openExternal = (...args) => __getSdk().openExternal(...args);
export const renderSvgToPng = (...args) => __getSdk().renderSvgToPng(...args);
export const savePng = (...args) => __getSdk().savePng(...args);
export const saveText = (...args) => __getSdk().saveText(...args);
export const copyText = (...args) => __getSdk().copyText(...args);
export const toast = (...args) => __getSdk().toast(...args);
export const initialPayload = (...args) => __getSdk().initialPayload(...args);
export const ui = new Proxy({}, { get(_t, p) { return __getSdk().ui[p]; } });
export const canvas = new Proxy({}, { get(_t, p) { return __getSdk().canvas[p]; } });
export const storage = new Proxy({}, { get(_t, p) { return __getSdk().storage[p]; } });
export const workspace = new Proxy({}, { get(_t, p) { return __getSdk().workspace[p]; } });
`;
}

function createSandboxGuardPlugin(enabled) {
  return {
    name: "xcpc:sandbox-guard",
    enforce: "pre",
    resolveId(source, importer) {
      if (!enabled) return null;
      const forbidden = isForbiddenNodeImport(source);
      if (forbidden) {
        const hint =
          NODE_REPLACEMENT_HINTS[forbidden] ??
          "Node.js built-in modules are unavailable in the QuickJS plugin sandbox.";
        this.error(
          `[xcpc-plugin-sdk] Forbidden Node.js module '${source}' imported${importer ? ` by ${importer}` : ""}. ${hint}`,
        );
      }
      return null;
    },
    transform(code, id) {
      if (!enabled) return null;
      if (id.includes("node_modules") || id.startsWith("\0")) return null;
      if (!/\.(?:[cm]?[jt]s)$/.test(id)) return null;
      const err = checkQuickJsForbiddenSource(code, id);
      if (err) {
        this.error(err);
      }
      return null;
    },
  };
}

function createUiVirtualSdkPlugin() {
  return {
    name: "xcpc:ui-virtual-sdk",
    enforce: "pre",
    resolveId(source) {
      if (source === "@xcpc/plugin-sdk/ui") {
        return VIRTUAL_UI_ID;
      }
      return null;
    },
    load(id) {
      if (id === VIRTUAL_UI_ID) {
        return createVirtualUiModuleCode();
      }
      return null;
    },
  };
}

/**
 * Zero-config Vite plugin for XCPC Workstation plugins.
 * - Reads `manifest.toml` to infer `index.js` and optional `ui.js` outputs.
 * - Bundles `src/index.{ts,js}` to a single CJS file in `dist/` with QuickJS sandbox guardrails.
 * - Bundles `src/ui.{tsx,ts,jsx,js}` (when present) to a single CJS file in `dist/` with virtual `@xcpc/plugin-sdk/ui`.
 * - Copies `manifest.toml` into `dist/` after each build (and watches `src/ui.*` + `manifest.toml` in `--watch` mode).
 */
export function xcpcPlugin(options = {}) {
  const manifestRel = options.manifest ?? "manifest.toml";
  const outDirRel = options.outDir ?? "dist";
  const sandboxGuard = options.sandboxGuard ?? true;

  let projectRoot = process.cwd();
  let manifestPath = "";
  let manifestMeta = { id: undefined, entry: "index.js", uiEntry: undefined };
  let indexInput = null;
  let uiInput = null;
  let uiOutFile = "ui.js";

  const mainPlugin = {
    name: "xcpc:plugin",
    config(userConfig) {
      projectRoot = path.resolve(userConfig.root ?? process.cwd());
      manifestPath = path.resolve(projectRoot, manifestRel);
      if (fs.existsSync(manifestPath)) {
        manifestMeta = parseManifestEntries(fs.readFileSync(manifestPath, "utf8"));
      }

      indexInput = options.entry
        ? path.resolve(projectRoot, options.entry)
        : findFirstExisting(projectRoot, [
            "src/index.ts",
            "src/index.js",
            "index.ts",
            "index.js",
          ]);

      if (options.uiEntry === false) {
        uiInput = null;
      } else if (typeof options.uiEntry === "string") {
        uiInput = path.resolve(projectRoot, options.uiEntry);
      } else {
        uiInput = findFirstExisting(projectRoot, [
          "src/ui.tsx",
          "src/ui.ts",
          "src/ui.jsx",
          "src/ui.js",
        ]);
      }
      uiOutFile = manifestMeta.uiEntry || "ui.js";

      if (!indexInput) {
        throw new Error(
          `[xcpc-plugin-sdk] Could not locate plugin entry (expected src/index.ts or src/index.js in ${projectRoot})`,
        );
      }

      return {
        build: {
          outDir: outDirRel,
          emptyOutDir: true,
          target: "es2022",
          minify: false,
          sourcemap: false,
          lib: {
            entry: indexInput,
            formats: ["cjs"],
            fileName: () => manifestMeta.entry || "index.js",
          },
          rollupOptions: {
            output: {
              inlineDynamicImports: true,
              exports: "named",
            },
          },
        },
      };
    },

    buildStart() {
      if (manifestPath && fs.existsSync(manifestPath)) {
        this.addWatchFile(manifestPath);
      }
      if (uiInput && fs.existsSync(uiInput)) {
        this.addWatchFile(uiInput);
      }
    },

    async writeBundle() {
      const outDirAbs = path.resolve(projectRoot, outDirRel);
      fs.mkdirSync(outDirAbs, { recursive: true });

      if (uiInput && fs.existsSync(uiInput)) {
        const req = createRequire(path.join(projectRoot, "package.json"));
        let viteMod;
        try {
          const viteEntry = req.resolve("vite");
          viteMod = await import(pathToFileURL(viteEntry).href);
        } catch {
          viteMod = await import("vite");
        }
        const viteBuild = viteMod.build;
        await viteBuild({
          root: projectRoot,
          configFile: false,
          logLevel: "silent",
          esbuild: {
            jsx: "transform",
            jsxFactory: "h",
            jsxFragment: "Fragment",
          },
          plugins: [createUiVirtualSdkPlugin()],
          build: {
            outDir: outDirRel,
            emptyOutDir: false,
            target: "es2022",
            minify: false,
            sourcemap: false,
            lib: {
              entry: uiInput,
              formats: ["cjs"],
              fileName: () => uiOutFile,
            },
            rollupOptions: {
              output: {
                inlineDynamicImports: true,
                exports: "named",
              },
            },
          },
        });
      }

      if (manifestPath && fs.existsSync(manifestPath)) {
        fs.copyFileSync(manifestPath, path.join(outDirAbs, "manifest.toml"));
      }
    },
  };

  return [createSandboxGuardPlugin(sandboxGuard), mainPlugin];
}

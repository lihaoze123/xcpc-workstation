import fs from "node:fs";
import { createRequire } from "node:module";
import path from "node:path";
import { pathToFileURL } from "node:url";

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
    hint: "QuickJS sandbox has no DOM. Move interactive UI to `index.html` / `src/main.ts` or use `host.html.select(html, selector)`.",
  },
  {
    re: /\b(?:localStorage|sessionStorage)\b/,
    name: "localStorage/sessionStorage",
    hint: "Use `host.kv.get/set/delete` in `index.js` (or `window.xcpc.storage` in WebView UI).",
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

function escapeRegExp(str) {
  return str.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

/**
 * Inlines bundled CSS assets and IIFE JS chunks into a single standalone HTML string.
 * Safely escapes `</script` as `<\/script` and `</style` as `<\/style`.
 */
export function inlineHtmlBundle(html, { jsChunks = [], cssAssets = [] } = {}) {
  let out = String(html ?? "");

  // Remove modulepreload links emitted by Vite.
  out = out.replace(/<link\b[^>]*\brel=["']modulepreload["'][^>]*>\s*/gi, "");

  for (const css of cssAssets) {
    const escapedCss = String(css.source ?? "").replace(/<\/style/gi, "<\\/style");
    const styleTag = `<style>\n${escapedCss}\n</style>`;
    const escapedName = escapeRegExp(css.fileName);
    const linkRe = new RegExp(
      `<link\\b[^>]*\\bhref=["'][^"']*${escapedName}["'][^>]*>`,
      "i",
    );
    if (linkRe.test(out)) {
      out = out.replace(linkRe, () => styleTag);
    } else if (/<\/head>/i.test(out)) {
      out = out.replace(/<\/head>/i, () => `${styleTag}\n</head>`);
    } else {
      out = `${styleTag}\n${out}`;
    }
  }

  for (const chunk of jsChunks) {
    const escapedJs = String(chunk.code ?? "").replace(/<\/script/gi, "<\\/script");
    const scriptTag = `<script>\n${escapedJs}\n</script>`;
    const escapedName = escapeRegExp(chunk.fileName);
    const scriptRe = new RegExp(
      `<script\\b[^>]*\\bsrc=["'][^"']*${escapedName}["'][^>]*>\\s*<\\/script>`,
      "i",
    );
    if (scriptRe.test(out)) {
      out = out.replace(scriptRe, () => scriptTag);
    } else if (/<\/body>/i.test(out)) {
      out = out.replace(/<\/body>/i, () => `${scriptTag}\n</body>`);
    } else {
      out = `${out}\n${scriptTag}`;
    }
  }

  return out;
}

function createSandboxGuardPlugin(isEnabled) {
  return {
    name: "xcpc:sandbox-guard",
    enforce: "pre",
    resolveId(source, importer) {
      if (!isEnabled()) return null;
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
      if (!isEnabled()) return null;
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

/**
 * Zero-config Vite plugin for XCPC Workstation plugins.
 * - When `index.html` (or `ui.html`) exists: builds a single-file IIFE HTML bundle (`dist/ui.html`)
 *   with CSS, JS, and assets inlined (safely escaping `</script>`).
 * - Also builds `src/index.{ts,js}` -> `dist/index.js` (CJS) with QuickJS sandbox guardrails when present.
 * - Copies `manifest.toml` into `dist/` after each build and works with both `vite build` and `vite build --watch`.
 */
export function xcpcPlugin(options = {}) {
  const manifestRel = options.manifest ?? "manifest.toml";
  const outDirRel = options.outDir ?? "dist";
  const sandboxGuard = options.sandboxGuard ?? true;

  let projectRoot = process.cwd();
  let manifestPath = "";
  let manifestMeta = { id: undefined, entry: "index.js", uiEntry: undefined };
  let indexInput = null;
  let htmlInput = null;
  let uiOutFile = "ui.html";
  let isHtmlPrimaryBuild = false;

  const guardPlugin = createSandboxGuardPlugin(
    () => sandboxGuard && !isHtmlPrimaryBuild,
  );

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
        htmlInput = null;
      } else if (typeof options.uiEntry === "string") {
        htmlInput = path.resolve(projectRoot, options.uiEntry);
      } else {
        htmlInput = findFirstExisting(projectRoot, ["index.html", "ui.html"]);
      }

      uiOutFile = manifestMeta.uiEntry || "ui.html";
      isHtmlPrimaryBuild = Boolean(htmlInput);

      if (isHtmlPrimaryBuild) {
        return {
          base: "./",
          build: {
            outDir: outDirRel,
            emptyOutDir: true,
            target: "es2022",
            cssCodeSplit: false,
            assetsInlineLimit: Infinity,
            sourcemap: false,
            rollupOptions: {
              input: htmlInput,
              output: {
                format: "iife",
                inlineDynamicImports: true,
              },
            },
          },
        };
      }

      if (!indexInput) {
        throw new Error(
          `[xcpc-plugin-sdk] Could not locate plugin entry (expected index.html, ui.html, src/index.ts, or src/index.js in ${projectRoot})`,
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
      if (isHtmlPrimaryBuild && indexInput && fs.existsSync(indexInput)) {
        this.addWatchFile(indexInput);
      }
    },

    async writeBundle() {
      const outDirAbs = path.resolve(projectRoot, outDirRel);
      fs.mkdirSync(outDirAbs, { recursive: true });

      if (isHtmlPrimaryBuild && indexInput && fs.existsSync(indexInput)) {
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
          plugins: [createSandboxGuardPlugin(() => sandboxGuard)],
          build: {
            outDir: outDirRel,
            emptyOutDir: false,
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
        });
      }

      if (manifestPath && fs.existsSync(manifestPath)) {
        fs.copyFileSync(manifestPath, path.join(outDirAbs, "manifest.toml"));
      }
    },
  };

  const inlineHtmlPlugin = {
    name: "xcpc:inline-html",
    enforce: "post",
    generateBundle(_opts, bundle) {
      if (!isHtmlPrimaryBuild) return;

      const htmlKeys = Object.keys(bundle).filter(
        (key) => bundle[key].type === "asset" && key.endsWith(".html"),
      );
      if (htmlKeys.length === 0) return;

      const htmlKey = htmlKeys[0];
      const htmlAsset = bundle[htmlKey];
      const jsChunks = [];
      const cssAssets = [];
      const toDelete = [];

      for (const [key, item] of Object.entries(bundle)) {
        if (item.type === "chunk" && key.endsWith(".js")) {
          jsChunks.push({ fileName: item.fileName, code: item.code });
          toDelete.push(key);
        } else if (item.type === "asset" && key.endsWith(".css")) {
          const source =
            typeof item.source === "string"
              ? item.source
              : Buffer.from(item.source).toString("utf8");
          cssAssets.push({ fileName: item.fileName, source });
          toDelete.push(key);
        }
      }

      const rawHtml =
        typeof htmlAsset.source === "string"
          ? htmlAsset.source
          : Buffer.from(htmlAsset.source).toString("utf8");
      const inlined = inlineHtmlBundle(rawHtml, { jsChunks, cssAssets });

      for (const key of toDelete) {
        delete bundle[key];
      }

      if (htmlKey !== uiOutFile) {
        delete bundle[htmlKey];
      }
      htmlAsset.fileName = uiOutFile;
      htmlAsset.source = inlined;
      bundle[uiOutFile] = htmlAsset;
    },
  };

  return [guardPlugin, mainPlugin, inlineHtmlPlugin];
}

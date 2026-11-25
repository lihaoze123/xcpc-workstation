// @xcpc/plugin-sdk/vite — Zero-config Vite plugin for XCPC Workstation plugins

export interface XcpcVitePluginOptions {
  /** Path to manifest.toml relative to project root (default: "manifest.toml"). */
  manifest?: string;
  /** Output directory relative to project root (default: "dist"). */
  outDir?: string;
  /** Override QuickJS sandbox entry path (default: auto-detected src/index.{ts,js}). */
  entry?: string;
  /** Override WebView HTML entry path (default: auto-detected index.html or ui.html; pass false to disable). */
  uiEntry?: string | false;
  /** Whether to enforce QuickJS sandbox import/global guardrails on index.js (default: true). */
  sandboxGuard?: boolean;
}

export interface XcpcVitePluginInstance {
  name: string;
  enforce?: "pre" | "post";
  [key: string]: unknown;
}

export interface InlineHtmlBundleInput {
  jsChunks?: Array<{ fileName: string; code: string }>;
  cssAssets?: Array<{ fileName: string; source: string }>;
}

export declare function xcpcPlugin(
  options?: XcpcVitePluginOptions,
): XcpcVitePluginInstance[];

export declare function parseManifestEntries(manifestToml: string): {
  id?: string;
  entry: string;
  uiEntry?: string;
};

export declare function checkQuickJsForbiddenSource(
  code: string,
  id: string,
): string | null;

export declare function inlineHtmlBundle(
  html: string,
  assets?: InlineHtmlBundleInput,
): string;

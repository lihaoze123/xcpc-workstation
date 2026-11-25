// @xcpc/plugin-sdk/vite — Zero-config Vite plugin for XCPC Workstation plugins

export interface XcpcVitePluginOptions {
  /** Path to manifest.toml relative to project root (default: "manifest.toml"). */
  manifest?: string;
  /** Output directory relative to project root (default: "dist"). */
  outDir?: string;
  /** Override sandbox entry path (default: auto-detected src/index.{ts,js}). */
  entry?: string;
  /** Override desktop UI entry path (default: auto-detected src/ui.{tsx,ts,jsx,js}). */
  uiEntry?: string | false;
  /** Whether to enforce QuickJS sandbox import/global guardrails (default: true). */
  sandboxGuard?: boolean;
}

export interface XcpcVitePluginInstance {
  name: string;
  enforce?: "pre" | "post";
  [key: string]: any;
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

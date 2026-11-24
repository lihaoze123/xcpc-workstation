// Shared TypeScript contracts and pure helpers for Phase 4C Toolbox, Plugin SDK,
// Commands, Importers, and Exporters (mirroring `crates/xcpc-plugin`).

export interface ToolInputSpec {
  name: string;
  label: string;
  kind: string;
  default?: unknown;
  options?: string[];
  required?: boolean;
}

export interface ToolSpec {
  id: string;
  name: string;
  description?: string | null;
  inputs: ToolInputSpec[];
}

export interface PluginToolGroup {
  plugin_id: string;
  plugin_name: string;
  plugin_description: string;
  has_ui: boolean;
  ui_entry?: string | null;
  tools: ToolSpec[];
  is_builtin: boolean;
}

export interface GraphNode {
  id: string;
  label?: string | null;
  x?: number | null;
  y?: number | null;
  color?: string | null;
}

export interface GraphEdge {
  from: string;
  to: string;
  weight?: string | null;
  label?: string | null;
  directed?: boolean | null;
}

export type ToolOutputBlock =
  | {
      type: "text";
      title?: string | null;
      content: string;
      monospace?: boolean | null;
    }
  | {
      type: "markdown";
      title?: string | null;
      content: string;
    }
  | {
      type: "table";
      title?: string | null;
      headers: string[];
      rows: string[][];
      align?: string[] | null;
    }
  | {
      type: "graph";
      title?: string | null;
      directed?: boolean | null;
      weighted?: boolean | null;
      one_indexed?: boolean | null;
      nodes: GraphNode[];
      edges: GraphEdge[];
      layout?: string | null;
    }
  | {
      type: "svg";
      title?: string | null;
      svg: string;
      width?: number | null;
      height?: number | null;
    }
  | {
      type: "image";
      title?: string | null;
      png_base64: string;
      width?: number | null;
      height?: number | null;
    };

export interface ToolExecutionResult {
  blocks: ToolOutputBlock[];
  summary?: string | null;
}

export interface CommandSpec {
  id: string;
  title: string;
  context: "global" | "problem" | "contest";
}

export interface RegisteredPluginCommand {
  plugin_id: string;
  plugin_name: string;
  command: CommandSpec;
}

export type PluginCommandAction =
  | { type: "message"; text: string }
  | {
      type: "open_tool";
      plugin_id: string;
      tool_id?: string | null;
      initial_inputs?: unknown;
    }
  | { type: "tool_result"; result: ToolExecutionResult }
  | { type: "none" };

export interface ImporterSpec {
  id: string;
  name: string;
  extensions: string[];
}

export interface RegisteredImporter {
  plugin_id: string;
  plugin_name: string;
  importer: ImporterSpec;
}

export interface ImportAppliedReport {
  kind: string;
  created_ids: string[];
  skipped_ids: string[];
}

export interface ExporterSpec {
  id: string;
  name: string;
  extension: string;
}

export interface RegisteredExporter {
  plugin_id: string;
  plugin_name: string;
  exporter: ExporterSpec;
}

export interface PluginExportReport {
  summary?: string | null;
  out_dir: string;
  files: string[];
}

/** Parses a `JsonReportResultJs` payload safely, returning `fallback` on failure. */
export function parseJsonReport<T>(
  res: { ok: boolean; reportJson?: string | null } | undefined,
  fallback: T,
): T {
  if (!res?.ok || !res.reportJson) return fallback;
  try {
    return JSON.parse(res.reportJson) as T;
  } catch {
    return fallback;
  }
}

/** Builds initial form state from a `ToolSpec`'s declared defaults. */
export function defaultInputsForTool(tool: ToolSpec): Record<string, unknown> {
  const out: Record<string, unknown> = {};
  for (const field of tool.inputs ?? []) {
    if (field.default !== undefined && field.default !== null) {
      out[field.name] = field.default;
    } else if (field.kind === "boolean") {
      out[field.name] = false;
    } else if (field.kind === "number") {
      out[field.name] = 0;
    } else if (field.kind === "select") {
      out[field.name] = field.options?.[0] ?? "";
    } else {
      out[field.name] = "";
    }
  }
  return out;
}

/** Coerces UI form values into the JSON types expected by the plugin's `runTool`. */
export function coerceToolInputs(
  tool: ToolSpec,
  raw: Record<string, unknown>,
): Record<string, unknown> {
  const out: Record<string, unknown> = {};
  for (const field of tool.inputs ?? []) {
    const val = raw[field.name];
    if (field.kind === "number") {
      const n = typeof val === "number" ? val : Number(String(val ?? "").trim());
      out[field.name] = Number.isFinite(n) ? n : Number(field.default ?? 0);
    } else if (field.kind === "boolean") {
      out[field.name] = Boolean(val);
    } else {
      out[field.name] = val == null ? "" : String(val);
    }
  }
  return out;
}

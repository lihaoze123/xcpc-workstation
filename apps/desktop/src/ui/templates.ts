// Client-side types and helpers for declarative Typst template schemas (`#let schema = (...)`)
// and three-tier namespaced template options (`workspace.toml` -> `contest.toml` -> `problem.toml` / `[[problem]]`).

import type { LocalizedText } from "./i18n";
import { preview } from "./store";

export type TemplateOptionValue = boolean | number | string | string[];
export type TemplateOptionMap = Record<string, TemplateOptionValue>;
export type TemplateOptionsByTemplate = Record<string, TemplateOptionMap>;

export type TemplateFieldType = "bool" | "enum" | "string" | "lines" | "number" | "length" | "color" | "font" | "image";
export type TemplateFieldScope = "book" | "problem";

/** Namespace key used in `contest.toml` when a contest has its own `contests/<id>/template.typ`. */
export const LOCAL_TEMPLATE_NAMESPACE = "local";

export interface TemplateChoice {
  value: string;
  label: LocalizedText;
}

export interface TemplateGroup {
  id: string;
  label: LocalizedText;
}

export interface TemplateField {
  key: string;
  label: LocalizedText;
  description?: LocalizedText;
  type: TemplateFieldType;
  scope: TemplateFieldScope;
  group?: string;
  default: TemplateOptionValue;
  choices?: TemplateChoice[];
  presets?: string[];
  min?: number;
  max?: number;
  step?: number;
  placeholder?: LocalizedText;
  when?: Record<string, TemplateOptionValue>;
}

export interface TemplateSchema {
  name?: LocalizedText;
  description?: LocalizedText;
  groups: TemplateGroup[];
  fields: TemplateField[];
}

export type TemplateSource =
  | { kind: "workspace" }
  | { kind: "local" }
  | { kind: "plugin"; plugin_id: string; plugin_name: string; template_id: string };

export interface TemplateInfo {
  id: string;
  path: string;
  name: LocalizedText;
  description?: LocalizedText;
  source?: TemplateSource;
  schema: TemplateSchema;
}

/** Parses `"plugin:<pluginId>/<templateId>"` into its components, or returns `null`. */
export function parsePluginTemplateRef(id: string): { pluginId: string; templateId: string } | null {
  const trimmed = id.trim();
  if (!trimmed.startsWith("plugin:")) return null;
  const rest = trimmed.slice("plugin:".length);
  const slash = rest.indexOf("/");
  if (slash <= 0 || slash !== rest.lastIndexOf("/") || slash >= rest.length - 1) return null;
  return {
    pluginId: rest.slice(0, slash),
    templateId: rest.slice(slash + 1),
  };
}

export interface PresetInfo {
  id: string;
  name: LocalizedText;
  description: LocalizedText;
}

export interface TemplatesCatalog {
  default: string;
  workspaceOptions: TemplateOptionsByTemplate;
  templates: TemplateInfo[];
  presets: PresetInfo[];
  fonts: string[];
}

const EMPTY_CATALOG: TemplatesCatalog = {
  default: "olymp-classic",
  workspaceOptions: {},
  templates: [],
  presets: [],
  fonts: [],
};

/** Loads all workspace templates (`templates/*.typ`), installable preset seeds, workspace defaults, and loaded fonts. */
export function loadTemplatesCatalog(): TemplatesCatalog {
  try {
    const raw = preview.listTemplatesJson();
    const parsed = JSON.parse(raw) as Partial<TemplatesCatalog>;
    return {
      default: typeof parsed.default === "string" && parsed.default ? parsed.default : "olymp-classic",
      workspaceOptions: parsed.workspaceOptions && typeof parsed.workspaceOptions === "object" ? parsed.workspaceOptions : {},
      templates: Array.isArray(parsed.templates) ? parsed.templates : [],
      presets: Array.isArray(parsed.presets) ? parsed.presets : [],
      fonts: Array.isArray(parsed.fonts) ? parsed.fonts : [],
    };
  } catch {
    return EMPTY_CATALOG;
  }
}

/** Parses a JSON-encoded `TemplateInfo` for a contest-local `contests/<id>/template.typ`. */
export function parseContestTemplateInfo(json: string | null | undefined): TemplateInfo | undefined {
  if (!json) return undefined;
  try {
    const parsed = JSON.parse(json);
    return parsed && typeof parsed === "object" ? (parsed as TemplateInfo) : undefined;
  } catch {
    return undefined;
  }
}

/** Parses a JSON-encoded `TemplateOptionsByTemplate` string from the native bridge. */
export function parseOptionsByTemplate(json: string | undefined): TemplateOptionsByTemplate {
  if (!json) return {};
  try {
    const parsed = JSON.parse(json);
    return parsed && typeof parsed === "object" ? (parsed as TemplateOptionsByTemplate) : {};
  } catch {
    return {};
  }
}

/** Deep equality for `TemplateOptionValue` (including `string[]` lines). */
export function optionValuesEqual(a: TemplateOptionValue | undefined, b: TemplateOptionValue | undefined): boolean {
  if (a === b) return true;
  if (Array.isArray(a) && Array.isArray(b)) {
    if (a.length !== b.length) return false;
    for (let i = 0; i < a.length; i++) {
      if (a[i] !== b[i]) return false;
    }
    return true;
  }
  return false;
}

/** Evaluates a field's `when` condition map against resolved values + general book switches. */
export function isFieldVisible(
  field: TemplateField,
  resolved: TemplateOptionMap,
  context?: Record<string, TemplateOptionValue>,
): boolean {
  const when = field.when;
  if (!when) return true;
  for (const [k, expected] of Object.entries(when)) {
    const actual = k in resolved ? resolved[k] : context?.[k];
    if (!optionValuesEqual(actual, expected)) return false;
  }
  return true;
}

/** Encodes a single template option update into the JSON patch expected by the native bridge. */
export function encodeSingleOptionPatch(templateId: string, key: string, value: TemplateOptionValue | null): string {
  return JSON.stringify({ [templateId]: { [key]: value } });
}

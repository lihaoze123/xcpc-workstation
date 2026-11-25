// Pure UI logic for Snippets and Living TRDs (no NSH / native imports so unit tests run headless).

import type {
  SnippetSummaryItem,
  SnippetVerifyStatus,
  TrdChapter,
  TrdPreflightIssue,
  TrdSummaryItem,
} from "./types";

export type SnippetStatusFilter = "all" | SnippetVerifyStatus;

/** Filters snippets by search text (title, slug, tags, description), verify status filter, and optional tag. */
export function filterSnippets(
  list: readonly SnippetSummaryItem[],
  opts: {
    query?: string;
    statusFilter?: SnippetStatusFilter;
    tag?: string;
  },
): SnippetSummaryItem[] {
  const q = (opts.query ?? "").trim().toLowerCase();
  const sf = opts.statusFilter ?? "all";
  const tag = opts.tag?.trim() || "";

  return list.filter((s) => {
    if (sf !== "all") {
      if (!s.has_code || s.status !== sf) return false;
    }
    const tags = s.tags ?? [];
    if (tag && !tags.includes(tag)) return false;
    if (!q) return true;
    return (
      s.slug.toLowerCase().includes(q) ||
      s.title.toLowerCase().includes(q) ||
      tags.some((tg) => tg.toLowerCase().includes(q)) ||
      (s.description ?? "").toLowerCase().includes(q)
    );
  });
}

/** Splits TRDs into active and archived lists, preserving order. */
export function partitionTrds(list: readonly TrdSummaryItem[]): {
  active: TrdSummaryItem[];
  archived: TrdSummaryItem[];
} {
  const active: TrdSummaryItem[] = [];
  const archived: TrdSummaryItem[] = [];
  for (const item of list) {
    if (item.archived) archived.push(item);
    else active.push(item);
  }
  return { active, archived };
}

/** Collects unique missing dependency slugs reported by `missing_dependency` preflight issues. */
export function collectMissingDeps(issues: readonly TrdPreflightIssue[]): string[] {
  const seen = new Set<string>();
  const out: string[] = [];
  for (const issue of issues) {
    if (issue.kind !== "missing_dependency") continue;
    for (const dep of issue.missing_deps) {
      if (dep && !seen.has(dep)) {
        seen.add(dep);
        out.push(dep);
      }
    }
  }
  return out;
}

/**
 * Inserts missing upstream dependencies into `chapters` immediately before the first
 * snippet that depends on them, avoiding duplicates. Pure function returning a new chapter array.
 */
export function fillMissingDepsInChapters(
  chapters: readonly TrdChapter[],
  issues: readonly TrdPreflightIssue[],
): TrdChapter[] {
  const missingIssues = issues.filter((i) => i.kind === "missing_dependency" && i.missing_deps.length > 0);
  if (missingIssues.length === 0) return chapters.map((c) => ({ title: c.title, items: [...c.items] }));

  const present = new Set<string>();
  for (const ch of chapters) {
    for (const slug of ch.items) present.add(slug);
  }

  const next = chapters.map((c) => ({ title: c.title, items: [...c.items] }));
  for (const issue of missingIssues) {
    const toInsert = issue.missing_deps.filter((d) => d && !present.has(d));
    if (toInsert.length === 0) continue;

    let placed = false;
    for (const ch of next) {
      const idx = ch.items.indexOf(issue.snippet);
      if (idx >= 0) {
        ch.items.splice(idx, 0, ...toInsert);
        for (const d of toInsert) present.add(d);
        placed = true;
        break;
      }
    }
    if (!placed && next.length > 0) {
      next[0].items.unshift(...toInsert);
      for (const d of toInsert) present.add(d);
    }
  }
  return next;
}

/** Extracts a fenced code block from Markdown. When `cursorOffset` is provided, only returns the block enclosing the cursor. */
export function extractCodeBlockFromMarkdown(markdown: string, cursorOffset?: number): string | undefined {
  const re = /```[^\n]*\n([\s\S]*?)```/g;
  const blocks: { start: number; end: number; code: string }[] = [];
  let match: RegExpExecArray | null;
  while ((match = re.exec(markdown)) !== null) {
    blocks.push({
      start: match.index,
      end: match.index + match[0].length,
      code: match[1].trimEnd(),
    });
  }
  if (blocks.length === 0) return undefined;
  if (cursorOffset !== undefined) {
    const inside = blocks.find((b) => cursorOffset >= b.start && cursorOffset <= b.end);
    return inside?.code;
  }
  return blocks[0].code;
}

/** Picks the preview book target for a single snippet: `"trd:<id>"` when referenced by a TRD (so the
 * Core inherits that TRD's template AND options), otherwise falls back to `defaultTemplate`. */
export function resolveSnippetPreviewTemplate(
  slug: string,
  trds: readonly { id?: string; template?: string | null; chapters?: readonly TrdChapter[] }[],
  defaultTemplate = "trd-compact",
): string {
  for (const trd of trds) {
    const has = (trd.chapters ?? []).some((ch) => (ch.items ?? []).includes(slug));
    if (has) {
      if (trd.id && trd.id.trim()) return `trd:${trd.id.trim()}`;
      if (trd.template && trd.template.trim()) return trd.template.trim();
    }
  }
  return defaultTemplate;
}

/** Encodes an optional string value into serde's canonical `Patch<String>` JSON representation. */
export function encodeStringPatch(value: string | null | undefined): "Clear" | { Set: string } {
  const trimmed = (value ?? "").trim();
  return trimmed ? { Set: trimmed } : "Clear";
}

/** Encodes a single template option update into serde's canonical `TemplateOptionsPatch` JSON map. */
export function encodeTrdOptionPatch(
  templateId: string,
  key: string,
  value: unknown,
): Record<string, Record<string, "Clear" | { Set: unknown }>> {
  return {
    [templateId]: {
      [key]: value === null || value === undefined ? "Clear" : { Set: value },
    },
  };
}

/** Generates a clean ASCII slug suggestion from a title or filename. */
export function suggestSnippetSlug(input: string): string {
  const cleaned = input
    .trim()
    .toLowerCase()
    .replace(/\.[a-z0-9]+$/i, "")
    .replace(/[^a-z0-9_-]+/g, "-")
    .replace(/^-+|-+$/g, "");
  return cleaned || "snippet";
}

/** Returns badge visual tokens for a snippet verification status (or pure formula card). */
export function snippetStatusStyle(
  status: SnippetVerifyStatus,
  hasCode: boolean,
): { bg: string; fg: string; labelKey: string } {
  if (!hasCode) {
    return { bg: "rgba(113,113,122,0.12)", fg: "#71717A", labelKey: "snippets.status.formula" };
  }
  switch (status) {
    case "passed":
      return { bg: "rgba(16,185,129,0.12)", fg: "#10B981", labelKey: "snippets.status.passed" };
    case "failed":
      return { bg: "rgba(244,63,94,0.12)", fg: "#F43F5E", labelKey: "snippets.status.failed" };
    case "stale":
      return { bg: "rgba(245,158,11,0.12)", fg: "#F59E0B", labelKey: "snippets.status.stale" };
    default:
      return { bg: "rgba(113,113,122,0.10)", fg: "#71717A", labelKey: "snippets.status.unverified" };
  }
}

/** Helper to safely parse a `JsonReportResultJs` payload. */
export function parseJsonPayload<T>(res: { ok: boolean; reportJson?: string | null } | undefined): T | undefined {
  if (!res || !res.ok || !res.reportJson) return undefined;
  try {
    return JSON.parse(res.reportJson) as T;
  } catch {
    return undefined;
  }
}

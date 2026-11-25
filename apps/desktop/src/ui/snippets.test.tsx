import { describe, expect, test } from "nsh/test";
import { shortcutForSpace, visibleSpaces } from "./logic";
import {
  collectMissingDeps,
  encodeStringPatch,
  encodeTrdOptionPatch,
  extractCodeBlockFromMarkdown,
  fillMissingDepsInChapters,
  filterSnippets,
  partitionTrds,
  resolveSnippetPreviewTemplate,
  snippetStatusStyle,
  suggestSnippetSlug,
} from "./snippets/snippetLogic";
import type { SnippetSummaryItem, TrdChapter, TrdPreflightIssue, TrdSummaryItem } from "./snippets/types";

describe("Phase 5 NavRail dynamic shortcuts", () => {
  test("assigns mod+1..6 when workspace, snippets, and training are all enabled", () => {
    const opts = { hasWorkspace: true, snippetsEnabled: true, trainingEnabled: true };
    expect(visibleSpaces(opts)).toEqual(["problems", "contests", "snippets", "training", "tools", "settings"]);
    expect(shortcutForSpace("problems", opts)).toBe("mod+1");
    expect(shortcutForSpace("contests", opts)).toBe("mod+2");
    expect(shortcutForSpace("snippets", opts)).toBe("mod+3");
    expect(shortcutForSpace("training", opts)).toBe("mod+4");
    expect(shortcutForSpace("tools", opts)).toBe("mod+5");
    expect(shortcutForSpace("settings", opts)).toBe("mod+6");
  });

  test("hides snippets and shifts shortcuts cleanly when snippets = false", () => {
    const opts = { hasWorkspace: true, snippetsEnabled: false, trainingEnabled: true };
    expect(visibleSpaces(opts)).toEqual(["problems", "contests", "training", "tools", "settings"]);
    expect(shortcutForSpace("snippets", opts)).toBeUndefined();
    expect(shortcutForSpace("training", opts)).toBe("mod+3");
    expect(shortcutForSpace("tools", opts)).toBe("mod+4");
    expect(shortcutForSpace("settings", opts)).toBe("mod+5");
  });

  test("shows only tools and settings when no workspace is open", () => {
    const opts = { hasWorkspace: false, snippetsEnabled: true, trainingEnabled: true };
    expect(visibleSpaces(opts)).toEqual(["tools", "settings"]);
    expect(shortcutForSpace("tools", opts)).toBe("mod+1");
    expect(shortcutForSpace("settings", opts)).toBe("mod+2");
  });
});

describe("Phase 5 Snippet & TRD pure logic", () => {
  const sampleSnippets: SnippetSummaryItem[] = [
    {
      slug: "modint",
      title: "ModInt",
      tags: ["math"],
      has_code: true,
      has_notes: true,
      verify_count: 1,
      deps: [],
      code_lines: 42,
      status: "passed",
      reverse_deps: ["ntt"],
      related_problems: [],
    },
    {
      slug: "ntt",
      title: "快速数论变换",
      tags: ["math", "poly"],
      has_code: true,
      has_notes: true,
      verify_count: 1,
      deps: ["modint"],
      code_lines: 65,
      status: "stale",
      reverse_deps: [],
      related_problems: ["poly-mul"],
    },
    {
      slug: "nt-facts",
      title: "数论结论表",
      tags: ["math"],
      has_code: false,
      has_notes: true,
      verify_count: 0,
      deps: [],
      code_lines: 0,
      status: "unverified",
      reverse_deps: [],
      related_problems: [],
    },
  ];

  test("filters snippets by status, tag, and query (including tagless snippets)", () => {
    const withTagless: SnippetSummaryItem[] = [
      ...sampleSnippets,
      {
        slug: "plain",
        title: "Plain Template",
        tags: [],
        has_code: true,
        has_notes: true,
        verify_count: 0,
        deps: [],
        code_lines: 10,
        status: "unverified",
        reverse_deps: [],
        related_problems: [],
      },
    ];
    expect(filterSnippets(withTagless, { statusFilter: "passed" }).map((s) => s.slug)).toEqual(["modint"]);
    expect(filterSnippets(withTagless, { statusFilter: "stale" }).map((s) => s.slug)).toEqual(["ntt"]);
    expect(filterSnippets(withTagless, { statusFilter: "unverified" }).map((s) => s.slug)).toEqual(["plain"]);
    expect(filterSnippets(withTagless, { tag: "poly" }).map((s) => s.slug)).toEqual(["ntt"]);
    expect(filterSnippets(withTagless, { query: "数论" }).map((s) => s.slug)).toEqual(["ntt", "nt-facts"]);
    // Defensive against missing tags field from legacy JSON payloads (review 0.1)
    const missingTags = [{ ...withTagless[3], tags: undefined as unknown as string[] }];
    expect(filterSnippets(missingTags, { query: "plain" }).map((s) => s.slug)).toEqual(["plain"]);
  });

  test("resolves snippet preview template from referencing TRD (inheriting id) or default", () => {
    const trds = [
      { id: "icpc-regional", template: "trd-compact", chapters: [{ title: "Math", items: ["modint", "ntt"] }] },
      { template: "trd-book", chapters: [{ title: "DS", items: ["dsu"] }] },
    ];
    expect(resolveSnippetPreviewTemplate("ntt", trds)).toBe("trd:icpc-regional");
    expect(resolveSnippetPreviewTemplate("dsu", trds)).toBe("trd-book");
    expect(resolveSnippetPreviewTemplate("other", trds, "trd-compact")).toBe("trd-compact");
  });

  test("encodes canonical serde Patch<T> values for TRD and Snippet metadata updates", () => {
    expect(encodeStringPatch("Team A")).toEqual({ Set: "Team A" });
    expect(encodeStringPatch("   ")).toBe("Clear");
    expect(encodeStringPatch(null)).toBe("Clear");
    expect(encodeTrdOptionPatch("trd-compact", "font-size", "8pt")).toEqual({
      "trd-compact": { "font-size": { Set: "8pt" } },
    });
    expect(encodeTrdOptionPatch("trd-compact", "font-size", null)).toEqual({
      "trd-compact": { "font-size": "Clear" },
    });
  });

  test("collects and auto-fills missing upstream dependencies before dependent snippet", () => {
    const chapters: TrdChapter[] = [
      { title: "Math", items: ["ntt", "nt-facts"] },
    ];
    const issues: TrdPreflightIssue[] = [
      {
        severity: "error",
        kind: "missing_dependency",
        snippet: "ntt",
        missing_deps: ["modint"],
        message: "ntt requires modint",
      },
    ];
    expect(collectMissingDeps(issues)).toEqual(["modint"]);
    const filled = fillMissingDepsInChapters(chapters, issues);
    expect(filled[0].items).toEqual(["modint", "ntt", "nt-facts"]);
  });

  test("partitions active and archived TRDs cleanly", () => {
    const trds: TrdSummaryItem[] = [
      { id: "icpc-2026", name: "ICPC 2026", frozen: false, archived: false, chapter_count: 2, item_count: 5 },
      { id: "old-2024", name: "ICPC 2024", frozen: true, archived: true, chapter_count: 1, item_count: 2 },
    ];
    const parts = partitionTrds(trds);
    expect(parts.active.map((x) => x.id)).toEqual(["icpc-2026"]);
    expect(parts.archived.map((x) => x.id)).toEqual(["old-2024"]);
  });

  test("extracts fenced code block from editorial Markdown", () => {
    const md = "## Approach\n\nUse NTT:\n\n```cpp\nint add(int a, int b) { return a + b; }\n```\n\nDone.";
    expect(extractCodeBlockFromMarkdown(md)).toBe("int add(int a, int b) { return a + b; }");
    expect(extractCodeBlockFromMarkdown("No code here")).toBeUndefined();
  });

  test("suggests valid ASCII slugs and maps status styles", () => {
    expect(suggestSnippetSlug("Fast NTT.cpp")).toBe("fast-ntt");
    expect(suggestSnippetSlug("快速数论变换")).toBe("snippet");
    expect(snippetStatusStyle("passed", true).labelKey).toBe("snippets.status.passed");
    expect(snippetStatusStyle("unverified", false).labelKey).toBe("snippets.status.formula");
  });
});

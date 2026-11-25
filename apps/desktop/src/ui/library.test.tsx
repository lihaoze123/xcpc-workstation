import { describe, expect, test } from "nsh/test";
import type { FuzzyHitJs, ProblemSummaryJs } from "xcpc-workstation/native";
import {
  applyRowSelection,
  buildPaletteItems,
  computeLibraryColumns,
  EMPTY_SELECTION,
  errorCopy,
  groupProblems,
  headerCheckboxState,
  partitionBatchDelete,
  type PaletteActionInput,
} from "./logic";

function makeProblem(overrides: Partial<ProblemSummaryJs> & { id: string }): ProblemSummaryJs {
  return {
    title: overrides.id,
    group: null,
    rating: null,
    tags: [],
    sources: [],
    hasEditorial: false,
    hasSolutions: false,
    statementPath: null,
    packageStatus: null,
    contestIds: [],
    ...overrides,
  };
}

function makeHit(overrides: Partial<FuzzyHitJs> & { kind: string; id: string; title: string }): FuzzyHitJs {
  return {
    contestId: null,
    label: null,
    visitKey: `${overrides.kind}:${overrides.id}`,
    archived: false,
    score: 100,
    ...overrides,
  };
}

describe("Library multi-select state machine", () => {
  const visible = ["p1", "p2", "p3", "p4", "p5"];

  test("plain click clears multi-select and sets anchorId & focusedId", () => {
    let s = applyRowSelection(EMPTY_SELECTION, visible, { kind: "selectAll" });
    expect(s.selectedIds.size).toBe(5);
    s = applyRowSelection(s, visible, { kind: "click", id: "p3" });
    expect([...s.selectedIds]).toEqual([]);
    expect(s.focusedId).toBe("p3");
    expect(s.anchorId).toBe("p3");
  });

  test("checkbox click toggles individual row without clearing others", () => {
    let s = applyRowSelection(EMPTY_SELECTION, visible, { kind: "click", id: "p2", checkbox: true });
    expect([...s.selectedIds]).toEqual(["p2"]);
    s = applyRowSelection(s, visible, { kind: "click", id: "p4", checkbox: true });
    expect([...s.selectedIds]).toEqual(["p2", "p4"]);
    s = applyRowSelection(s, visible, { kind: "click", id: "p2", checkbox: true });
    expect([...s.selectedIds]).toEqual(["p4"]);
  });

  test("Ctrl/Cmd+click promotes focusedId when selectedIds is empty", () => {
    let s = applyRowSelection(EMPTY_SELECTION, visible, { kind: "click", id: "p1" });
    expect([...s.selectedIds]).toEqual([]);
    expect(s.focusedId).toBe("p1");

    s = applyRowSelection(s, visible, { kind: "click", id: "p3", mod: true });
    expect([...s.selectedIds].sort()).toEqual(["p1", "p3"]);
    expect(s.anchorId).toBe("p3");
    expect(s.focusedId).toBe("p3");

    // Subsequent Ctrl/Cmd+click toggles normally.
    s = applyRowSelection(s, visible, { kind: "click", id: "p1", mod: true });
    expect([...s.selectedIds]).toEqual(["p3"]);
  });

  test("Shift+click selects closed range while preserving anchorId", () => {
    let s = applyRowSelection(EMPTY_SELECTION, visible, { kind: "click", id: "p2" });
    s = applyRowSelection(s, visible, { kind: "click", id: "p4", shift: true });
    expect([...s.selectedIds]).toEqual(["p2", "p3", "p4"]);
    expect(s.anchorId).toBe("p2");
    expect(s.focusedId).toBe("p4");

    // Shift+click above anchor shrinks/flips range around the same anchor.
    s = applyRowSelection(s, visible, { kind: "click", id: "p1", shift: true });
    expect([...s.selectedIds]).toEqual(["p1", "p2"]);
    expect(s.anchorId).toBe("p2");
  });

  test("Ctrl+A, Esc, toggleAll, toggleGroup, and filter pruning", () => {
    let s = applyRowSelection(EMPTY_SELECTION, visible, { kind: "selectAll" });
    expect(headerCheckboxState(s.selectedIds, visible)).toBe("all");

    // Prune when filter narrows visible rows to ["p2", "p4"].
    s = applyRowSelection(s, ["p2", "p4"], { kind: "prune" });
    expect([...s.selectedIds]).toEqual(["p2", "p4"]);

    // Partial header state over full list.
    expect(headerCheckboxState(s.selectedIds, visible)).toBe("partial");

    // toggleAll from partial selects all visible.
    s = applyRowSelection(s, visible, { kind: "toggleAll" });
    expect(headerCheckboxState(s.selectedIds, visible)).toBe("all");

    // toggleAll from all clears all.
    s = applyRowSelection(s, visible, { kind: "toggleAll" });
    expect(headerCheckboxState(s.selectedIds, visible)).toBe("none");

    // toggleGroup adds/removes only that group's IDs.
    s = applyRowSelection(s, visible, { kind: "toggleGroup", ids: ["p1", "p2"] });
    expect([...s.selectedIds]).toEqual(["p1", "p2"]);
    s = applyRowSelection(s, visible, { kind: "toggleGroup", ids: ["p1", "p2"] });
    expect([...s.selectedIds]).toEqual([]);

    // Esc clear.
    s = applyRowSelection(s, visible, { kind: "selectAll" });
    s = applyRowSelection(s, visible, { kind: "clear" });
    expect(s.selectedIds.size).toBe(0);
    expect(headerCheckboxState(new Set(), [])).toBe("none");
  });
});

describe("groupProblems dynamic grouping (§5.3)", () => {
  const rows: ProblemSummaryJs[] = [
    makeProblem({ id: "a", group: "省赛候选", rating: 1950, sources: [{ platform: "Codeforces", id: "100A" }], packageStatus: "verified_fresh", contestIds: ["c1"] }),
    makeProblem({ id: "b", group: "未声明组", rating: 1300, sources: [{ platform: "AtCoder", id: "abc100_a" }], packageStatus: "stale", contestIds: ["c1", "c2"] }),
    makeProblem({ id: "c", group: null, rating: null, sources: [], packageStatus: null, contestIds: [] }),
    makeProblem({ id: "d", group: "校赛备选", rating: 2500, sources: [{ platform: "Codeforces", id: "200D" }], packageStatus: "failed", contestIds: ["c2"] }),
    makeProblem({ id: "e", group: "省赛候选", rating: 1100, sources: [], packageStatus: "unverified", contestIds: ["c1"] }),
  ];
  const declared = ["校赛备选", "省赛候选", "空分组"];
  const contests = [
    { id: "c1", name: "Round 1" },
    { id: "c2", name: "Round 2" },
  ];

  test("group mode follows declared order, then undeclared alphabetically, then ungrouped", () => {
    const secs = groupProblems(rows, "group", declared, contests);
    expect(secs.map((s) => s.key)).toEqual([
      "group:校赛备选",
      "group:省赛候选",
      "group:未声明组",
      "group:__ungrouped__",
    ]);
    expect(secs[1].rows.map((r) => r.id)).toEqual(["a", "e"]);
  });

  test("rating mode follows TIER_RANGES order with unrated last", () => {
    const secs = groupProblems(rows, "rating", declared, contests);
    expect(secs.map((s) => s.key)).toEqual([
      "rating:newbie",
      "rating:pupil",
      "rating:candidate_master",
      "rating:grandmaster",
      "rating:unrated",
    ]);
  });

  test("platform mode sorts alphabetically with no-platform last", () => {
    const secs = groupProblems(rows, "platform", declared, contests);
    expect(secs.map((s) => s.key)).toEqual([
      "platform:AtCoder",
      "platform:Codeforces",
      "platform:__none__",
    ]);
  });

  test("contest mode places multi-contest problems in __multi__ without duplicating IDs", () => {
    const secs = groupProblems(rows, "contest", declared, contests);
    expect(secs.map((s) => s.key)).toEqual([
      "contest:c1",
      "contest:c2",
      "contest:__multi__",
      "contest:__none__",
    ]);
    const allIds = secs.flatMap((s) => s.rows.map((r) => r.id));
    expect(new Set(allIds).size).toBe(rows.length);
    expect(secs[2].rows.map((r) => r.id)).toEqual(["b"]);
  });

  test("package_status mode groups 有问题 > 未检查 (incl. stale) > 已通过 > none", () => {
    const secs = groupProblems(rows, "package_status", declared, contests);
    expect(secs.map((s) => s.key)).toEqual([
      "pkg:failed",
      "pkg:unverified",
      "pkg:verified_fresh",
      "pkg:none",
    ]);
    expect(secs[1].rows.map((r) => r.id).sort()).toEqual(["b", "e"]);
  });
});

describe("computeLibraryColumns responsive budget (§5.2)", () => {
  test("includes all 6 columns at 810pt with inline ID and 3 max tags", () => {
    const layout = computeLibraryColumns(810);
    expect(layout.columns.map((c) => c.key)).toEqual(["select", "title", "ratingText", "status", "tagsText", "source"]);
    expect(layout.showInlineId).toBe(true);
    expect(layout.maxTags).toBe(3);
    expect(layout.columns.find((c) => c.key === "select")?.width).toBe(36);
    expect(layout.columns.find((c) => c.key === "ratingText")?.width).toBe(78);
    expect(layout.columns.find((c) => c.key === "status")?.width).toBe(76);
    expect(layout.columns.find((c) => c.key === "tagsText")?.width).toBe(176);
    expect(layout.columns.find((c) => c.key === "source")?.width).toBe(132);
  });

  test("drops source below 780pt and narrows tags at 480..559pt while keeping fixed indices stable", () => {
    const mid = computeLibraryColumns(650);
    expect(mid.columns.map((c) => c.key)).toEqual(["select", "title", "ratingText", "status", "tagsText"]);
    expect(mid.columns.find((c) => c.key === "tagsText")?.width).toBe(176);

    const compact = computeLibraryColumns(520);
    expect(compact.columns.map((c) => c.key)).toEqual(["select", "title", "ratingText", "status", "tagsText"]);
    expect(compact.columns.find((c) => c.key === "tagsText")?.width).toBe(124);
    expect(compact.maxTags).toBe(2);
    expect(compact.showInlineId).toBe(false); // titleWidth < 220

    const narrow = computeLibraryColumns(420);
    expect(narrow.columns.map((c) => c.key)).toEqual(["select", "title", "ratingText", "status"]);
  });
});

describe("partitionBatchDelete (§5.4)", () => {
  test("unfrozen contests block regardless of archived; frozen contests do NOT block", () => {
    const contests = [
      { id: "c-live", name: "Live", frozen: false, archived: false, items: [{ id: "p1" }] },
      { id: "c-arch-unfrozen", name: "Archived Unfrozen", frozen: false, archived: true, items: [{ id: "p2" }] },
      { id: "c-frozen", name: "Frozen", frozen: true, archived: false, items: [{ id: "p3" }] },
    ];
    const part = partitionBatchDelete(["p1", "p2", "p3", "p4", "p1"], contests);
    expect(part.deletable).toEqual(["p3", "p4"]);
    expect(part.blocked.map((b) => b.id)).toEqual(["p1", "p2"]);
    expect(part.blocked[0].contests.map((c) => c.id)).toEqual(["c-live"]);
    expect(part.blocked[1].contests.map((c) => c.id)).toEqual(["c-arch-unfrozen"]);
  });
});

describe("buildPaletteItems (§6)", () => {
  const actions: PaletteActionInput[] = [
    { id: "a:new-problem", label: "New Problem", keywords: ["create", "problem"] },
    { id: "a:new-group", label: "New Group", keywords: ["folder", "group"] },
    { id: "a:export", label: "Export Statements", keywords: ["pdf", "export"] },
  ];
  const jumps: FuzzyHitJs[] = [
    ...Array.from({ length: 10 }, (_, i) =>
      makeHit({ kind: "contest_item", id: `cp${i}`, title: `Contest Prob ${i}`, contestId: "c1", label: String.fromCharCode(65 + i), visitKey: `contest_item:c1:${String.fromCharCode(65 + i)}` }),
    ),
    ...Array.from({ length: 10 }, (_, i) =>
      makeHit({ kind: "problem", id: `prob-${i}`, title: `Problem ${i}` }),
    ),
    ...Array.from({ length: 6 }, (_, i) =>
      makeHit({ kind: "contest", id: `con-${i}`, title: `Contest ${i}` }),
    ),
  ];

  test("'>' prefix enters action-only mode and skips all jump hits", () => {
    const res = buildPaletteItems("> new", jumps, actions, null);
    expect(res.actionMode).toBe(true);
    expect(res.query).toBe("new");
    expect(res.items.every((i) => i.groupKind === "action")).toBe(true);
    expect(res.items.map((i) => i.id)).toEqual(["a:new-problem", "a:new-group"]);
  });

  test("empty query outside contest caps recent at 5 followed immediately by actions", () => {
    const res = buildPaletteItems("", jumps, actions, null);
    expect(res.actionMode).toBe(false);
    const recent = res.items.filter((i) => i.groupKind === "recent");
    const acts = res.items.filter((i) => i.groupKind === "action");
    expect(recent.length).toBe(5);
    expect(acts.length).toBe(actions.length);
    expect(res.items[5].groupKind).toBe("action");
  });

  test("empty query inside contest caps currentContest at 8 in A..Z order followed by actions", () => {
    const res = buildPaletteItems("", jumps, actions, "c1");
    const cur = res.items.filter((i) => i.groupKind === "currentContest");
    expect(cur.length).toBe(8);
    expect(cur[0].hit?.label).toBe("A");
    expect(cur[7].hit?.label).toBe("H");
    expect(res.items[8].groupKind).toBe("action");
  });

  test("non-empty query enforces per-group caps (6 / 8 / 4) and promotes strong action matches", () => {
    // Prefix match on "export" (score 70) with no exact object hit promotes actions first.
    const promoted = buildPaletteItems("export", jumps, actions, "c1");
    expect(promoted.items[0].groupKind).toBe("action");
    expect(promoted.items.filter((i) => i.groupKind === "currentContest").length).toBe(6);
    expect(promoted.items.filter((i) => i.groupKind === "problem").length).toBe(8);
    expect(promoted.items.filter((i) => i.groupKind === "contest").length).toBe(4);

    // Exact object ID hit prevents action promotion even when an action matches.
    const exactHit = [makeHit({ kind: "problem", id: "export", title: "Export Problem" })];
    const notPromoted = buildPaletteItems("export", exactHit, actions, null);
    expect(notPromoted.items[0].groupKind).toBe("problem");
  });

  test("prioritizeActions puts tool/plugin commands ahead of recent jumps when in Tools space", () => {
    const res = buildPaletteItems("", jumps, actions, null, { prioritizeActions: true });
    expect(res.items[0].groupKind).toBe("action");
    expect(res.items[0].id).toBe("a:new-problem");
    const firstRecentIdx = res.items.findIndex((i) => i.groupKind === "recent");
    expect(firstRecentIdx).toBe(actions.length);
  });
});

describe("New Stage 4 errorCopy codes", () => {
  test("maps invalid_group_name, group_already_exists, cancelled, not_a_package", () => {
    expect(errorCopy("invalid_group_name").rule).toBe(false);
    expect(errorCopy("group_already_exists").rule).toBe(false);
    expect(errorCopy("cancelled").rule).toBe(true);
    expect(errorCopy("not_a_package").rule).toBe(true);
  });
});

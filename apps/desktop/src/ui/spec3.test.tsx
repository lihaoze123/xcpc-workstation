import { describe, expect, test } from "nsh/test";
import {
  errorCopy,
  filterAddableContests,
  partitionContests,
  suggestDuplicateId,
  togglePickProblem,
  validId,
} from "./logic";
import { nextEnabledIndex } from "./PickProblemsModal";
import { languagePref } from "./i18n";

describe("Duplicate problem ID suggestion", () => {
  test("appends -copy and increments -copy-2, -copy-3 on collision", () => {
    expect(suggestDuplicateId("two-sum", ["two-sum"])).toBe("two-sum-copy");
    expect(suggestDuplicateId("two-sum", ["two-sum", "two-sum-copy"])).toBe("two-sum-copy-2");
    expect(
      suggestDuplicateId("two-sum", ["two-sum", "two-sum-copy", "two-sum-copy-2"]),
    ).toBe("two-sum-copy-3");
  });

  test("truncates long source IDs to stay within 64-character validId limit", () => {
    const longId = "a".repeat(64);
    const first = suggestDuplicateId(longId, [longId]);
    expect(first.length).toBeLessThanOrEqual(64);
    expect(first.endsWith("-copy")).toBe(true);
    expect(validId(first)).toBe(true);

    const second = suggestDuplicateId(longId, [longId, first]);
    expect(second.length).toBeLessThanOrEqual(64);
    expect(second.endsWith("-copy-2")).toBe(true);
    expect(validId(second)).toBe(true);
  });
});

describe("PickProblemsModal selection rules", () => {
  test("preserves selection order across toggles and blocks already-in-contest problems", () => {
    const existing = new Set(["p1", "p2"]);
    let picked: string[] = [];

    // Already-in-contest items cannot be toggled.
    picked = togglePickProblem(picked, "p1", existing);
    expect(picked).toEqual([]);

    // Pick in order: p4, p3, p5.
    picked = togglePickProblem(picked, "p4", existing);
    picked = togglePickProblem(picked, "p3", existing);
    picked = togglePickProblem(picked, "p5", existing);
    expect(picked).toEqual(["p4", "p3", "p5"]);

    // Unchecking removes only that item while keeping order of the rest.
    picked = togglePickProblem(picked, "p3", existing);
    expect(picked).toEqual(["p4", "p5"]);
  });

  test("enforces 26-problem contest capacity while still allowing unchecking", () => {
    const existing = new Set(Array.from({ length: 24 }, (_, i) => `e${i}`));
    let picked: string[] = [];

    picked = togglePickProblem(picked, "n1", existing, 26);
    picked = togglePickProblem(picked, "n2", existing, 26);
    expect(picked).toEqual(["n1", "n2"]);

    // 24 existing + 2 picked == 26: third pick is ignored.
    picked = togglePickProblem(picked, "n3", existing, 26);
    expect(picked).toEqual(["n1", "n2"]);

    // Unchecking frees a slot for n3.
    picked = togglePickProblem(picked, "n1", existing, 26);
    picked = togglePickProblem(picked, "n3", existing, 26);
    expect(picked).toEqual(["n2", "n3"]);
  });

  test("nextEnabledIndex skips disabled rows in both directions and stays on current at bounds", () => {
    const rows = [
      { id: "a", off: true },
      { id: "b", off: false },
      { id: "c", off: true },
      { id: "d", off: true },
      { id: "e", off: false },
      { id: "f", off: true },
    ];
    const isOff = (r: { off: boolean }) => r.off;

    // From b (1) moving down skips c, d and lands on e (4).
    expect(nextEnabledIndex(rows, 1, 1, isOff)).toBe(4);
    // From e (4) moving down skips f (disabled) and stays on e (4).
    expect(nextEnabledIndex(rows, 4, 1, isOff)).toBe(4);
    // From e (4) moving up skips d, c and lands on b (1).
    expect(nextEnabledIndex(rows, 4, -1, isOff)).toBe(1);
    // From b (1) moving up skips a (disabled) and stays on b (1).
    expect(nextEnabledIndex(rows, 1, -1, isOff)).toBe(1);
    // When all rows are disabled, returns -1.
    expect(nextEnabledIndex(rows, 0, 1, () => true)).toBe(-1);
  });
});

describe("Contest archive partitioning and filtering", () => {
  const sampleContests = [
    { id: "c1", name: "Active 1", archived: false, frozen: false },
    { id: "c2", name: "Archived 1", archived: true, frozen: true },
    { id: "c3", name: "Active 2", archived: false, frozen: true },
    { id: "c4", name: "Archived 2", archived: true, frozen: false },
  ];

  test("partitions active and archived contests in order", () => {
    const { active, archived } = partitionContests(sampleContests);
    expect(active.map((c) => c.id)).toEqual(["c1", "c3"]);
    expect(archived.map((c) => c.id)).toEqual(["c2", "c4"]);

    const noArchived = partitionContests(sampleContests.filter((c) => !c.archived));
    expect(noArchived.archived).toEqual([]);
  });

  test("AddToContest excludes archived contests", () => {
    const addable = filterAddableContests(sampleContests);
    expect(addable.map((c) => c.id)).toEqual(["c1", "c3"]);
  });

  test("errorCopy maps contest_archived as a rule refusal", () => {
    const prev = languagePref();
    try {
      languagePref.set("zh-CN");
      const zhCopy = errorCopy("contest_archived");
      expect(zhCopy.rule).toBe(true);
      expect(zhCopy.message).toContain("归档");

      languagePref.set("en");
      const enCopy = errorCopy("contest_archived");
      expect(enCopy.rule).toBe(true);
      expect(enCopy.message.toLowerCase()).toContain("archived");
    } finally {
      languagePref.set(prev);
    }
  });
});

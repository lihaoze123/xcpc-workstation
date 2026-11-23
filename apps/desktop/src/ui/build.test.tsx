import { describe, expect, test } from "nsh/test";
import {
  buildMatrixLookup,
  formatAcVsTleMargin,
  formatBytesCompact,
  formatMemoryKib,
  formatStdTimeRatio,
  hasInvalidCxxOverride,
  isPackageIncomplete,
  matrixCellKey,
  normalizeCxxOverrideInput,
  selectMatrixCell,
  shouldShowBuildTab,
  toggleSubsetSelection,
  verdictBadgeStyle,
  type MatrixCellData,
} from "./studio/buildLogic";

describe("Build tab visibility & package completeness", () => {
  test("shows Build tab only for single problems with a package", () => {
    expect(shouldShowBuildTab("problem", true)).toBe(true);
    expect(shouldShowBuildTab("problem", false)).toBe(false);
    expect(shouldShowBuildTab("contest", true)).toBe(false);
    expect(shouldShowBuildTab("contest", false)).toBe(false);
    expect(shouldShowBuildTab(undefined, true)).toBe(false);
  });

  test("detects incomplete package scaffolds", () => {
    expect(isPackageIncomplete(undefined)).toBe(true);
    expect(isPackageIncomplete({})).toBe(true);
    expect(
      isPackageIncomplete({
        validator: "validator.cpp",
        solutions: ["std.cpp"],
        validator_self_tests_valid: ["01.in"],
        validator_self_tests_invalid: [],
      }),
    ).toBe(true);
    expect(
      isPackageIncomplete({
        validator: "validator.cpp",
        solutions: ["std.cpp"],
        validator_self_tests_valid: ["01.in"],
        validator_self_tests_invalid: ["01.in"],
      }),
    ).toBe(false);
  });
});

describe("Safety margin formatting", () => {
  test("formats bytes, memory, std time ratio, and AC vs TLE margin", () => {
    expect(formatBytesCompact(0)).toBe("0 B");
    expect(formatBytesCompact(512)).toBe("512 B");
    expect(formatBytesCompact(2048)).toBe("2.0 KB");
    expect(formatBytesCompact(1_572_864)).toBe("1.50 MB");
    expect(formatMemoryKib(512)).toBe("512 KB");
    expect(formatMemoryKib(16_384)).toBe("16.0 MB");

    const margin = {
      max_input_bytes: 1_572_864,
      time_limit_ms: 2000,
      memory_limit_kib: 262_144,
      std_max_time_ms: 380,
      std_time_ratio: 0.19,
      std_max_memory_kib: 16_384,
      slowest_ac_solution: "std",
      slowest_ac_time_ms: 380,
      fastest_tle_solution: "brute",
      fastest_tle_time_ms: 2150,
    };
    expect(formatStdTimeRatio(margin)).toBe("0.38s / 2.00s = 19%");
    expect(formatAcVsTleMargin(margin)).toBe("0.38s vs 2.15s (5.7×)");
    expect(formatAcVsTleMargin({ ...margin, fastest_tle_time_ms: null })).toBe("–");
  });
});

describe("Matrix selection & subset filters", () => {
  const cells: MatrixCellData[] = [
    {
      test_index: 1,
      test_name: "sample-01",
      solution: "std",
      verdict: "AC",
      cpu_time_ms: 4,
      wall_time_ms: 8,
      peak_memory_kib: 3200,
      cache_hit: false,
      checker_message: "ok 1 token",
    },
    {
      test_index: 2,
      test_name: "max-01",
      solution: "greedy_wa",
      verdict: "WA",
      cpu_time_ms: 12,
      wall_time_ms: 15,
      peak_memory_kib: 4096,
      cache_hit: true,
      checker_message: "wrong answer expected 42, found 41",
      repro_cmd: "xcpc package build a --tests 2 --solutions greedy_wa",
    },
  ];

  test("indexes and selects matrix cells accurately", () => {
    const map = buildMatrixLookup(cells);
    expect(map.get(matrixCellKey(1, "std"))?.verdict).toBe("AC");
    expect(map.get(matrixCellKey(2, "greedy_wa"))?.verdict).toBe("WA");
    expect(map.get(matrixCellKey(2, "std"))).toBeUndefined();

    const picked = selectMatrixCell(cells, { testIndex: 2, solution: "greedy_wa" });
    expect(picked?.checker_message).toContain("expected 42");
    expect(selectMatrixCell(cells, undefined)).toBeUndefined();
  });

  test("toggles subset selection preserving order", () => {
    let sel: string[] = [];
    sel = toggleSubsetSelection(sel, "std");
    sel = toggleSubsetSelection(sel, "brute");
    expect(sel).toEqual(["std", "brute"]);
    sel = toggleSubsetSelection(sel, "std");
    expect(sel).toEqual(["brute"]);
  });

  test("verdict badge styles produce distinct light/dark colors", () => {
    const acLight = verdictBadgeStyle("AC", false);
    const acDark = verdictBadgeStyle("AC", true);
    const waLight = verdictBadgeStyle("WA", false);
    expect(acLight.fg).not.toBe(acDark.fg);
    expect(acLight.fg).not.toBe(waLight.fg);
  });
});

describe("Toolchain override state helpers", () => {
  test("normalizes whitespace and detects invalid overrides", () => {
    expect(normalizeCxxOverrideInput("   ")).toBeNull();
    expect(normalizeCxxOverrideInput("  /usr/bin/g++  ")).toBe("/usr/bin/g++");
    expect(hasInvalidCxxOverride({ overridePath: "/no/such/g++", active: null })).toBe(true);
    expect(
      hasInvalidCxxOverride({
        overridePath: "/usr/bin/g++",
        active: { name: "g++", path: "/usr/bin/g++", version: "g++ 16", kind: "gpp" },
      }),
    ).toBe(false);
    expect(hasInvalidCxxOverride({ overridePath: null, active: null })).toBe(false);
  });
});

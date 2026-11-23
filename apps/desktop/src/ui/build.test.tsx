import { describe, expect, test } from "nsh/test";
import {
  buildMatrixLookup,
  customOverrideInputValue,
  dedupDetectedCompilers,
  formatAcVsTleMargin,
  formatBytesCompact,
  formatMemoryKib,
  formatStdTimeRatio,
  hasInvalidCxxOverride,
  humanizeCheckText,
  isCellVerdictAnomaly,
  isMissingCompilerError,
  isPackageIncomplete,
  matrixCellKey,
  nextActionForCheck,
  normalizeCxxOverrideInput,
  partitionGates,
  problemSlugFromDiagnosticFile,
  resolveSolutionHeaderSpecs,
  selectMatrixCell,
  shouldShowBuildTab,
  solutionRole,
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
    expect(formatStdTimeRatio(margin)).toBe("0.38 s / 2.0 s (19%)");
    expect(formatStdTimeRatio({ ...margin, std_max_time_ms: 1, time_limit_ms: 1000, std_time_ratio: 0.001 })).toBe(
      "1 ms / 1.0 s (<1%)",
    );
    expect(formatAcVsTleMargin(margin)).toBe("0.38s vs 2.15s (5.7×)");
    expect(formatAcVsTleMargin({ ...margin, fastest_tle_time_ms: null })).toBeUndefined();
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

  test("deduplicates compilers by name+version while preserving active override", () => {
    const wrapped = { name: "g++", path: "/nix/store/wrap/bin/g++", version: "g++ (GCC) 16.2.0", kind: "gpp" };
    const unwrapped = { name: "g++", path: "/nix/store/raw/bin/g++", version: "g++ (GCC) 16.2.0", kind: "gpp" };
    const clang = { name: "clang++", path: "/nix/store/clang/bin/clang++", version: "clang 20.1.0", kind: "clangpp" };

    expect(dedupDetectedCompilers([wrapped, unwrapped, clang], wrapped)).toEqual([wrapped, clang]);
    // Manual override pointing at the second duplicate path stays visible as active.
    expect(dedupDetectedCompilers([wrapped, unwrapped, clang], unwrapped)).toEqual([unwrapped, clang]);
  });
});

describe("Pre-build solution headers & restrained gate partitioning", () => {
  test("reads configured expect verdicts from inspection before first build", () => {
    const headers = resolveSolutionHeaderSpecs(
      {
        problem_id: "sum",
        has_package: true,
        package_hash: "abc",
        freshness: { status: "unverified" },
        checker: "wcmp",
        files: { solutions: ["std.cpp", "wrong_wa.cpp"] },
        solutions: [
          { name: "std", is_main: true, expected: ["AC"] },
          { name: "wrong_wa", is_main: false, expect: "WA" },
        ],
        expanded_tests: [],
      },
      undefined,
    );
    expect(headers).toEqual([
      { name: "std", source_rel: "solutions/std.cpp", is_main: true, expected: ["AC"], tags: null },
      { name: "wrong_wa", source_rel: "solutions/wrong_wa.cpp", is_main: false, expected: ["WA"], tags: null },
    ]);
    expect(solutionRole(headers[0])).toBe("main");
    expect(solutionRole(headers[1])).toBe("wrong");
    expect(solutionRole({ is_main: false, expected: ["AC"] })).toBe("alt");
  });

  test("partitions fail/warn/passed checks and maps actionable next steps", () => {
    const { failed, warn, attention, passed } = partitionGates([
      { id: "val", title: "Validator", status: "pass", message: "ok" },
      { id: "std", title: "Std Time", status: "warn", message: "60%" },
      { id: "wa", title: "Wrong Sol", status: "fail", message: "got AC" },
      { id: "chk", title: "Checker", status: "skip", message: "builtin" },
    ]);
    expect(failed.map((g) => g.id)).toEqual(["wa"]);
    expect(warn.map((g) => g.id)).toEqual(["std"]);
    expect(attention.map((g) => g.id)).toEqual(["std", "wa"]);
    expect(passed.map((g) => g.id)).toEqual(["val", "chk"]);

    expect(humanizeCheckText("validator 存在与自测", true)).toBe("校验器存在与自测");
    expect(humanizeCheckText("内置 checker", true)).toBe("内置评测器");
    expect(humanizeCheckText("使用内置标准校验器 wcmp", true)).toBe("使用内置标准评测器 wcmp");
    expect(humanizeCheckText("存在判定与 expect 不符的解法", true)).toBe("存在判定与预期不符的解法");
    expect(humanizeCheckText("valid 1 组、invalid 1 组全部通过", true)).toBe("有效 1 组、无效 1 组全部通过");
    expect(humanizeCheckText("validator.cpp missing", true)).toBe("validator.cpp missing");

    expect(
      nextActionForCheck(
        { id: "validator_all_tests", title: "Validator", status: "fail", message: "failed on test 2" },
        {
          problem_id: "p",
          package_hash: "h",
          mode: "verify",
          verified: false,
          ok: false,
          subset: false,
          built_at_epoch_secs: 0,
          checker: "wcmp",
          tests: [
            { index: 1, name: "01", is_sample: true, size_bytes: 10, validator_ok: true },
            { index: 2, name: "02", is_sample: false, size_bytes: 20, validator_ok: false },
          ],
          safety_margin: {
            max_input_bytes: 20,
            time_limit_ms: 1000,
            memory_limit_kib: 262_144,
            std_max_time_ms: 1,
            std_time_ratio: 0.001,
            std_max_memory_kib: 1024,
          },
        },
        { validator: "validator.cpp" },
      ),
    ).toEqual({ kind: "view_test", testIndex: 2 });

    expect(
      nextActionForCheck(
        { id: "alt_solution", title: "Alt", status: "warn", message: "missing" },
        undefined,
        { solutions: ["std.cpp"] },
      ),
    ).toEqual({ kind: "open_file", relPath: "solutions" });

    expect(
      nextActionForCheck(
        { id: "solutions_expect", title: "Solutions", status: "fail", message: "mismatch" },
        {
          problem_id: "p",
          package_hash: "h",
          mode: "verify",
          verified: false,
          ok: false,
          subset: false,
          built_at_epoch_secs: 0,
          checker: "wcmp",
          solutions: [
            {
              name: "std",
              is_main: true,
              expected: ["AC"],
              actual_verdict: "WA",
              matched: false,
              max_time_ms: 1,
              max_memory_kib: 1024,
              passed_tests: 0,
              total_tests: 1,
            },
          ],
          matrix: [
            {
              test_index: 1,
              test_name: "sample-01",
              solution: "std",
              verdict: "WA",
              cpu_time_ms: 1,
              wall_time_ms: 2,
              peak_memory_kib: 1024,
              cache_hit: false,
            },
          ],
          safety_margin: {
            max_input_bytes: 20,
            time_limit_ms: 1000,
            memory_limit_kib: 262_144,
            std_max_time_ms: 1,
            std_time_ratio: 0.001,
            std_max_memory_kib: 1024,
          },
        },
        { solutions: ["std.cpp"] },
      ),
    ).toEqual({ kind: "select_cell", testIndex: 1, solution: "std" });
  });

  test("detects expectation anomalies, missing compiler errors, diagnostic slugs, and custom override input", () => {
    expect(isCellVerdictAnomaly("AC", ["AC"], true)).toBe(false);
    expect(isCellVerdictAnomaly("WA", ["AC"], true)).toBe(true);
    expect(isCellVerdictAnomaly("WA", ["WA"], false, true)).toBe(false);
    expect(isCellVerdictAnomaly("RE", ["WA"], false, false)).toBe(true);
    expect(isCellVerdictAnomaly("AC", ["WA"], false, true)).toBe(false);
    expect(isCellVerdictAnomaly("AC", ["WA"], false, false)).toBe(true);

    expect(isMissingCompilerError("no C++ compiler found on PATH")).toBe(true);
    expect(isMissingCompilerError("validator rejected test 1")).toBe(false);

    expect(problemSlugFromDiagnosticFile("problems/a-plus-b/problem.toml")).toBe("a-plus-b");
    expect(problemSlugFromDiagnosticFile("contests/round-1/contest.toml")).toBeUndefined();

    const detected = [{ name: "g++", path: "/usr/bin/g++", version: "g++ 16", kind: "gpp" }];
    expect(customOverrideInputValue("/usr/bin/g++", detected)).toBe("");
    expect(customOverrideInputValue("/opt/custom/g++", detected)).toBe("/opt/custom/g++");
  });
});

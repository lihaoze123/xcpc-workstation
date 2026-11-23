// Pure TypeScript types and helpers for the Local Polygon Build Console & Toolchain settings.
// Kept pure (no NSH JSX dependencies) so every state transition and formatter is unit-tested.

import type { DetectedCompilerJs, ToolchainStatusJs } from "xcpc-workstation/native";

export type GateStatusKind = "pass" | "fail" | "warn" | "skip";

export interface GateResultData {
  id: string;
  title: string;
  status: GateStatusKind;
  message: string;
}

export interface SafetyMarginData {
  max_input_bytes: number;
  time_limit_ms: number;
  memory_limit_kib: number;
  std_max_time_ms: number;
  std_time_ratio: number;
  std_max_memory_kib: number;
  slowest_ac_solution?: string | null;
  slowest_ac_time_ms?: number | null;
  fastest_tle_solution?: string | null;
  fastest_tle_time_ms?: number | null;
}

export interface MatrixCellData {
  test_index: number;
  test_name: string;
  solution: string;
  verdict: string;
  cpu_time_ms: number;
  wall_time_ms: number;
  peak_memory_kib: number;
  cache_hit: boolean;
  checker_message?: string;
  stderr?: string;
  output_preview?: string | null;
  stale?: boolean;
  repro_cmd?: string | null;
}

export interface SolutionSummaryData {
  name: string;
  is_main: boolean;
  expected: string[];
  actual_verdict: string;
  matched: boolean;
  max_time_ms: number;
  max_memory_kib: number;
  passed_tests: number;
  total_tests: number;
  tags?: string[] | null;
}

export interface TestSummaryData {
  index: number;
  name: string;
  tag?: string | null;
  is_sample: boolean;
  size_bytes: number;
  gen_cmd?: string | null;
  validator_ok?: boolean | null;
  validator_message?: string | null;
}

export interface CompileDiagnosticData {
  file?: string | null;
  line?: number | null;
  column?: number | null;
  severity: string;
  message: string;
}

export interface CompileFailureData {
  target: string;
  source_path: string;
  stderr: string;
  diagnostics: CompileDiagnosticData[];
}

export interface DirtyInputIssueData {
  path: string;
  has_crlf: boolean;
  has_bom: boolean;
  missing_trailing_newline: boolean;
}

export interface SelfTestResultData {
  kind: string;
  name: string;
  passed: boolean;
  expected: string;
  actual: string;
  message?: string;
}

export interface BuildReportData {
  problem_id: string;
  package_hash: string;
  mode: "debug" | "verify";
  verified: boolean;
  ok: boolean;
  subset: boolean;
  built_at_epoch_secs: number;
  checker: string;
  compile_failures?: CompileFailureData[];
  dirty_inputs?: DirtyInputIssueData[];
  self_tests?: SelfTestResultData[];
  tests?: TestSummaryData[];
  solutions?: SolutionSummaryData[];
  matrix?: MatrixCellData[];
  gates?: GateResultData[];
  safety_margin: SafetyMarginData;
  diagnostics?: string[];
  repro_commands?: string[];
  next_steps?: string[];
}

export interface PackageFilesData {
  validator?: string | null;
  custom_checker?: string | null;
  generators?: string[];
  solutions?: string[];
  samples?: string[];
  hand_tests?: string[];
  validator_self_tests_valid?: string[];
  validator_self_tests_invalid?: string[];
  checker_self_tests?: string[];
  self_test_files?: string[];
}

export interface PackageSolutionSpecData {
  name: string;
  source_rel?: string;
  is_main?: boolean;
  main?: boolean;
  expected?: string[];
  expect?: string | string[];
  tags?: string[] | null;
}

export interface ExpandedTestData {
  index: number;
  name: string;
  tag?: string | null;
  is_sample: boolean;
  rule_index?: number | null;
  gen_cmd?: string | null;
  source?:
    | { kind: "sample"; stem: string; input_rel: string; manual_ans_rel?: string | null }
    | { kind: "file"; file_rel: string }
    | { kind: "generated"; gen_name: string; args: string[]; args_raw: string };
}

export interface PackageInspectionData {
  problem_id: string;
  has_package: boolean;
  package_hash: string;
  freshness: { status: string; summary?: string };
  checker: string;
  files: PackageFilesData;
  solutions?: PackageSolutionSpecData[];
  expanded_tests: ExpandedTestData[];
  dirty_inputs?: DirtyInputIssueData[];
  last_report?: BuildReportData | null;
  gates?: GateResultData[];
  next_steps?: string[];
}

export interface SelectedCellKey {
  testIndex: number;
  solution: string;
}

export function parsePackageInspection(json: string | null | undefined): PackageInspectionData | undefined {
  if (!json) return undefined;
  try {
    const parsed = JSON.parse(json) as PackageInspectionData;
    return parsed && typeof parsed === "object" ? parsed : undefined;
  } catch {
    return undefined;
  }
}

export function parseBuildReport(json: string | null | undefined): BuildReportData | undefined {
  if (!json) return undefined;
  try {
    const parsed = JSON.parse(json) as BuildReportData;
    return parsed && typeof parsed === "object" ? parsed : undefined;
  } catch {
    return undefined;
  }
}

/** The 4th "Build" tab is shown only for single problems that already have a package. */
export function shouldShowBuildTab(target: "problem" | "contest" | undefined, hasPackage: boolean): boolean {
  return target === "problem" && hasPackage;
}

/** Returns true when core scaffold files (validator, solutions, or validator self-tests) are missing. */
export function isPackageIncomplete(files: PackageFilesData | undefined): boolean {
  if (!files) return true;
  if (!files.validator) return true;
  if (!files.solutions || files.solutions.length === 0) return true;
  if (!files.validator_self_tests_valid || files.validator_self_tests_valid.length === 0) return true;
  if (!files.validator_self_tests_invalid || files.validator_self_tests_invalid.length === 0) return true;
  return false;
}

export function formatBytesCompact(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";
  if (bytes < 1024) return `${Math.round(bytes)} B`;
  const kb = bytes / 1024;
  if (kb < 1024) return `${kb.toFixed(kb >= 100 ? 0 : 1)} KB`;
  const mb = kb / 1024;
  return `${mb.toFixed(mb >= 100 ? 0 : 2)} MB`;
}

export function formatMemoryKib(kib: number): string {
  if (!Number.isFinite(kib) || kib <= 0) return "0 MB";
  if (kib < 1024) return `${Math.round(kib)} KB`;
  return `${(kib / 1024).toFixed(1)} MB`;
}

export function formatSecondsFromMs(msVal: number): string {
  if (!Number.isFinite(msVal) || msVal < 0) return "0.00s";
  return `${(msVal / 1000).toFixed(2)}s`;
}

/** Formats slowest std ratio cleanly, e.g. `1 ms / 1.0 s (<1%)` for sub-100ms or `0.38 s / 2.0 s (19%)`. */
export function formatStdTimeRatio(margin: SafetyMarginData | undefined): string {
  if (!margin || margin.time_limit_ms <= 0) return "–";
  const stdStr = margin.std_max_time_ms < 100
    ? `${Math.round(margin.std_max_time_ms)} ms`
    : `${(margin.std_max_time_ms / 1000).toFixed(2)} s`;
  const limitStr = `${(margin.time_limit_ms / 1000).toFixed(1)} s`;
  const pctNum = margin.std_time_ratio * 100;
  const pctStr = pctNum > 0 && pctNum < 1 ? "<1%" : `${Math.round(pctNum)}%`;
  return `${stdStr} / ${limitStr} (${pctStr})`;
}

/** Formats AC vs TLE margin when both slowest AC and fastest TLE exist, else returns `undefined` so the UI hides it. */
export function formatAcVsTleMargin(margin: SafetyMarginData | undefined): string | undefined {
  if (!margin || margin.slowest_ac_time_ms == null || margin.fastest_tle_time_ms == null) {
    return undefined;
  }
  const ac = margin.slowest_ac_time_ms;
  const tle = margin.fastest_tle_time_ms;
  const ratio = ac > 0 ? `${(tle / ac).toFixed(1)}×` : "∞";
  const acStr = ac < 100 ? `${Math.round(ac)} ms` : formatSecondsFromMs(ac);
  const tleStr = tle < 100 ? `${Math.round(tle)} ms` : formatSecondsFromMs(tle);
  return `${acStr} vs ${tleStr} (${ratio})`;
}

/**
 * Returns `true` when a single matrix cell's verdict violates the solution's configured `expect`
 * (e.g. `std` / `AC`-expected solution getting non-`AC`, a wrong solution getting an unexpected
 * failure verdict like `RE` when expecting `WA`, or a wrong solution unexpectedly passing all tests).
 */
export function isCellVerdictAnomaly(
  verdict: string,
  expected: readonly string[],
  isMain: boolean,
  solutionMatched?: boolean,
): boolean {
  const v = verdict.toUpperCase();
  if (!v) return false;
  if (isMain) return v !== "AC";
  const exp = expected.map((e) => e.toUpperCase());
  if (exp.length === 0) return v !== "AC";
  if (exp.length === 1 && exp[0] === "AC") return v !== "AC";
  if (v === "AC") return solutionMatched === false;
  return !exp.includes(v);
}

/** Returns `true` when a build error string indicates no C++ compiler was found. */
export function isMissingCompilerError(error: string | null | undefined): boolean {
  if (!error) return false;
  const lower = error.toLowerCase();
  return (
    lower.includes("no c++ compiler") ||
    lower.includes("compiler not found") ||
    lower.includes("未检测到") ||
    lower.includes("g++") && lower.includes("not found")
  );
}

/** Extracts problem slug from a `problems/<slug>/...` diagnostic file path (for contest-mode Preflight jump). */
export function problemSlugFromDiagnosticFile(file: string | null | undefined): string | undefined {
  if (!file) return undefined;
  const m = file.match(/^problems\/([^/]+)\//);
  return m ? m[1] : undefined;
}

/**
 * Returns the custom path text to show in Settings -> Toolchain's custom input box:
 * only populates it when `overridePath` is set and is NOT one of the detected PATH compilers.
 */
export function customOverrideInputValue(
  overridePath: string | null | undefined,
  detectedCompilers: readonly DetectedCompilerJs[],
): string {
  if (!overridePath) return "";
  if (detectedCompilers.some((c) => c.path === overridePath)) return "";
  return overridePath;
}

/** Accessible light/dark semantic badge colors for judge verdicts. */
export function verdictBadgeStyle(verdict: string, dark: boolean): { bg: string; fg: string } {
  switch (verdict.toUpperCase()) {
    case "AC":
      return dark
        ? { bg: "rgba(34, 197, 94, 0.16)", fg: "#4ADE80" }
        : { bg: "rgba(21, 128, 61, 0.11)", fg: "#15803D" };
    case "WA":
      return dark
        ? { bg: "rgba(248, 113, 113, 0.16)", fg: "#F87171" }
        : { bg: "rgba(185, 28, 28, 0.11)", fg: "#B91C1C" };
    case "TLE":
      return dark
        ? { bg: "rgba(251, 191, 36, 0.16)", fg: "#FBBF24" }
        : { bg: "rgba(180, 83, 9, 0.12)", fg: "#B45309" };
    case "MLE":
      return dark
        ? { bg: "rgba(251, 146, 60, 0.16)", fg: "#FB923C" }
        : { bg: "rgba(194, 65, 12, 0.12)", fg: "#C2410C" };
    case "RE":
      return dark
        ? { bg: "rgba(192, 132, 252, 0.16)", fg: "#C084FC" }
        : { bg: "rgba(126, 34, 206, 0.11)", fg: "#7E22CE" };
    case "PE":
      return dark
        ? { bg: "rgba(34, 211, 238, 0.16)", fg: "#22D3EE" }
        : { bg: "rgba(14, 116, 144, 0.11)", fg: "#0E7490" };
    case "CE":
    case "JE":
    default:
      return dark
        ? { bg: "rgba(161, 161, 170, 0.18)", fg: "#D4D4D8" }
        : { bg: "rgba(82, 82, 91, 0.11)", fg: "#52525B" };
  }
}

export function gateBadgeStyle(status: GateStatusKind, dark: boolean): { bg: string; fg: string } {
  switch (status) {
    case "pass":
      return verdictBadgeStyle("AC", dark);
    case "fail":
      return verdictBadgeStyle("WA", dark);
    case "warn":
      return verdictBadgeStyle("TLE", dark);
    case "skip":
    default:
      return verdictBadgeStyle("CE", dark);
  }
}

export function matrixCellKey(testIndex: number, solution: string): string {
  return `${testIndex}:${solution}`;
}

export function buildMatrixLookup(cells: readonly MatrixCellData[] | undefined): Map<string, MatrixCellData> {
  const map = new Map<string, MatrixCellData>();
  if (!cells) return map;
  for (const c of cells) {
    map.set(matrixCellKey(c.test_index, c.solution), c);
  }
  return map;
}

export function selectMatrixCell(
  cells: readonly MatrixCellData[] | undefined,
  selection: SelectedCellKey | undefined,
): MatrixCellData | undefined {
  if (!cells || !selection) return undefined;
  return cells.find((c) => c.test_index === selection.testIndex && c.solution === selection.solution);
}

/** Toggles an item in a subset filter array while preserving order. */
export function toggleSubsetSelection(current: readonly string[], item: string): string[] {
  if (current.includes(item)) {
    return current.filter((x) => x !== item);
  }
  return [...current, item];
}

/** Normalizes a manual C++ compiler path input: trimmed string or `null` when empty. */
export function normalizeCxxOverrideInput(raw: string): string | null {
  const trimmed = raw.trim();
  return trimmed.length > 0 ? trimmed : null;
}

/** True when the user configured a manual compiler override path that failed detection. */
export function hasInvalidCxxOverride(status: Pick<ToolchainStatusJs, "overridePath" | "active">): boolean {
  return Boolean(status.overridePath && !status.active);
}

/** Strips `.cpp` / `.17.cpp` / `.20.cpp` / `.23.cpp` suffix from a solution filename. */
export function logicalSolutionName(filename: string): string {
  return filename
    .replace(/\.(17|20|23)\.(cpp|cc|cxx)$/i, "")
    .replace(/\.(cpp|cc|cxx)$/i, "");
}

export interface SolutionHeaderSpec {
  name: string;
  source_rel?: string;
  is_main: boolean;
  expected: string[];
  tags?: string[] | null;
}

function normalizeExpected(spec: PackageSolutionSpecData): string[] {
  if (Array.isArray(spec.expected) && spec.expected.length > 0) return spec.expected;
  if (Array.isArray(spec.expect) && spec.expect.length > 0) return spec.expect;
  if (typeof spec.expect === "string" && spec.expect.trim().length > 0) return [spec.expect.trim()];
  return ["AC"];
}

/**
 * Resolves solution column headers with accurate `is_main`, `expected`, `source_rel`, and `tags`.
 *
 * P0-2 / Fix A-2: Structure ALWAYS comes from live disk `inspection` so adding/removing
 * solutions or editing `expect` updates immediately without waiting for a rebuild.
 */
export function resolveSolutionHeaderSpecs(
  inspection: PackageInspectionData | undefined,
  report: BuildReportData | undefined,
): SolutionHeaderSpec[] {
  const sourceByName = new Map<string, string>();
  for (const s of inspection?.solutions ?? []) {
    if (s.source_rel) sourceByName.set(s.name, s.source_rel);
  }
  for (const file of inspection?.files?.solutions ?? []) {
    const stem = logicalSolutionName(file);
    if (stem && !sourceByName.has(stem)) sourceByName.set(stem, `solutions/${file}`);
  }

  if (inspection?.solutions && inspection.solutions.length > 0) {
    return inspection.solutions.map((s) => ({
      name: s.name,
      source_rel: s.source_rel ?? sourceByName.get(s.name),
      is_main: Boolean(s.is_main ?? s.main),
      expected: normalizeExpected(s),
      tags: s.tags ?? null,
    }));
  }
  if (inspection?.files?.solutions && inspection.files.solutions.length > 0) {
    const names: string[] = [];
    for (const file of inspection.files.solutions) {
      const stem = logicalSolutionName(file);
      if (stem && !names.includes(stem)) names.push(stem);
    }
    return names.map((name) => ({
      name,
      source_rel: sourceByName.get(name),
      is_main: name === "std",
      expected: ["AC"],
      tags: null,
    }));
  }
  if (report?.solutions && report.solutions.length > 0) {
    return report.solutions.map((s) => ({
      name: s.name,
      source_rel: sourceByName.get(s.name),
      is_main: s.is_main,
      expected: s.expected.length > 0 ? s.expected : ["AC"],
      tags: s.tags ?? null,
    }));
  }
  return [];
}

/** Classifies a solution's role for badge display (`main` | `alt` | `wrong`). */
export function solutionRole(spec: Pick<SolutionHeaderSpec, "is_main" | "expected">): "main" | "alt" | "wrong" {
  if (spec.is_main) return "main";
  const exp = spec.expected.map((e) => e.toUpperCase());
  if (exp.length === 1 && exp[0] === "AC") return "alt";
  return "wrong";
}

/** Splits gates into `failed`, `warn`, and `passed` buckets, with `attention` strictly ordered Failed → Warn. */
export function partitionGates(gates: readonly GateResultData[]): {
  failed: GateResultData[];
  warn: GateResultData[];
  attention: GateResultData[];
  passed: GateResultData[];
} {
  const failed: GateResultData[] = [];
  const warn: GateResultData[] = [];
  const passed: GateResultData[] = [];
  for (const g of gates) {
    if (g.status === "fail") {
      failed.push(g);
    } else if (g.status === "warn") {
      warn.push(g);
    } else {
      passed.push(g);
    }
  }
  return { failed, warn, attention: [...failed, ...warn], passed };
}

/** Humanizes Core gate titles/messages in Chinese so no raw `validator`/`expect`/`门禁` jargon or awkward spaces leak. */
export function humanizeCheckText(text: string, isZh: boolean): string {
  if (!isZh) return text;
  let out = text
    .replace(/使用内置标准校验器/g, "使用内置标准评测器")
    .replace(/ *\bvalidator(?!\.(?:(?:17|20|23)\.)?(?:cpp|cc|cxx)\b)\b */gi, "校验器")
    .replace(/ *\bchecker(?!\.(?:(?:17|20|23)\.)?(?:cpp|cc|cxx)\b)\b */gi, "评测器")
    .replace(/ *\bexpect\b */gi, "预期")
    .replace(/\bvalid\b/gi, "有效")
    .replace(/\binvalid\b/gi, "无效")
    .replace(/门禁/g, "检查");
  // Preserve clean spacing between CJK and ASCII/numbers while avoiding leading/trailing spaces.
  out = out.replace(/([\u4e00-\u9fff]) +([\u4e00-\u9fff])/g, "$1$2");
  return out.trim();
}

export type CheckAction =
  | { kind: "view_test"; testIndex: number }
  | { kind: "select_cell"; testIndex: number; solution: string }
  | { kind: "open_file"; relPath: string }
  | { kind: "open_programs" };

/** Computes the most helpful one-click next-step action for a failed/warning check item. */
export function nextActionForCheck(
  gate: GateResultData,
  report: BuildReportData | undefined,
  files: PackageFilesData | undefined,
): CheckAction | undefined {
  const id = gate.id.toLowerCase();
  if (id.includes("validator") && id.includes("test") && !id.includes("self")) {
    const failedTest = report?.tests?.find((t) => t.validator_ok === false);
    if (failedTest) return { kind: "view_test", testIndex: failedTest.index };
    if (files?.validator) return { kind: "open_file", relPath: files.validator };
    return { kind: "open_programs" };
  }
  if (id.includes("validator")) {
    if (files?.validator) return { kind: "open_file", relPath: files.validator };
    return { kind: "open_programs" };
  }
  if (id.includes("checker")) {
    if (files?.custom_checker) return { kind: "open_file", relPath: files.custom_checker };
    return { kind: "open_programs" };
  }
  if (id.includes("alt") || id.includes("wrong") || id.includes("coverage") || id.includes("diversity") || (gate.status === "warn" && id.includes("solution"))) {
    return { kind: "open_file", relPath: "solutions" };
  }
  if (id.includes("solution") || id.includes("std")) {
    if (report?.matrix && report.solutions) {
      const solMap = new Map(report.solutions.map((s) => [s.name, s]));
      const anomalous = report.matrix.find((c) => {
        const s = solMap.get(c.solution);
        return s ? isCellVerdictAnomaly(c.verdict, s.expected, s.is_main, s.matched) : false;
      });
      if (anomalous) {
        return { kind: "select_cell", testIndex: anomalous.test_index, solution: anomalous.solution };
      }
    }
    return { kind: "open_file", relPath: "solutions" };
  }
  return undefined;
}

export const BUILTIN_CHECKERS = [
  "wcmp",
  "ncmp",
  "fcmp",
  "lcmp",
  "rcmp4",
  "rcmp6",
  "rcmp9",
  "yesno",
  "nyesno",
] as const;

export const EXPECT_PRESETS: readonly (readonly string[])[] = [
  ["AC"],
  ["WA"],
  ["TLE"],
  ["MLE"],
  ["RE"],
  ["WA", "TLE"],
];

export const MAX_INLINE_TEST_EDIT_BYTES = 64 * 1024;

/** True when a test row has a validator failure or at least one expectation-violating cell. */
export function isRowAnomalous(
  row: Pick<TestSummaryData, "index" | "validator_ok">,
  cols: readonly SolutionHeaderSpec[],
  matrixLookup: ReadonlyMap<string, MatrixCellData>,
  summaryByName: ReadonlyMap<string, SolutionSummaryData>,
): boolean {
  if (row.validator_ok === false) return true;
  for (const col of cols) {
    const cell = matrixLookup.get(matrixCellKey(row.index, col.name));
    if (!cell) continue;
    const sum = summaryByName.get(col.name);
    if (isCellVerdictAnomaly(cell.verdict, col.expected, col.is_main, sum?.matched)) {
      return true;
    }
  }
  return false;
}

/** True when a test case is a small hand-written/sample file safe for inline editing (<= 64 KiB). */
export function canInlineEditTest(preview: {
  ok: boolean;
  fileRel?: string | null;
  sizeBytes: number;
} | undefined): boolean {
  if (!preview?.ok || !preview.fileRel) return false;
  if (!preview.fileRel.startsWith("tests/") && !preview.fileRel.startsWith("samples/")) return false;
  return preview.sizeBytes <= MAX_INLINE_TEST_EDIT_BYTES;
}

/** Matches a `self_test_files` relative path against its latest `SelfTestResultData` entry. */
export function matchSelfTestResult(
  relPath: string,
  results: readonly SelfTestResultData[] | undefined,
): SelfTestResultData | undefined {
  if (!results || results.length === 0) return undefined;
  const parts = relPath.split("/");
  const fileName = parts[parts.length - 1] ?? "";
  if (relPath.startsWith("tests/validator/valid/")) {
    return results.find((r) => r.kind === "validator_valid" && r.name === fileName);
  }
  if (relPath.startsWith("tests/validator/invalid/")) {
    return results.find((r) => r.kind === "validator_invalid" && r.name === fileName);
  }
  if (relPath.startsWith("tests/checker/")) {
    const dirName = parts[2] ?? "";
    return results.find((r) => r.kind.startsWith("checker_") && r.name === dirName);
  }
  return undefined;
}

/**
 * Defensively deduplicates detected compilers by `(name, version)` preserving the first
 * PATH occurrence, while ensuring a manually active override path is never dropped.
 */
export function dedupDetectedCompilers(
  compilers: readonly DetectedCompilerJs[],
  active?: DetectedCompilerJs | null,
): DetectedCompilerJs[] {
  const out: DetectedCompilerJs[] = [];
  const seen = new Set<string>();
  for (const c of compilers) {
    const key = `${c.name}\0${c.version}`;
    if (seen.has(key)) {
      // If the active compiler was manually overridden to this duplicate path, replace the kept entry.
      if (active && c.path === active.path) {
        const idx = out.findIndex((x) => `${x.name}\0${x.version}` === key);
        if (idx >= 0) out[idx] = c;
      }
      continue;
    }
    seen.add(key);
    out.push(c);
  }
  if (active && !out.some((c) => c.path === active.path)) {
    out.unshift(active);
  }
  return out;
}

/** Middle-truncates long filesystem paths (e.g. Nix store paths) keeping the leading prefix and binary tail. */
export function middleTruncatePath(path: string, maxLen = 40): string {
  if (path.length <= maxLen) return path;
  const parts = path.split("/");
  if (parts.length >= 4) {
    const tail = parts.slice(-2).join("/");
    const head = parts.slice(0, 2).join("/");
    const candidate = `${head}/…/${tail}`;
    if (candidate.length <= maxLen) return candidate;
  }
  const keepTail = Math.max(12, Math.floor(maxLen * 0.55));
  const keepHead = Math.max(8, maxLen - keepTail - 1);
  return `${path.slice(0, keepHead)}…${path.slice(path.length - keepTail)}`;
}

/**
 * Merges persisted `report.matrix` cells with streaming `liveCells` from an active run
 * (live cells take precedence so results populate in real time without wiping untouched cells).
 */
export function mergeLiveMatrixCells(
  reportCells: readonly MatrixCellData[] | undefined,
  liveCells: readonly MatrixCellData[] | undefined,
): Map<string, MatrixCellData> {
  const map = new Map<string, MatrixCellData>();
  if (reportCells) {
    for (const c of reportCells) {
      map.set(matrixCellKey(c.test_index, c.solution), c);
    }
  }
  if (liveCells) {
    for (const c of liveCells) {
      map.set(matrixCellKey(c.test_index, c.solution), { ...c, stale: false });
    }
  }
  return map;
}

/**
 * P0-2 Quiet Results Table ("only anomalies speak"):
 * - Expected AC: transparent bg, no border, quiet muted text.
 * - Expected non-AC (wrong solution getting expected WA/TLE): transparent bg, no border, bold neutral foreground.
 * - Unexpected / mismatch ONLY: solid semantic fill (red for should-pass-but-failed, amber for should-fail-but-passed; NEVER green!).
 */
export function cellVisualStyle(
  verdict: string,
  isAnomaly: boolean,
  dark: boolean,
): { bg: string; fg: string; border: string; bold: boolean; solid: boolean } {
  const upper = verdict.toUpperCase();
  if (!isAnomaly) {
    if (upper === "AC") {
      return dark
        ? { bg: "transparent", fg: "#a1a1aa", border: "transparent", bold: false, solid: false }
        : { bg: "transparent", fg: "#71717a", border: "transparent", bold: false, solid: false };
    }
    return dark
      ? { bg: "transparent", fg: "#f4f4f5", border: "transparent", bold: true, solid: false }
      : { bg: "transparent", fg: "#18181b", border: "transparent", bold: true, solid: false };
  }
  // Unexpected AC (wrong solution passed!) -> solid vivid amber, NEVER green.
  if (upper === "AC") {
    return dark
      ? { bg: "#d97706", fg: "#ffffff", border: "transparent", bold: true, solid: true }
      : { bg: "#d97706", fg: "#ffffff", border: "transparent", bold: true, solid: true };
  }
  // Unexpected failure -> solid vivid red.
  return dark
    ? { bg: "#dc2626", fg: "#ffffff", border: "transparent", bold: true, solid: true }
    : { bg: "#dc2626", fg: "#ffffff", border: "transparent", bold: true, solid: true };
}

/**
 * P1-3 Conclusion-first diagnostic headline:
 * semantic color ONLY on the conclusion line (red = unexpected fail, amber = unexpected pass, muted = expected).
 */
export function formatCellConclusion(
  verdict: string,
  expected: readonly string[],
  isAnomaly: boolean,
  isZh: boolean,
): { text: string; tone: "red" | "amber" | "muted" } {
  const upper = verdict.toUpperCase();
  const expText = (expected.length > 0 ? expected : ["AC"]).join("/");
  if (!isAnomaly) {
    return {
      text: isZh ? `符合预期（${upper}）` : `Expected (${upper})`,
      tone: "muted",
    };
  }
  if (upper === "AC") {
    return {
      text: isZh ? `应为 ${expText}，实际通过` : `Expected ${expText}, got AC`,
      tone: "amber",
    };
  }
  return {
    text: isZh ? `应为 ${expText}，实际 ${upper}` : `Expected ${expText}, got ${upper}`,
    tone: "red",
  };
}

/**
 * P1-5 & P1-6 Human-readable solution summary conclusion:
 * - Never stuck on "运行中" when not actively running
 * - Preserves previous summary with `stale: true` on untouched columns after subset runs
 */
export function formatSolutionConclusion(
  summary: SolutionSummaryData | undefined,
  cellsForSolution: readonly MatrixCellData[],
  isZh: boolean,
  isRunning = false,
  prevSummary?: SolutionSummaryData,
): { text: string; tone: "ok" | "fail" | "muted"; stale?: boolean } {
  const effective = summary ?? prevSummary;
  const isStale = !summary && (Boolean(prevSummary) || (cellsForSolution.length > 0 && cellsForSolution.every((c) => c.stale)));
  if (!effective && cellsForSolution.length === 0) {
    return { text: isRunning ? (isZh ? "运行中" : "Running") : (isZh ? "未运行" : "Not run"), tone: "muted" };
  }
  if (effective?.matched) {
    return { text: isZh ? "符合预期" : "Expected", tone: "ok", stale: isStale };
  }
  const primary = effective?.actual_verdict ?? cellsForSolution[0]?.verdict ?? "ERR";
  const count = cellsForSolution.filter((c) => c.verdict.toUpperCase() === primary.toUpperCase()).length;
  if (effective && !effective.matched) {
    return {
      text: count > 0 ? `${primary} ×${count}` : primary,
      tone: "fail",
      stale: isStale,
    };
  }
  if (isRunning) {
    return { text: isZh ? "运行中" : "Running", tone: "muted" };
  }
  return { text: isZh ? "已缓存" : "Cached", tone: "muted", stale: true };
}

/** P1-5: Preserves the last valid full-run safety margin when a subset run excludes `std`. */
export function resolveEffectiveSafetyMargin(
  report: BuildReportData | undefined,
  lastFullMargin: SafetyMarginData | undefined,
): SafetyMarginData | undefined {
  if (!report) return lastFullMargin;
  if (report.subset && report.safety_margin.std_max_time_ms === 0 && lastFullMargin) {
    return lastFullMargin;
  }
  return report.safety_margin;
}

export type SingleStatusKind = "ready" | "needs_reverify" | "issues" | "unverified";

/**
 * P0-2 Single Status Model: collapses all freshness + gate + compile failure signals into ONE
 * top-bar status pill ("可用于比赛" / "需要重新验证" / "N 项问题" / "尚未验证").
 */
export function resolveSingleStatusBadge(
  freshness: string | undefined,
  failedGateCount: number,
  compileFailureCount: number,
  hasReport: boolean,
): { kind: SingleStatusKind; issueCount: number } {
  const issues = failedGateCount + compileFailureCount;
  if (freshness === "verified_fresh" && issues === 0) {
    return { kind: "ready", issueCount: 0 };
  }
  if (freshness === "failed" || issues > 0) {
    return { kind: "issues", issueCount: Math.max(1, issues) };
  }
  if (freshness === "stale" || hasReport) {
    return { kind: "needs_reverify", issueCount: 0 };
  }
  return { kind: "unverified", issueCount: 0 };
}

export interface OnboardingStep {
  id: "validator" | "std" | "tests" | "wrong";
  done: boolean;
  targetFile?: string;
}

/**
 * Computes the 4-step onboarding checklist for a problem package:
 * 1. Validator present & self-tested (or exists)
 * 2. Main solution (`std`) exists and passes AC (or exists when unbuilt)
 * 3. Non-sample tests (generator or hand tests) configured
 * 4. At least one non-main (wrong/alt) solution added
 * Auto-hides once all 4 steps are done and the package is verified.
 */
export function computeOnboardingChecklist(
  inspection: PackageInspectionData | undefined,
  report: BuildReportData | undefined,
): { show: boolean; steps: OnboardingStep[] } {
  if (!inspection?.has_package) return { show: false, steps: [] };
  const hasValidator = Boolean(inspection.files?.validator);
  const selfTestsOk =
    (report?.self_tests?.length ?? 0) > 0 && report!.self_tests!.every((s) => s.passed);
  const validatorDone = hasValidator && (report ? selfTestsOk : false);

  const sols = resolveSolutionHeaderSpecs(inspection, report);
  const mainSol = sols.find((s) => s.is_main);
  const mainReport = report?.solutions?.find((s) => s.is_main);
  const stdDone = Boolean(mainSol && mainReport?.matched);

  const nonSampleTests = (inspection.expanded_tests ?? []).filter((t) => !t.is_sample);
  const testsDone = nonSampleTests.length > 0 && Boolean(report?.tests?.some((t) => !t.is_sample && t.validator_ok));

  const wrongSol = sols.find((s) => !s.is_main);
  const wrongDone = Boolean(wrongSol && report?.solutions?.some((s) => !s.is_main && s.matched));

  const steps: OnboardingStep[] = [
    { id: "validator", done: validatorDone, targetFile: inspection.files?.validator ?? undefined },
    { id: "std", done: stdDone, targetFile: mainSol?.source_rel },
    { id: "tests", done: testsDone, targetFile: inspection.files?.generators?.[0] ? `generators/${inspection.files.generators[0]}` : undefined },
    { id: "wrong", done: wrongDone, targetFile: wrongSol?.source_rel },
  ];
  const allDone = steps.every((s) => s.done) && report?.verified === true;
  return { show: !allDone, steps };
}

/** Counts how many expanded test cases belong to a given 0-based `[[test]]` rule index. */
export function countTestsInRule(
  expandedTests: readonly ExpandedTestData[] | undefined,
  ruleIndex: number | null | undefined,
): number {
  if (!expandedTests || ruleIndex == null) return 0;
  return expandedTests.filter((t) => t.rule_index === ruleIndex).length;
}

export interface LogicalTestGroup {
  id: string;
  kind: "samples" | "rule";
  ruleIndex: number | null;
  label: string;
  genCmd?: string | null;
  tag?: string | null;
  tests: TestSummaryData[];
  hasValidatorFail: boolean;
}

/**
 * Groups expanded test cases into clean logical tree entries:
 * - One `samples` group (if any sample exists)
 * - One `rule` group per `[[test]]` rule (single hand test shows its name; multi-test batch shows `<stem> ×<count>`)
 */
export function groupTestRows(
  expandedTests: readonly ExpandedTestData[] | undefined,
  testRows: readonly TestSummaryData[],
): LogicalTestGroup[] {
  const rowByIdx = new Map<number, TestSummaryData>();
  for (const r of testRows) rowByIdx.set(r.index, r);

  const samples: TestSummaryData[] = [];
  const byRule = new Map<number, { tests: TestSummaryData[]; genCmd?: string | null }>();
  const ruleOrder: number[] = [];

  if (expandedTests && expandedTests.length > 0) {
    for (const et of expandedTests) {
      const row = rowByIdx.get(et.index) ?? {
        index: et.index,
        name: et.name,
        tag: et.tag ?? null,
        is_sample: et.is_sample,
        size_bytes: 0,
        gen_cmd: et.gen_cmd ?? null,
        validator_ok: null,
        validator_message: null,
      };
      if (et.is_sample || et.rule_index == null) {
        samples.push(row);
      } else {
        let bucket = byRule.get(et.rule_index);
        if (!bucket) {
          bucket = { tests: [], genCmd: et.gen_cmd ?? row.gen_cmd };
          byRule.set(et.rule_index, bucket);
          ruleOrder.push(et.rule_index);
        }
        bucket.tests.push(row);
      }
    }
  } else {
    for (const row of testRows) {
      if (row.is_sample) samples.push(row);
      else {
        const fakeIdx = row.index;
        byRule.set(fakeIdx, { tests: [row], genCmd: row.gen_cmd });
        ruleOrder.push(fakeIdx);
      }
    }
  }

  const out: LogicalTestGroup[] = [];
  if (samples.length > 0) {
    out.push({
      id: "samples",
      kind: "samples",
      ruleIndex: null,
      label: `samples`,
      tests: samples,
      hasValidatorFail: samples.some((t) => t.validator_ok === false),
    });
  }
  for (const rIdx of ruleOrder) {
    const bucket = byRule.get(rIdx)!;
    const first = bucket.tests[0];
    const batchTag = first?.tag?.trim() || null;
    let label = first?.name ?? `rule-${rIdx + 1}`;
    if (bucket.tests.length > 1) {
      const genStem = bucket.genCmd?.trim().split(/\s+/)[0];
      const base =
        genStem && genStem.length > 0
          ? genStem
          : label.replace(/-\d+$/, "");
      label = batchTag ? `${base} (${batchTag}) ×${bucket.tests.length}` : `${base} ×${bucket.tests.length}`;
    }
    out.push({
      id: `rule:${rIdx}`,
      kind: "rule",
      ruleIndex: rIdx,
      label,
      genCmd: bucket.genCmd ?? null,
      tag: batchTag,
      tests: bucket.tests,
      hasValidatorFail: bucket.tests.some((t) => t.validator_ok === false),
    });
  }
  return out;
}

// Pure TypeScript types and helpers for the Local Polygon Build Console & Toolchain settings.
// Kept pure (no NSH JSX dependencies) so every state transition and formatter is unit-tested.

import type { ToolchainStatusJs } from "xcpc-workstation/native";

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
}

export interface ExpandedTestData {
  index: number;
  name: string;
  tag?: string | null;
  is_sample: boolean;
  gen_cmd?: string | null;
}

export interface PackageInspectionData {
  problem_id: string;
  has_package: boolean;
  package_hash: string;
  freshness: { status: string; summary?: string };
  checker: string;
  files: PackageFilesData;
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

/** Formats slowest std ratio, e.g. `0.38s / 2.00s = 19%`. */
export function formatStdTimeRatio(margin: SafetyMarginData | undefined): string {
  if (!margin || margin.time_limit_ms <= 0) return "–";
  const pct = Math.round(margin.std_time_ratio * 100);
  return `${formatSecondsFromMs(margin.std_max_time_ms)} / ${formatSecondsFromMs(margin.time_limit_ms)} = ${pct}%`;
}

/** Formats AC vs TLE margin when both slowest AC and fastest TLE exist. */
export function formatAcVsTleMargin(margin: SafetyMarginData | undefined): string {
  if (!margin || margin.slowest_ac_time_ms == null || margin.fastest_tle_time_ms == null) {
    return "–";
  }
  const ac = margin.slowest_ac_time_ms;
  const tle = margin.fastest_tle_time_ms;
  const ratio = ac > 0 ? `${(tle / ac).toFixed(1)}×` : "∞";
  return `${formatSecondsFromMs(ac)} vs ${formatSecondsFromMs(tle)} (${ratio})`;
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

// Pure TypeScript types & helpers for the Phase 4B Training space.
// Free of `nsh` runtime imports so unit tests can exercise all data transforms headlessly.

import { ratingTier, type TierName } from "../logic";

export type TimeWindow = "30d" | "90d" | "year" | "all";
export type TrainingTab = "overview" | "contests" | "submissions" | "editorials";
export type HeatmapMode = "count" | "rating";
export type ManualLogStatus = "ac" | "upsolved" | "attempted" | "todo";

export interface EditorialSourceItem {
  platform: string;
  id: string;
  url?: string | null;
}

export interface EditorialSummaryItem {
  problemId: string;
  title: string;
  rating?: number | null;
  tags: string[];
  sources: EditorialSourceItem[];
  excerpt: string;
  updatedAt: string;
}

export interface EditorialFilter {
  search: string;
  tag: string;
  minRating: number | null;
  maxRating: number | null;
}

export const DEFAULT_EDITORIAL_FILTER: EditorialFilter = {
  search: "",
  tag: "",
  minRating: null,
  maxRating: null,
};

export interface PlatformAccountMetric {
  platform: string;
  handle: string;
  rating?: number | null;
  max_rating?: number | null;
  rank?: string | null;
  solved_count: number;
  submission_count: number;
  updated_at?: string | null;
}

export interface TrainingKpi {
  today_ac: number;
  today_submissions: number;
  week_ac: number;
  week_submissions: number;
  month_ac: number;
  month_submissions: number;
  window_ac: number;
  window_submissions: number;
  total_ac: number;
  total_submissions: number;
  streak_days: number;
  longest_streak_days: number;
  unsolved_attempted_count: number;
  library_todo_count: number;
  library_attempted_count: number;
}

export interface HeatmapCell {
  date: string;
  ac_count: number;
  submission_count: number;
  max_rating?: number | null;
}

export interface RatingPoint {
  contest_id: string;
  contest_name: string;
  rating_before?: number | null;
  rating_after: number;
  delta?: number | null;
  rank?: number | null;
  at: number;
}

export interface PlatformRatingSeries {
  platform: string;
  points: RatingPoint[];
}

export interface DifficultyTrendPoint {
  period: string;
  p75?: number | null;
  p90?: number | null;
  median?: number | null;
  ac_count: number;
  rated_ac_count: number;
}

export interface DifficultyBucket {
  label: string;
  min_rating?: number | null;
  max_rating?: number | null;
  ac_count: number;
}

export interface TagStat {
  tag: string;
  ac_count: number;
  avg_rating?: number | null;
}

export interface SubmissionViewRow {
  platform: string;
  submission_id: string;
  handle: string;
  contest_id?: string | null;
  problem_index?: string | null;
  problem_key: string;
  problem_title?: string | null;
  problem_url?: string | null;
  raw_rating?: number | null;
  normalized_rating?: number | null;
  tags: string[];
  verdict: string;
  language?: string | null;
  submitted_at: number;
  attempt_count: number;
  solved: boolean;
  library_problem_id?: string | null;
}

export interface WorkspaceBacklogItem {
  problem_id: string;
  title: string;
  status: string;
  rating?: number | null;
  tags: string[];
}

export interface TrainingDashboardData {
  window: TimeWindow;
  last_synced_at?: string | null;
  has_local_records?: boolean;
  local_solved_count?: number;
  accounts: PlatformAccountMetric[];
  kpi: TrainingKpi;
  available_years: number[];
  heatmap_year: number;
  heatmap: HeatmapCell[];
  rating_series: PlatformRatingSeries[];
  difficulty_trend: DifficultyTrendPoint[];
  difficulty_buckets: DifficultyBucket[];
  tag_distribution: TagStat[];
  unsolved_problems: SubmissionViewRow[];
  workspace_backlog: WorkspaceBacklogItem[];
}

export interface ContestCalendarItem {
  platform: string;
  contest_id: string;
  name: string;
  start_time: number;
  duration_sec: number;
  end_time: number;
  status: "running" | "upcoming" | "finished" | string;
  url?: string | null;
  updated_at: string;
}

export interface SubmissionsPage {
  total: number;
  offset: number;
  limit: number;
  items: SubmissionViewRow[];
}

export interface SubmissionsFilter {
  byProblem: boolean;
  platform: string;
  verdict: string;
  unsolvedOnly: boolean;
  minRating: number | null;
  maxRating: number | null;
  tag: string;
  search: string;
}

export const DEFAULT_SUBMISSIONS_FILTER: SubmissionsFilter = {
  byProblem: false,
  platform: "",
  verdict: "",
  unsolvedOnly: false,
  minRating: null,
  maxRating: null,
  tag: "",
  search: "",
};

/** Returns true when any submission filter (beyond raw mode) is active. */
export function hasActiveSubmissionsFilter(f: SubmissionsFilter): boolean {
  return (
    f.platform !== "" ||
    f.verdict !== "" ||
    f.unsolvedOnly ||
    f.minRating !== null ||
    f.maxRating !== null ||
    f.tag !== "" ||
    f.search.trim() !== ""
  );
}

/** Builds a submissions filter when drilling down from a difficulty bucket bar on Overview. */
export function drilldownFromBucket(bucket: DifficultyBucket): SubmissionsFilter {
  return {
    ...DEFAULT_SUBMISSIONS_FILTER,
    byProblem: true,
    verdict: "AC",
    minRating: bucket.min_rating ?? null,
    maxRating: bucket.max_rating ?? null,
  };
}

/** Builds a submissions filter when drilling down from a tag bar on Overview. */
export function drilldownFromTag(tag: string, minRating: number | null): SubmissionsFilter {
  return {
    ...DEFAULT_SUBMISSIONS_FILTER,
    byProblem: true,
    verdict: "AC",
    tag,
    minRating,
  };
}

// ── Platform & verdict formatting ────────────────────────────────────────────

export function platformDisplayName(platform: string, isEn = false): string {
  switch (platform.trim().toLowerCase()) {
    case "local":
      return isEn ? "Local" : "本地";
    case "codeforces":
      return "Codeforces";
    case "atcoder":
      return "AtCoder";
    case "luogu":
      return "洛谷";
    case "vjudge":
      return "VJudge";
    case "hdu":
      return "HDU";
    case "poj":
      return "POJ";
    case "uva":
      return "UVA";
    default:
      return platform ? platform.charAt(0).toUpperCase() + platform.slice(1) : "";
  }
}

export type VerdictTone = "ac" | "wa" | "tle" | "attempted" | "todo" | "other";

export function verdictTone(verdict: string): VerdictTone {
  const v = verdict.trim().toUpperCase();
  if (v === "AC" || v === "OK") return "ac";
  if (v === "WA" || v === "RE" || v === "CE") return "wa";
  if (v === "TLE" || v === "MLE" || v === "ILE") return "tle";
  if (v === "ATTEMPTED") return "attempted";
  if (v === "TODO") return "todo";
  return "other";
}

export function verdictDisplayLabel(verdict: string, isEn = false): string {
  const v = verdict.trim().toUpperCase();
  if (v === "ATTEMPTED") return isEn ? "Attempted" : "尝试中";
  if (v === "TODO") return isEn ? "Todo" : "待补";
  return verdict;
}

export function formatIsoDateShort(iso: string | null | undefined): string {
  if (!iso) return "";
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso.slice(0, 10);
  return `${d.getFullYear()}-${pad2(d.getMonth() + 1)}-${pad2(d.getDate())}`;
}

// ── Algorithm tag localization (zh-CN concise labels) ────────────────────────

const TAG_ZH_LABELS: Record<string, string> = {
  dp: "动态规划",
  graphs: "图论",
  trees: "树",
  math: "数学",
  "data structures": "数据结构",
  greedy: "贪心",
  strings: "字符串",
  "binary search": "二分",
  implementation: "模拟",
  "constructive algorithms": "构造",
  "number theory": "数论",
  combinatorics: "组合数学",
  geometry: "几何",
  flows: "网络流",
  bitmasks: "位运算",
  "dfs and similar": "搜索",
  "two pointers": "双指针",
  sortings: "排序",
  "brute force": "暴力",
  "shortest paths": "最短路",
  dsu: "并查集",
  games: "博弈",
  probabilities: "概率",
  "divide and conquer": "分治",
  interactive: "交互",
  hashing: "哈希",
  matrices: "矩阵",
  fft: "FFT",
  "graph matchings": "图匹配",
  "string suffix structures": "后缀结构",
  "ternary search": "三分",
  "meet-in-the-middle": "折半搜索",
  "2-sat": "2-SAT",
  "chinese remainder theorem": "中国剩余定理",
  "expression parsing": "表达式解析",
};

/** Translates common OJ algorithm tags to concise Chinese in zh-CN; falls back to raw tag. */
export function formatTagLabel(tag: string, isEn = false): string {
  if (isEn) return tag;
  const key = tag.trim().toLowerCase();
  return TAG_ZH_LABELS[key] ?? tag;
}

// ── Rating bands (classic Codeforces / AtCoder scale) ────────────────────────

export interface RatingBand {
  min: number;
  max: number;
  lightFill: string;
  darkFill: string;
}

export const RATING_BANDS: readonly RatingBand[] = [
  { min: 0, max: 1200, lightFill: "rgba(161,161,170,0.05)", darkFill: "rgba(161,161,170,0.05)" },
  { min: 1200, max: 1400, lightFill: "rgba(21,128,61,0.045)", darkFill: "rgba(74,222,128,0.05)" },
  { min: 1400, max: 1600, lightFill: "rgba(14,116,144,0.05)", darkFill: "rgba(34,211,238,0.05)" },
  { min: 1600, max: 1900, lightFill: "rgba(29,78,216,0.045)", darkFill: "rgba(96,165,250,0.05)" },
  { min: 1900, max: 2100, lightFill: "rgba(126,34,206,0.045)", darkFill: "rgba(192,132,252,0.05)" },
  { min: 2100, max: 2400, lightFill: "rgba(194,65,12,0.045)", darkFill: "rgba(251,146,60,0.05)" },
  { min: 2400, max: 3000, lightFill: "rgba(185,28,28,0.045)", darkFill: "rgba(248,113,113,0.05)" },
  { min: 3000, max: 4500, lightFill: "rgba(153,27,27,0.065)", darkFill: "rgba(239,68,68,0.07)" },
];

/** Generates clean round Y-axis tick values covering `[minY, maxY]` (including 3000+). */
export function niceRatingTicks(minY: number, maxY: number): number[] {
  const span = Math.max(200, maxY - minY);
  const step = span <= 800 ? 200 : span <= 1600 ? 400 : 500;
  const first = Math.ceil(minY / step) * step;
  const ticks: number[] = [];
  for (let v = first; v <= maxY; v += step) {
    ticks.push(v);
  }
  return ticks;
}

/** Picks evenly spaced X-axis tick indices for a chart with `count` points. */
export function niceXTickIndices(count: number, maxTicks = 5): number[] {
  if (count <= 0) return [];
  if (count === 1) return [0];
  if (count <= maxTicks) return Array.from({ length: count }, (_, i) => i);
  const res: number[] = [0];
  const segments = maxTicks - 1;
  for (let i = 1; i < segments; i++) {
    const idx = Math.round((i / segments) * (count - 1));
    if (idx > res[res.length - 1] && idx < count - 1) res.push(idx);
  }
  res.push(count - 1);
  return res;
}

// ── Annual dual-mode heatmap grid builder ────────────────────────────────────

export interface HeatmapGridDay {
  date: string;
  weekIndex: number;
  dayOfWeek: number; // 0 = Sun .. 6 = Sat
  month: number; // 1..12
  acCount: number;
  submissionCount: number;
  maxRating: number | null;
}

export interface HeatmapGrid {
  year: number;
  weeks: number;
  days: HeatmapGridDay[];
  monthStarts: { month: number; weekIndex: number }[];
}

function pad2(n: number): string {
  return n < 10 ? `0${n}` : String(n);
}

/** Builds a full-year calendar grid (Sunday-start weeks, 52–54 columns) merged with sparse `HeatmapCell`s. */
export function buildYearHeatmapGrid(year: number, cells: readonly HeatmapCell[]): HeatmapGrid {
  const byDate = new Map<string, HeatmapCell>();
  for (const c of cells) byDate.set(c.date, c);

  const jan1 = new Date(Date.UTC(year, 0, 1));
  const startDow = jan1.getUTCDay(); // 0..6
  const isLeap = (year % 4 === 0 && year % 100 !== 0) || year % 400 === 0;
  const totalDays = isLeap ? 366 : 365;

  const days: HeatmapGridDay[] = [];
  const monthStarts: { month: number; weekIndex: number }[] = [];
  let lastMonth = 0;

  for (let i = 0; i < totalDays; i++) {
    const dt = new Date(Date.UTC(year, 0, 1 + i));
    const m = dt.getUTCMonth() + 1;
    const d = dt.getUTCDate();
    const date = `${year}-${pad2(m)}-${pad2(d)}`;
    const weekIndex = Math.floor((startDow + i) / 7);
    const dayOfWeek = (startDow + i) % 7;
    if (m !== lastMonth) {
      monthStarts.push({ month: m, weekIndex });
      lastMonth = m;
    }
    const cell = byDate.get(date);
    days.push({
      date,
      weekIndex,
      dayOfWeek,
      month: m,
      acCount: cell?.ac_count ?? 0,
      submissionCount: cell?.submission_count ?? 0,
      maxRating: cell?.max_rating ?? null,
    });
  }

  const weeks = days.length > 0 ? days[days.length - 1].weekIndex + 1 : 53;
  return { year, weeks, days, monthStarts };
}

const COUNT_PALETTE_LIGHT = ["#EFEFF1", "#BBE6D0", "#6ED2A0", "#22A96B", "#147A4B"];
const COUNT_PALETTE_DARK = ["#1C1C20", "#113A28", "#16623F", "#22965E", "#4ADE80"];

const RATING_HEAT_LIGHT: Record<TierName, string> = {
  newbie: "#9CA3AF",
  pupil: "#22C55E",
  specialist: "#06B6D4",
  expert: "#3B82F6",
  candidate_master: "#A855F7",
  master: "#F97316",
  grandmaster: "#EF4444",
  unrated: "#CBD5E1",
};

const RATING_HEAT_DARK: Record<TierName, string> = {
  newbie: "#71717A",
  pupil: "#4ADE80",
  specialist: "#22D3EE",
  expert: "#60A5FA",
  candidate_master: "#C084FC",
  master: "#FB923C",
  grandmaster: "#F87171",
  unrated: "#52525B",
};

/** Returns the fill color for one heatmap cell in `"count"` or `"rating"` mode. */
export function heatmapCellColor(day: HeatmapGridDay, mode: HeatmapMode, dark: boolean): string {
  const empty = dark ? COUNT_PALETTE_DARK[0] : COUNT_PALETTE_LIGHT[0];
  if (mode === "count") {
    if (day.acCount <= 0) return empty;
    const level = day.acCount >= 6 ? 4 : day.acCount >= 4 ? 3 : day.acCount >= 2 ? 2 : 1;
    return (dark ? COUNT_PALETTE_DARK : COUNT_PALETTE_LIGHT)[level];
  }
  if (day.acCount <= 0) return empty;
  if (day.maxRating == null) {
    return dark ? RATING_HEAT_DARK.unrated : RATING_HEAT_LIGHT.unrated;
  }
  const tier = ratingTier(day.maxRating);
  return dark ? RATING_HEAT_DARK[tier] : RATING_HEAT_LIGHT[tier];
}

// ── Time formatting ──────────────────────────────────────────────────────────

export function formatUnixDateTime(unixSec: number): string {
  if (!Number.isFinite(unixSec) || unixSec <= 0) return "";
  const d = new Date(unixSec * 1000);
  if (Number.isNaN(d.getTime())) return "";
  return `${d.getFullYear()}-${pad2(d.getMonth() + 1)}-${pad2(d.getDate())} ${pad2(d.getHours())}:${pad2(d.getMinutes())}`;
}

export function formatUnixDateShort(unixSec: number): string {
  if (!Number.isFinite(unixSec) || unixSec <= 0) return "";
  const d = new Date(unixSec * 1000);
  if (Number.isNaN(d.getTime())) return "";
  return `${d.getFullYear()}-${pad2(d.getMonth() + 1)}-${pad2(d.getDate())}`;
}

export function formatUnixYearMonth(unixSec: number): string {
  if (!Number.isFinite(unixSec) || unixSec <= 0) return "";
  const d = new Date(unixSec * 1000);
  if (Number.isNaN(d.getTime())) return "";
  return `${d.getFullYear()}-${pad2(d.getMonth() + 1)}`;
}

const WEEKDAYS_ZH = ["周日", "周一", "周二", "周三", "周四", "周五", "周六"] as const;
const WEEKDAYS_EN = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"] as const;

export function formatContestStartBrief(unixSec: number, isEn = false): string {
  if (!Number.isFinite(unixSec) || unixSec <= 0) return "";
  const d = new Date(unixSec * 1000);
  if (Number.isNaN(d.getTime())) return "";
  const wd = (isEn ? WEEKDAYS_EN : WEEKDAYS_ZH)[d.getDay()];
  return `${wd} ${pad2(d.getHours())}:${pad2(d.getMinutes())}`;
}

export function formatDuration(durationSec: number, isEn = false): string {
  const mins = Math.max(0, Math.round(durationSec / 60));
  const totalHours = Math.floor(mins / 60);
  const remMins = mins % 60;
  if (totalHours >= 48) {
    const days = Math.floor(totalHours / 24);
    const remHours = totalHours % 24;
    if (isEn) {
      return remHours === 0 ? `${days}d` : `${days}d ${remHours}h`;
    }
    return remHours === 0 ? `${days} 天` : `${days} 天 ${remHours} 小时`;
  }
  if (isEn) {
    if (totalHours === 0) return `${remMins}m`;
    if (remMins === 0) return `${totalHours}h`;
    return `${totalHours}h ${remMins}m`;
  }
  if (totalHours === 0) return `${remMins} 分`;
  if (remMins === 0) return `${totalHours} 小时`;
  return `${totalHours} 小时 ${remMins} 分`;
}

export function formatCountdown(targetUnixSec: number, nowUnixSec: number, isEn = false): string {
  const diff = Math.max(0, Math.floor(targetUnixSec - nowUnixSec));
  const days = Math.floor(diff / 86_400);
  const hours = Math.floor((diff % 86_400) / 3_600);
  const mins = Math.floor((diff % 3_600) / 60);
  if (isEn) {
    if (days > 0) return hours > 0 ? `${days}d ${hours}h` : `${days}d`;
    if (hours > 0) return mins > 0 ? `${hours}h ${mins}m` : `${hours}h`;
    return `${Math.max(1, mins)}m`;
  }
  if (days > 0) return `${days} 天`;
  if (hours > 0) return mins > 0 ? `${hours} 小时 ${mins} 分` : `${hours} 小时`;
  return `${Math.max(1, mins)} 分`;
}

export type ContestTimeGroup = "today" | "thisWeek" | "later" | "finished";

export interface GroupedContests {
  group: ContestTimeGroup;
  items: ContestCalendarItem[];
}

/** Groups contests into Today / This Week / Later / Finished buckets using local calendar boundaries. */
export function groupContestsByTime(
  contests: readonly ContestCalendarItem[],
  nowUnixSec: number,
): GroupedContests[] {
  const nowDate = new Date(nowUnixSec * 1000);
  const startOfToday = new Date(nowDate.getFullYear(), nowDate.getMonth(), nowDate.getDate()).getTime() / 1000;
  const endOfToday = startOfToday + 86_400;
  const endOfWeek = startOfToday + 7 * 86_400;

  const buckets: Record<ContestTimeGroup, ContestCalendarItem[]> = {
    today: [],
    thisWeek: [],
    later: [],
    finished: [],
  };

  for (const c of contests) {
    if (c.status === "finished") {
      buckets.finished.push(c);
    } else if (c.status === "running" || c.start_time < endOfToday) {
      buckets.today.push(c);
    } else if (c.start_time < endOfWeek) {
      buckets.thisWeek.push(c);
    } else {
      buckets.later.push(c);
    }
  }

  const order: ContestTimeGroup[] = ["today", "thisWeek", "later", "finished"];
  return order
    .filter((g) => buckets[g].length > 0)
    .map((group) => ({ group, items: buckets[group] }));
}

export function formatSyncTimestamp(iso: string | null | undefined, neverText: string): string {
  if (!iso) return neverText;
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  return `${d.getFullYear()}-${pad2(d.getMonth() + 1)}-${pad2(d.getDate())} ${pad2(d.getHours())}:${pad2(d.getMinutes())}`;
}

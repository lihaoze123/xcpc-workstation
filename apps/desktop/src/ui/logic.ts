// Pure UI logic, free of `nsh` and the native bridge, so `App.test.tsx` runs it
// headless: preview geometry, ratings, reordering, samples, export state, titles and
// the error wording of docs/design-system.md §7.3.

import type {
  ExportBookStatusJs,
  PreviewDiagnostic,
  PreviewView,
  SamplePairJs,
} from "xcpc-workstation/native";
import { t } from "./i18n";

// ── Preview geometry ─────────────────────────────────────────────────────────

export const GAP = 16; // points between pages, at zoom 1
export const MIN_ZOOM = 0.1;
export const MAX_ZOOM = 8;
export const TILE = 512; // pixels, as the native side tiles
export const BASE_SCALE = 1.5; // above this the native side tiles the part in view

export type PageSize = { width: number; height: number };
export const DEFAULT_PAGE: PageSize = { width: 595.28, height: 841.89 }; // A4 in points

/** Pixels per point to rasterize at, in half steps so a zoom does not re-rasterize on every tick. */
export function scaleFor(z: number, dpr: number): number {
  return Math.min(Math.ceil(Math.max(z, 0.5) * dpr * 2) / 2, 8);
}

export function fitZoomFor(list: readonly { width: number }[], paneWidth: number): number {
  if (list.length === 0 || paneWidth <= 0) return 0;
  return Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, (paneWidth - 2 * GAP) / list[0].width));
}

export function contentHeightFor(list: readonly { height: number }[], z: number): number {
  if (list.length === 0 || z <= 0) return 0;
  return list.reduce((h, p) => h + (p.height + GAP) * z, GAP * z);
}

/** Where page `index` starts in the scroll view. */
export function pageTopFor(list: readonly { height: number }[], z: number, index: number): number {
  return list.slice(0, index).reduce((y, p) => y + (p.height + GAP) * z, 0);
}

/** The page whose area holds the middle of the viewport (0-based). */
export function pageAt(list: readonly { height: number }[], z: number, scrollY: number, paneHeight: number): number {
  if (list.length === 0 || z <= 0) return 0;
  const mid = scrollY + paneHeight / 3;
  let y = GAP * z;
  for (let i = 0; i < list.length; i++) {
    y += (list[i].height + GAP) * z;
    if (mid < y) return i;
  }
  return list.length - 1;
}

/** The column's left edge: centered when it fits, else where the user panned it. */
export function leftFor(pageWidth: number, z: number, paneWidth: number, curPanX: number): number {
  const w = pageWidth * z;
  const room = paneWidth - w;
  if (room >= 2 * GAP) return room / 2;
  return Math.min(GAP, Math.max(room - GAP, curPanX));
}

/** The part of each page in view, in its points, and the pages on either side (whole-page layers only). */
export function computeView(
  list: readonly PageSize[],
  z: number,
  curScrollY: number,
  curPanX: number,
  curPane: { width: number; height: number },
): PreviewView[] {
  const edge = (page: number): PreviewView => ({ page, x: 0, y: 0, width: 0, height: 0 });
  if (list.length === 0 || z === 0) return [edge(0), edge(1)];
  const curLeft = leftFor(list[0].width, z, curPane.width, curPanX);
  const top = curScrollY;
  const bottom = top + curPane.height;
  const x0 = Math.max(0, -curLeft / z);
  const out: PreviewView[] = [];
  let y = GAP * z;
  list.forEach((page, i) => {
    const h = page.height * z;
    if (y + h >= top && y <= bottom) {
      const y0 = Math.max(0, (top - y) / z);
      const y1 = Math.min(page.height, (bottom - y) / z);
      const x1 = Math.min(page.width, (curPane.width - curLeft) / z);
      out.push({ page: i, x: x0, y: y0, width: Math.max(0, x1 - x0), height: Math.max(0, y1 - y0) });
    }
    y += h + GAP * z;
  });
  if (out.length === 0) return [edge(0), edge(1)];
  if (out[0].page > 0) out.unshift(edge(out[0].page - 1));
  if (out[out.length - 1].page < list.length - 1) out.push(edge(out[out.length - 1].page + 1));
  return out;
}

/** A point in the scroll content (zoomed points, from the content's top left) as a page and its typographic points, or `null` between pages. */
export function contentToPage(
  list: readonly PageSize[],
  z: number,
  left: number,
  x: number,
  y: number,
): { page: number; xPt: number; yPt: number } | null {
  if (list.length === 0 || z <= 0) return null;
  let top = GAP * z;
  for (let i = 0; i < list.length; i++) {
    const h = list[i].height * z;
    if (y >= top && y <= top + h) {
      const xPt = (x - left) / z;
      const yPt = (y - top) / z;
      if (xPt < 0 || xPt > list[i].width) return null;
      return { page: i, xPt, yPt };
    }
    top += h + GAP * z;
  }
  return null;
}

// ── Ratings ──────────────────────────────────────────────────────────────────

export type TierName =
  | "newbie"
  | "pupil"
  | "specialist"
  | "expert"
  | "candidate_master"
  | "master"
  | "grandmaster"
  | "unrated";

/** The 7 rated tiers with their ranges (`max` exclusive), as §3.3 lists them. */
export const TIER_RANGES: { tier: TierName; min: number; max: number | null; label: string }[] = [
  { tier: "newbie", min: 0, max: 1200, label: "< 1200" },
  { tier: "pupil", min: 1200, max: 1400, label: "1200 – 1399" },
  { tier: "specialist", min: 1400, max: 1600, label: "1400 – 1599" },
  { tier: "expert", min: 1600, max: 1900, label: "1600 – 1899" },
  { tier: "candidate_master", min: 1900, max: 2100, label: "1900 – 2099" },
  { tier: "master", min: 2100, max: 2400, label: "2100 – 2399" },
  { tier: "grandmaster", min: 2400, max: null, label: "≥ 2400" },
];

export function ratingTier(rating: number | null | undefined): TierName {
  if (rating == null) return "unrated";
  for (const r of TIER_RANGES) {
    if (rating >= r.min && (r.max === null || rating < r.max)) return r.tier;
  }
  return "unrated";
}

// ── Lists ────────────────────────────────────────────────────────────────────

/** `list` with the item at `from` moved to `to` (the index it ends up at). */
export function moveItem<T>(list: readonly T[], from: number, to: number): T[] {
  const out = list.slice();
  if (from < 0 || from >= out.length) return out;
  const [item] = out.splice(from, 1);
  out.splice(Math.max(0, Math.min(out.length, to)), 0, item);
  return out;
}

/**
 * Where a dragged row lands: the slot whose middle the pointer passed, given the rows'
 * tops and heights measured when the drag began. Returns the index the row ends up at.
 */
export function dropIndex(slots: readonly { top: number; height: number }[], from: number, pointerY: number): number {
  let to = 0;
  for (let i = 0; i < slots.length; i++) {
    if (pointerY > slots[i].top + slots[i].height / 2) to = i + 1;
  }
  return to > from ? to - 1 : to;
}

/** The contest labels `A..Z` for `n` problems. */
export function labels(n: number): string[] {
  return Array.from({ length: Math.min(n, 26) }, (_, i) => String.fromCharCode(65 + i));
}

// ── Samples ──────────────────────────────────────────────────────────────────

/** A sample as the files hold it: no UTF-8 BOM, no `\r`. */
export function cleanSample(text: string): string {
  return text.replace(/^﻿/, "").replace(/﻿/g, "").replace(/\r\n?/g, "\n");
}

export function samplesEqual(a: readonly SamplePairJs[], b: readonly SamplePairJs[]): boolean {
  return a.length === b.length && a.every((s, i) => s.input === b[i].input && s.output === b[i].output);
}

// ── Export ───────────────────────────────────────────────────────────────────

export type Book = "statements" | "editorial";

/** What the export button's body does for `book`: reveal a PDF that matches, else export. */
export function exportAction(status: ExportBookStatusJs | undefined, book: Book): "reveal" | "export" {
  const s = status?.[book];
  return s && s.exists && s.fresh ? "reveal" : "export";
}

/** How the `▾` menu shows "在文件夹中显示" for `book`: unavailable, stale, or current. */
export function revealState(status: ExportBookStatusJs | undefined, book: Book): "none" | "stale" | "fresh" {
  const s = status?.[book];
  if (!s || !s.exists) return "none";
  return s.fresh ? "fresh" : "stale";
}

// ── Titles ───────────────────────────────────────────────────────────────────

export type Space = "problems" | "contests" | "settings";

/** Reactive localized display name for a top-level space. */
export function spaceName(s: Space): string {
  switch (s) {
    case "problems":
      return t("space.problems");
    case "contests":
      return t("space.contests");
    case "settings":
      return t("space.settings");
  }
}

export const APP_NAME = "XCPC Workstation";

/** The window's title (§6.4): the object open in the studio, else the space (with workspace), else the app. */
export function windowTitle(state: {
  workspace: boolean;
  workspaceName?: string;
  space: Space;
  studio?: { problemTitle?: string; label?: string; contestName?: string };
}): string {
  if (!state.workspace) return APP_NAME;
  const s = state.studio;
  if (s) {
    const problem = s.problemTitle ? (s.label ? `${s.label}. ${s.problemTitle}` : s.problemTitle) : undefined;
    if (problem && s.contestName) return `${problem} — ${s.contestName}`;
    return problem ?? s.contestName ?? APP_NAME;
  }
  const base = spaceName(state.space);
  return state.workspaceName ? `${base} — ${state.workspaceName}` : base;
}

/** The last segment (folder name) of a POSIX or Windows path. */
export function pathBase(path: string): string {
  const trimmed = path.replace(/[\\/]+$/, "");
  if (!trimmed) return path;
  const parts = trimmed.split(/[\\/]/);
  return parts[parts.length - 1] || path;
}

/** Abbreviates the user's home directory across Linux, macOS, and Windows paths. */
export function abbreviateHome(path: string): string {
  return path
    .replace(/^\/home\/[^/]+/, "~")
    .replace(/^\/Users\/[^/]+/, "~")
    .replace(/^[A-Za-z]:\\Users\\[^\\]+/, "~");
}

/** A problem's limits as one line (`1 秒  256 MB`). */
export function limitsLine(limits: { time?: string | null; memory?: string | null }): string {
  return [limits.time, limits.memory].filter(Boolean).join("  ");
}

// ── Split ratio ──────────────────────────────────────────────────────────────

export function clampRatio(r: number | null | undefined): number {
  if (r == null || !Number.isFinite(r)) return 0.5;
  return Math.min(0.9, Math.max(0.1, r));
}

/** The editor's share of a `ResizablePanelGroup`'s reported sizes. */
export function ratioOf(sizes: readonly number[]): number | undefined {
  if (sizes.length < 2) return undefined;
  const total = sizes[0] + sizes[1];
  return total > 0 ? clampRatio(sizes[0] / total) : undefined;
}

// ── Text ─────────────────────────────────────────────────────────────────────

/** A position in `text` (line and UTF-16 column, from 0) of UTF-16 offset `offset`. */
export function positionOf(text: string, offset: number): { line: number; column: number } {
  let line = 0;
  let start = 0;
  for (let i = 0; i < offset; i++) {
    if (text.charCodeAt(i) === 10) {
      line += 1;
      start = i + 1;
    }
  }
  return { line, column: offset - start };
}

/**
 * The smallest single edit that turns `from` into `to` (common prefix and suffix kept),
 * so an external change applied through `EditorRef.edit` leaves the carets and the
 * scroll where they were. `null` when the texts are equal.
 */
export function minimalEdit(
  from: string,
  to: string,
): { range: { start: { line: number; column: number }; end: { line: number; column: number } }; text: string } | null {
  if (from === to) return null;
  let p = 0;
  const max = Math.min(from.length, to.length);
  while (p < max && from.charCodeAt(p) === to.charCodeAt(p)) p++;
  let s = 0;
  while (s < max - p && from.charCodeAt(from.length - 1 - s) === to.charCodeAt(to.length - 1 - s)) s++;
  return {
    range: { start: positionOf(from, p), end: positionOf(from, from.length - s) },
    text: to.slice(p, to.length - s),
  };
}

// ── Diagnostics ──────────────────────────────────────────────────────────────

export function diagnosticCounts(list: readonly PreviewDiagnostic[]): { errors: number; warnings: number } {
  let errors = 0;
  let warnings = 0;
  for (const d of list) {
    if (d.severity === "error") errors += 1;
    else warnings += 1;
  }
  return { errors, warnings };
}

// ── Errors (§7.3) ────────────────────────────────────────────────────────────

/** How an error shows: what happened in plain words, the next step when there is one, and whether it is a rule's refusal (secondary text, not red). */
export interface ErrorCopy {
  message: string;
  next?: string;
  rule: boolean;
}

const RULES = new Set(["contest_frozen", "contest_archived", "problem_referenced", "read_only", "already_frozen", "not_frozen"]);

/** The words for a Core error `code` (the raw `error` stays for Copy Details). */
export function errorCopy(code: string | null | undefined, raw?: string | null): ErrorCopy {
  const rule = code ? RULES.has(code) : false;
  switch (code) {
    case "contest_frozen":
      return { message: t("error.contestFrozen"), rule };
    case "contest_archived":
      return { message: t("error.contestArchived"), next: t("error.contestArchivedNext"), rule };
    case "problem_referenced":
      return { message: t("error.problemReferenced"), next: t("error.problemReferencedNext"), rule };
    case "read_only":
      return { message: t("error.readOnly"), rule };
    case "already_frozen":
      return { message: t("error.alreadyFrozen"), rule };
    case "not_frozen":
      return { message: t("error.notFrozen"), rule };
    case "invalid_id":
      return { message: t("error.invalidId"), rule };
    case "too_many_problems":
      return { message: t("error.tooManyProblems"), rule };
    case "conflict":
      if (raw && /already exists|已存在|expected\s+""/i.test(raw)) {
        return { message: t("error.conflictExists"), next: t("error.conflictExistsNext"), rule };
      }
      return { message: t("error.conflictModified"), rule };
    case "not_found":
      return { message: t("error.notFound"), rule };
    case "path_escape":
    case "disallowed_path":
      return { message: t("error.pathEscape"), rule };
    case "invalid_file":
      return { message: t("error.invalidFile"), rule };
    case "io":
      return { message: t("error.io"), rule };
    case "preflight":
      return { message: t("error.preflight"), rule };
    case "no_workspace":
      return { message: t("error.noWorkspace"), rule };
    case "missing_template":
      return { message: t("error.missingTemplate"), next: t("error.missingTemplateNext"), rule };
    default:
      break;
  }
  if (raw && /already exists/.test(raw)) return { message: t("error.duplicateIdExists"), rule: false };
  if (raw && /read-only/.test(raw)) return { message: t("error.readOnly"), rule: true };
  if (raw && /no workspace/i.test(raw)) return { message: t("error.notWorkspace"), next: t("error.notWorkspaceNext"), rule: false };
  return { message: t("error.generic"), rule: false };
}

/** The text 复制详情 puts on the clipboard. */
export function errorDetails(code: string | null | undefined, raw: string | null | undefined, context?: string): string {
  return [context, code ? `code: ${code}` : undefined, raw ? `error: ${raw}` : undefined].filter(Boolean).join("\n");
}

/** A problem/contest ID as the Core accepts it (`xcpc_domain::InvalidId`). */
export function validId(id: string): boolean {
  return /^[a-z0-9][a-z0-9_-]{0,63}$/.test(id);
}

/** Suggest a non-colliding duplicate problem ID (`<id>-copy`, `<id>-copy-2`, …) within 64 chars. */
export function suggestDuplicateId(sourceId: string, existingIds: Iterable<string>): string {
  const used = new Set(existingIds);
  for (let n = 1; n < 10_000; n++) {
    const suffix = n === 1 ? "-copy" : `-copy-${n}`;
    const maxBase = Math.max(1, 64 - suffix.length);
    const trimmed = sourceId.slice(0, maxBase).replace(/[-_]+$/, "");
    const base = trimmed || sourceId.slice(0, 1) || "p";
    const candidate = `${base}${suffix}`;
    if (!used.has(candidate)) return candidate;
  }
  return `${sourceId.slice(0, 56)}-copy`;
}

/** Toggle a problem in the contest picker (append-only, respects existing problems & 26-problem cap). */
export function togglePickProblem(
  selected: readonly string[],
  id: string,
  existingIds: ReadonlySet<string>,
  maxTotal = 26,
): string[] {
  if (existingIds.has(id)) return [...selected];
  if (selected.includes(id)) return selected.filter((x) => x !== id);
  const remaining = Math.max(0, maxTotal - existingIds.size);
  if (selected.length >= remaining) return [...selected];
  return [...selected, id];
}

/** Split contests into active (`archived === false`) and archived (`archived === true`). */
export function partitionContests<T extends { archived: boolean }>(
  contests: readonly T[],
): { active: T[]; archived: T[] } {
  const active: T[] = [];
  const archived: T[] = [];
  for (const c of contests) {
    if (c.archived) archived.push(c);
    else active.push(c);
  }
  return { active, archived };
}

/** Filter contests shown in "Add to Contest" (excludes archived contests). */
export function filterAddableContests<T extends { archived: boolean }>(contests: readonly T[]): T[] {
  return contests.filter((c) => !c.archived);
}

// ── Unfreeze ─────────────────────────────────────────────────────────────────

/** The words for an unfreeze difference kind. */
export function diffKind(kind: string): string {
  switch (kind) {
    case "statement_changed":
      return t("diff.statementChanged");
    case "samples_changed":
      return t("diff.samplesChanged");
    case "editorial_changed":
      return t("diff.editorialChanged");
    case "assets_changed":
      return t("diff.assetsChanged");
    case "meta_changed":
      return t("diff.metaChanged");
    case "problem_deleted":
      return t("diff.problemDeleted");
    default:
      return kind;
  }
}

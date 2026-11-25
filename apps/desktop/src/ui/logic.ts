// Pure UI logic, free of `nsh` and the native bridge, so `App.test.tsx` runs it
// headless: preview geometry, ratings, reordering, samples, export state, titles and
// the error wording of docs/design-system.md §7.3.

import type {
  ExportBookStatusJs,
  FuzzyHitJs,
  PreviewDiagnostic,
  PreviewView,
  ProblemSummaryJs,
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

export type Space = "problems" | "contests" | "snippets" | "training" | "tools" | "settings";

/** Ordered visible spaces for NavRail and dynamic `mod+1..N` shortcut assignment. */
export function visibleSpaces(opts: {
  hasWorkspace: boolean;
  snippetsEnabled: boolean;
  trainingEnabled: boolean;
}): Space[] {
  const out: Space[] = [];
  if (opts.hasWorkspace) {
    out.push("problems", "contests");
    if (opts.snippetsEnabled) out.push("snippets");
    if (opts.trainingEnabled) out.push("training");
  }
  out.push("tools", "settings");
  return out;
}

/** Returns the dynamic `mod+1..N` shortcut string for `target`, or `undefined` when hidden. */
export function shortcutForSpace(
  target: Space,
  opts: { hasWorkspace: boolean; snippetsEnabled: boolean; trainingEnabled: boolean },
): string | undefined {
  const idx = visibleSpaces(opts).indexOf(target);
  return idx >= 0 ? `mod+${idx + 1}` : undefined;
}

/** Reactive localized display name for a top-level space. */
export function spaceName(s: Space): string {
  switch (s) {
    case "problems":
      return t("space.problems");
    case "contests":
      return t("space.contests");
    case "snippets":
      return t("space.snippets");
    case "training":
      return t("space.training");
    case "tools":
      return t("space.tools");
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
  if (!state.workspace) {
    if (state.space === "tools" || state.space === "settings") {
      return `${spaceName(state.space)} — ${APP_NAME}`;
    }
    return APP_NAME;
  }
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

const RULES = new Set([
  "contest_frozen",
  "contest_archived",
  "problem_referenced",
  "read_only",
  "already_frozen",
  "not_frozen",
  "cancelled",
  "not_a_package",
]);

/**
 * Judge codes whose Core `human_message` is already the three-part plain-language
 * copy (authoring-ui-refactor §6.2): the GUI shows it as-is.
 */
export const CORE_WORDED_CODES = new Set([
  "no_compiler",
  "compiler_not_found",
  "no_main_solution",
  "multiple_main_solutions",
  "unknown_checker",
  "test_not_found",
  "generator_not_found",
  "solution_not_found",
  "invalid_limit",
  "invalid_config",
  "internal",
]);

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
    case "invalid_archive":
      return { message: t("error.invalidArchive"), next: t("error.invalidArchiveNext"), rule };
    case "zip_slip":
      return { message: t("error.zipSlip"), rule };
    case "archive_too_large":
      return { message: t("error.archiveTooLarge"), rule };
    case "too_many_entries":
      return { message: t("error.tooManyEntries"), rule };
    case "target_exists":
      return { message: t("error.targetExists"), next: t("error.targetExistsNext"), rule };
    case "not_verified":
      return { message: t("error.notVerified"), next: t("error.notVerifiedNext"), rule: true };
    case "missing_testdata":
      return { message: t("error.missingTestdata"), next: t("error.missingTestdataNext"), rule: true };
    case "pdf_compile_failed":
      return { message: t("error.pdfCompileFailed"), next: t("error.pdfCompileFailedNext"), rule };
    case "invalid_format":
      return { message: t("error.invalidFormat"), rule };
    case "unsupported":
      return { message: t("error.unsupported"), rule };
    case "invalid_group_name":
      return { message: t("error.invalidGroupName"), rule };
    case "group_already_exists":
      return { message: t("error.groupAlreadyExists"), rule };
    case "cancelled":
      return { message: t("error.cancelled"), rule: true };
    case "not_a_package":
      return { message: t("error.notAPackage"), rule: true };
    default:
      break;
  }
  if (code && CORE_WORDED_CODES.has(code) && raw) return { message: raw, rule: false };
  if (raw && /already exists/.test(raw)) return { message: t("error.duplicateIdExists"), rule: false };
  if (raw && /read-only/.test(raw)) return { message: t("error.readOnly"), rule: true };
  if (raw && /no workspace/i.test(raw)) return { message: t("error.notWorkspace"), next: t("error.notWorkspaceNext"), rule: false };
  // Any other Core error that already arrives in plain Chinese beats the generic line.
  if (raw && /[\u4e00-\u9fff]/.test(raw)) return { message: raw, rule: false };
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

// ── Library Multi-Selection (§5.4) ───────────────────────────────────────────

export interface SelectionState {
  selectedIds: ReadonlySet<string>;
  anchorId: string | undefined;
  focusedId: string | undefined;
}

export const EMPTY_SELECTION: SelectionState = {
  selectedIds: new Set<string>(),
  anchorId: undefined,
  focusedId: undefined,
};

export type RowSelectionAction =
  | { kind: "click"; id: string; shift?: boolean; mod?: boolean; checkbox?: boolean }
  | { kind: "selectAll" }
  | { kind: "toggleAll" }
  | { kind: "toggleGroup"; ids: readonly string[] }
  | { kind: "clear" }
  | { kind: "prune" };

export type CheckboxTriState = "none" | "partial" | "all";

/** Tri-state header/group checkbox state over `visibleIds`. */
export function headerCheckboxState(
  selectedIds: ReadonlySet<string>,
  visibleIds: readonly string[],
): CheckboxTriState {
  if (visibleIds.length === 0) return "none";
  let count = 0;
  for (const id of visibleIds) {
    if (selectedIds.has(id)) count += 1;
  }
  if (count === 0) return "none";
  return count === visibleIds.length ? "all" : "partial";
}

/** Pure state machine for library multi-select (plain click, checkbox, Ctrl/Cmd, Shift range, Ctrl+A, Esc, prune). */
export function applyRowSelection(
  state: SelectionState,
  visibleIds: readonly string[],
  action: RowSelectionAction,
): SelectionState {
  switch (action.kind) {
    case "clear":
      return {
        selectedIds: new Set<string>(),
        anchorId: state.anchorId,
        focusedId: state.focusedId,
      };
    case "selectAll":
      return {
        selectedIds: new Set<string>(visibleIds),
        anchorId: state.anchorId ?? visibleIds[0],
        focusedId: state.focusedId ?? visibleIds[0],
      };
    case "toggleAll": {
      const tri = headerCheckboxState(state.selectedIds, visibleIds);
      if (tri === "all") {
        return { selectedIds: new Set<string>(), anchorId: state.anchorId, focusedId: state.focusedId };
      }
      return {
        selectedIds: new Set<string>(visibleIds),
        anchorId: state.anchorId ?? visibleIds[0],
        focusedId: state.focusedId ?? visibleIds[0],
      };
    }
    case "toggleGroup": {
      const tri = headerCheckboxState(state.selectedIds, action.ids);
      const next = new Set<string>(state.selectedIds);
      if (tri === "all") {
        for (const id of action.ids) next.delete(id);
      } else {
        for (const id of action.ids) next.add(id);
      }
      return {
        selectedIds: next,
        anchorId: state.anchorId ?? action.ids[0],
        focusedId: state.focusedId ?? action.ids[0],
      };
    }
    case "prune": {
      const visibleSet = new Set<string>(visibleIds);
      let changed = false;
      const next = new Set<string>();
      for (const id of state.selectedIds) {
        if (visibleSet.has(id)) next.add(id);
        else changed = true;
      }
      const anchorId = state.anchorId && visibleSet.has(state.anchorId) ? state.anchorId : undefined;
      if (!changed && anchorId === state.anchorId) return state;
      return {
        selectedIds: next,
        anchorId,
        focusedId: state.focusedId,
      };
    }
    case "click": {
      const { id, shift, mod, checkbox } = action;
      if (shift) {
        const anchor = state.anchorId ?? state.focusedId;
        const aIdx = anchor ? visibleIds.indexOf(anchor) : -1;
        const tIdx = visibleIds.indexOf(id);
        if (aIdx >= 0 && tIdx >= 0) {
          const lo = Math.min(aIdx, tIdx);
          const hi = Math.max(aIdx, tIdx);
          const next = mod ? new Set<string>(state.selectedIds) : new Set<string>();
          for (let i = lo; i <= hi; i++) next.add(visibleIds[i]);
          return {
            selectedIds: next,
            anchorId: anchor,
            focusedId: id,
          };
        }
        const next = new Set<string>(state.selectedIds);
        next.add(id);
        return { selectedIds: next, anchorId: id, focusedId: id };
      }
      if (checkbox) {
        const next = new Set<string>(state.selectedIds);
        if (next.has(id)) next.delete(id);
        else next.add(id);
        return { selectedIds: next, anchorId: id, focusedId: id };
      }
      if (mod) {
        const next = new Set<string>(state.selectedIds);
        if (next.size === 0 && state.focusedId && state.focusedId !== id && visibleIds.includes(state.focusedId)) {
          next.add(state.focusedId);
          next.add(id);
        } else if (next.has(id)) {
          next.delete(id);
        } else {
          next.add(id);
        }
        return { selectedIds: next, anchorId: id, focusedId: id };
      }
      // Plain row body click: clear multi-selection and focus the clicked row.
      return {
        selectedIds: new Set<string>(),
        anchorId: id,
        focusedId: id,
      };
    }
  }
}

// ── Library Dynamic Grouping (§5.3) ──────────────────────────────────────────

export type LibraryGroupBy = "none" | "group" | "rating" | "platform" | "contest" | "package_status";

export function isLibraryGroupBy(value: string | null | undefined): value is LibraryGroupBy {
  return (
    value === "none" ||
    value === "group" ||
    value === "rating" ||
    value === "platform" ||
    value === "contest" ||
    value === "package_status"
  );
}

export interface ProblemGroupSection<T> {
  key: string;
  label: string;
  rows: T[];
}

export function groupProblems<
  T extends Pick<ProblemSummaryJs, "id" | "group" | "rating" | "sources" | "packageStatus" | "contestIds">,
>(
  rows: readonly T[],
  groupBy: LibraryGroupBy,
  declaredGroups: readonly string[],
  contests: readonly { id: string; name: string }[],
): ProblemGroupSection<T>[] {
  if (groupBy === "none" || rows.length === 0) return [];

  if (groupBy === "group") {
    const map = new Map<string, T[]>();
    const ungrouped: T[] = [];
    for (const r of rows) {
      const g = r.group?.trim();
      if (!g) ungrouped.push(r);
      else {
        const list = map.get(g);
        if (list) list.push(r);
        else map.set(g, [r]);
      }
    }
    const out: ProblemGroupSection<T>[] = [];
    const seen = new Set<string>();
    for (const name of declaredGroups) {
      seen.add(name);
      const list = map.get(name);
      if (list && list.length > 0) out.push({ key: `group:${name}`, label: name, rows: list });
    }
    const extra = [...map.keys()].filter((k) => !seen.has(k)).sort((a, b) => a.localeCompare(b));
    for (const name of extra) {
      const list = map.get(name)!;
      out.push({ key: `group:${name}`, label: name, rows: list });
    }
    if (ungrouped.length > 0) {
      out.push({ key: "group:__ungrouped__", label: t("library.groupBucket.ungrouped"), rows: ungrouped });
    }
    return out;
  }

  if (groupBy === "rating") {
    const map = new Map<TierName, T[]>();
    for (const r of rows) {
      const tier = ratingTier(r.rating);
      const list = map.get(tier);
      if (list) list.push(r);
      else map.set(tier, [r]);
    }
    const out: ProblemGroupSection<T>[] = [];
    for (const range of TIER_RANGES) {
      const list = map.get(range.tier);
      if (list && list.length > 0) {
        out.push({ key: `rating:${range.tier}`, label: range.label, rows: list });
      }
    }
    const unrated = map.get("unrated");
    if (unrated && unrated.length > 0) {
      out.push({ key: "rating:unrated", label: t("library.groupBucket.unrated"), rows: unrated });
    }
    return out;
  }

  if (groupBy === "platform") {
    const map = new Map<string, T[]>();
    const none: T[] = [];
    for (const r of rows) {
      const p = r.sources[0]?.platform?.trim();
      if (!p) none.push(r);
      else {
        const list = map.get(p);
        if (list) list.push(r);
        else map.set(p, [r]);
      }
    }
    const out: ProblemGroupSection<T>[] = [...map.keys()]
      .sort((a, b) => a.localeCompare(b))
      .map((p) => ({ key: `platform:${p}`, label: p, rows: map.get(p)! }));
    if (none.length > 0) {
      out.push({ key: "platform:__none__", label: t("library.groupBucket.noPlatform"), rows: none });
    }
    return out;
  }

  if (groupBy === "contest") {
    const byContest = new Map<string, T[]>();
    const multi: T[] = [];
    const none: T[] = [];
    for (const r of rows) {
      const cids = r.contestIds ?? [];
      if (cids.length === 0) none.push(r);
      else if (cids.length >= 2) multi.push(r);
      else {
        const cid = cids[0];
        const list = byContest.get(cid);
        if (list) list.push(r);
        else byContest.set(cid, [r]);
      }
    }
    const out: ProblemGroupSection<T>[] = [];
    const seen = new Set<string>();
    for (const c of contests) {
      seen.add(c.id);
      const list = byContest.get(c.id);
      if (list && list.length > 0) {
        out.push({ key: `contest:${c.id}`, label: c.name, rows: list });
      }
    }
    const extra = [...byContest.keys()].filter((k) => !seen.has(k)).sort((a, b) => a.localeCompare(b));
    for (const cid of extra) {
      out.push({ key: `contest:${cid}`, label: cid, rows: byContest.get(cid)! });
    }
    if (multi.length > 0) {
      out.push({ key: "contest:__multi__", label: t("library.groupBucket.multipleContests"), rows: multi });
    }
    if (none.length > 0) {
      out.push({ key: "contest:__none__", label: t("library.groupBucket.noContest"), rows: none });
    }
    return out;
  }

  // package_status
  const order: { key: string; label: string }[] = [
    // Three states only (authoring-ui-refactor §2): stale counts as not checked.
    { key: "failed", label: t("build.status.failed") },
    { key: "unverified", label: t("build.status.unverified") },
    { key: "verified_fresh", label: t("build.status.verifiedFresh") },
    { key: "none", label: t("library.groupBucket.noPackage") },
  ];
  const map = new Map<string, T[]>();
  for (const r of rows) {
    const st = r.packageStatus;
    const key =
      st === "stale" ? "unverified" : st === "verified_fresh" || st === "failed" || st === "unverified" ? st : "none";
    const list = map.get(key);
    if (list) list.push(r);
    else map.set(key, [r]);
  }
  const out: ProblemGroupSection<T>[] = [];
  for (const bucket of order) {
    const list = map.get(bucket.key);
    if (list && list.length > 0) {
      out.push({ key: `pkg:${bucket.key}`, label: bucket.label, rows: list });
    }
  }
  return out;
}

// ── Library Column Width Budget (§5.2) ───────────────────────────────────────

export type LibraryColumnKey = "select" | "title" | "ratingText" | "tagsText" | "status" | "source";

export interface LibraryColumnSpec {
  key: LibraryColumnKey;
  title: string;
  width: number;
  align?: "start" | "center" | "end";
  resizable: false;
  sortable?: boolean;
  selectable?: boolean;
}

export interface LibraryColumnsLayout {
  columns: LibraryColumnSpec[];
  titleWidth: number;
  tagsWidth: number;
  showInlineId: boolean;
  maxTags: number;
}

export function computeLibraryColumns(width: number): LibraryColumnsLayout {
  const selectW = 36;
  const ratingW = 78;
  const statusW = 76;
  const tagsW = width >= 560 ? 176 : width >= 480 ? 124 : 0;
  const sourceW = width >= 780 ? 132 : 0;
  const used = selectW + ratingW + statusW + tagsW + sourceW;
  const titleW = Math.max(136, width - used - 2);
  // Keep fixed columns first (indices 0..3) and append optional responsive columns at the end
  // so NSH DataTable's index-based header/cell recycling never shifts column indices on resize.
  const columns: LibraryColumnSpec[] = [
    { key: "select", title: "", width: selectW, align: "center", resizable: false, sortable: false, selectable: false },
    { key: "title", title: t("library.col.title"), width: titleW, resizable: false, sortable: true },
    { key: "ratingText", title: t("library.col.rating"), width: ratingW, align: "end", resizable: false, sortable: true },
    { key: "status", title: t("library.col.status"), width: statusW, align: "center", resizable: false },
    ...(tagsW > 0 ? [{ key: "tagsText" as const, title: t("library.col.tags"), width: tagsW, resizable: false as const }] : []),
    ...(sourceW > 0 ? [{ key: "source" as const, title: t("library.col.source"), width: sourceW, resizable: false as const }] : []),
  ];
  return {
    columns,
    titleWidth: titleW,
    tagsWidth: tagsW,
    showInlineId: titleW >= 220,
    maxTags: tagsW >= 176 ? 3 : 2,
  };
}

// ── Batch Delete Pre-Check Partition (§5.4) ──────────────────────────────────

export interface BatchDeleteBlockedItem {
  id: string;
  contests: { id: string; name: string }[];
}

export interface BatchDeletePartition {
  deletable: string[];
  blocked: BatchDeleteBlockedItem[];
}

/** Partitions selected problem IDs into deletable vs blocked by unfrozen contests (regardless of `archived`; frozen contests do NOT block). */
export function partitionBatchDelete(
  selectedIds: readonly string[],
  contests: readonly { id: string; name: string; frozen: boolean; archived?: boolean; items: readonly { id: string }[] }[],
): BatchDeletePartition {
  const refs = new Map<string, { id: string; name: string }[]>();
  for (const c of contests) {
    if (c.frozen) continue;
    for (const item of c.items) {
      const list = refs.get(item.id);
      if (list) {
        if (!list.some((x) => x.id === c.id)) list.push({ id: c.id, name: c.name });
      } else {
        refs.set(item.id, [{ id: c.id, name: c.name }]);
      }
    }
  }
  const seen = new Set<string>();
  const deletable: string[] = [];
  const blocked: BatchDeleteBlockedItem[] = [];
  for (const id of selectedIds) {
    if (seen.has(id)) continue;
    seen.add(id);
    const referencing = refs.get(id);
    if (referencing && referencing.length > 0) {
      blocked.push({ id, contests: referencing });
    } else {
      deletable.push(id);
    }
  }
  return { deletable, blocked };
}

// ── Command Palette Ranking & Caps (§6) ──────────────────────────────────────

export interface PaletteActionInput {
  id: string;
  label: string;
  keywords?: readonly string[];
  shortcut?: string | null;
}

export type PaletteGroupKind = "recent" | "currentContest" | "problem" | "contest" | "action";

export interface PaletteResultItem {
  id: string;
  label: string;
  groupKind: PaletteGroupKind;
  groupLabel: string;
  shortcut?: string | null;
  hit?: FuzzyHitJs;
}

export interface PaletteBuildResult {
  actionMode: boolean;
  query: string;
  items: PaletteResultItem[];
}

function scorePaletteAction(action: PaletteActionInput, needle: string): number {
  if (!needle) return 1;
  const label = action.label.toLowerCase();
  const kws = (action.keywords ?? []).map((k) => k.toLowerCase());
  if (label === needle || kws.some((k) => k === needle)) return 100;
  if (label.startsWith(needle) || kws.some((k) => k.startsWith(needle))) return 70;
  if (label.includes(needle)) return 40;
  if (kws.some((k) => k.includes(needle))) return 20;
  return 0;
}

function groupLabelFor(kind: PaletteGroupKind): string {
  switch (kind) {
    case "recent":
      return t("palette.group.recent");
    case "currentContest":
      return t("palette.group.currentContest");
    case "problem":
      return t("palette.group.problem");
    case "contest":
      return t("palette.group.contest");
    case "action":
      return t("palette.group.action");
  }
}

export function buildPaletteItems(
  rawQuery: string,
  jumps: readonly FuzzyHitJs[],
  actions: readonly PaletteActionInput[],
  activeContest: string | null | undefined,
  opts?: { prioritizeActions?: boolean },
): PaletteBuildResult {
  const trimmedStart = rawQuery.trimStart();
  const actionMode = trimmedStart.startsWith(">");
  const query = (actionMode ? trimmedStart.slice(1) : trimmedStart).trim();
  const needle = query.toLowerCase();

  const scoredActions = actions
    .map((a, idx) => ({ a, idx, score: scorePaletteAction(a, needle) }))
    .filter((x) => x.score > 0);
  if (needle) {
    scoredActions.sort((x, y) => y.score - x.score || x.idx - y.idx);
  }
  const actionItems: PaletteResultItem[] = scoredActions.map(({ a }) => ({
    id: a.id,
    label: a.label,
    groupKind: "action",
    groupLabel: groupLabelFor("action"),
    shortcut: a.shortcut ?? null,
  }));

  if (actionMode) {
    return { actionMode: true, query, items: actionItems };
  }

  const toJumpItem = (h: FuzzyHitJs, groupKind: PaletteGroupKind): PaletteResultItem => ({
    id: `j:${h.visitKey}`,
    label: h.title,
    groupKind,
    groupLabel: groupLabelFor(groupKind),
    hit: h,
  });

  if (!query) {
    if (activeContest) {
      const contestItems = jumps
        .filter((h) => h.kind === "contest_item")
        .sort((a, b) => (a.label ?? "").localeCompare(b.label ?? ""))
        .slice(0, 8)
        .map((h) => toJumpItem(h, "currentContest"));
      return { actionMode: false, query, items: [...contestItems, ...actionItems] };
    }
    const recentItems = jumps.slice(0, 5).map((h) => toJumpItem(h, "recent"));
    return {
      actionMode: false,
      query,
      items: opts?.prioritizeActions ? [...actionItems, ...recentItems] : [...recentItems, ...actionItems],
    };
  }

  // Non-empty query: per-group caps (contest_item 6, problem 8, contest 4, actions uncapped).
  const contestItems = jumps
    .filter((h) => h.kind === "contest_item")
    .slice(0, 6)
    .map((h) => toJumpItem(h, "currentContest"));
  const problemItems = jumps
    .filter((h) => h.kind === "problem")
    .slice(0, 8)
    .map((h) => toJumpItem(h, "problem"));
  const contestList = jumps
    .filter((h) => h.kind === "contest")
    .slice(0, 4)
    .map((h) => toJumpItem(h, "contest"));

  const jumpItems = [...contestItems, ...problemItems, ...contestList];
  const bestActionScore = scoredActions[0]?.score ?? 0;
  const hasExactObjectHit = jumpItems.some((item) => {
    const h = item.hit;
    return !!h && (h.id.toLowerCase() === needle || h.title.toLowerCase() === needle);
  });

  const promoteActions = (opts?.prioritizeActions ? bestActionScore > 0 : bestActionScore >= 70) && !hasExactObjectHit;
  return {
    actionMode: false,
    query,
    items: promoteActions ? [...actionItems, ...jumpItems] : [...jumpItems, ...actionItems],
  };
}

/**
 * Adaptive preview compile debounce (Phase 2):
 * - Single edits / slow pauses (`interKeystrokeMs >= 200`): fire rapidly (35ms problem / 65ms contest).
 * - Rapid typing bursts (`interKeystrokeMs < 110`): coalesce proportionally to `lastCompileMs`
 *   so fast bursts never flood the worker, yet pausing lands a fresh frame immediately.
 */
export function adaptiveCompileDelayMs(
  isContest: boolean,
  interKeystrokeMs: number,
  lastCompileMs: number,
): number {
  const safeCompile = Number.isFinite(lastCompileMs) && lastCompileMs > 0 ? lastCompileMs : 0;
  if (interKeystrokeMs >= 200) {
    return isContest ? 65 : 35;
  }
  const minBurst = isContest ? 90 : 55;
  const maxBurst = isContest ? 160 : 110;
  if (interKeystrokeMs < 110) {
    return Math.min(maxBurst, Math.max(minBurst, Math.round(safeCompile * 1.5)));
  }
  // Intermediate cadence: linear blend between pause and burst.
  const t = (200 - interKeystrokeMs) / 90;
  const pause = isContest ? 65 : 35;
  const burst = Math.min(maxBurst, Math.max(minBurst, Math.round(safeCompile * 1.5)));
  return Math.round(pause + (burst - pause) * t);
}

/**
 * Computes adjacent pages (`[min - 1, max + 1, max + 2]`) outside the current viewport
 * to speculatively pre-warm in the background PIXELS cache.
 */
export function neighborPagesToPrewarm(
  viewPages: readonly number[],
  totalPages: number,
): number[] {
  if (totalPages <= 1 || viewPages.length === 0) return [];
  const inView = new Set(viewPages);
  let minP = totalPages;
  let maxP = -1;
  for (const p of viewPages) {
    if (p < minP) minP = p;
    if (p > maxP) maxP = p;
  }
  const candidates = [maxP + 1, minP - 1, maxP + 2];
  const out: number[] = [];
  for (const p of candidates) {
    if (p >= 0 && p < totalPages && !inView.has(p) && !out.includes(p)) {
      out.push(p);
    }
  }
  return out;
}

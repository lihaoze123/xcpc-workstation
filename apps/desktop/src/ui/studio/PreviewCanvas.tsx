// The paper preview (§6.2, §8.2): pages scroll in a native scroll view, laid out at the
// zoom's size; both zoom (pinch / Ctrl+wheel) and left-drag panning run strictly on the
// GPU compositor (`transform`) until the gesture settles, then commit in a single batch
// with zero mid-drag re-rasterization or `scrollTo` race. Over it, a single unified
// floating HUD capsule that glides with the Inspector drawer and hides cleanly on narrow
// panes so it is never clipped.

import {
  batch,
  cleanup,
  colorScheme,
  computed,
  effect,
  pixelRatio,
  Scroll,
  signal,
  Text,
  untracked,
  View,
  type ScrollRef,
  type ViewRef,
} from "nsh";
import { Popover } from "nsh/base";
import {
  PageImage,
  type PreviewDiagnostic,
  type PreviewPage,
  type PreviewRequest,
  type PreviewResult,
  type PreviewStart,
  type PreviewView,
  type SourceLocationJs,
} from "xcpc-workstation/native";
import { Icon, type IconName } from "../Icon";
import { t } from "../i18n";
import { DelayedSpinner, tip } from "../kit";
import {
  BASE_SCALE,
  DEFAULT_PAGE,
  diagnosticCounts,
  GAP,
  leftFor,
  MAX_ZOOM,
  MIN_ZOOM,
  neighborPagesToPrewarm,
  scaleFor,
  TILE,
  type PageSize,
} from "../logic";
import { info, preview } from "../store";
import { panelTransition } from "../motion";
import { EASE_STANDARD, ms, popoverShadow } from "../theme";
import { traceInteraction } from "../trace";

import { CONTROL, FONT_SIZE, FONT_WEIGHT, LINE_HEIGHT, RADIUS, SPACE } from "../tokens";
type ViewportMemory = { zoom: number; scrollY: number; panX: number; autoFit: boolean; pageSizes: PageSize[] };

// Kept across studio sessions: going back to a contest finds it where it was left.
const viewports = new Map<string, ViewportMemory>();

const TOP_PAD = 28; // breathing room above page 1 when content overflows vertically
const BOTTOM_HUD_PAD = 76; // clears the floating HUD capsule (bottom 16 + height 32 + 28 breathing room)
const BOTTOM_NO_HUD_PAD = 28;
const FIT_SIDE_PAD = 36; // comfortable horizontal margin for fit-to-width
const MAX_FIT_ZOOM = 1.25; // crisp and generous without blowing up on ultrawide monitors
const READER_SIDE_PAD = SPACE[16]; // §8.1 reader mode: 16pt left/right margin
const READER_PAGE_GAP = SPACE[12]; // §8.1 reader mode: 12pt vertical gap between pages
const READER_VERT_PAD = SPACE[16]; // §8.1 reader mode: 16pt top/bottom breathing room

function requestKey(req: { target: string; id: string; book: string }): string {
  return `${req.target}:${req.id}:${req.book}`;
}

/** Clamped visual gap between pages so extreme zoom levels never stretch or collapse spacing. */
function pageGap(z: number, reader = false): number {
  if (reader) return READER_PAGE_GAP;
  if (z <= 0) return GAP;
  return Math.max(10, Math.min(32, GAP * z));
}

/** Generous fit-to-width zoom with comfortable side margins and a sensible upper bound. */
function fitZoom(list: readonly { width: number }[], paneWidth: number, reader = false): number {
  if (list.length === 0 || paneWidth <= 0) return 0;
  if (reader) {
    const raw = (paneWidth - 2 * READER_SIDE_PAD) / list[0].width;
    return Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, raw));
  }
  const pad = Math.min(FIT_SIDE_PAD, Math.max(GAP, paneWidth * 0.06));
  const raw = (paneWidth - 2 * pad) / list[0].width;
  return Math.min(MAX_FIT_ZOOM, Math.max(MIN_ZOOM, raw));
}

function pageLeftFor(pageWidth: number, z: number, paneWidth: number, curPanX: number, reader = false): number {
  if (reader) {
    return Math.max(0, (paneWidth - pageWidth * z) / 2);
  }
  return leftFor(pageWidth, z, paneWidth, curPanX);
}

function bottomPadFor(hasHud: boolean, reader = false): number {
  if (reader) return READER_VERT_PAD;
  return hasHud ? BOTTOM_HUD_PAD : BOTTOM_NO_HUD_PAD;
}

/** Pure height of the page stack (all pages plus gaps between them, excluding top/bottom padding). */
function stackHeight(list: readonly { height: number }[], z: number, reader = false): number {
  if (list.length === 0 || z <= 0) return 0;
  const g = pageGap(z, reader);
  let h = 0;
  for (let i = 0; i < list.length; i++) {
    h += list[i].height * z;
    if (i > 0) h += g;
  }
  return h;
}

/**
 * Top padding of the page column: vertically centers the stack when shorter than the viewport
 * (while still clearing the bottom HUD without introducing a spurious scrollbar), otherwise
 * keeps a comfortable `TOP_PAD` breathing margin.
 */
function topPadFor(list: readonly { height: number }[], z: number, paneHeight: number, hasHud: boolean, reader = false): number {
  if (reader) return READER_VERT_PAD;
  if (list.length === 0 || z <= 0 || paneHeight <= 0) return TOP_PAD;
  const sh = stackHeight(list, z, false);
  const minBottom = bottomPadFor(hasHud, false);
  const room = paneHeight - sh;
  if (room <= TOP_PAD + minBottom) return TOP_PAD;
  const centered = Math.floor(room / 2);
  return Math.max(TOP_PAD, Math.min(centered, room - minBottom));
}

function heightFor(list: readonly { height: number }[], z: number, paneHeight: number, hasHud: boolean, reader = false): number {
  if (list.length === 0 || z <= 0) return 0;
  return topPadFor(list, z, paneHeight, hasHud, reader) + stackHeight(list, z, reader) + bottomPadFor(hasHud, reader);
}

/** Y offset of page `index`'s top edge in scroll-content coordinates. */
function pageYFor(list: readonly { height: number }[], z: number, index: number, topPad: number, reader = false): number {
  const g = pageGap(z, reader);
  let y = topPad;
  for (let i = 0; i < index && i < list.length; i++) {
    y += list[i].height * z + g;
  }
  return y;
}

/** Target `scrollY` to jump to page `index`. */
function topFor(list: readonly { height: number }[], z: number, index: number, topPad: number, reader = false): number {
  if (index <= 0) return 0;
  return Math.max(0, pageYFor(list, z, index, topPad, reader) - pageGap(z, reader));
}

function pageIndexAt(
  list: readonly { height: number }[],
  z: number,
  scrollY: number,
  paneHeight: number,
  topPad: number,
  reader = false,
): number {
  if (list.length === 0 || z <= 0) return 0;
  const g = pageGap(z, reader);
  const mid = scrollY + paneHeight / 3;
  let y = topPad;
  for (let i = 0; i < list.length; i++) {
    y += list[i].height * z + g;
    if (mid < y) return i;
  }
  return list.length - 1;
}

function viewFor(
  list: readonly PageSize[],
  z: number,
  curScrollY: number,
  curPanX: number,
  curPane: { width: number; height: number },
  topPad: number,
  reader = false,
): PreviewView[] {
  const edge = (page: number): PreviewView => ({ page, x: 0, y: 0, width: 0, height: 0 });
  if (list.length === 0 || z === 0) return [edge(0), edge(1)];
  const g = pageGap(z, reader);
  const curLeft = pageLeftFor(list[0].width, z, curPane.width, curPanX, reader);
  const top = curScrollY;
  const bottom = top + curPane.height;
  const x0 = Math.max(0, -curLeft / z);
  const out: PreviewView[] = [];
  let y = topPad;
  list.forEach((page, i) => {
    const h = page.height * z;
    if (y + h >= top && y <= bottom) {
      const y0 = Math.max(0, (top - y) / z);
      const y1 = Math.min(page.height, (bottom - y) / z);
      const x1 = Math.min(page.width, (curPane.width - curLeft) / z);
      out.push({ page: i, x: x0, y: y0, width: Math.max(0, x1 - x0), height: Math.max(0, y1 - y0) });
    }
    y += h + g;
  });
  if (out.length === 0) return [edge(0), edge(1)];
  if (out[0].page > 0) out.unshift(edge(out[0].page - 1));
  if (out[out.length - 1].page < list.length - 1) out.push(edge(out[out.length - 1].page + 1));
  return out;
}

function hitPage(
  list: readonly PageSize[],
  z: number,
  left: number,
  x: number,
  y: number,
  topPad: number,
): { page: number; xPt: number; yPt: number } | null {
  if (list.length === 0 || z <= 0) return null;
  const g = pageGap(z);
  let top = topPad;
  for (let i = 0; i < list.length; i++) {
    const h = list[i].height * z;
    if (y >= top && y <= top + h) {
      const xPt = (x - left) / z;
      const yPt = (y - top) / z;
      if (xPt < 0 || xPt > list[i].width) return null;
      return { page: i, xPt, yPt };
    }
    top += h + g;
  }
  return null;
}

export interface CanvasController {
  /** Typesets the request again (its text changed). */
  compile(): void;
  /** Last completed compile+render duration in ms (feeds adaptive typing debounce). */
  lastCompileMs(): number;
  /** Scrolls to `label`'s first page: smoothly when it is in the pages shown, else once the next result says where. */
  jumpTo(label: string): void;
  /** Scrolls to page `index` (0-based). */
  goPage(index: number, animated?: boolean): void;
  /** Scrolls smoothly to `(page, yPt)` (for forward source-to-preview locate). */
  scrollToLocation(page: number, yPt: number, animated?: boolean): void;
  /** Zooms by `factor` around the middle of the pane. */
  zoomBy(factor: number): void;
  /** Fits the page width. */
  fitWidth(): void;
  /** Closes the preflight popover if open; returns true if it was open. */
  closeHud(): boolean;
}

export function PreviewCanvas(props: {
  request: () => PreviewRequest | undefined;
  controller: (c: CanvasController) => void;
  onResult?: (result: PreviewResult) => void;
  onLocate?: (loc: SourceLocationJs) => void;
  onDiagnostic?: (d: PreviewDiagnostic) => void;
  /** Room the drawer covers on the right: the HUD centers in what is left. */
  inset?: number;
  /** The floating HUD (the studio's); a glance elsewhere goes without. */
  hud?: boolean;
  /** Reader mode (§8.1): fit-width continuous vertical scroll, 16pt side margins, 12pt page gap, no HUD/zoom/Ctrl+click jump. */
  readerMode?: boolean;
  /** False while hidden (`display: none`): keeps rendered pages warm without compiling or rasterizing. */
  active?: boolean;
}) {
  const pages = signal<PreviewPage[]>([]);
  const diagnostics = signal<PreviewDiagnostic[]>([]);
  /** Why there are no pages to show (the first error), while there are none. */
  const failure = signal<PreviewDiagnostic | undefined>(undefined);
  const zoom = signal(0); // 0 until the first fit
  const pane = signal({ width: 0, height: 0 });
  const scrollY = signal(0);
  const panX = signal(0); // where the pages' left edge is, when wider than the pane
  const resizing = signal(false);
  let resizeTimer: number | undefined;
  let scroll: ScrollRef | undefined;
  let paneView: ViewRef | undefined;
  let paneOrigin = { x: 0, y: 0 };

  let shownKey: string | undefined;
  let needsFit = false;
  let autoFit = true;
  let pendingJumpPage: number | undefined;
  let lastStarts: PreviewStart[] = [];
  let coldStartTried = false;
  let showingSnapshot = false;
  let snapshotTimer: number | undefined;

  // The content's laid-out height, so a scroll target beyond the previous maximum
  // settles once layout grows, and stays interruptible by the user.
  let contentHeightLaidOut = 0;
  let pendingScroll: { y: number; minContentHeight: number; raf?: number } | undefined;

  const isReader = () => props.readerMode === true;
  const hasHud = () => !isReader() && props.hud !== false;
  const isActive = () => props.active !== false;
  let deferredCompile = false;
  let pendingPane: { width: number; height: number } | undefined;
  let lastCommittedInset = untracked(() => props.inset ?? 0);

  const key = computed(() => {
    const r = props.request();
    return r ? requestKey(r) : undefined;
  });

  function cancelPendingScroll() {
    if (pendingScroll?.raf !== undefined) cancelAnimationFrame(pendingScroll.raf);
    pendingScroll = undefined;
  }

  function currentMaxScroll(): number {
    const curPaneH = pane().height;
    const h = contentHeightLaidOut > 0 ? contentHeightLaidOut : heightFor(pages(), zoom(), curPaneH, hasHud(), isReader());
    return Math.max(0, h - curPaneH);
  }

  function clampScroll(list: readonly PageSize[], z: number, y: number, paneH = pane().height): number {
    const max = Math.max(0, heightFor(list, z, paneH, hasHud(), isReader()) - paneH);
    return Math.min(max, Math.max(0, y));
  }

  function scrollTo(y: number, animated = false) {
    scroll?.scrollTo({ y, animated: animated && ms(1) > 0 });
  }

  function commitScroll(targetY: number, oldMaxScroll: number, newMaxScroll: number) {
    const clampedY = Math.min(newMaxScroll, Math.max(0, targetY));
    cancelPendingScroll();
    scrollTo(clampedY);
    if (clampedY > oldMaxScroll + 0.5) {
      const entry: { y: number; minContentHeight: number; raf?: number } = { y: clampedY, minContentHeight: clampedY + pane().height };
      pendingScroll = entry;
      entry.raf = requestAnimationFrame(() => {
        if (pendingScroll !== entry) return;
        pendingScroll = undefined;
        scrollY.set(clampedY);
        scrollTo(clampedY);
      });
    }
  }

  function saveViewport() {
    if (!shownKey || !hasHud()) return;
    const list = pages();
    const z = zoom();
    if (list.length === 0 || z === 0) return;
    viewports.set(shownKey, {
      zoom: z,
      scrollY: scrollY(),
      panX: panX(),
      autoFit,
      pageSizes: list.map((p) => ({ width: p.width, height: p.height })),
    });
  }

  // The live compositor transform of a pinch/Ctrl+wheel zoom, splitter/drawer resize, or left-drag pan.
  const live = signal({ k: 1, tx: 0, ty: 0 });
  const resizeTransform = signal({ k: 1, tx: 0, ty: 0 });
  const panOffset = signal({ dx: 0, dy: 0 });
  const dragging = signal(false);
  let dragStart: { x: number; y: number } | undefined;

  let baseScrollY = 0;
  let zoomTargetK = 1;
  let zoomAnchor = { x: 0, y: 0 };
  let zoomRaf: number | undefined;
  let zoomLastTime = 0;
  let pinchCommitTimer: number | undefined;

  const liveTransform = computed(() => {
    const l = live();
    const r = resizeTransform();
    const p = panOffset();
    const k = l.k * r.k;
    const tx = l.tx + r.tx + p.dx;
    const ty = l.ty + r.ty + p.dy;
    return k === 1 && tx === 0 && ty === 0 ? "none" : `translate(${tx}px, ${ty}px) scale(${k})`;
  });

  const inset = () => props.inset ?? 0;
  const availableWidth = computed(() => pane().width - inset());
  /** Unobscured horizontal width available to the paper column when the Inspector drawer is open. */
  const effectiveWidth = computed(() => (pane().width > 0 ? Math.max(120, pane().width - inset()) : 0));

  const scale = computed(() => scaleFor(zoom(), pixelRatio()));
  const left = computed(() => pageLeftFor(pages()[0]?.width ?? 0, zoom(), effectiveWidth(), panX(), isReader()));
  const topPad = computed(() => topPadFor(pages(), zoom(), pane().height, hasHud(), isReader()));
  const bottomPad = computed(() => bottomPadFor(hasHud(), isReader()));
  const view = computed<PreviewView[]>(() =>
    viewFor(pages(), zoom(), scrollY(), panX(), { width: effectiveWidth(), height: pane().height }, topPad(), isReader()),
  );
  const currentPage = computed(() => pageIndexAt(pages(), zoom(), scrollY(), pane().height, topPad(), isReader()));

  function tiles(v: PreviewView, s: number, margin: number) {
    const t = (pt: number, m: number) => Math.max(0, Math.floor((pt * s + m) / TILE));
    return [t(v.x, -margin), t(v.y, -margin), t(v.x + v.width, margin), t(v.y + v.height, margin)];
  }

  let asked = { key: "", scale: 0, pages: "", tiles: new Map<number, number[]>() };
  function due() {
    // Read every dependency before deciding, so the effect tracks them all.
    const active = isActive();
    const curKey = key();
    const s = scale();
    const list = view();
    const isResizing = resizing();
    const isDragging = dragging();
    const liveK = live().k;
    if (!active || !curKey || curKey !== shownKey || isResizing || isDragging || liveK !== 1) return false;
    if (curKey !== asked.key || s !== asked.scale || list.map((v) => v.page).join() !== asked.pages) return true;
    if (s <= BASE_SCALE) return false;
    return list.some((v) => {
      if (v.width <= 0) return false;
      const [x0, y0, x1, y1] = tiles(v, s, 0);
      const [a0, b0, a1, b1] = asked.tiles.get(v.page) ?? [1, 1, 0, 0];
      return x0 < a0 || y0 < b0 || x1 > a1 || y1 > b1;
    });
  }

  let jumpTo: string | undefined;
  let retries = 0;
  let compileSeq = 0;
  let inFlight = 0;
  let again = false;
  let lastCompileDurationMs = 0;
  let prewarmTimer: number | undefined;

  function cancelPrewarm() {
    if (prewarmTimer !== undefined) {
      clearTimeout(prewarmTimer);
      prewarmTimer = undefined;
    }
  }

  function schedulePrewarm(reqKey: string, s: number, viewPages: number[], totalPages: number) {
    cancelPrewarm();
    const neighbors = neighborPagesToPrewarm(viewPages, totalPages);
    if (neighbors.length === 0) return;
    const seq = compileSeq;
    prewarmTimer = setTimeout(() => {
      prewarmTimer = undefined;
      if (seq !== compileSeq || reqKey !== untracked(key) || !untracked(isActive)) return;
      if (untracked(dragging) || untracked(resizing) || untracked(live).k !== 1) return;
      void preview.prewarmNeighbors(s, neighbors);
    }, 40);
  }

  function tryHydrateColdStart(req: PreviewRequest, reqKey: string): boolean {
    if (!hasHud() || coldStartTried || untracked(pages).length > 0 || shownKey) return false;
    coldStartTried = true;
    try {
      const snap = preview.loadColdStartSnapshot();
      const wsRoot = untracked(info).root;
      if (
        !snap ||
        snap.pages.length === 0 ||
        !wsRoot ||
        snap.workspace !== wsRoot ||
        snap.target !== req.target ||
        snap.id !== req.id ||
        snap.book !== req.book
      ) {
        return false;
      }
      const curPane = untracked(pane);
      const effW = untracked(effectiveWidth);
      // Always re-fit against the current effective pane width so window-size changes never jump on frame 1.
      const restoredZoom = effW > 0 ? fitZoom(snap.pages, effW) : 0;
      const rawScrollY = snap.scrollY >= 0 ? snap.scrollY : 0;
      const restoredScrollY =
        restoredZoom > 0 && curPane.height > 0 ? clampScroll(snap.pages, restoredZoom, rawScrollY, curPane.height) : rawScrollY;
      const nextNeedsFit = restoredZoom === 0;
      batch(() => {
        shownKey = reqKey;
        showingSnapshot = true;
        needsFit = nextNeedsFit;
        autoFit = true;
        pages.set(snap.pages);
        zoom.set(restoredZoom);
        panX.set(0);
        scrollY.set(restoredScrollY);
      });
      if (restoredZoom > 0 && curPane.height > 0) {
        const newMaxScroll = Math.max(0, heightFor(snap.pages, restoredZoom, curPane.height, hasHud()) - curPane.height);
        commitScroll(restoredScrollY, 0, newMaxScroll);
      }
      return true;
    } catch {
      return false;
    }
  }

  let lastSnapshotSig = "";

  function saveColdStartSnapshotNow() {
    if (!hasHud() || showingSnapshot || inFlight > 0) return;
    if (untracked(live).k !== 1 || untracked(dragging) || untracked(resizing)) return;
    const req = untracked(props.request);
    if (!req || requestKey(req) !== shownKey) return;
    const list = untracked(pages);
    const z = untracked(zoom);
    const wsRoot = untracked(info).root;
    if (!wsRoot || list.length === 0 || z <= 0) return;

    const curView = untracked(view);
    const visible = curView.filter((v) => v.width > 0 && v.height > 0).map((v) => v.page);
    const chosen = visible.length > 0 ? visible.slice(0, 2) : [untracked(currentPage)];
    const renderScale = Math.max(1, z * pixelRatio());
    const metaJson = JSON.stringify({
      workspace: wsRoot,
      target: req.target,
      id: req.id,
      book: req.book,
      zoom: Math.round(z * 1000) / 1000,
      scrollY: Math.round(untracked(scrollY)),
    });
    const sig = `${metaJson}|${chosen.join(",")}|${Math.round(renderScale * 100)}`;
    if (sig === lastSnapshotSig) return;
    lastSnapshotSig = sig;
    try {
      void preview.saveColdStartSnapshot(metaJson, chosen, renderScale);
    } catch {
      // Snapshot persistence is best-effort and never blocks preview.
    }
  }

  function scheduleColdStartSnapshot() {
    if (!hasHud() || showingSnapshot) return;
    clearTimeout(snapshotTimer);
    snapshotTimer = setTimeout(() => {
      snapshotTimer = undefined;
      saveColdStartSnapshotNow();
    }, 1500);
  }

  function applyResult(reqKey: string, result: PreviewResult) {
    const isSwitch = reqKey !== shownKey;
    props.onResult?.(result);
    if (result.pages.length === 0) {
      if (!result.error && result.diagnostics.length === 0 && retries < 3) {
        retries += 1;
        setTimeout(compile, 60);
        return;
      }
      batch(() => {
        diagnostics.set(result.diagnostics);
        if (isSwitch) {
          shownKey = reqKey;
          showingSnapshot = false;
          pages.set([]);
        }
        if (pages().length === 0) failure.set(result.diagnostics.find((d) => d.severity === "error") ?? { severity: "error", message: result.error ?? "" });
      });
      return;
    }
    retries = 0;
    showingSnapshot = false;
    failure.set(undefined);
    lastStarts = result.starts;
    const start = jumpTo === undefined ? undefined : result.starts.find((x) => x.label === jumpTo);
    jumpTo = undefined;

    if (!isSwitch) {
      const oldMaxScroll = currentMaxScroll();
      batch(() => {
        diagnostics.set(result.diagnostics);
        pages.set(result.pages);
        if (needsFit) fit();
      });
      const z = zoom();
      const curPaneH = pane().height;
      const newMaxScroll = Math.max(0, heightFor(result.pages, z, curPaneH, hasHud(), isReader()) - curPaneH);
      if (start) {
        if (z > 0) {
          const tp = topPadFor(result.pages, z, curPaneH, hasHud(), isReader());
          const targetY = clampScroll(result.pages, z, topFor(result.pages, z, start.page, tp, isReader()), curPaneH);
          scrollY.set(targetY);
          commitScroll(targetY, oldMaxScroll, newMaxScroll);
        } else {
          pendingJumpPage = start.page;
        }
      } else if (z > 0 && scrollY() > newMaxScroll + 0.5) {
        scrollY.set(newMaxScroll);
        commitScroll(newMaxScroll, oldMaxScroll, newMaxScroll);
      }
      scheduleColdStartSnapshot();
      return;
    }

    const saved = hasHud() ? viewports.get(reqKey) : undefined;
    const curPane = pane();
    const oldMaxScroll = currentMaxScroll();
    let nextZoom: number;
    let nextPanX: number;
    let nextScrollY: number;
    let nextNeedsFit: boolean;
    let nextAutoFit: boolean;
    pendingJumpPage = undefined;
    if (saved && !saved.autoFit) {
      nextZoom = saved.zoom;
      nextPanX = saved.panX;
      nextNeedsFit = false;
      nextAutoFit = false;
      const tp = topPadFor(result.pages, nextZoom, curPane.height, hasHud(), isReader());
      const rawY = start ? topFor(result.pages, nextZoom, start.page, tp, isReader()) : saved.scrollY;
      nextScrollY = clampScroll(result.pages, nextZoom, rawY, curPane.height);
    } else {
      nextZoom = fitZoom(result.pages, untracked(effectiveWidth), isReader());
      nextNeedsFit = nextZoom === 0;
      nextAutoFit = true;
      nextPanX = 0;
      if (start && nextZoom === 0) {
        pendingJumpPage = start.page;
        nextScrollY = 0;
      } else {
        const tp = topPadFor(result.pages, nextZoom, curPane.height, hasHud(), isReader());
        const rawY = start ? topFor(result.pages, nextZoom, start.page, tp, isReader()) : saved ? saved.scrollY : 0;
        nextScrollY = clampScroll(result.pages, nextZoom, rawY, curPane.height);
      }
    }
    const newMaxScroll = Math.max(0, heightFor(result.pages, nextZoom, curPane.height, hasHud(), isReader()) - curPane.height);
    cancelLiveZoom();
    cancelPan();
    cancelPendingScroll();
    batch(() => {
      shownKey = reqKey;
      needsFit = nextNeedsFit;
      autoFit = nextAutoFit;
      diagnostics.set(result.diagnostics);
      pages.set(result.pages);
      zoom.set(nextZoom);
      panX.set(nextPanX);
      scrollY.set(nextScrollY);
    });
    commitScroll(nextScrollY, oldMaxScroll, newMaxScroll);
    scheduleColdStartSnapshot();
  }

  let inFlightSig = "";

  function compile() {
    const req = untracked(props.request);
    if (!req) return;
    if (!untracked(isActive)) {
      deferredCompile = true;
      return;
    }
    deferredCompile = false;
    if (inFlight >= 2) {
      again = true;
      return;
    }
    const reqKey = requestKey(req);
    const isSwitch = reqKey !== shownKey;

    const curPane = pane();
    const effPane = { width: untracked(effectiveWidth), height: curPane.height };
    const dpr = pixelRatio();
    let s: number;
    let list: PreviewView[];
    if (isSwitch) {
      const saved = hasHud() ? viewports.get(reqKey) : undefined;
      if (saved && !saved.autoFit) {
        s = scaleFor(saved.zoom, dpr);
        const tp = topPadFor(saved.pageSizes, saved.zoom, effPane.height, hasHud());
        list = viewFor(saved.pageSizes, saved.zoom, saved.scrollY, saved.panX, effPane, tp);
      } else {
        const estZoom = fitZoom([DEFAULT_PAGE], effPane.width, isReader());
        s = scaleFor(estZoom, dpr);
        const estPages = [DEFAULT_PAGE, DEFAULT_PAGE];
        const tp = topPadFor(estPages, estZoom, effPane.height, hasHud(), isReader());
        list =
          estZoom > 0 && effPane.height > 0
            ? viewFor(estPages, estZoom, 0, 0, effPane, tp, isReader())
            : [
                { page: 0, x: 0, y: 0, width: 0, height: 0 },
                { page: 1, x: 0, y: 0, width: 0, height: 0 },
              ];
      }
    } else {
      s = untracked(scale);
      list = untracked(view);
    }

    const pagesKey = list.map((v) => v.page).join();
    const tileEntries = list.filter((v) => v.width > 0).map((v) => [v.page, tiles(v, s, TILE / 2)] as const);
    const sig = `${reqKey}|${s}|${pagesKey}|${tileEntries.map(([p, t]) => `${p}:${t}`).join(";")}`;
    // Coalesce duplicate in-flight compiles triggered in the same frame by multiple effects.
    if (inFlight > 0 && sig === inFlightSig) {
      return;
    }

    const seq = ++compileSeq;
    inFlight += 1;
    inFlightSig = sig;

    asked = {
      key: reqKey,
      scale: s,
      pages: pagesKey,
      tiles: new Map(tileEntries),
    };

    cancelPrewarm();
    void preview.compile(req, s, list).then((result) => {
      inFlight -= 1;
      if (inFlight === 0) inFlightSig = "";
      const stale = seq !== compileSeq || reqKey !== untracked(key);
      if (!stale) {
        lastCompileDurationMs = (result.compileMs ?? 0) + (result.renderMs ?? 0);
        applyResult(reqKey, result);
        if (result.pages.length > 1) {
          schedulePrewarm(
            reqKey,
            s,
            list.map((v) => v.page),
            result.pages.length,
          );
        }
      }
      if (again) {
        again = false;
        compile();
      }
    });
  }

  effect(() => {
    if (due()) untracked(compile);
  });

  // Throttle cold-start snapshot persistence when viewport settles after scroll/zoom.
  effect(() => {
    const pLen = pages().length;
    const z = zoom();
    void scrollY();
    const settled = live().k === 1 && !dragging() && !resizing();
    if (hasHud() && pLen > 0 && z > 0 && settled && !showingSnapshot) {
      untracked(scheduleColdStartSnapshot);
    }
  });

  let lastKey: string | undefined;
  effect(() => {
    const next = key();
    untracked(() => {
      if (next === lastKey) return;
      lastKey = next;
      cancelLiveZoom();
      cancelPan();
      cancelPendingScroll();
      clearTimeout(resizeTimer);
      resizeTimer = undefined;
      pendingPane = undefined;
      resizeTransform.set({ k: 1, tx: 0, ty: 0 });
      if (untracked(resizing)) resizing.set(false);
      saveViewport();
      clearTimeout(snapshotTimer);
      snapshotTimer = undefined;
      compileSeq += 1;
      again = false;
      pendingJumpPage = undefined;
      if (!next) {
        deferredCompile = false;
        shownKey = undefined;
        showingSnapshot = false;
        needsFit = false;
        autoFit = true;
        lastStarts = [];
        contentHeightLaidOut = 0;
        asked = { key: "", scale: 0, pages: "", tiles: new Map() };
        batch(() => {
          pages.set([]);
          diagnostics.set([]);
          failure.set(undefined);
          zoom.set(0);
          panX.set(0);
          scrollY.set(0);
        });
        scrollTo(0);
        return;
      }
      if (!untracked(isActive)) {
        deferredCompile = true;
        return;
      }
      const req = untracked(props.request);
      if (req) tryHydrateColdStart(req, next);
      compile();
    });
  });

  // When a hidden warm canvas becomes active again, only compile if its request changed
  // or was invalidated while hidden; otherwise its already-rendered pages show on frame 0.
  effect(() => {
    if (!isActive()) return;
    untracked(() => {
      const curKey = key();
      if (!curKey) return;
      if (deferredCompile || curKey !== shownKey) {
        const req = untracked(props.request);
        if (req) tryHydrateColdStart(req, curKey);
        compile();
      }
    });
  });

  cleanup(() => {
    clearTimeout(resizeTimer);
    clearTimeout(pinchCommitTimer);
    clearTimeout(snapshotTimer);
    cancelPrewarm();
    if (zoomRaf !== undefined) cancelAnimationFrame(zoomRaf);
    cancelPendingScroll();
  });

  function cancelPan() {
    dragStart = undefined;
    if (dragging() || panOffset().dx !== 0 || panOffset().dy !== 0) {
      batch(() => {
        dragging.set(false);
        panOffset.set({ dx: 0, dy: 0 });
      });
    }
  }

  function commitPan() {
    const p = panOffset();
    dragStart = undefined;
    if (!dragging() && p.dx === 0 && p.dy === 0) return;
    const z = zoom();
    const list = pages();
    if (z === 0 || list.length === 0 || (p.dx === 0 && p.dy === 0)) {
      cancelPan();
      return;
    }
    const curPaneH = pane().height;
    const oldMaxScroll = currentMaxScroll();
    const newMaxScroll = Math.max(0, heightFor(list, z, curPaneH, hasHud()) - curPaneH);
    const nextPanX = left() + p.dx;
    const nextScrollY = Math.min(newMaxScroll, Math.max(0, scrollY() - p.dy));
    batch(() => {
      dragging.set(false);
      panOffset.set({ dx: 0, dy: 0 });
      panX.set(nextPanX);
      scrollY.set(nextScrollY);
    });
    commitScroll(nextScrollY, oldMaxScroll, newMaxScroll);
    scheduleColdStartSnapshot();
  }

  function cancelLiveZoom() {
    if (zoomRaf !== undefined) {
      cancelAnimationFrame(zoomRaf);
      zoomRaf = undefined;
    }
    clearTimeout(pinchCommitTimer);
    pinchCommitTimer = undefined;
    zoomTargetK = 1;
    live.set({ k: 1, tx: 0, ty: 0 });
  }

  function viewportAt(pagePosition: { x: number; y: number }) {
    return { x: pagePosition.x - paneOrigin.x, y: pagePosition.y - paneOrigin.y };
  }

  function applyLiveScale(nextK: number, at: { x: number; y: number }) {
    const z = zoom();
    if (z === 0) return;
    const cur = live();
    if (cur.k === 1 && cur.tx === 0 && cur.ty === 0) baseScrollY = scrollY();
    const clampedK = Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, z * nextK)) / z;
    const r = clampedK / cur.k;
    const tx = at.x - r * (at.x - cur.tx);
    const ty = at.y + baseScrollY - r * (at.y + baseScrollY - cur.ty);
    live.set({ k: clampedK, tx, ty });
  }

  function commitZoom() {
    if (zoomRaf !== undefined) {
      cancelAnimationFrame(zoomRaf);
      zoomRaf = undefined;
    }
    clearTimeout(pinchCommitTimer);
    pinchCommitTimer = undefined;
    const cur = live();
    const z = zoom();
    if (z === 0 || (cur.k === 1 && cur.tx === 0 && cur.ty === 0)) {
      zoomTargetK = 1;
      return;
    }
    autoFit = false;
    const list = pages();
    const curPaneH = pane().height;
    const nextZoom = Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, z * cur.k));
    const nextPanX = cur.tx + cur.k * left();
    const oldTopPad = topPadFor(list, z, curPaneH, hasHud());
    const newTopPad = topPadFor(list, nextZoom, curPaneH, hasHud());
    const oldMaxScroll = currentMaxScroll();
    const newMaxScroll = Math.max(0, heightFor(list, nextZoom, curPaneH, hasHud()) - curPaneH);
    // Keep the page point under the cursor fixed across any topPad change (e.g. centered -> overflowing).
    const rawScrollY = baseScrollY - cur.ty + (newTopPad - cur.k * oldTopPad);
    const nextScrollY = Math.min(newMaxScroll, Math.max(0, rawScrollY));
    zoomTargetK = 1;
    batch(() => {
      live.set({ k: 1, tx: 0, ty: 0 });
      zoom.set(nextZoom);
      panX.set(nextPanX);
      scrollY.set(nextScrollY);
    });
    commitScroll(nextScrollY, oldMaxScroll, newMaxScroll);
    scheduleColdStartSnapshot();
  }

  function stepWheelZoom(now: number) {
    zoomRaf = undefined;
    const dt = zoomLastTime === 0 ? 16 : Math.min(48, Math.max(1, now - zoomLastTime));
    zoomLastTime = now;
    const cur = live();
    const diff = zoomTargetK - cur.k;
    if (Math.abs(diff) / cur.k < 0.0015 || ms(1) === 0) {
      applyLiveScale(zoomTargetK, zoomAnchor);
      commitZoom();
      return;
    }
    const alpha = 1 - Math.exp(-dt / 32);
    applyLiveScale(cur.k + diff * alpha, zoomAnchor);
    zoomRaf = requestAnimationFrame(stepWheelZoom);
  }

  function applyResizePreview(w: number, h: number, nextInset: number) {
    const curPane = untracked(pane);
    const list = untracked(pages);
    const curZoom = untracked(zoom);
    if (list.length === 0 || curZoom <= 0 || w <= 0 || curPane.width <= 0) return;
    const reader = isReader();
    const oldEffW = Math.max(120, curPane.width - lastCommittedInset);
    const newEffW = Math.max(120, w - nextInset);
    const oldLeft = pageLeftFor(list[0].width, curZoom, oldEffW, untracked(panX), reader);
    if (autoFit || reader) {
      const nextZoom = fitZoom(list, newEffW, reader);
      if (nextZoom <= 0) return;
      const k = nextZoom / curZoom;
      const newLeft = pageLeftFor(list[0].width, nextZoom, newEffW, 0, reader);
      const tx = newLeft - k * oldLeft;
      const oldTop = topPadFor(list, curZoom, curPane.height, hasHud(), reader);
      const newTop = topPadFor(list, nextZoom, h, hasHud(), reader);
      const curScroll = untracked(scrollY);
      const ty = newTop - k * oldTop + (1 - k) * curScroll;
      resizeTransform.set({ k, tx, ty });
    } else {
      const newLeft = pageLeftFor(list[0].width, curZoom, newEffW, untracked(panX), false);
      resizeTransform.set({ k: 1, tx: newLeft - oldLeft, ty: 0 });
    }
  }

  function commitResize() {
    clearTimeout(resizeTimer);
    resizeTimer = undefined;
    const targetPane = pendingPane ?? untracked(pane);
    pendingPane = undefined;
    const curInset = untracked(inset);
    lastCommittedInset = curInset;
    paneView?.measure().then((m) => {
      if (m) paneOrigin = m.pagePosition;
    });
    const list = untracked(pages);
    const curZoom = untracked(zoom);
    if (targetPane.width <= 0 || list.length === 0 || curZoom <= 0) {
      batch(() => {
        resizeTransform.set({ k: 1, tx: 0, ty: 0 });
        if (targetPane.width > 0) pane.set(targetPane);
        resizing.set(false);
      });
      if (needsFit) fit();
      return;
    }
    const reader = isReader();
    const oldMaxScroll = currentMaxScroll();
    const effW = Math.max(120, targetPane.width - curInset);
    const shouldRefit = (autoFit || reader) && !untracked(dragging) && untracked(live).k === 1;
    const nextZoom = shouldRefit ? fitZoom(list, effW, reader) : curZoom;
    const zoomChanged = shouldRefit && nextZoom > 0 && Math.abs(nextZoom - curZoom) > 0.001;
    const finalZoom = zoomChanged ? nextZoom : curZoom;
    const newMaxScroll = Math.max(0, heightFor(list, finalZoom, targetPane.height, hasHud(), reader) - targetPane.height);
    const curScroll = untracked(scrollY);
    const nextScrollY = Math.min(newMaxScroll, Math.max(0, curScroll));
    batch(() => {
      resizeTransform.set({ k: 1, tx: 0, ty: 0 });
      pane.set(targetPane);
      resizing.set(false);
      if (zoomChanged) {
        zoom.set(finalZoom);
        panX.set(0);
        scrollY.set(nextScrollY);
      } else if (curScroll > newMaxScroll + 0.5) {
        scrollY.set(newMaxScroll);
      }
    });
    if (zoomChanged || curScroll > newMaxScroll + 0.5) {
      commitScroll(nextScrollY, oldMaxScroll, newMaxScroll);
    }
  }

  function zoomWheel(factor: number, at: { x: number; y: number }) {
    if (pendingPane || untracked(resizeTransform).k !== 1 || untracked(resizeTransform).tx !== 0) {
      commitResize();
    }
    const z = zoom();
    if (z === 0) return;
    autoFit = false;
    commitPan();
    cancelPendingScroll();
    clearTimeout(pinchCommitTimer);
    pinchCommitTimer = undefined;
    const cur = live();
    if (cur.k === 1 && cur.tx === 0 && cur.ty === 0 && zoomRaf === undefined) {
      baseScrollY = scrollY();
      zoomTargetK = 1;
    }
    zoomAnchor = at;
    zoomTargetK = Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, z * zoomTargetK * factor)) / z;
    if (zoomRaf === undefined) {
      zoomLastTime = 0;
      zoomRaf = requestAnimationFrame(stepWheelZoom);
    }
  }

  function zoomPinch(delta: number, phase: "start" | "move" | "end", at: { x: number; y: number }) {
    if (zoom() === 0) return;
    // On Linux GTK4, Ctrl+wheel is delivered via `onPinch` as discrete steps (|delta| >= 0.04)
    // followed by a delayed `phase === "end"`. Route discrete steps through exponential damping
    // so wheel zooming glides smoothly instead of jumping notch-by-notch or committing mid-gesture.
    if (phase === "end") {
      if (zoomRaf !== undefined) return; // let the active wheel damping tail settle and commit cleanly
      commitZoom();
      return;
    }
    if (Math.abs(delta) >= 0.04) {
      zoomWheel(1 + delta, at);
      return;
    }
    if (pendingPane || untracked(resizeTransform).k !== 1 || untracked(resizeTransform).tx !== 0) {
      commitResize();
    }
    autoFit = false;
    commitPan();
    cancelPendingScroll();
    if (zoomRaf !== undefined) {
      cancelAnimationFrame(zoomRaf);
      zoomRaf = undefined;
    }
    clearTimeout(pinchCommitTimer);
    pinchCommitTimer = undefined;
    const cur = live();
    if (phase === "start" && cur.k === 1 && cur.tx === 0 && cur.ty === 0) baseScrollY = scrollY();
    if (delta !== 0) applyLiveScale(cur.k * (1 + delta), at);
    pinchCommitTimer = setTimeout(commitZoom, 240);
  }

  function fit() {
    const list = pages();
    const curPane = pane();
    const effW = effectiveWidth();
    const reader = isReader();
    if (!needsFit || list.length === 0 || effW === 0) return;
    const nextZoom = fitZoom(list, effW, reader);
    if (nextZoom === 0) return;
    needsFit = false;
    autoFit = true;
    const oldMaxScroll = currentMaxScroll();
    const jumpPage = pendingJumpPage;
    pendingJumpPage = undefined;
    const tp = topPadFor(list, nextZoom, curPane.height, hasHud(), reader);
    const nextScrollY =
      jumpPage !== undefined
        ? clampScroll(list, nextZoom, topFor(list, nextZoom, jumpPage, tp, reader), curPane.height)
        : clampScroll(list, nextZoom, scrollY(), curPane.height);
    batch(() => {
      zoom.set(nextZoom);
      panX.set(0);
      if (jumpPage !== undefined || nextScrollY !== scrollY()) scrollY.set(nextScrollY);
    });
    if (jumpPage !== undefined) {
      const newMaxScroll = Math.max(0, heightFor(list, nextZoom, curPane.height, hasHud(), reader) - curPane.height);
      commitScroll(nextScrollY, oldMaxScroll, newMaxScroll);
    }
  }

  function fitWidth() {
    const list = pages();
    if (list.length === 0) return;
    const reader = isReader();
    clearTimeout(resizeTimer);
    resizeTimer = undefined;
    if (pendingPane) {
      pane.set(pendingPane);
      pendingPane = undefined;
    }
    lastCommittedInset = untracked(inset);
    resizeTransform.set({ k: 1, tx: 0, ty: 0 });
    if (untracked(resizing)) resizing.set(false);
    cancelLiveZoom();
    cancelPan();
    cancelPendingScroll();
    const curPane = pane();
    const page = currentPage();
    const nextZoom = fitZoom(list, effectiveWidth(), reader);
    if (nextZoom === 0) return;
    autoFit = true;
    const oldMaxScroll = currentMaxScroll();
    const tp = topPadFor(list, nextZoom, curPane.height, hasHud(), reader);
    const nextScrollY = clampScroll(list, nextZoom, topFor(list, nextZoom, page, tp, reader), curPane.height);
    batch(() => {
      zoom.set(nextZoom);
      panX.set(0);
      scrollY.set(nextScrollY);
    });
    commitScroll(nextScrollY, oldMaxScroll, Math.max(0, heightFor(list, nextZoom, curPane.height, hasHud(), reader) - curPane.height));
  }

  // Preview the Inspector drawer inset via GPU compositor transform during the drawer slide,
  // then commit layout + rasterization once after the slide settles (zero mid-slide reflow).
  let lastInset = untracked(inset);
  effect(() => {
    const nextInset = inset();
    if (nextInset === lastInset) return;
    lastInset = nextInset;
    untracked(() => {
      const list = pages();
      const curPane = pendingPane ?? pane();
      const curZoom = zoom();
      if (list.length === 0 || curPane.width <= 0 || curZoom <= 0) {
        lastCommittedInset = nextInset;
        return;
      }
      if (!dragging() && live().k === 1) {
        pendingPane = curPane;
        if (!resizing()) resizing.set(true);
        applyResizePreview(curPane.width, curPane.height, nextInset);
        clearTimeout(resizeTimer);
        resizeTimer = setTimeout(commitResize, 220);
      } else {
        lastCommittedInset = nextInset;
      }
    });
  });

  function goPage(index: number, animated = true) {
    const list = pages();
    const z = zoom();
    if (list.length === 0 || z === 0) return;
    cancelLiveZoom();
    cancelPan();
    cancelPendingScroll();
    const i = Math.max(0, Math.min(list.length - 1, index));
    const curPaneH = pane().height;
    const tp = topPadFor(list, z, curPaneH, hasHud());
    const y = clampScroll(list, z, topFor(list, z, i, tp), curPaneH);
    if (!animated) scrollY.set(y);
    scrollTo(y, animated);
  }

  function scrollToLocation(page: number, yPt: number, animated = true) {
    const list = pages();
    const z = zoom();
    if (list.length === 0 || z === 0) return;
    cancelLiveZoom();
    cancelPan();
    cancelPendingScroll();
    const i = Math.max(0, Math.min(list.length - 1, page));
    const curPaneH = pane().height;
    const tp = topPadFor(list, z, curPaneH, hasHud());
    const pageTop = pageYFor(list, z, i, tp);
    const rawY = pageTop + yPt * z - curPaneH * 0.3;
    const y = clampScroll(list, z, rawY, curPaneH);
    if (!animated) scrollY.set(y);
    scrollTo(y, animated);
  }

  const counts = computed(() => diagnosticCounts(diagnostics()));
  const flagged = computed(() => diagnostics().filter((d) => d.severity === "error" || d.severity === "warning"));
  const hudOpen = signal(false);

  props.controller({
    compile,
    lastCompileMs: () => lastCompileDurationMs,
    jumpTo(label: string) {
      const start = lastStarts.find((x) => x.label === label);
      if (start && shownKey === key() && zoom() > 0) {
        goPage(start.page, true);
        return;
      }
      jumpTo = label;
    },
    goPage,
    scrollToLocation,
    zoomBy: (factor: number) => {
      if (isReader()) return;
      traceInteraction("preview.zoom_step", () =>
        zoomWheel(factor, { x: effectiveWidth() / 2, y: pane().height / 2 }),
      );
    },
    fitWidth,
    closeHud() {
      if (!hudOpen()) return false;
      hudOpen.set(false);
      return true;
    },
  });

  function locate(e: { pagePosition: { x: number; y: number } }) {
    const at = viewportAt(e.pagePosition);
    const hit = hitPage(pages(), zoom(), left(), at.x, at.y + scrollY(), topPad());
    if (!hit) return;
    const loc = preview.locateClick(hit.page, hit.xPt, hit.yPt);
    if (loc) props.onLocate?.(loc);
  }

  // Hide HUD smoothly on narrow panes when the Inspector drawer is open so it is never clipped.
  const hudHidden = computed(() => inset() > 0 && pane().width > 0 && availableWidth() < 320);
  const hudCompact = computed(() => pane().width > 0 && availableWidth() < 380);

  const preflightLabel = () => {
    const c = counts();
    if (c.errors > 0 && c.warnings > 0) return t("preview.errorsAndWarnings", { errors: c.errors, warnings: c.warnings });
    if (c.errors > 0) return t("preview.errorsOnly", { count: c.errors });
    return t("preview.warningsOnly", { count: c.warnings });
  };

  return (
    <View
      ref={(r) => (paneView = r)}
      style={{ flex: 1, overflow: "hidden" }}
      class="bg-sidebar"
      onLayout={(e) => {
        const prev = untracked(pane);
        const w = e.bounds.size.width;
        const h = e.bounds.size.height;
        const moved = prev.width > 0 && (Math.abs(prev.width - w) > 0.5 || Math.abs(prev.height - h) > 0.5);
        const list = untracked(pages);
        const curZoom = untracked(zoom);
        // Continuous split/window resize: drive the page stack purely via GPU compositor
        // `resizeTransform` with zero per-frame layout reflow, `scrollTo`, `measure`, or
        // rasterization, then commit layout + crisp tiles once 120ms after release.
        if (moved && !needsFit && list.length > 0 && curZoom > 0 && !untracked(dragging) && untracked(live).k === 1) {
          pendingPane = { width: w, height: h };
          if (!untracked(resizing)) resizing.set(true);
          applyResizePreview(w, h, untracked(inset));
          clearTimeout(resizeTimer);
          resizeTimer = setTimeout(commitResize, 120);
          return;
        }
        pane.set({ width: w, height: h });
        if (!moved) {
          paneView?.measure().then((m) => {
            if (m) paneOrigin = m.pagePosition;
          });
        } else {
          resizing.set(true);
          clearTimeout(resizeTimer);
          resizeTimer = setTimeout(commitResize, 120);
        }
        if (needsFit) {
          fit();
          return;
        }
        if (list.length === 0 || w <= 0) return;
        if (curZoom > 0) {
          const newMaxScroll = Math.max(0, heightFor(list, curZoom, h, hasHud(), isReader()) - h);
          if (untracked(scrollY) > newMaxScroll + 0.5) {
            scrollY.set(newMaxScroll);
            scrollTo(newMaxScroll);
          }
        }
      }}
    >
      <Scroll
        ref={(r) => (scroll = r)}
        style={{ flex: 1 }}
        onScrollDragStart={() => cancelPendingScroll()}
        onScroll={(e) => {
          if (e.contentSize.height > 0) contentHeightLaidOut = e.contentSize.height;
          if (live().k !== 1) {
            commitZoom();
            return;
          }
          if (pendingScroll) {
            if (Math.abs(e.offset.y - pendingScroll.y) <= 1) {
              cancelPendingScroll();
              scrollY.set(e.offset.y);
              return;
            }
            if (e.contentSize.height < pendingScroll.minContentHeight - 1) return;
            cancelPendingScroll();
          }
          scrollY.set(e.offset.y);
        }}
      >
        <View
          panActivation={{ activeOffsetX: 10 }}
          onLayout={(e) => {
            const h = e.bounds.size.height;
            if (h > 0) contentHeightLaidOut = h;
            if (pendingScroll && h >= pendingScroll.minContentHeight - 1) {
              const targetY = pendingScroll.y;
              cancelPendingScroll();
              scrollY.set(targetY);
              scrollTo(targetY);
            }
          }}
          onWheel={(e) => {
            cancelPendingScroll();
            if (isReader()) return;
            if (e.modifiers.control) {
              const steps = e.deltaMode === "lines" ? e.delta.y : e.delta.y / 40;
              zoomWheel(Math.pow(1.12, -steps), viewportAt(e.pagePosition));
            } else {
              if (live().k !== 1) commitZoom();
              const k = e.deltaMode === "lines" ? 40 : 1;
              if (e.delta.x !== 0) panX.set(left() - e.delta.x * k);
            }
          }}
          onPinch={(e) => {
            if (isReader()) return;
            zoomPinch(e.delta, e.phase, viewportAt(e.pagePosition));
          }}
          onPress={(e) => {
            if (isReader()) return;
            if (e.modifiers.control || e.modifiers.platform) locate(e);
          }}
          onMouseMove={(e) => {
            if (isReader()) return;
            if (e.pressedButton !== "left" || e.modifiers.control) {
              if (dragStart || dragging()) commitPan();
              return;
            }
            cancelPendingScroll();
            if (live().k !== 1) commitZoom();
            if (!dragStart) {
              dragStart = e.pagePosition;
              return;
            }
            const dx = e.pagePosition.x - dragStart.x;
            const dy = e.pagePosition.y - dragStart.y;
            if (!dragging() && Math.hypot(dx, dy) < 3) return;
            batch(() => {
              dragging.set(true);
              panOffset.set({ dx, dy });
            });
          }}
          onMouseUp={() => {
            if (isReader()) return;
            commitPan();
          }}
          onMouseLeave={() => {
            if (isReader()) return;
            commitPan();
          }}
          style={{
            paddingTop: topPad(),
            paddingBottom: bottomPad(),
            gap: pageGap(zoom(), isReader()),
            opacity: zoom() === 0 ? 0 : 1,
            transform: liveTransform(),
            transformOrigin: "0 0",
            cursor: isReader() ? "default" : dragging() ? "grabbing" : zoom() > 0 ? "grab" : "default",
          }}
        >
          <Each of={pages()} key="index">
            {(page) => {
              const dark = () => colorScheme() === "dark";
              return (
                <View
                  style={{
                    width: page().width * zoom(),
                    height: page().height * zoom(),
                    transform: `translateX(${left()}px)`,
                    backgroundColor: "#ffffff",
                    borderWidth: 1,
                    borderColor: dark() ? "rgba(255,255,255,0.09)" : "rgba(0,0,0,0.06)",
                    boxShadow: dark()
                      ? "0 2px 10px rgba(0,0,0,0.38)"
                      : "0 2px 8px rgba(0,0,0,0.08)",
                    overflow: "hidden",
                  }}
                >
                  <PageImage layers={page().layers} style={{ width: "100%", height: "100%" }} />
                </View>
              );
            }}
          </Each>
        </View>
      </Scroll>

      {/* Initial compile >150 ms loading indicator */}
      <If when={pages().length === 0 && !failure() && !!key()}>
        <View style={{ position: "absolute", top: 0, left: 0, right: 0, bottom: 0, alignItems: "center", justifyContent: "center", pointerEvents: "none" }}>
          <DelayedSpinner active={true} size={18} />
        </View>
      </If>

      {/* Typesetting failure overlay */}
      <If when={pages().length === 0 ? failure() : undefined}>
        {(f) => (
          <View style={{ position: "absolute", top: 0, left: 0, right: 0, bottom: 0, alignItems: "center", justifyContent: "center", padding: SPACE[32], gap: SPACE[8] }}>
            <Text style={{ fontSize: FONT_SIZE.body, fontWeight: FONT_WEIGHT.semibold, lineHeight: LINE_HEIGHT.strong }} class="text-secondary-foreground">
              {t("preview.typesetFailed")}
            </Text>
            <If when={!!f().message}>
              <Text
                style={{ fontSize: FONT_SIZE.caption, lineHeight: LINE_HEIGHT.body, textAlign: "center", maxWidth: 420, numberOfLines: 3, textOverflow: "ellipsis" }}
                class="text-muted-foreground"
              >
                {f().message}
              </Text>
            </If>
            <If when={f().file}>
              <Text
                style={{ fontSize: FONT_SIZE.caption, lineHeight: LINE_HEIGHT.body, textDecorationLine: props.onDiagnostic ? "underline" : "none" }}
                class={props.onDiagnostic ? "text-primary" : "text-muted-foreground"}
                onPress={() => props.onDiagnostic?.(f())}
              >
                {`${f().file}${f().line ? `:${f().line}` : ""}`}
              </Text>
            </If>
          </View>
        )}
      </If>

      {/* Unified floating HUD capsule */}
      <If when={hasHud()}>
        <View
          style={{
            position: "absolute",
            left: 0,
            right: 0,
            bottom: 16,
            alignItems: "center",
            pointerEvents: hudHidden() ? "none" : "box-none",
            opacity: hudHidden() ? 0 : 1,
            transform: `translateX(${-inset() / 2}px)`,
            ...panelTransition(),
          }}
        >
          <View
            style={{
              height: CONTROL.lg,
              flexDirection: "row",
              alignItems: "center",
              borderRadius: RADIUS.full,
              borderWidth: 1,
              paddingLeft: SPACE[4],
              paddingRight: SPACE[4],
              backdropFilter: "blur(12px)",
              boxShadow: popoverShadow(),
            }}
            class="bg-popover border-border"
          >
            {/* Zoom group */}
            <If when={!hudCompact()}>
              <HudButton
                icon="minus"
                disabled={zoom() === 0 || zoom() <= MIN_ZOOM + 0.001}
                onPress={() => zoomWheel(1 / 1.25, { x: effectiveWidth() / 2, y: pane().height / 2 })}
                tooltip={tip(t("preview.zoomOut"), "mod+minus")}
              />
            </If>
            <View
              onPress={() => zoom() > 0 && fitWidth()}
              tooltip={tip(t("preview.fitWidth"), "mod+0")}
              class={zoom() > 0 ? "hover:bg-muted active:bg-accent" : ""}
              style={{
                height: CONTROL.sm,
                minWidth: 42,
                paddingLeft: SPACE[6],
                paddingRight: SPACE[6],
                borderRadius: RADIUS.full,
                alignItems: "center",
                justifyContent: "center",
                transitionProperty: "colors",
                transitionDuration: ms(120),
                transitionTimingFunction: EASE_STANDARD,
              }}
            >
              <Text style={{ textAlign: "center", fontSize: FONT_SIZE.caption, fontVariant: ["tabular-nums"] }} class="text-secondary-foreground">
                {zoom() > 0 ? `${Math.round(zoom() * live().k * resizeTransform().k * 100)}%` : "–"}
              </Text>
            </View>
            <If when={!hudCompact()}>
              <HudButton
                icon="plus"
                disabled={zoom() === 0 || zoom() >= MAX_ZOOM - 0.001}
                onPress={() => zoomWheel(1.25, { x: effectiveWidth() / 2, y: pane().height / 2 })}
                tooltip={tip(t("preview.zoomIn"), "mod+plus")}
              />
            </If>

            <View style={{ width: 1, height: 14, marginLeft: SPACE[4], marginRight: SPACE[4] }} class="bg-border" />

            {/* Page group */}
            <HudButton
              icon="chevronLeft"
              disabled={pages().length === 0 || currentPage() <= 0}
              onPress={() => goPage(currentPage() - 1)}
              tooltip={tip(t("preview.prevPage"))}
            />
            <Text style={{ minWidth: 44, textAlign: "center", fontSize: FONT_SIZE.caption, fontVariant: ["tabular-nums"] }} class="text-secondary-foreground">
              {pages().length > 0 ? `${currentPage() + 1} / ${pages().length}` : "–"}
            </Text>
            <HudButton
              icon="chevronRight"
              disabled={pages().length === 0 || currentPage() >= pages().length - 1}
              onPress={() => goPage(currentPage() + 1)}
              tooltip={tip(t("preview.nextPage"))}
            />

            {/* Preflight diagnostics */}
            <If when={flagged().length > 0}>
              <View style={{ width: 1, height: 14, marginLeft: SPACE[4], marginRight: SPACE[4] }} class="bg-border" />
              <Popover
                open={hudOpen()}
                onOpenChange={(e) => hudOpen.set(e.open)}
                placement="top"
                offset={8}
                popupClass="bg-popover border border-border rounded-[10px] p-1.5"
                popupStyle={{ width: 360, maxHeight: 320, boxShadow: popoverShadow() }}
                trigger={
                  <View
                    testID="preflight-badge"
                    style={{
                      height: CONTROL.sm,
                      paddingLeft: SPACE[8],
                      paddingRight: SPACE[8],
                      borderRadius: RADIUS.full,
                      flexDirection: "row",
                      alignItems: "center",
                      gap: SPACE[4],
                      transitionProperty: "colors",
                      transitionDuration: ms(120),
                      transitionTimingFunction: EASE_STANDARD,
                    }}
                    class="hover:bg-muted active:bg-accent"
                  >
                    <View style={{ width: 6, height: 6, borderRadius: RADIUS.xs }} class={counts().errors > 0 ? "bg-destructive" : "bg-chart-4"} />
                    <Text style={{ fontSize: FONT_SIZE.caption, fontVariant: ["tabular-nums"] }} class={counts().errors > 0 ? "text-destructive" : "text-chart-4"}>
                      {preflightLabel()}
                    </Text>
                  </View>
                }
              >
                <Scroll style={{ maxHeight: 308 }}>
                  <Each of={flagged()} key={(_, i) => String(i)}>
                    {(d) => (
                      <View
                        onPress={() => {
                          hudOpen.set(false);
                          const x = d();
                          if (x.page != null && !x.file) goPage(x.page);
                          else props.onDiagnostic?.(x);
                        }}
                        class="hover:bg-muted active:bg-accent"
                        style={{ paddingTop: SPACE[6], paddingBottom: SPACE[6], paddingLeft: SPACE[8], paddingRight: SPACE[8], borderRadius: RADIUS.sm, gap: SPACE[2] }}
                      >
                        <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[6] }}>
                          <View style={{ width: 6, height: 6, borderRadius: RADIUS.xs }} class={d().severity === "error" ? "bg-destructive" : "bg-chart-4"} />
                          <Text style={{ numberOfLines: 2, textOverflow: "ellipsis", flex: 1, fontSize: FONT_SIZE.caption }}>
                            {d().message}
                          </Text>
                        </View>
                        <If when={Boolean(d().file || d().page != null)}>
                          <Text style={{ fontSize: FONT_SIZE.caption, marginLeft: SPACE[12], fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
                            {d().file ? `${d().file}${d().line ? `:${d().line}` : ""}` : t("preview.pageNumber", { page: d().page! + 1 })}
                          </Text>
                        </If>
                      </View>
                    )}
                  </Each>
                </Scroll>
              </Popover>
            </If>
          </View>
        </View>
      </If>
    </View>
  );
}

function HudButton(props: { icon: IconName; onPress: () => void; disabled?: boolean; tooltip?: any }) {
  return (
    <View
      disabled={props.disabled}
      onPress={() => !props.disabled && props.onPress()}
      tooltip={props.tooltip}
      class={props.disabled ? "" : "hover:bg-muted active:bg-accent"}
      style={{
        width: 24,
        height: CONTROL.sm,
        borderRadius: RADIUS.full,
        alignItems: "center",
        justifyContent: "center",
        opacity: props.disabled ? 0.35 : 1,
        transitionProperty: "colors",
        transitionDuration: ms(120),
        transitionTimingFunction: EASE_STANDARD,
      }}
    >
      <Icon name={props.icon} size="sm" />
    </View>
  );
}

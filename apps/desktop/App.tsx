import process from "process";
import { batch, computed, effect, mount, pixelRatio, Scroll, type ScrollRef, signal, Text, untracked, View, type ViewRef } from "nsh";
import { Button, Editor, type EditorDiagnostic, Select, Toggle, ToggleGroup } from "nsh/base";
import {
  PageImage,
  Preview,
  type PreviewDiagnostic,
  type PreviewPage,
  type PreviewRequest,
  type PreviewResult,
  type PreviewStart,
  type PreviewView,
  type WorkspaceFile,
  type WorkspaceInfo,
} from "xcpc-workstation/native";

// Phase 1: a problem or a contest of the workspace. The bar picks it (and, in a contest,
// the problem to edit) and the book; the editor holds one Markdown file, saved a moment
// after each edit; the preview typesets the whole problem or contest with the editor's
// text as it is, saved or not.
//
// The pages scroll in a native scroll view (momentum, moved on the UI thread) and are
// laid out at the zoom's size; a zoom stretches what is there until sharper layers
// arrive. Each page keeps its view across results (`key="index"`), and `PageImage`
// keeps the old layers until the new ones have loaded, so nothing blinks. Only what is
// in view is rasterized at the zoom's scale.

const preview = new Preview(process.dataDir);
const GAP = 16; // points between pages, at zoom 1
const MIN_ZOOM = 0.1;
const MAX_ZOOM = 8;
const TILE = 512; // pixels, as the native side tiles
const BASE_SCALE = 1.5; // above this the native side tiles the part in view

/** What the bar picked: `problem:<id>` or `contest:<id>`. */
type Pick = { target: "problem" | "contest"; id: string };
type Book = "statements" | "editorial";
type PageSize = { width: number; height: number };
type ViewportMemory = { zoom: number; scrollY: number; panX: number; pageSizes: PageSize[] };
const DEFAULT_PAGE: PageSize = { width: 595.28, height: 841.89 }; // A4 in points
const SAVE_AFTER = 500; // ms after the last edit
const OPEN_WORKSPACE = "workspace:open"; // the bar's item that opens another workspace

function parsePick(value: string | null | undefined): Pick | undefined {
  const [target, id] = (value ?? "").split(":");
  return (target === "problem" || target === "contest") && id ? { target, id } : undefined;
}

function targetKey(p: Pick | undefined, b: Book): string | undefined {
  return p ? `${p.target}:${p.id}:${b}` : undefined;
}

function requestKey(req: PreviewRequest): string {
  return `${req.target}:${req.id}:${req.book}`;
}

function scaleFor(z: number, dpr: number): number {
  return Math.min(Math.ceil(Math.max(z, 0.5) * dpr * 2) / 2, 8);
}

function fitZoomFor(list: readonly { width: number }[], paneWidth: number): number {
  if (list.length === 0 || paneWidth <= 0) return 0;
  return Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, (paneWidth - 2 * GAP) / list[0].width));
}

function contentHeightFor(list: readonly { height: number }[], z: number): number {
  if (list.length === 0 || z <= 0) return 0;
  return list.reduce((h, p) => h + (p.height + GAP) * z, GAP * z);
}

function pageTopFor(list: readonly { height: number }[], z: number, index: number): number {
  return list.slice(0, index).reduce((y, p) => y + (p.height + GAP) * z, 0);
}

function leftFor(pageWidth: number, z: number, paneWidth: number, curPanX: number): number {
  const w = pageWidth * z;
  const room = paneWidth - w;
  if (room >= 2 * GAP) return room / 2;
  return Math.min(GAP, Math.max(room - GAP, curPanX));
}

function computeView(
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

function App() {
  const info = signal<WorkspaceInfo>(preview.workspace());
  const pick = signal<Pick | undefined>(undefined);
  const label = signal<string | undefined>(undefined); // the contest's problem in the editor
  const book = signal<Book>("statements");
  const file = signal<WorkspaceFile | undefined>(undefined);
  const diagnostics = signal<PreviewDiagnostic[]>([]);
  const saveError = signal<string | undefined>(undefined);

  // The editor's text, kept outside signals: typing must not re-render the bar.
  let text = "";
  let saveTimer: number | undefined;
  let dirty = false;

  const contest = computed(() => {
    const p = pick();
    return p?.target === "contest" ? info().contests.find((c) => c.id === p.id) : undefined;
  });
  // The file to edit: the problem's, or the contest's problem's.
  const path = computed(() => {
    const p = pick();
    if (!p) return undefined;
    const files =
      p.target === "problem"
        ? info().problems.find((x) => x.id === p.id)
        : contest()?.items.find((x) => x.label === label()) ?? contest()?.items[0];
    if (!files) return undefined;
    return book() === "editorial" ? files.editorial : files.statement;
  });

  function save() {
    clearTimeout(saveTimer);
    saveTimer = undefined;
    const f = file();
    if (!dirty || !f || f.readOnly) return;
    dirty = false;
    const res = preview.save(f.path, text, f.hash);
    if (res.ok) {
      file.set({ ...f, hash: res.hash });
      saveError.set(undefined);
    } else {
      saveError.set(res.error ?? undefined);
    }
  }

  // A new target, book, or file: save the old file when the path changes, read the new
  // one (the editor is made anew per path so undo never crosses files), and trigger a
  // compile whenever the target, book, or file changes.
  effect(() => {
    const nextPick = pick();
    const nextBook = book();
    const nextPath = path();
    untracked(() => {
      if (nextPath !== file()?.path) {
        save();
        if (!nextPath) {
          file.set(undefined);
        } else {
          const f = preview.read(nextPath);
          text = f.text;
          file.set(f);
        }
      }
      if (nextPick) {
        void nextBook;
        compile();
      }
    });
  });

  function request(): PreviewRequest | undefined {
    const p = pick();
    if (!p) return undefined;
    const f = file();
    return { target: p.target, id: p.id, book: book(), path: f?.path ?? "", text: f ? text : "" };
  }

  const editorDiagnostics = computed<EditorDiagnostic[]>(() =>
    diagnostics()
      .filter((d) => d.file === file()?.path && d.line != null)
      .map((d) => {
        const at = { line: d.line! - 1, column: (d.column ?? 1) - 1 };
        return { range: { start: at, end: at }, severity: d.severity === "error" ? "error" : "warning", message: d.message };
      }),
  );

  const pages = signal<PreviewPage[]>([]);
  const error = signal<string | undefined>(undefined);
  const zoom = signal(0);           // 0 until the first fit
  const pane = signal({ width: 0, height: 0 });
  const scrollY = signal(0);
  const panX = signal(0);           // where the pages' left edge is, when wider than the pane
  let scroll: ScrollRef | undefined;
  let paneView: ViewRef | undefined;
  let paneOrigin = { x: 0, y: 0 };

  // SWR & viewport memory keyed by `(target, id, book)`: old pages stay visible until
  // the new target's first frame arrives, avoiding white flashes on switch.
  const viewports = new Map<string, ViewportMemory>();
  let shownKey: string | undefined;
  let needsFit = false;
  let pendingJumpPage: number | undefined;
  let lastStarts: PreviewStart[] = [];

  // Track the content container's laid-out height so post-zoom/post-switch scroll
  // targets beyond the previous maxScroll can settle atomically once layout grows,
  // while remaining immediately interruptible by any user scroll or gesture.
  let contentHeightLaidOut = 0;
  let pendingScroll: { y: number; minContentHeight: number; raf?: number } | undefined;

  function cancelPendingScroll() {
    if (pendingScroll?.raf !== undefined) {
      cancelAnimationFrame(pendingScroll.raf);
    }
    pendingScroll = undefined;
  }

  function currentMaxScroll(): number {
    const h = contentHeightLaidOut > 0 ? contentHeightLaidOut : contentHeightFor(pages(), zoom());
    return Math.max(0, h - pane().height);
  }

  function clampScroll(list: readonly PageSize[], z: number, y: number): number {
    const max = Math.max(0, contentHeightFor(list, z) - pane().height);
    return Math.min(max, Math.max(0, y));
  }

  function commitScroll(targetY: number, oldMaxScroll: number, newMaxScroll: number) {
    const clampedY = Math.min(newMaxScroll, Math.max(0, targetY));
    cancelPendingScroll();
    scrollTo(clampedY);
    if (clampedY > oldMaxScroll + 0.5) {
      const entry: { y: number; minContentHeight: number; raf?: number } = {
        y: clampedY,
        minContentHeight: clampedY + pane().height,
      };
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
    if (!shownKey) return;
    const list = pages();
    const z = zoom();
    if (list.length === 0 || z === 0) return;
    viewports.set(shownKey, {
      zoom: z,
      scrollY: scrollY(),
      panX: panX(),
      pageSizes: list.map((p) => ({ width: p.width, height: p.height })),
    });
  }

  // Live GPU-composited zoom transform while a pinch or Ctrl+wheel gesture is active:
  // avoids re-laying out pages and racing Scroll.scrollTo on every event, then commits
  // once when the gesture settles.
  const live = signal({ k: 1, tx: 0, ty: 0 });
  let baseScrollY = 0;
  let zoomTargetK = 1;
  let zoomAnchor = { x: 0, y: 0 };
  let zoomRaf: number | undefined;
  let zoomLastTime = 0;
  let pinchCommitTimer: number | undefined;
  const liveTransform = computed(() => {
    const l = live();
    return l.k === 1 && l.tx === 0 && l.ty === 0 ? "none" : `translate(${l.tx}px, ${l.ty}px) scale(${l.k})`;
  });

  // Rasterize sharp enough for the current zoom on this screen, in half-pixel
  // steps so zooming does not re-rasterize on every wheel tick.
  const scale = computed(() => scaleFor(zoom(), pixelRatio()));

  // The column's left edge: centered when it fits, else where the user panned it.
  const left = computed(() => leftFor(pages()[0]?.width ?? 0, zoom(), pane().width, panX()));

  // The part of each page in view, in its points, and the pages on either side
  // (whole-page layers only).
  const view = computed<PreviewView[]>(() => computeView(pages(), zoom(), scrollY(), panX(), pane()));
  // The tiles a part of a page in view needs (`margin` pixels around it: the native
  // side rasterizes half a tile around what it is asked for), as `[x0, y0, x1, y1]`.
  function tiles(v: PreviewView, s: number, margin: number) {
    const t = (pt: number, m: number) => Math.max(0, Math.floor((pt * s + m) / TILE));
    return [t(v.x, -margin), t(v.y, -margin), t(v.x + v.width, margin), t(v.y + v.height, margin)];
  }
  // A request is due when the scale or the pages in view change, or a tile in view
  // was not rasterized with the last one.
  let asked = { key: "", scale: 0, pages: "", tiles: new Map<number, number[]>() };
  function due() {
    const curKey = targetKey(pick(), book());
    if (!curKey || curKey !== shownKey) return false;
    const s = scale();
    const list = view();
    if (curKey !== asked.key || s !== asked.scale || list.map((v) => v.page).join() !== asked.pages) return true;
    if (s <= BASE_SCALE) return false;
    return list.some((v) => {
      if (v.width <= 0) return false;
      const [x0, y0, x1, y1] = tiles(v, s, 0);
      const [a0, b0, a1, b1] = asked.tiles.get(v.page) ?? [1, 1, 0, 0];
      return x0 < a0 || y0 < b0 || x1 > a1 || y1 > b1;
    });
  }

  // Jump to a problem's first page once the next result says where it is.
  let jumpTo: string | undefined;
  // Sequence protection + bounded concurrency (at most 2 in flight so a superseding
  // request immediately bumps the Rust generation counter and cancels stale tiles,
  // while rapid bursts still coalesce via `again`).
  let compileSeq = 0;
  let inFlight = 0;
  let again = false;

  function applyResult(reqKey: string, result: PreviewResult) {
    const isSwitch = reqKey !== shownKey;
    if (result.pages.length === 0) {
      batch(() => {
        error.set(result.error ?? undefined);
        diagnostics.set(result.diagnostics);
      });
      return;
    }

    lastStarts = result.starts;
    const start = jumpTo === undefined ? undefined : result.starts.find((x) => x.label === jumpTo);
    jumpTo = undefined;

    if (!isSwitch) {
      const oldMaxScroll = currentMaxScroll();
      batch(() => {
        error.set(result.error ?? undefined);
        diagnostics.set(result.diagnostics);
        pages.set(result.pages);
        if (needsFit) fit();
      });
      if (start) {
        const z = zoom();
        if (z > 0) {
          const targetY = clampScroll(result.pages, z, pageTopFor(result.pages, z, start.page));
          const newMaxScroll = Math.max(0, contentHeightFor(result.pages, z) - pane().height);
          scrollY.set(targetY);
          commitScroll(targetY, oldMaxScroll, newMaxScroll);
        } else {
          pendingJumpPage = start.page;
        }
      }
      return;
    }

    // First frame of a switched target/book: restore remembered viewport, or fit width
    // and scroll to top (or `jumpTo` problem's first page) in one atomic batch.
    const saved = viewports.get(reqKey);
    const curPane = pane();
    const oldMaxScroll = currentMaxScroll();
    let nextZoom: number;
    let nextPanX: number;
    let nextScrollY: number;
    let nextNeedsFit: boolean;
    pendingJumpPage = undefined;
    if (saved) {
      nextZoom = saved.zoom;
      nextPanX = saved.panX;
      nextNeedsFit = false;
      const rawY = start ? pageTopFor(result.pages, nextZoom, start.page) : saved.scrollY;
      nextScrollY = clampScroll(result.pages, nextZoom, rawY);
    } else {
      nextZoom = fitZoomFor(result.pages, curPane.width);
      nextNeedsFit = nextZoom === 0;
      nextPanX = 0;
      if (start && nextZoom === 0) {
        pendingJumpPage = start.page;
        nextScrollY = 0;
      } else {
        const rawY = start ? pageTopFor(result.pages, nextZoom, start.page) : 0;
        nextScrollY = clampScroll(result.pages, nextZoom, rawY);
      }
    }
    const newMaxScroll = Math.max(0, contentHeightFor(result.pages, nextZoom) - curPane.height);
    cancelLiveZoom();
    cancelPendingScroll();
    batch(() => {
      shownKey = reqKey;
      needsFit = nextNeedsFit;
      error.set(result.error ?? undefined);
      diagnostics.set(result.diagnostics);
      pages.set(result.pages);
      zoom.set(nextZoom);
      panX.set(nextPanX);
      scrollY.set(nextScrollY);
    });
    commitScroll(nextScrollY, oldMaxScroll, newMaxScroll);
  }

  function compile() {
    const req = request();
    if (!req) return;
    if (inFlight >= 2) {
      again = true;
      return;
    }
    const reqKey = requestKey(req);
    const isSwitch = reqKey !== shownKey;
    const seq = ++compileSeq;
    inFlight += 1;

    const curPane = pane();
    const dpr = pixelRatio();
    let s: number;
    let list: PreviewView[];
    if (isSwitch) {
      const saved = viewports.get(reqKey);
      if (saved) {
        s = scaleFor(saved.zoom, dpr);
        list = computeView(saved.pageSizes, saved.zoom, saved.scrollY, saved.panX, curPane);
      } else {
        const estZoom = fitZoomFor([DEFAULT_PAGE], curPane.width);
        s = scaleFor(estZoom, dpr);
        list =
          estZoom > 0 && curPane.height > 0
            ? computeView([DEFAULT_PAGE, DEFAULT_PAGE], estZoom, 0, 0, curPane)
            : [
                { page: 0, x: 0, y: 0, width: 0, height: 0 },
                { page: 1, x: 0, y: 0, width: 0, height: 0 },
              ];
      }
    } else {
      s = scale();
      list = view();
    }

    asked = {
      key: reqKey,
      scale: s,
      pages: list.map((v) => v.page).join(),
      tiles: new Map(list.filter((v) => v.width > 0).map((v) => [v.page, tiles(v, s, TILE / 2)])),
    };

    const t = Date.now();
    void preview.compile(req, s, list).then((result) => {
      inFlight -= 1;
      console.log(
        `[preview] compile ${result.compileMs.toFixed(0)}ms, render ${result.rendered} layers ${result.renderMs.toFixed(0)}ms, round trip ${Date.now() - t}ms`,
      );
      const stale = seq !== compileSeq || reqKey !== targetKey(pick(), book());
      if (!stale) {
        applyResult(reqKey, result);
      }
      if (again) {
        again = false;
        compile();
      }
    });
  }

  // The view needs other pixels: ask (the source is unchanged, so Rust reuses the
  // cached `Compiled` with zero recompile cost).
  effect(() => {
    if (due()) untracked(compile);
  });

  function scrollTo(y: number) {
    scroll?.scrollTo({ y, animated: false });
  }

  // Where page `index` starts in the scroll view.
  function pageTop(index: number) {
    return pageTopFor(pages(), zoom(), index);
  }

  async function exportPdf() {
    save();
    const req = request();
    if (!req) return;
    const out = await preview.export(req);
    saveError.set(out.error ?? undefined);
  }

  // Switch problem/contest using SWR: save the current viewport and keep old pages
  // visible until the new target's first frame arrives.
  function choose(next: Pick | undefined) {
    const cur = pick();
    if (next && cur && next.target === cur.target && next.id === cur.id) return;
    cancelLiveZoom();
    cancelPendingScroll();
    save();
    compileSeq += 1;
    again = false;
    jumpTo = undefined;
    pendingJumpPage = undefined;
    if (!next) {
      shownKey = undefined;
      needsFit = false;
      lastStarts = [];
      contentHeightLaidOut = 0;
      asked = { key: "", scale: 0, pages: "", tiles: new Map() };
      batch(() => {
        pick.set(undefined);
        label.set(undefined);
        pages.set([]);
        error.set(undefined);
        diagnostics.set([]);
        zoom.set(0);
        panX.set(0);
        scrollY.set(0);
      });
      scrollTo(0);
      return;
    }
    saveViewport();
    batch(() => {
      pick.set(next);
      label.set(undefined);
    });
  }

  // Switch statements <-> editorial using SWR and per-(target, id, book) viewport memory.
  function chooseBook(next: Book) {
    if (next === book()) return;
    cancelLiveZoom();
    cancelPendingScroll();
    saveViewport();
    save();
    compileSeq += 1;
    again = false;
    jumpTo = undefined;
    pendingJumpPage = undefined;
    book.set(next);
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

  // Apply a live compositor scale `nextK` keeping the content point under `at` (viewport
  // coordinates) pixel-locked.
  function applyLiveScale(nextK: number, at: { x: number; y: number }) {
    const z = zoom();
    if (z === 0) return;
    const cur = live();
    if (cur.k === 1 && cur.tx === 0 && cur.ty === 0) {
      baseScrollY = scrollY();
    }
    const clampedK = Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, z * nextK)) / z;
    const r = clampedK / cur.k;
    const tx = at.x - r * (at.x - cur.tx);
    const ty = at.y + baseScrollY - r * (at.y + baseScrollY - cur.ty);
    live.set({ k: clampedK, tx, ty });
  }

  // Commit the live transform into laid-out zoom, panX and scrollY in one atomic batch.
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
    const list = pages();
    const nextZoom = Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, z * cur.k));
    const nextPanX = cur.tx + cur.k * left();
    const oldMaxScroll = currentMaxScroll();
    const newMaxScroll = Math.max(0, contentHeightFor(list, nextZoom) - pane().height);
    const nextScrollY = Math.min(newMaxScroll, Math.max(0, baseScrollY - cur.ty));
    zoomTargetK = 1;
    batch(() => {
      live.set({ k: 1, tx: 0, ty: 0 });
      zoom.set(nextZoom);
      panX.set(nextPanX);
      scrollY.set(nextScrollY);
    });
    commitScroll(nextScrollY, oldMaxScroll, newMaxScroll);
  }

  function stepWheelZoom(now: number) {
    zoomRaf = undefined;
    const dt = zoomLastTime === 0 ? 16 : Math.min(48, Math.max(1, now - zoomLastTime));
    zoomLastTime = now;
    const cur = live();
    const diff = zoomTargetK - cur.k;
    if (Math.abs(diff) / cur.k < 0.0015) {
      applyLiveScale(zoomTargetK, zoomAnchor);
      commitZoom();
      return;
    }
    // Exponential damping (~120ms settle time, matching design-system.md §8).
    const alpha = 1 - Math.exp(-dt / 32);
    applyLiveScale(cur.k + diff * alpha, zoomAnchor);
    zoomRaf = requestAnimationFrame(stepWheelZoom);
  }

  function zoomWheel(factor: number, at: { x: number; y: number }) {
    const z = zoom();
    if (z === 0) return;
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
    cancelPendingScroll();
    if (zoomRaf !== undefined) {
      cancelAnimationFrame(zoomRaf);
      zoomRaf = undefined;
    }
    clearTimeout(pinchCommitTimer);
    pinchCommitTimer = undefined;
    const cur = live();
    if (phase === "start" && cur.k === 1 && cur.tx === 0 && cur.ty === 0) {
      baseScrollY = scrollY();
    }
    if (delta !== 0) {
      applyLiveScale(cur.k * (1 + delta), at);
    }
    if (phase === "end") {
      commitZoom();
    } else {
      // Fallback commit if the platform drops the final "end" event.
      pinchCommitTimer = setTimeout(commitZoom, 140);
    }
  }

  // Fit the page width, with a margin, once there are pages and a pane.
  function fit() {
    const list = pages();
    if (!needsFit || list.length === 0 || pane().width === 0) return;
    const nextZoom = fitZoomFor(list, pane().width);
    if (nextZoom === 0) return;
    needsFit = false;
    const oldMaxScroll = currentMaxScroll();
    const jumpPage = pendingJumpPage;
    pendingJumpPage = undefined;
    const nextScrollY =
      jumpPage !== undefined ? clampScroll(list, nextZoom, pageTopFor(list, nextZoom, jumpPage)) : scrollY();
    batch(() => {
      zoom.set(nextZoom);
      if (jumpPage !== undefined) scrollY.set(nextScrollY);
    });
    if (jumpPage !== undefined) {
      const newMaxScroll = Math.max(0, contentHeightFor(list, nextZoom) - pane().height);
      commitScroll(nextScrollY, oldMaxScroll, newMaxScroll);
    }
  }

  let drag: { x: number; y: number } | undefined;

  const items = computed(() => [
    ...info().contests.map((c) => ({ value: `contest:${c.id}`, title: c.name, group: "比赛" })),
    ...info().problems.map((p) => ({ value: `problem:${p.id}`, title: p.title, group: "题目" })),
    { value: OPEN_WORKSPACE, title: "打开其他工作区…", group: "工作区" },
  ]);

  const pickError = signal<string | undefined>(undefined);
  async function pickWorkspace() {
    save();
    const next = await preview.pickWorkspace();
    pickError.set(next.root === "" ? next.error ?? undefined : undefined);
    if (next.root !== "" && next.root !== info().root) {
      viewports.clear();
      choose(undefined);
    }
    saveError.set(next.root !== "" ? next.error ?? undefined : undefined);
    info.set(next);
  }

  const empty = (
    <View style={{ flex: 1, alignItems: "center", justifyContent: "center", gap: 12 }} class="bg-background">
      <Button onPress={pickWorkspace} style={{ height: 32, paddingLeft: 14, paddingRight: 14, borderRadius: 8, justifyContent: "center" }} class="bg-primary">
        <Text class="text-primary-foreground" style={{ fontSize: 13 }}>打开工作区</Text>
      </Button>
      {pickError() && <Text class="text-destructive" style={{ fontSize: 12 }}>{pickError()}</Text>}
    </View>
  );

  const studio = (
    <View style={{ flex: 1 }} class="bg-background">
      <View
        style={{ height: 44, flexDirection: "row", alignItems: "center", gap: 8, paddingLeft: 12, paddingRight: 12, borderBottomWidth: 1 }}
        class="border-border"
      >
        <Select
          style={{ width: 280 }}
          items={items()}
          value={pick() ? `${pick()!.target}:${pick()!.id}` : null}
          placeholder="打开题目或比赛"
          searchable
          searchPlaceholder="搜索"
          onChange={(e) => (e.value === OPEN_WORKSPACE ? pickWorkspace() : choose(parsePick(e.value)))}
          trigger={
            <View
              style={{ height: 28, paddingLeft: 10, paddingRight: 10, borderRadius: 6, borderWidth: 1, flexDirection: "row", alignItems: "center" }}
              class="border-border bg-background hover:bg-muted"
            >
              <Text style={{ flex: 1, fontSize: 13 }} class={pick() ? "" : "text-muted-foreground"}>
                {items().find((i) => i.value === (pick() ? `${pick()!.target}:${pick()!.id}` : ""))?.title ?? "打开题目或比赛"}
              </Text>
              <Text class="text-muted-foreground" style={{ fontSize: 11 }}>▾</Text>
            </View>
          }
          popupClass="bg-background border border-border rounded-[10px] p-1 shadow-md"
          listClass="max-h-[420px]"
          itemClass="h-8 px-2 rounded-[6px] justify-center text-[13px] focus:bg-muted selected:bg-accent"
          groupClass="px-2 pt-2 pb-1 text-[11.5px] text-muted-foreground"
          searchClass="h-8 px-2 mb-1 text-[13px] border-b border-border"
        />
        {contest() && (
          <ToggleGroup style={{ flexDirection: "row", gap: 2 }}>
            <Each of={contest()!.items} key={(item) => item.label}>
              {(item) => (
                <Toggle
                  pressed={(label() ?? contest()!.items[0]?.label) === item().label}
                  onChange={() => {
                    const nextLabel = item().label;
                    const curLabel = label() ?? contest()!.items[0]?.label;
                    if (nextLabel === curLabel && shownKey === targetKey(pick(), book())) {
                      const start = lastStarts.find((x) => x.label === nextLabel);
                      if (start && zoom() > 0) {
                        cancelLiveZoom();
                        cancelPendingScroll();
                        const y = clampScroll(pages(), zoom(), pageTop(start.page));
                        scrollY.set(y);
                        scrollTo(y);
                      }
                      return;
                    }
                    jumpTo = nextLabel;
                    label.set(nextLabel);
                  }}
                  style={{ minWidth: 28, height: 28, borderRadius: 6, alignItems: "center", justifyContent: "center" }}
                  class="aria-pressed:bg-accent"
                >
                  <Text style={{ fontSize: 13, fontVariant: ["tabular-nums"] }}>{item().label}</Text>
                </Toggle>
              )}
            </Each>
          </ToggleGroup>
        )}
        <View style={{ flex: 1 }} />
        {pick() && (
          <ToggleGroup style={{ flexDirection: "row", gap: 2 }}>
            <Toggle
              pressed={book() === "statements"}
              onChange={() => chooseBook("statements")}
              style={{ height: 28, paddingLeft: 10, paddingRight: 10, borderRadius: 6, justifyContent: "center" }}
              class="aria-pressed:bg-accent"
            >
              <Text style={{ fontSize: 13 }}>题面</Text>
            </Toggle>
            <Toggle
              pressed={book() === "editorial"}
              onChange={() => chooseBook("editorial")}
              style={{ height: 28, paddingLeft: 10, paddingRight: 10, borderRadius: 6, justifyContent: "center" }}
              class="aria-pressed:bg-accent"
            >
              <Text style={{ fontSize: 13 }}>题解</Text>
            </Toggle>
          </ToggleGroup>
        )}
        {pick() && (
          <Button onPress={exportPdf} style={{ height: 28, paddingLeft: 10, paddingRight: 10, borderRadius: 6, justifyContent: "center" }} class="bg-primary">
            <Text class="text-primary-foreground" style={{ fontSize: 13 }}>PDF</Text>
          </Button>
        )}
      </View>
      <View style={{ flex: 1, flexDirection: "row" }}>
      <View style={{ flex: 1, borderRightWidth: 1 }} class="border-border">
        <Each of={file() ? [file()!] : []} key={(f) => f.path}>
          {(f) => (
            <Editor
              style={{ flex: 1 }}
              defaultValue={f().text}
              language="markdown"
              softWrap
              readOnly={f().readOnly}
              diagnostics={editorDiagnostics()}
              onChange={(e) => {
                text = e.value;
                dirty = true;
                clearTimeout(saveTimer);
                saveTimer = setTimeout(save, SAVE_AFTER);
                compile();
              }}
            />
          )}
        </Each>
      </View>
      <View
        ref={(r) => (paneView = r)}
        style={{ flex: 1 }}
        class="bg-muted"
        onLayout={(e) => {
          pane.set({ width: e.bounds.size.width, height: e.bounds.size.height });
          paneView?.measure().then((m) => {
            if (m) paneOrigin = m.pagePosition;
          });
          fit();
        }}
      >
        <Scroll
          ref={(r) => (scroll = r)}
          style={{ flex: 1 }}
          onScrollDragStart={() => {
            cancelPendingScroll();
          }}
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
              if (e.contentSize.height < pendingScroll.minContentHeight - 1) {
                return;
              }
              cancelPendingScroll();
            }
            scrollY.set(e.offset.y);
          }}
        >
          <View
            // Takes Ctrl + wheel (zoom) and sideways scrolling; the scroll view keeps
            // the rest, with its momentum.
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
              if (e.modifiers.control) {
                const steps = e.deltaMode === "lines" ? e.delta.y : e.delta.y / 40;
                zoomWheel(Math.pow(1.12, -steps), viewportAt(e.pagePosition));
              } else {
                if (live().k !== 1) commitZoom();
                if (e.delta.y !== 0) cancelPendingScroll();
                const k = e.deltaMode === "lines" ? 40 : 1;
                panX.set(left() - e.delta.x * k);
              }
            }}
            onPinch={(e) => zoomPinch(e.delta, e.phase, viewportAt(e.pagePosition))}
            onMouseMove={(e) => {
              if (e.pressedButton !== "left") {
                drag = undefined;
                return;
              }
              cancelPendingScroll();
              if (live().k !== 1) commitZoom();
              if (drag) {
                cancelPendingScroll();
                const nextY = clampScroll(pages(), zoom(), scrollY() - (e.pagePosition.y - drag.y));
                panX.set(left() + e.pagePosition.x - drag.x);
                scrollY.set(nextY);
                scrollTo(nextY);
              }
              drag = e.pagePosition;
            }}
            style={{
              paddingTop: GAP * zoom(),
              paddingBottom: GAP * zoom(),
              gap: GAP * zoom(),
              opacity: zoom() === 0 ? 0 : 1,
              transform: liveTransform(),
              transformOrigin: "0 0",
            }}
          >
            <Each of={pages()} key="index">
              {(page) => (
                <View
                  style={{
                    width: page().width * zoom(),
                    height: page().height * zoom(),
                    transform: `translateX(${left()}px)`,
                    backgroundColor: "#ffffff",
                    borderWidth: 1,
                    borderColor: "rgba(0,0,0,0.08)",
                    boxShadow: "0 1px 3px rgba(0,0,0,0.06), 0 4px 12px rgba(0,0,0,0.04)",
                  }}
                >
                  <PageImage layers={page().layers} style={{ width: "100%", height: "100%" }} />
                </View>
              )}
            </Each>
          </View>
        </Scroll>
        {(error() || saveError()) && (
          <View style={{ position: "absolute", left: 12, right: 12, bottom: 12, padding: 8, borderRadius: 8 }} class="bg-background">
            <Text class="text-destructive" style={{ fontSize: 12 }}>{saveError() ?? error()}</Text>
          </View>
        )}
      </View>
      </View>
    </View>
  );

  return <View style={{ flex: 1 }}>{info().root === "" ? empty : studio}</View>;
}

mount(App);

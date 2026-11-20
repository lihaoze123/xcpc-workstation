import process from "process";
import { computed, effect, mount, pixelRatio, Scroll, type ScrollRef, signal, Text, untracked, View, type ViewRef } from "nsh";
import { Button, Editor, type EditorDiagnostic, Select, Toggle, ToggleGroup } from "nsh/base";
import {
  PageImage,
  Preview,
  type PreviewDiagnostic,
  type PreviewPage,
  type PreviewRequest,
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
const SAVE_AFTER = 500; // ms after the last edit
const OPEN_WORKSPACE = "workspace:open"; // the bar's item that opens another workspace

function parsePick(value: string | null | undefined): Pick | undefined {
  const [target, id] = (value ?? "").split(":");
  return (target === "problem" || target === "contest") && id ? { target, id } : undefined;
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
    saveError.set(preview.save(f.path, text) ?? undefined);
  }

  // A new file: save the old one first, then read it (the editor is made anew for it,
  // so undo never reaches into another file).
  effect(() => {
    const next = path();
    untracked(() => {
      save();
      if (!next) {
        file.set(undefined);
        return;
      }
      const f = preview.read(next);
      text = f.text;
      file.set(f);
      compile();
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
  const scale = computed(() => Math.min(Math.ceil(Math.max(zoom(), 0.5) * pixelRatio() * 2) / 2, 8));

  // The column's left edge: centered when it fits, else where the user panned it.
  const width = computed(() => (pages()[0]?.width ?? 0) * zoom());
  const left = computed(() => {
    const room = pane().width - width();
    if (room >= 2 * GAP) return room / 2;
    return Math.min(GAP, Math.max(room - GAP, panX()));
  });

  // The part of each page in view, in its points, and the pages on either side
  // (whole-page layers only).
  const view = computed<PreviewView[]>(() => {
    const list = pages();
    const z = zoom();
    const edge = (page: number) => ({ page, x: 0, y: 0, width: 0, height: 0 });
    if (list.length === 0 || z === 0) return [edge(0), edge(1)];
    const top = scrollY();
    const bottom = top + pane().height;
    const x0 = Math.max(0, -left() / z);
    const out: PreviewView[] = [];
    let y = GAP * z;
    list.forEach((page, i) => {
      const h = page.height * z;
      if (y + h >= top && y <= bottom) {
        const y0 = Math.max(0, (top - y) / z);
        const y1 = Math.min(page.height, (bottom - y) / z);
        const x1 = Math.min(page.width, (pane().width - left()) / z);
        out.push({ page: i, x: x0, y: y0, width: Math.max(0, x1 - x0), height: Math.max(0, y1 - y0) });
      }
      y += h + GAP * z;
    });
    if (out.length === 0) return out;
    if (out[0].page > 0) out.unshift(edge(out[0].page - 1));
    if (out[out.length - 1].page < list.length - 1) out.push(edge(out[out.length - 1].page + 1));
    return out;
  });
  // The tiles a part of a page in view needs (`margin` pixels around it: the native
  // side rasterizes half a tile around what it is asked for), as `[x0, y0, x1, y1]`.
  function tiles(v: PreviewView, s: number, margin: number) {
    const t = (pt: number, m: number) => Math.max(0, Math.floor((pt * s + m) / TILE));
    return [t(v.x, -margin), t(v.y, -margin), t(v.x + v.width, margin), t(v.y + v.height, margin)];
  }
  // A request is due when the scale or the pages in view change, or a tile in view
  // was not rasterized with the last one.
  let asked = { scale: 0, pages: "", tiles: new Map<number, number[]>() };
  function due() {
    const s = scale();
    const list = view();
    if (s !== asked.scale || list.map((v) => v.page).join() !== asked.pages) return true;
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
  // One request at a time: what changes meanwhile (an edit, the view) makes one more
  // when it is back, with the source and view as they are then. Nothing waits for
  // typing to pause: a request takes about as long as a frame or two.
  let running = false;
  let again = false;
  async function compile() {
    if (running) {
      again = true;
      return;
    }
    const req = request();
    if (!req) return;
    running = true;
    const t = Date.now();
    const s = scale();
    const list = view();
    asked = {
      scale: s,
      pages: list.map((v) => v.page).join(),
      tiles: new Map(list.filter((v) => v.width > 0).map((v) => [v.page, tiles(v, s, TILE / 2)])),
    };
    const result = await preview.compile(req, s, list);
    running = false;
    console.log(`[preview] compile ${result.compileMs.toFixed(0)}ms, render ${result.rendered} layers ${result.renderMs.toFixed(0)}ms, round trip ${Date.now() - t}ms`);
    error.set(result.error ?? undefined);
    diagnostics.set(result.diagnostics);
    // A failed compilation keeps the last pages.
    if (result.pages.length > 0) {
      pages.set(result.pages);
      fit();
      const start = jumpTo === undefined ? undefined : result.starts.find((x) => x.label === jumpTo);
      jumpTo = undefined;
      if (start) scrollTo(pageTop(start.page));
    }
    if (again) {
      again = false;
      compile();
    }
  }
  compile();
  // The view needs other pixels: ask (the source is unchanged, so compiling again
  // costs next to nothing).
  effect(() => {
    if (due()) untracked(compile);
  });

  function scrollTo(y: number) {
    scroll?.scrollTo({ y, animated: false });
  }

  // Where page `index` starts in the scroll view.
  function pageTop(index: number) {
    const z = zoom();
    return pages()
      .slice(0, index)
      .reduce((y, p) => y + (p.height + GAP) * z, 0);
  }

  async function exportPdf() {
    save();
    const req = request();
    if (!req) return;
    const out = await preview.export(req);
    saveError.set(out.error ?? undefined);
  }

  // Another problem or contest: new pages, fitted again.
  function choose(next: Pick | undefined) {
    cancelLiveZoom();
    save();
    pick.set(next);
    label.set(undefined);
    pages.set([]);
    zoom.set(0);
    scrollY.set(0);
    scrollTo(0);
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
    const nextZoom = Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, z * cur.k));
    const nextPanX = cur.tx + cur.k * left();
    const contentHeight =
      pages().reduce((h, p) => h + (p.height + GAP) * nextZoom, GAP * nextZoom);
    const maxScroll = Math.max(0, contentHeight - pane().height);
    const nextScrollY = Math.min(maxScroll, Math.max(0, baseScrollY - cur.ty));
    zoomTargetK = 1;
    live.set({ k: 1, tx: 0, ty: 0 });
    zoom.set(nextZoom);
    panX.set(nextPanX);
    scrollY.set(nextScrollY);
    scrollTo(nextScrollY);
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
    if (zoom() !== 0 || list.length === 0 || pane().width === 0) return;
    zoom.set((pane().width - 2 * GAP) / list[0].width);
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
    if (next.root !== "" && next.root !== info().root) choose(undefined);
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
                    label.set(item().label);
                    jumpTo = item().label;
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
              onChange={() => book.set("statements")}
              style={{ height: 28, paddingLeft: 10, paddingRight: 10, borderRadius: 6, justifyContent: "center" }}
              class="aria-pressed:bg-accent"
            >
              <Text style={{ fontSize: 13 }}>题面</Text>
            </Toggle>
            <Toggle
              pressed={book() === "editorial"}
              onChange={() => book.set("editorial")}
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
          onScroll={(e) => {
            if (live().k !== 1) commitZoom();
            scrollY.set(e.offset.y);
          }}
        >
          <View
            // Takes Ctrl + wheel (zoom) and sideways scrolling; the scroll view keeps
            // the rest, with its momentum.
            panActivation={{ activeOffsetX: 10 }}
            onWheel={(e) => {
              if (e.modifiers.control) {
                const steps = e.deltaMode === "lines" ? e.delta.y : e.delta.y / 40;
                zoomWheel(Math.pow(1.12, -steps), viewportAt(e.pagePosition));
              } else {
                if (live().k !== 1) commitZoom();
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
              if (live().k !== 1) commitZoom();
              if (drag) {
                panX.set(left() + e.pagePosition.x - drag.x);
                scrollTo(scrollY() - (e.pagePosition.y - drag.y));
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

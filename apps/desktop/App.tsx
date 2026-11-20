import { computed, effect, mount, pixelRatio, Scroll, type ScrollRef, signal, Text, View } from "nsh";
import { Editor } from "nsh/base";
import { PageImage, Preview, type PreviewPage, type PreviewView } from "xcpc-workstation/native";

// Phase 0: a Typst source on the left, its pages on the right.
//
// The pages scroll in a native scroll view (momentum, moved on the UI thread) and are
// laid out at the zoom's size; a zoom stretches what is there until sharper layers
// arrive. Each page keeps its view across results (`key="index"`), and `PageImage`
// keeps the old layers until the new ones have loaded, so nothing blinks. Only what is
// in view is rasterized at the zoom's scale.

const preview = new Preview();
const GAP = 16; // points between pages, at zoom 1
const MIN_ZOOM = 0.1;
const MAX_ZOOM = 8;
const TILE = 512; // pixels, as the native side tiles
const WHOLE_SCALE = 3; // above this the native side tiles the part in view

function App() {
  const pages = signal<PreviewPage[]>([]);
  const error = signal<string | undefined>(undefined);
  const zoom = signal(0);           // 0 until the first fit
  const pane = signal({ width: 0, height: 0 });
  const scrollY = signal(0);
  const panX = signal(0);           // where the pages' left edge is, when wider than the pane
  let scroll: ScrollRef | undefined;

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
    if (s <= WHOLE_SCALE) return false;
    return list.some((v) => {
      if (v.width <= 0) return false;
      const [x0, y0, x1, y1] = tiles(v, s, 0);
      const [a0, b0, a1, b1] = asked.tiles.get(v.page) ?? [1, 1, 0, 0];
      return x0 < a0 || y0 < b0 || x1 > a1 || y1 > b1;
    });
  }

  let source = Preview.sample();
  let latest = 0;
  let timer: number | undefined;
  async function compile() {
    const id = ++latest;
    const t = Date.now();
    const s = scale();
    const list = view();
    asked = {
      scale: s,
      pages: list.map((v) => v.page).join(),
      tiles: new Map(list.filter((v) => v.width > 0).map((v) => [v.page, tiles(v, s, TILE / 2)])),
    };
    const result = await preview.compile(source, s, list);
    if (id !== latest) return;      // a newer request is on its way
    console.log(`[preview] compile ${result.compileMs.toFixed(0)}ms, render ${result.rendered} layers ${result.renderMs.toFixed(0)}ms, round trip ${Date.now() - t}ms`);
    error.set(result.error ?? undefined);
    if (!result.error) {
      pages.set(result.pages);
      fit();
    }
  }
  // An edit waits for typing to pause (`debounce`); the view asks at most once a
  // frame while it keeps moving, so sharper pixels follow a long scroll or zoom.
  function schedule(delay: number, debounce = true) {
    if (timer !== undefined) {
      if (!debounce) return;
      clearTimeout(timer);
    }
    timer = setTimeout(() => {
      timer = undefined;
      compile();
    }, delay);
  }
  compile();
  // The view needs other pixels: ask (the source is unchanged, so compiling again
  // costs next to nothing).
  effect(() => {
    if (due()) schedule(16, false);
  });

  function scrollTo(y: number) {
    scroll?.scrollTo({ y, animated: false });
  }

  // Zooms to `next` keeping the content point under `at` (content coordinates) still.
  function setZoom(next: number, at: { x: number; y: number }) {
    const z = zoom();
    if (z === 0) return;
    next = Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, next));
    const viewportY = at.y - scrollY();
    const docX = (at.x - left()) / z;
    zoom.set(next);
    panX.set(at.x - docX * next);
    scrollTo((at.y * next) / z - viewportY);
  }

  // Fit the page width, with a margin, once there are pages and a pane.
  function fit() {
    const list = pages();
    if (zoom() !== 0 || list.length === 0 || pane().width === 0) return;
    zoom.set((pane().width - 2 * GAP) / list[0].width);
  }

  let drag: { x: number; y: number } | undefined;

  return (
    <View style={{ flex: 1, flexDirection: "row" }} class="bg-background">
      <View style={{ flex: 1 }}>
        <Editor
          style={{ flex: 1 }}
          defaultValue={source}
          language="typst"
          softWrap
          onChange={(e) => {
            source = e.value;
            schedule(120);
          }}
        />
      </View>
      <View
        style={{ flex: 1 }}
        class="bg-muted"
        onLayout={(e) => {
          pane.set({ width: e.bounds.size.width, height: e.bounds.size.height });
          fit();
        }}
      >
        <Scroll
          ref={(r) => (scroll = r)}
          style={{ flex: 1 }}
          onScroll={(e) => scrollY.set(e.offset.y)}
        >
          <View
            // Takes Ctrl + wheel (zoom) and sideways scrolling; the scroll view keeps
            // the rest, with its momentum.
            panActivation={{ activeOffsetX: 10 }}
            onWheel={(e) => {
              if (e.modifiers.control) {
                const steps = e.deltaMode === "lines" ? e.delta.y : e.delta.y / 40;
                setZoom(zoom() * Math.pow(1.1, -steps), e.position);
              } else {
                const k = e.deltaMode === "lines" ? 40 : 1;
                panX.set(left() - e.delta.x * k);
              }
            }}
            onPinch={(e) => setZoom(zoom() * (1 + e.delta), e.position)}
            onMouseMove={(e) => {
              if (e.pressedButton !== "left") {
                drag = undefined;
                return;
              }
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
                  }}
                >
                  <PageImage layers={page().layers} style={{ width: "100%", height: "100%" }} />
                </View>
              )}
            </Each>
          </View>
        </Scroll>
        {error() && (
          <View style={{ position: "absolute", left: 12, right: 12, bottom: 12, padding: 8, borderRadius: 8 }} class="bg-background">
            <Text class="text-destructive" style={{ fontSize: 12 }}>{error()}</Text>
          </View>
        )}
      </View>
    </View>
  );
}

mount(App);

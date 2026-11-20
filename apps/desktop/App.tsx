import { computed, effect, mount, pixelRatio, signal, Text, View } from "nsh";
import { Editor } from "nsh/base";
import { PageImage, Preview, type PreviewPage } from "xcpc-workstation/native";

// Phase 0: a Typst source on the left, its pages on the right. The pages are pixel
// buffers shown as images in one view that pans and zooms by its transform, so moving
// them is compositing; a zoom stretches what is there until sharper pages arrive.
// Only the pages in view are rasterized at the zoom's scale.

const preview = new Preview();
const GAP = 16; // points between pages, at zoom 1
const MIN_ZOOM = 0.1;
const MAX_ZOOM = 8;

function App() {
  const pages = signal<PreviewPage[]>([]);
  const error = signal<string | undefined>(undefined);
  const zoom = signal(0);           // 0 until the first fit
  const pan = signal({ x: 0, y: 0 });

  const pane = signal({ width: 0, height: 0 });

  // Rasterize sharp enough for the current zoom on this screen, in half-pixel
  // steps so zooming does not re-rasterize on every wheel tick.
  const scale = computed(() => Math.min(Math.ceil(Math.max(zoom(), 0.5) * pixelRatio() * 2) / 2, 8));

  // The pages in view and one on each side: only these are rasterized at `scale`.
  const view = computed(() => {
    const list = pages();
    const z = zoom();
    if (list.length === 0 || z === 0) return { first: 0, last: 1 };
    const top = -pan().y / z;
    const bottom = top + pane().height / z;
    let first = -1;
    let last = -1;
    let y = 0;
    list.forEach((page, i) => {
      if (y + page.height >= top && y <= bottom) {
        if (first < 0) first = i;
        last = i;
      }
      y += page.height + GAP;
    });
    if (first < 0) return { first: 0, last: -1 };
    return { first: Math.max(0, first - 1), last: Math.min(list.length - 1, last + 1) };
  }, { equals: (a, b) => a.first === b.first && a.last === b.last });

  let source = Preview.sample();
  let latest = 0;
  let timer: number | undefined;
  // What the last request asked for; the view asks again when it wants more.
  let asked = { scale: 0, first: 0, last: -1 };
  async function compile() {
    const id = ++latest;
    const t = Date.now();
    const { first, last } = view();
    asked = { scale: scale(), first, last };
    const result = await preview.compile(source, asked.scale, Math.max(first, 0), Math.max(last, 0));
    if (id !== latest) return;      // a newer request is on its way
    console.log(`[preview] compile ${result.compileMs.toFixed(0)}ms, render ${result.rendered} pages ${result.renderMs.toFixed(0)}ms, round trip ${Date.now() - t}ms`);
    error.set(result.error ?? undefined);
    if (!result.error) {
      pages.set(result.pages);
      fit();
    }
  }
  function schedule(delay: number) {
    clearTimeout(timer);
    timer = setTimeout(compile, delay);
  }
  compile();

  // Other pages in view, or a new scale: rasterize them (the source is unchanged, so
  // compiling again costs next to nothing).
  effect(() => {
    const { first, last } = view();
    const s = scale();
    if (s !== asked.scale || first < asked.first || last > asked.last) schedule(30);
  });

  function setZoom(next: number, at: { x: number; y: number }) {
    const z = zoom();
    next = Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, next));
    const p = pan();
    pan.set({ x: at.x - (at.x - p.x) * (next / z), y: at.y - (at.y - p.y) * (next / z) });
    zoom.set(next);
  }

  let drag: { x: number; y: number } | undefined;
  // Fit the page width, with a margin, once there are pages and a pane.
  function fit() {
    const list = pages();
    if (zoom() !== 0 || list.length === 0 || pane().width === 0) return;
    zoom.set((pane().width - 2 * GAP) / list[0].width);
    pan.set({ x: GAP, y: GAP });
  }

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
        style={{ flex: 1, overflow: "hidden" }}
        class="bg-muted"
        onLayout={(e) => {
          pane.set({ width: e.bounds.size.width, height: e.bounds.size.height });
          fit();
        }}
        onWheel={(e) => {
          if (e.modifiers.control) {
            const steps = e.deltaMode === "lines" ? e.delta.y : e.delta.y / 40;
            setZoom(zoom() * Math.pow(1.1, -steps), e.position);
          } else {
            const k = e.deltaMode === "lines" ? 40 : 1;
            const p = pan();
            pan.set({ x: p.x - e.delta.x * k, y: p.y - e.delta.y * k });
          }
        }}
        onPinch={(e) => setZoom(zoom() * (1 + e.delta), e.position)}
        onMouseMove={(e) => {
          if (e.pressedButton !== "left") {
            drag = undefined;
            return;
          }
          if (drag) {
            const p = pan();
            pan.set({ x: p.x + e.position.x - drag.x, y: p.y + e.position.y - drag.y });
          }
          drag = e.position;
        }}
      >
        <View
          style={{
            position: "absolute",
            left: 0,
            top: 0,
            gap: GAP,
            transformOrigin: "top left",
            transform: `translate(${pan().x}px, ${pan().y}px) scale(${zoom() || 1})`,
            opacity: zoom() === 0 ? 0 : 1,
          }}
        >
          {pages().map((page) => (
            <View style={{ width: page.width, height: page.height, backgroundColor: "#ffffff" }}>
              <PageImage page={page.key} style={{ width: page.width, height: page.height }} />
            </View>
          ))}
        </View>
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

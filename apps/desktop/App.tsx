import { Canvas, computed, mount, pixelRatio, signal, Text, View } from "nsh";
import { Editor } from "nsh/base";
import { Preview, type PreviewPage } from "xcpc-workstation/native";

// Phase 0: a Typst source on the left, its pages on a canvas on the right.

const preview = new Preview();
const GAP = 16; // points between pages, in canvas space
const MIN_ZOOM = 0.1;
const MAX_ZOOM = 8;

function App() {
  const pages = signal<PreviewPage[]>([]);
  const error = signal<string | undefined>(undefined);
  const zoom = signal(0);           // 0 until the first fit
  const pan = signal({ x: 0, y: 0 });

  // Rasterize sharp enough for the current zoom on this screen, in half-pixel
  // steps so zooming does not re-rasterize on every wheel tick.
  const scale = computed(() => Math.min(Math.ceil(Math.max(zoom(), 0.5) * pixelRatio() * 2) / 2, 8));

  let source = Preview.sample();
  let latest = 0;
  let timer: number | undefined;
  async function compile() {
    const id = ++latest;
    const t = Date.now();
    const result = await preview.compile(source, scale());
    if (id !== latest) return;      // a newer edit is on its way
    console.log(`[preview] compile ${result.compileMs.toFixed(0)}ms, render ${result.rendered} pages ${result.renderMs.toFixed(0)}ms, round trip ${Date.now() - t}ms`);
    error.set(result.error ?? undefined);
    if (!result.error) pages.set(result.pages);
  }
  function schedule() {
    clearTimeout(timer);
    timer = setTimeout(compile, 120);
  }
  compile();

  let lastScale = scale();
  function setZoom(next: number, at: { x: number; y: number }) {
    const z = zoom();
    next = Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, next));
    const p = pan();
    pan.set({ x: at.x - (at.x - p.x) * (next / z), y: at.y - (at.y - p.y) * (next / z) });
    zoom.set(next);
    if (scale() !== lastScale) {
      lastScale = scale();
      schedule();
    }
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
            schedule();
          }}
        />
      </View>
      <View
        style={{ flex: 1 }}
        class="bg-muted"
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
        <Canvas
          style={{ flex: 1 }}
          draw={(cx, s) => {
            const list = pages();
            if (list.length === 0) return;
            if (zoom() === 0) {
              // Fit the page width, with a margin.
              const z = (s.width - 2 * GAP) / list[0].width;
              zoom.set(z);
              pan.set({ x: GAP, y: GAP });
              return;
            }
            const z = zoom();
            const p = pan();
            let y = 0;
            for (const page of list) {
              const w = page.width * z;
              const h = page.height * z;
              const top = p.y + y * z;
              if (top < s.height && top + h > 0) {
                cx.fillStyle = "#ffffff";
                cx.fillRect(p.x, top, w, h);
                cx.drawImage(page.url, p.x, top, w, h);
              }
              y += page.height + GAP;
            }
          }}
        />
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

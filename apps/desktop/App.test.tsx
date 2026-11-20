// `nsh test` runs this file in a headless runner it builds with the crate.
import { expect, test } from "nsh/test";
import { Preview, type PreviewView } from "xcpc-workstation/native";

// The pixels are shared by every Preview, so each test uses scales of its own.

/** Pages `pages` in view, whole (`parts`) or with no part (whole-page layers only). */
function inView(pages: number[], part?: Omit<PreviewView, "page">): PreviewView[] {
  return pages.map((page) => ({ page, x: 0, y: 0, width: 0, height: 0, ...part }));
}

test("compiles the sample to pages", async () => {
  const preview = new Preview();
  const result = await preview.compile(Preview.sample(), 1, inView([0, 1, 2, 3, 4]));
  expect(result.error ?? undefined).toBe(undefined);
  expect(result.pages.length).toBe(5);
  expect(result.pages[0].layers.length).toBe(1);
});

test("reports a Typst error with its place", async () => {
  const preview = new Preview();
  const result = await preview.compile("#let x = (", 1, inView([0]));
  expect(result.pages.length).toBe(0);
  expect(result.error).toMatch(/^1:\d+: /);
});

test("rasterizes only the pages an edit changed", async () => {
  const preview = new Preview();
  const sample = Preview.sample();
  const all = inView([0, 1, 2, 3, 4]);
  const first = await preview.compile(sample, 1.25, all);
  expect(first.rendered).toBe(5);
  const again = await preview.compile(sample, 1.25, all);
  expect(again.rendered).toBe(0);
  const edited = await preview.compile(sample.replace("请使用 64 位整数", "请使用 64 位有符号整数"), 1.25, all);
  expect(edited.rendered).toBe(1);
  expect(edited.pages[0].layers[0].key).toBe(first.pages[0].layers[0].key);
});

test("rasterizes the pages in view, and keeps the others' pixels at another scale", async () => {
  const preview = new Preview();
  const sample = Preview.sample();
  const first = await preview.compile(sample, 1.5, inView([0, 1]));
  expect(first.rendered).toBe(2);
  // Out of view: no new pixels (an earlier test's, at another scale, are fine).
  expect(first.pages[2].layers.every((l) => !l.key.endsWith("@1.5"))).toBe(true);
  const sharper = await preview.compile(sample, 1.75, inView([1, 2]));
  expect(sharper.rendered).toBe(2);
  expect(sharper.pages[0].layers[0].key).toBe(first.pages[0].layers[0].key);
});

test("tiles the part in view when the scale is above the whole page's", async () => {
  const preview = new Preview();
  const sample = Preview.sample();
  // The top left 100 × 100 points of page 1 at 4.5 px/pt: 450 px and half a tile
  // around it, so the 512 px tiles (0, 0), (1, 0), (0, 1) and (1, 1).
  const result = await preview.compile(sample, 4.5, inView([0], { x: 0, y: 0, width: 100, height: 100 }));
  const layers = result.pages[0].layers;
  expect(layers.length).toBe(5);
  expect(layers[0].key.endsWith("@2")).toBe(true);
  expect(layers[0].width).toBe(1);
  expect(layers.slice(1).every((l) => l.key.includes("@4.5#"))).toBe(true);
  const tile = layers[4];
  const page = result.pages[0];
  expect(Math.abs(tile.left - 512 / 4.5 / page.width) < 1e-9).toBe(true);
  expect(Math.abs(tile.width - 512 / 4.5 / page.width) < 1e-9).toBe(true);
  // Scrolling within what is tiled rasterizes nothing new.
  const nudged = await preview.compile(sample, 4.5, inView([0], { x: 0, y: 5, width: 100, height: 100 }));
  expect(nudged.rendered).toBe(0);
});

// `nsh test` runs this file in a headless runner it builds with the crate.
import { expect, test } from "nsh/test";
import { Preview, type PreviewRequest, type PreviewView } from "xcpc-workstation/native";

// The pixels are shared by every Preview, so each test uses scales of its own. Every
// test opens the contest-zh fixture read-only.

const C = "problems/contest-zh-c/statement.md";

function open() {
  const preview = new Preview(null);
  preview.openWorkspace(Preview.fixture(), true);
  return preview;
}

const A = "problems/contest-zh-a/statement.md";

/** The contest, with the statement `path` (problem C's) as `text` when given (unsaved). */
function contest(text?: string, path = C): PreviewRequest {
  return { target: "contest", id: "contest-zh", book: "statements", path: text === undefined ? "" : path, text: text ?? "" };
}

/** Pages `pages` in view, whole (`parts`) or with no part (whole-page layers only). */
function inView(pages: number[], part?: Omit<PreviewView, "page">): PreviewView[] {
  return pages.map((page) => ({ page, x: 0, y: 0, width: 0, height: 0, ...part }));
}

test("lists the workspace", () => {
  const info = open().workspace();
  expect(info.readOnly).toBe(true);
  expect(info.problems.map((p) => p.id).join()).toBe("contest-zh-a,contest-zh-b,contest-zh-c");
  const c = info.contests[0];
  expect(c.items.map((i) => i.label).join()).toBe("A,B,C");
  expect(c.items[2].statement).toBe(C);
});

test("compiles a contest to pages", async () => {
  const result = await open().compile(contest(), 1, inView([0, 1, 2, 3, 4]));
  expect(result.error ?? undefined).toBe(undefined);
  expect(result.pages.length).toBe(5);
  expect(result.pages[0].layers.length).toBe(1);
  expect(result.starts.map((s) => `${s.label}${s.page}`).join()).toBe("A1,B2,C3");
});

test("reports an error in the Markdown with its place, and keeps no pages", async () => {
  const preview = open();
  const text = preview.read(C).text + "\n公式 $\\left( x$\n";
  const result = await preview.compile(contest(text), 1, inView([0]));
  expect(result.pages.length).toBe(0);
  expect(result.error).toMatch(/^problems\/contest-zh-c\/statement\.md:\d+: /);
  expect(result.diagnostics[0].file).toBe(C);
});

test("refuses to save in a read-only workspace", () => {
  const preview = open();
  expect(preview.read(C).readOnly).toBe(true);
  expect(preview.save(C, "x")).toMatch(/read-only/);
});

test("rasterizes only the pages an edit changed", async () => {
  const preview = open();
  const sample = preview.read(C).text;
  const all = inView([0, 1, 2, 3, 4]);
  const first = await preview.compile(contest(sample), 1.25, all);
  expect(first.rendered).toBe(5);
  const again = await preview.compile(contest(sample), 1.25, all);
  expect(again.rendered).toBe(0);
  const edited = await preview.compile(contest(sample.replace("请使用 64 位整数", "请使用 64 位有符号整数")), 1.25, all);
  expect(edited.rendered).toBe(1);
  expect(edited.pages[1].layers[0].key).toBe(first.pages[1].layers[0].key);
});

test("rasterizes the pages in view, and keeps the others' pixels at another scale", async () => {
  const preview = open();
  const sample = preview.read(C).text;
  const first = await preview.compile(contest(sample), 0.75, inView([0, 1]));
  expect(first.rendered).toBe(2);
  // Out of view: no new pixels (an earlier test's, at another scale, are fine).
  expect(first.pages[2].layers.every((l) => !l.key.endsWith("@0.75"))).toBe(true);
  const sharper = await preview.compile(contest(sample), 1.1, inView([1, 2]));
  expect(sharper.rendered).toBe(2);
  expect(sharper.pages[0].layers[0].key).toBe(first.pages[0].layers[0].key);
});

test("tiles the part in view when the scale is above the whole page's", async () => {
  const preview = open();
  const sample = preview.read(C).text;
  // The top left 100 × 100 points of page 1 at 4.5 px/pt: 450 px and half a tile
  // around it, so the 512 px tiles (0, 0), (1, 0), (0, 1) and (1, 1).
  const result = await preview.compile(contest(sample), 4.5, inView([0], { x: 0, y: 0, width: 100, height: 100 }));
  const layers = result.pages[0].layers;
  expect(layers.length).toBe(5);
  expect(layers[0].key.endsWith("@1.5")).toBe(true);
  expect(layers[0].width).toBe(1);
  expect(layers.slice(1).every((l) => l.key.includes("@4.5#"))).toBe(true);
  const tile = layers[4];
  const page = result.pages[0];
  expect(Math.abs(tile.left - 512 / 4.5 / page.width) < 1e-9).toBe(true);
  expect(Math.abs(tile.width - 512 / 4.5 / page.width) < 1e-9).toBe(true);
  // Scrolling within what is tiled rasterizes nothing new.
  const nudged = await preview.compile(contest(sample), 4.5, inView([0], { x: 0, y: 5, width: 100, height: 100 }));
  expect(nudged.rendered).toBe(0);
});

test("an edit rasterizes again only the tiles it touches", async () => {
  const preview = open();
  const sample = preview.read(A).text;
  // All of page 2 (problem A; page 1 is the cover) at 2.25 px/pt: its whole-page
  // layer at 1.5 and 3 × 4 tiles.
  const page = inView([1], { x: 0, y: 0, width: 1000, height: 1000 });
  const first = await preview.compile(contest(sample, A), 2.25, page);
  expect(first.pages[1].layers.length).toBe(13);
  const edited = await preview.compile(contest(sample.replace("这是一道签到题", "这是一道很简单的签到题"), A), 2.25, page);
  // The whole-page layer and the tiles of the changed lines.
  expect(edited.rendered > 1 && edited.rendered < 5).toBe(true);
  const kept = edited.pages[1].layers.filter((l) => first.pages[1].layers.some((f) => f.key === l.key));
  expect(kept.length).toBe(13 - edited.rendered);
});

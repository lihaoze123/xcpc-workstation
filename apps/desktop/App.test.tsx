// `nsh test` runs this file in a headless runner it builds with the crate.
import { expect, test } from "nsh/test";
import { Preview } from "xcpc-workstation/native";

test("compiles the sample to pages", async () => {
  const preview = new Preview();
  const result = await preview.compile(Preview.sample(), 1, 0, 4);
  expect(result.error ?? undefined).toBe(undefined);
  expect(result.pages.length).toBe(5);
});

test("reports a Typst error with its place", async () => {
  const preview = new Preview();
  const result = await preview.compile("#let x = (", 1, 0, 0);
  expect(result.pages.length).toBe(0);
  expect(result.error).toMatch(/^1:\d+: /);
});

test("rasterizes only the pages an edit changed", async () => {
  // A scale of its own: the pixels are shared with the other tests.
  const preview = new Preview();
  const sample = Preview.sample();
  const first = await preview.compile(sample, 1.25, 0, 4);
  expect(first.rendered).toBe(5);
  const again = await preview.compile(sample, 1.25, 0, 4);
  expect(again.rendered).toBe(0);
  const edited = await preview.compile(sample.replace("请使用 64 位整数", "请使用 64 位有符号整数"), 1.25, 0, 4);
  expect(edited.rendered).toBe(1);
  expect(edited.pages[0].key).toBe(first.pages[0].key);
});

test("rasterizes the pages in view, and keeps the others' pixels at another scale", async () => {
  // Scales of its own: the pixels are shared with the other tests.
  const preview = new Preview();
  const sample = Preview.sample();
  const first = await preview.compile(sample, 1.5, 0, 1);
  expect(first.rendered).toBe(2);
  // Out of view: no new pixels (an earlier test's, at another scale, are fine).
  expect(first.pages[2].key.endsWith("@1.5")).toBe(false);
  const sharper = await preview.compile(sample, 1.75, 1, 2);
  expect(sharper.rendered).toBe(2);
  expect(sharper.pages[0].key).toBe(first.pages[0].key);
  expect(sharper.pages[3].key.endsWith("@1.75")).toBe(false);
});

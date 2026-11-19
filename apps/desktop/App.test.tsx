// `nsh test` runs this file in a headless runner it builds with the crate.
import { expect, test } from "nsh/test";
import { Preview } from "xcpc-workstation/native";

test("compiles the sample to pages", async () => {
  const preview = new Preview();
  const result = await preview.compile(Preview.sample(), 1);
  expect(result.error ?? undefined).toBe(undefined);
  expect(result.pages.length).toBe(5);
});

test("reports a Typst error with its place", async () => {
  const preview = new Preview();
  const result = await preview.compile("#let x = (", 1);
  expect(result.pages.length).toBe(0);
  expect(result.error).toMatch(/^1:\d+: /);
});

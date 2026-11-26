import { describe, expect, open, test } from "nsh/test";

describe("TruncText (Phase 6F §1)", () => {
  async function tooltipShownFor(id: string): Promise<boolean> {
    const app = await open("./truncTextHarness.tsx", { width: 480, height: 320, platform: "linux" });
    await app.wait(50);
    const before = app.texts().length;
    await app.hover(`id:${id}`);
    await app.wait(800);
    return app.texts().length > before;
  }

  test("a cut-off single line shows its full text on hover", async () => {
    expect(await tooltipShownFor("cut")).toBe(true);
  });

  test("a single line that fits, stretched or hugging its content, shows no tooltip", async () => {
    expect(await tooltipShownFor("fits")).toBe(false);
    expect(await tooltipShownFor("hug")).toBe(false);
  });

  test("a cut-off multi-line text shows its full text on hover", async () => {
    expect(await tooltipShownFor("cut2")).toBe(true);
  });
});

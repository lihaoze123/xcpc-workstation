// `nsh test` runs this file in a headless runner it builds with the crate.
import { expect, test } from "nsh/test";
import { Preview, type PreviewRequest, type PreviewView } from "xcpc-workstation/native";
import { extractMathAtCaret, toggleHeadingLine, togglePrefixLine } from "./src/ui/studio/MarkdownEditor";
import { INITIAL_VIM_STATE, stepVim } from "./src/ui/studio/vim";
import {
  matchSlashTrigger,
  matchesSlashCommand,
  SLASH_ADVANCED_COMMANDS,
  SLASH_BASIC_COMMANDS,
  SLASH_COMMANDS,
  SLASH_COMMON_COMMANDS,
} from "./src/ui/studio/SlashPopover";
import {
  abbreviateHome,
  cleanSample,
  clampRatio,
  computeView,
  contentToPage,
  dropIndex,
  errorCopy,
  exportAction,
  fitZoomFor,
  GAP,
  labels,
  minimalEdit,
  moveItem,
  pageAt,
  pageTopFor,
  pathBase,
  ratingTier,
  ratioOf,
  revealState,
  validId,
  windowTitle,
} from "./src/ui/logic";
import {
  indicatorTransition,
  overlayTransition,
  pageTransition,
  panelTransition,
  pressTransition,
  SPRING_GENTLE,
  SPRING_INDICATOR,
  SPRING_OVERLAY,
  SPRING_PAGE,
  SPRING_PANEL,
  SPRING_PRESS,
} from "./src/ui/motion";
import { formatAppVersion } from "./src/ui/SettingsView";
import { filterShortcutGroups } from "./src/ui/ShortcutsModal";

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
  const file = preview.read(C);
  expect(file.readOnly).toBe(true);
  expect(file.hash.length).toBe(64);
  const res = preview.save(C, "x", file.hash);
  expect(res.ok).toBe(false);
  expect(res.conflict).toBe(false);
  expect(res.error ?? "").toMatch(/read-only/);
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

test("reuses compiled document when only view or scale changes", async () => {
  const preview = open();
  const sample = preview.read(C).text;
  const first = await preview.compile(contest(sample), 0.9, inView([0]));
  expect(first.compileMs > 0).toBe(true);
  expect(first.pages.length).toBe(5);
  expect(first.rendered).toBe(1);
  // Out-of-view pages have not lazily computed fingerprints yet.
  expect(first.pages[1].layers.length).toBe(0);

  // Only scale and view changed: skips publish::document_with + compile completely.
  const zoomed = await preview.compile(contest(sample), 1.3, inView([1]));
  expect(zoomed.compileMs).toBe(0);
  expect(zoomed.rendered).toBe(1);
  expect(zoomed.pages[0].layers[0].key).toBe(first.pages[0].layers[0].key);
  expect(zoomed.pages[2].layers.length).toBe(0);

  // Scrolling back over already-rasterized page 1 keeps compileMs === 0.
  const scrolled = await preview.compile(contest(sample), 1.3, inView([0, 1]));
  expect(scrolled.compileMs).toBe(0);
  expect(scrolled.rendered).toBe(1);

  // Editing text invalidates the compiled cache and recompiles.
  const edited = await preview.compile(contest(sample + "\n"), 1.3, inView([0, 1]));
  expect(edited.compileMs > 0).toBe(true);
});

test("supersedes older in-flight compile calls with the latest generation", async () => {
  const preview = open();
  const sample = preview.read(C).text;
  const p1 = preview.compile(contest(sample), 4.0, inView([0, 1, 2], { x: 0, y: 0, width: 600, height: 850 }));
  const p2 = preview.compile(contest(sample), 1.0, inView([0]));
  const [stale, latest] = await Promise.all([p1, p2]);
  expect(stale.pages.length).toBe(0);
  expect(latest.error ?? undefined).toBe(undefined);
  expect(latest.pages.length).toBe(5);
  expect(latest.pages[0].layers.length).toBe(1);
});

test("exposes Phase 2 search, detail, stats, fuzzy jump, locateClick and MCP snippet", async () => {
  const preview = open();
  const compiled = await preview.compile(contest(), 1, inView([1]));
  expect(compiled.starts[0].id).toBe("contest-zh-a");
  expect(compiled.starts[0].first).toBe(2);
  expect(compiled.starts[0].count >= 1).toBe(true);

  // Locate a body point on page 1 (0-based, Problem A) back to Markdown.
  let hit = null;
  for (let y = 120; y < 500 && !hit; y += 16) {
    for (let x = 80; x < 450 && !hit; x += 32) {
      hit = preview.locateClick(1, x, y);
    }
  }
  expect(hit !== null).toBe(true);
  expect(hit!.file).toBe(A);
  expect(hit!.line >= 1 && hit!.column >= 1).toBe(true);

  // Problem library search, facets, and details.
  const all = preview.searchProblems({});
  expect(all.length).toBe(3);
  const facets = preview.workspaceFacets();
  expect(Array.isArray(facets.tags)).toBe(true);
  expect(Array.isArray(facets.platforms)).toBe(true);
  const pDetail = preview.problemDetail("contest-zh-a");
  expect(pDetail.error ?? undefined).toBe(undefined);
  expect(pDetail.statement.path).toBe(A);
  expect(pDetail.statement.hash.length).toBe(64);
  expect(pDetail.referencingContests.join()).toBe("contest-zh");

  // Contest detail, stats, and active-contest fuzzy jump.
  const cDetail = preview.contestDetail("contest-zh");
  expect(cDetail.error ?? undefined).toBe(undefined);
  expect(cDetail.items.length).toBe(3);
  expect(cDetail.items[0].overriddenTitle).toBe(false);
  const stats = preview.contestStats("contest-zh");
  expect(stats.problemCount).toBe(3);
  expect(stats.ratingDistribution.length).toBe(8);

  const labelHits = preview.fuzzyJump("B", "contest-zh");
  expect(labelHits[0].kind).toBe("contest_item");
  expect(labelHits[0].label).toBe("B");
  expect(labelHits[0].id).toBe("contest-zh-b");

  // Tri-state patch decoding across QuickJS FFI (hits read-only guard without PropError).
  const ovRes = preview.setOverride("contest-zh", "A", { title: "Override", time: null, showMetadata: false });
  expect(ovRes.ok).toBe(false);
  expect(ovRes.code).toBe("read_only");

  // MCP config snippet & preferences.
  const snippet = JSON.parse(preview.mcpConfigSnippet());
  expect(snippet.mcpServers.xcpc.command).toBe("xcpc");
  const prefs = preview.loadPrefs();
  expect(prefs.splitRatio).toBe(0.5);
  preview.savePrefs({ ...prefs, splitRatio: 0.45, propsOpen: false });
  expect(preview.loadPrefs().splitRatio).toBe(0.45);
  expect(preview.loadPrefs().propsOpen).toBe(false);

  // Export status, lastWorkspaceMissing, and revealInFolder sandbox checks.
  expect(preview.workspace().lastWorkspaceMissing).toBe(false);
  const status = preview.exportStatus(contest());
  expect(status.statements.relPath).toBe("build/第一届 XCPC 工作站测试赛-试题册.pdf");
  expect(status.editorial.relPath).toBe("build/第一届 XCPC 工作站测试赛-题解.pdf");
  const about = Preview.aboutInfo();
  expect(about.version.length > 0).toBe(true);
  expect(about.os.length > 0 && about.arch.length > 0).toBe(true);
  const escapeRes = await preview.revealInFolder("../outside.txt");
  expect(escapeRes.ok).toBe(false);
  expect(escapeRes.code).toBe("path_escape");
  const missingRes = await preview.revealInFolder("build/nonexistent.pdf");
  expect(missingRes.ok).toBe(false);
  expect(missingRes.code).toBe("not_found");
});

test("wakes nextChange cleanly when Preview is disposed", async () => {
  // 1. Disposed immediately before first poll.
  const immediate = open();
  const pendingImmediate = immediate.nextChange();
  immediate.dispose();
  const ev1 = await pendingImmediate;
  expect(ev1.kind).toBe("closed");
  expect(ev1.paths.length).toBe(0);

  // 2. Disposed while parked inside blocking mpsc::recv().
  const parked = open();
  const pendingParked = parked.nextChange();
  await new Promise((resolve) => setTimeout(resolve, 25));
  parked.dispose();
  const ev2 = await pendingParked;
  expect(ev2.kind).toBe("closed");
  expect(ev2.paths.length).toBe(0);
});

// ── UI state logic (src/ui/logic.ts) ─────────────────────────────────────────

const A4 = { width: 595.28, height: 841.89 };

test("preview geometry: fit, page tops, the page in view, and a click's page point", () => {
  const z = fitZoomFor([A4], 600 + 2 * GAP);
  expect(Math.abs(z * A4.width - 600) < 1e-9).toBe(true);
  const pages = [A4, A4, A4];
  expect(pageTopFor(pages, 1, 2)).toBe(2 * (A4.height + GAP));
  expect(pageAt(pages, 1, 0, 800)).toBe(0);
  expect(pageAt(pages, 1, pageTopFor(pages, 1, 1) + 10, 800)).toBe(1);
  const hit = contentToPage(pages, 1, 20, 20 + 100, GAP + A4.height + GAP + 50);
  expect(hit?.page).toBe(1);
  expect(Math.abs(hit!.xPt - 100) < 1e-9 && Math.abs(hit!.yPt - 50) < 1e-9).toBe(true);
  expect(contentToPage(pages, 1, 20, 5, GAP + 10)).toBe(null);
  const view = computeView(pages, 1, 0, 0, { width: 700, height: 400 });
  expect(view.map((v) => v.page).join()).toBe("0,1");
  expect(view[1].width).toBe(0);
});

test("ratings fall into the 7 tiers and unrated", () => {
  expect(ratingTier(null)).toBe("unrated");
  expect(ratingTier(800)).toBe("newbie");
  expect(ratingTier(1200)).toBe("pupil");
  expect(ratingTier(1899)).toBe("expert");
  expect(ratingTier(2100)).toBe("master");
  expect(ratingTier(3500)).toBe("grandmaster");
});

test("drag reordering: where a row lands and the order after it, labels follow", () => {
  const slots = [0, 40, 80, 120].map((top) => ({ top, height: 40 }));
  expect(dropIndex(slots, 3, 5)).toBe(0); // above the first row's middle
  expect(dropIndex(slots, 0, 70)).toBe(1);
  expect(dropIndex(slots, 0, 200)).toBe(3);
  expect(dropIndex(slots, 1, 50)).toBe(1); // stays
  expect(moveItem(["a", "b", "c", "d"], 3, 0).join()).toBe("d,a,b,c");
  expect(moveItem(["a", "b", "c"], 0, 2).join()).toBe("b,c,a");
  expect(labels(3).join()).toBe("A,B,C");
  expect(labels(30).length).toBe(26);
});

test("samples are cleaned of BOM and carriage returns", () => {
  expect(cleanSample("\uFEFF1 2\r\n3 4\r\n")).toBe("1 2\n3 4\n");
  expect(cleanSample("a\rb")).toBe("a\nb");
});

test("the export button reveals only a fresh PDF of the current book", () => {
  const f = (exists: boolean, fresh: boolean) => ({ relPath: "", absPath: "", exists, fresh });
  const status = { statements: f(true, true), editorial: f(true, false) };
  expect(exportAction(status, "statements")).toBe("reveal");
  expect(exportAction(status, "editorial")).toBe("export");
  expect(exportAction(undefined, "statements")).toBe("export");
  expect(revealState(status, "editorial")).toBe("stale");
  expect(revealState({ statements: f(false, false), editorial: f(false, false) }, "statements")).toBe("none");
});

test("the window title names what is open", () => {
  expect(windowTitle({ workspace: false, space: "problems" })).toBe("XCPC Workstation");
  expect(windowTitle({ workspace: true, space: "contests" })).toBe("比赛");
  expect(windowTitle({ workspace: true, workspaceName: "icpc-2026", space: "contests" })).toBe("比赛 — icpc-2026");
  expect(windowTitle({ workspace: true, space: "problems", studio: { problemTitle: "Two Sum", label: "A", contestName: "春季训练赛" } })).toBe(
    "A. Two Sum — 春季训练赛",
  );
  expect(windowTitle({ workspace: true, space: "problems", studio: { problemTitle: "Two Sum" } })).toBe("Two Sum");
});

test("path helpers handle POSIX, macOS, and Windows paths", () => {
  expect(pathBase("/home/alice/xcpc-ws/")).toBe("xcpc-ws");
  expect(pathBase("C:\\Users\\alice\\xcpc-ws\\")).toBe("xcpc-ws");
  expect(abbreviateHome("/home/alice/work")).toBe("~/work");
  expect(abbreviateHome("/Users/alice/work")).toBe("~/work");
  expect(abbreviateHome("C:\\Users\\alice\\work")).toBe("~\\work");
});

test("an external change lands as one minimal edit", () => {
  const e = minimalEdit("line one\nline two\nline three", "line one\nline 2\nline three");
  expect(e?.text).toBe("2");
  expect(e?.range.start.line).toBe(1);
  expect(e?.range.start.column).toBe(5);
  expect(e?.range.end.column).toBe(8);
  expect(minimalEdit("same", "same")).toBe(null);
  const tail = minimalEdit("abc", "abcdef");
  expect(tail?.text).toBe("def");
  expect(tail?.range.start.column).toBe(3);
});

test("rule refusals read as facts, errors hide codes, IDs validate", () => {
  expect(errorCopy("contest_frozen").rule).toBe(true);
  expect(errorCopy("problem_referenced").next !== undefined).toBe(true);
  expect(errorCopy("io").rule).toBe(false);
  expect(/[a-z_]{4,}/.test(errorCopy("io").message)).toBe(false);
  expect(errorCopy(undefined, "x already exists").message).toBe("同名 ID 已存在");
  expect(validId("two-sum")).toBe(true);
  expect(validId("Two")).toBe(false);
  expect(validId("-a")).toBe(false);
});

test("the split ratio is kept within its range", () => {
  expect(clampRatio(undefined)).toBe(0.5);
  expect(clampRatio(0.95)).toBe(0.9);
  expect(ratioOf([300, 700])).toBe(0.3);
  expect(ratioOf([500])).toBe(undefined);
});

test("motion springs converge to ζ ∈ [0.86, 0.90] without hardcoded transitionDuration", () => {
  const presets = [SPRING_OVERLAY, SPRING_INDICATOR, SPRING_PANEL, SPRING_PAGE, SPRING_PRESS, SPRING_GENTLE];
  for (const p of presets) {
    const k = p.stiffness ?? 0;
    const c = p.damping ?? 0;
    const m = p.mass ?? 1;
    const zeta = c / (2 * Math.sqrt(k * m));
    expect(zeta >= 0.86 && zeta <= 0.90).toBe(true);
  }
  const transitions = [overlayTransition(), indicatorTransition(), panelTransition(), pageTransition(), pressTransition()];
  for (const t of transitions) {
    if (String(t.transitionTimingFunction ?? "").startsWith("spring(")) {
      expect(t.transitionDuration).toBe(undefined);
    }
  }
});

test("formatAppVersion formats dev and release builds consistently", () => {
  expect(formatAppVersion({ version: "0.1.0", commit: "abcdef1234", isDev: true })).toBe("v0.1.0-dev (abcdef1)");
  expect(formatAppVersion({ version: "v0.1.0", commit: "unknown", isDev: true })).toBe("v0.1.0-dev");
  expect(formatAppVersion({ version: "0.1.0", commit: "abcdef1234", isDev: false })).toBe("v0.1.0");
});

test("shortcuts disambiguate the two Ctrl+F entries by context", () => {
  const groups = filterShortcutGroups("mod+f");
  const items = groups.flatMap((g) => g.items).filter((it) => it.keys === "mod+f");
  expect(items.length).toBe(2);
  expect(Boolean(items[0].context && items[1].context)).toBe(true);
  expect(items[0].context !== items[1].context).toBe(true);
});

test("slash commands group cleanly and enforce strict prefix matching", () => {
  expect(SLASH_BASIC_COMMANDS.length).toBe(6);
  expect(SLASH_COMMON_COMMANDS.length).toBe(4);
  expect(SLASH_ADVANCED_COMMANDS.length).toBe(6);
  expect(SLASH_COMMANDS.length).toBe(16);

  expect(matchSlashTrigger("/", 1)).toEqual({ startColumn: 0, endColumn: 1, query: "" });
  expect(matchSlashTrigger("hello /mat", 10)).toEqual({ startColumn: 6, endColumn: 10, query: "mat" });
  expect(matchSlashTrigger("a/b", 3)).toBe(undefined);
  expect(matchSlashTrigger("/ ", 2)).toBe(undefined);

  const matHits = SLASH_COMMANDS.filter((c) => matchesSlashCommand(c, "mat")).map((c) => c.id);
  expect(matHits).toEqual(["inline-math", "display-math", "cases", "matrix"]);
  const h2Hits = SLASH_COMMANDS.filter((c) => matchesSlashCommand(c, "h2")).map((c) => c.id);
  expect(h2Hits).toEqual(["h2", "hint", "interaction"]);
  const tuHits = SLASH_COMMANDS.filter((c) => matchesSlashCommand(c, "tu")).map((c) => c.id);
  expect(tuHits).toEqual(["image", "typst"]);
});

test("markdown heading and list prefix toggles handle add, switch, and remove cleanly", () => {
  expect(toggleHeadingLine("题目描述", 2)).toEqual({ replaceEnd: 0, insert: "## ", delta: 3 });
  expect(toggleHeadingLine("## 题目描述", 2)).toEqual({ replaceEnd: 3, insert: "", delta: -3 });
  expect(toggleHeadingLine("## 题目描述", 3)).toEqual({ replaceEnd: 3, insert: "### ", delta: 1 });

  expect(togglePrefixLine("第一项", "ul")).toEqual({ replaceStart: 0, replaceEnd: 0, insert: "- ", delta: 2 });
  expect(togglePrefixLine("  - 第一项", "ul")).toEqual({ replaceStart: 2, replaceEnd: 4, insert: "", delta: -2 });
  expect(togglePrefixLine("1. 第一项", "ul")).toEqual({ replaceStart: 0, replaceEnd: 3, insert: "- ", delta: -1 });
  expect(togglePrefixLine("- 第一项", "ol", 2)).toEqual({ replaceStart: 0, replaceEnd: 2, insert: "2. ", delta: 1 });
  expect(togglePrefixLine("引文", "quote")).toEqual({ replaceStart: 0, replaceEnd: 0, insert: "> ", delta: 2 });
  expect(togglePrefixLine("> 引文", "quote")).toEqual({ replaceStart: 0, replaceEnd: 2, insert: "", delta: -2 });
});

test("shortcuts disambiguate Ctrl+1/2/3 between home navigation and editor headings", () => {
  for (const key of ["mod+1", "mod+2", "mod+3"]) {
    const items = filterShortcutGroups(key)
      .flatMap((g) => g.items)
      .filter((it) => it.keys === key);
    expect(items.length).toBe(2);
    expect(Boolean(items[0].context && items[1].context)).toBe(true);
    expect(items[0].context !== items[1].context).toBe(true);
  }
});

test("extractMathAtCaret applies smart zero-noise trigger rules", () => {
  const text = "给定 $n$ 和 $\\frac{a}{b}$ 以及\n$$\n\\sum_{i=1}^n i\n$$";
  const spans = Preview.markdownSpans(text);

  // Simple inline `$n$`: suppressed when side-by-side preview is visible, shown when hidden.
  expect(extractMathAtCaret(text, spans, { line: 0, column: 4 }, false)).toBe(undefined);
  expect(extractMathAtCaret(text, spans, { line: 0, column: 4 }, true)).toEqual({
    latex: "n",
    display: false,
  });

  // Inline math with LaTeX command `\\frac{a}{b}`: shown in both modes.
  const fracCol = text.indexOf("\\frac") + 2;
  expect(extractMathAtCaret(text, spans, { line: 0, column: fracCol }, false)).toEqual({
    latex: "\\frac{a}{b}",
    display: false,
  });

  // Display math `$$...$$`: shown in both modes.
  expect(extractMathAtCaret(text, spans, { line: 2, column: 3 }, false)).toEqual({
    latex: "\\sum_{i=1}^n i",
    display: true,
  });
});

test("stepVim handles core normal, insert, visual, yank/paste, and text objects", () => {
  const noMod = { ctrl: false, alt: false, meta: false };
  const ctrlMod = { ctrl: true, alt: false, meta: false };
  const doc = ["hello $a+b$ world", "second (foo) line", "third line"];

  // Motions: w, $, 0, j, G, gg
  let st = INITIAL_VIM_STATE;
  let r = stepVim(st, "w", noMod, doc, { line: 0, column: 0 });
  expect(r.handled).toBe(true);
  expect(r.selection?.head).toEqual({ line: 0, column: 6 });

  r = stepVim(r.state, "$", noMod, doc, { line: 0, column: 6 });
  expect(r.selection?.head).toEqual({ line: 0, column: doc[0].length - 1 });

  r = stepVim(r.state, "0", noMod, doc, { line: 0, column: 10 });
  expect(r.selection?.head).toEqual({ line: 0, column: 0 });

  r = stepVim(r.state, "G", noMod, doc, { line: 0, column: 0 });
  expect(r.selection?.head).toEqual({ line: 2, column: 0 });

  r = stepVim(r.state, "g", noMod, doc, { line: 2, column: 0 });
  expect(r.state.pending).toBe("g");
  r = stepVim(r.state, "g", noMod, doc, { line: 2, column: 0 });
  expect(r.selection?.head).toEqual({ line: 0, column: 0 });

  // Insert & Escape: Esc in insert returns to normal; Esc in normal falls through.
  r = stepVim(r.state, "i", noMod, doc, { line: 0, column: 2 });
  expect(r.state.mode).toBe("insert");
  r = stepVim(r.state, "Escape", noMod, doc, { line: 0, column: 2 });
  expect(r.handled).toBe(true);
  expect(r.state.mode).toBe("normal");
  const escInNormal = stepVim(r.state, "Escape", noMod, doc, { line: 0, column: 1 });
  expect(escInNormal.handled).toBe(false);

  // Undo / redo commands
  expect(stepVim(r.state, "u", noMod, doc, { line: 0, column: 0 }).command).toBe("undo");
  expect(stepVim(r.state, "r", ctrlMod, doc, { line: 0, column: 0 }).command).toBe("redo");

  // Text object: ci$ inside `$a+b$`
  let cur = stepVim(INITIAL_VIM_STATE, "c", noMod, doc, { line: 0, column: 8 });
  cur = stepVim(cur.state, "i", noMod, doc, { line: 0, column: 8 });
  cur = stepVim(cur.state, "$", noMod, doc, { line: 0, column: 8 });
  expect(cur.handled).toBe(true);
  expect(cur.state.mode).toBe("insert");
  expect(cur.state.register).toBe("a+b");
  expect(cur.edit).toEqual({
    range: { start: { line: 0, column: 7 }, end: { line: 0, column: 10 } },
    text: "",
  });

  // Text object: di( inside `(foo)`
  cur = stepVim(INITIAL_VIM_STATE, "d", noMod, doc, { line: 1, column: 9 });
  cur = stepVim(cur.state, "i", noMod, doc, { line: 1, column: 9 });
  cur = stepVim(cur.state, "(", noMod, doc, { line: 1, column: 9 });
  expect(cur.state.mode).toBe("normal");
  expect(cur.state.register).toBe("foo");
  expect(cur.edit).toEqual({
    range: { start: { line: 1, column: 8 }, end: { line: 1, column: 11 } },
    text: "",
  });

  // Linewise yank & paste (yy -> p)
  cur = stepVim(INITIAL_VIM_STATE, "y", noMod, doc, { line: 1, column: 0 });
  cur = stepVim(cur.state, "y", noMod, doc, { line: 1, column: 0 });
  expect(cur.state.registerLinewise).toBe(true);
  expect(cur.state.register).toBe("second (foo) line");
  const pasted = stepVim(cur.state, "p", noMod, doc, { line: 0, column: 0 });
  expect(pasted.edit?.text).toBe("\nsecond (foo) line");
  expect(pasted.selection?.head).toEqual({ line: 1, column: 0 });

  // Visual mode delete (v -> l -> l -> d)
  cur = stepVim(INITIAL_VIM_STATE, "v", noMod, doc, { line: 0, column: 0 });
  expect(cur.state.mode).toBe("visual");
  cur = stepVim(cur.state, "l", noMod, doc, { line: 0, column: 0 });
  cur = stepVim(cur.state, "l", noMod, doc, { line: 0, column: 1 });
  cur = stepVim(cur.state, "d", noMod, doc, { line: 0, column: 2 });
  expect(cur.state.mode).toBe("normal");
  expect(cur.state.register).toBe("hel");
  expect(cur.edit?.range).toEqual({ start: { line: 0, column: 0 }, end: { line: 0, column: 3 } });
});

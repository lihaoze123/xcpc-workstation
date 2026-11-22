import { describe, expect, test } from "nsh/test";
import type { AppUpdate } from "nsh/updater";
import {
  formatUpdateSummary,
  parseInlineSpans,
  parseUpdateNotes,
  resolveReleasePageUrl,
  updateAvailableActionLabel,
  visibleUpdateNotes,
} from "./updateNotes";

function makeUpdate(overrides: Partial<AppUpdate> = {}): AppUpdate {
  return {
    version: "0.2.0-beta.2",
    notes: "### 新增\n\n- **更新日志**：支持 `CHANGELOG.md` 展示\n",
    url: "https://github.com/lihaoze123/xcpc-workstation/releases/download/v0.2.0-beta.2/xcpc-workstation-0.2.0-beta.2-linux-x86_64.tar.gz",
    canInstall: false,
    shell: false,
    ...overrides,
  };
}

describe("resolveReleasePageUrl", () => {
  test("converts GitHub release asset download URL to tag page (stable & prerelease)", () => {
    expect(
      resolveReleasePageUrl({
        version: "0.2.0",
        url: "https://github.com/lihaoze123/xcpc-workstation/releases/download/v0.2.0/xcpc-workstation-0.2.0-linux-x86_64.tar.gz",
      }),
    ).toBe("https://github.com/lihaoze123/xcpc-workstation/releases/tag/v0.2.0");

    expect(
      resolveReleasePageUrl({
        version: "0.2.0-beta.2",
        url: "https://github.com/lihaoze123/xcpc-workstation/releases/download/v0.2.0-beta.2/xcpc-workstation-0.2.0-beta.2-linux-x86_64.tar.gz",
      }),
    ).toBe("https://github.com/lihaoze123/xcpc-workstation/releases/tag/v0.2.0-beta.2");
  });

  test("preserves original tag without v prefix and supports fork repositories", () => {
    expect(
      resolveReleasePageUrl({
        version: "0.3.0-rc.1",
        url: "https://github.com/someone/xcpc-fork/releases/download/0.3.0-rc.1/app.tar.gz",
      }),
    ).toBe("https://github.com/someone/xcpc-fork/releases/tag/0.3.0-rc.1");
  });

  test("keeps an existing GitHub release tag page URL as-is", () => {
    expect(
      resolveReleasePageUrl({
        version: "0.2.0",
        url: "https://github.com/someone/xcpc-fork/releases/tag/v0.2.0",
      }),
    ).toBe("https://github.com/someone/xcpc-fork/releases/tag/v0.2.0");
  });

  test("falls back to default repo tag URL when update.url is non-standard", () => {
    expect(
      resolveReleasePageUrl({
        version: "0.2.0-beta.2",
        url: "https://mirror.example.com/downloads/xcpc.tar.gz",
      }),
    ).toBe("https://github.com/lihaoze123/xcpc-workstation/releases/tag/v0.2.0-beta.2");

    expect(
      resolveReleasePageUrl({
        version: "v0.2.0",
        url: "",
      }),
    ).toBe("https://github.com/lihaoze123/xcpc-workstation/releases/tag/v0.2.0");
  });

  test("falls back to update.url when version is also empty", () => {
    expect(
      resolveReleasePageUrl({
        version: "",
        url: "https://mirror.example.com/releases",
      }),
    ).toBe("https://mirror.example.com/releases");
  });
});

describe("parseInlineSpans", () => {
  test("parses bold, inline code, and plain text", () => {
    expect(parseInlineSpans("支持 **加粗** 与 `olymp-classic` 模板")).toEqual([
      { kind: "text", text: "支持 " },
      { kind: "bold", text: "加粗" },
      { kind: "text", text: " 与 " },
      { kind: "code", text: "olymp-classic" },
      { kind: "text", text: " 模板" },
    ]);
  });

  test("handles multi-backtick inline code spans", () => {
    expect(parseInlineSpans("透传 ```` ```typst ```` 原始块")).toEqual([
      { kind: "text", text: "透传 " },
      { kind: "code", text: "```typst" },
      { kind: "text", text: " 原始块" },
    ]);
  });

  test("gracefully preserves unclosed markers as plain text", () => {
    expect(parseInlineSpans("未闭合 **加粗 和 `代码")).toEqual([
      { kind: "text", text: "未闭合 **加粗 和 `代码" },
    ]);
  });
});

describe("parseUpdateNotes", () => {
  test("parses Keep a Changelog headings, two-level bullets, and paragraphs", () => {
    const md = [
      "首次公开发布，包含核心功能。",
      "第二行同一段落。",
      "",
      "### 新增",
      "",
      "- **Typst 外部库支持**：",
      "  - 内嵌 `@preview/cetz:0.5.3` 离线回退包。",
      "",
      "### 修复",
      "- 修复只读安装更新跳转问题。",
    ].join("\n");

    expect(parseUpdateNotes(md)).toEqual([
      {
        kind: "paragraph",
        spans: [{ kind: "text", text: "首次公开发布，包含核心功能。 第二行同一段落。" }],
      },
      { kind: "heading", text: "新增" },
      {
        kind: "bullet",
        level: 0,
        spans: [
          { kind: "bold", text: "Typst 外部库支持" },
          { kind: "text", text: "：" },
        ],
      },
      {
        kind: "bullet",
        level: 1,
        spans: [
          { kind: "text", text: "内嵌 " },
          { kind: "code", text: "@preview/cetz:0.5.3" },
          { kind: "text", text: " 离线回退包。" },
        ],
      },
      { kind: "heading", text: "修复" },
      {
        kind: "bullet",
        level: 0,
        spans: [{ kind: "text", text: "修复只读安装更新跳转问题。" }],
      },
    ]);
  });

  test("returns empty array for blank input", () => {
    expect(parseUpdateNotes("   \n\n  ")).toEqual([]);
  });
});

describe("About updater presentation states", () => {
  const fmtVer = (v: string) => (v.startsWith("v") ? v : `v${v}`);
  const fmtBytes = (b: number) => `${b} B`;

  test("uses '前往发布页' when canInstall is false and '下载更新' when true, with unified subtitle", () => {
    const roUpdate = makeUpdate({ canInstall: false, version: "0.2.0-beta.2" });
    const rwUpdate = makeUpdate({ canInstall: true, version: "0.2.0-beta.2" });

    expect(updateAvailableActionLabel(roUpdate)).toBe("前往发布页");
    expect(updateAvailableActionLabel(rwUpdate)).toBe("下载更新");

    expect(formatUpdateSummary({ kind: "available", update: roUpdate }, false, fmtVer, fmtBytes)).toBe(
      "发现新版本 v0.2.0-beta.2",
    );
    expect(formatUpdateSummary({ kind: "available", update: rwUpdate }, false, fmtVer, fmtBytes)).toBe(
      "发现新版本 v0.2.0-beta.2",
    );
  });

  test("keeps release notes visible across available, downloading, ready, and scheduled states", () => {
    const update = makeUpdate();
    const expected = update.notes!.trim();

    expect(visibleUpdateNotes({ kind: "idle" })).toBe("");
    expect(visibleUpdateNotes({ kind: "checking" })).toBe("");
    expect(visibleUpdateNotes({ kind: "upToDate" })).toBe("");
    expect(visibleUpdateNotes({ kind: "error", message: "err" })).toBe("");

    expect(visibleUpdateNotes({ kind: "available", update })).toBe(expected);
    expect(visibleUpdateNotes({ kind: "downloading", update, progress: { received: 50, total: 100 } })).toBe(expected);
    expect(visibleUpdateNotes({ kind: "ready", update })).toBe(expected);
    expect(visibleUpdateNotes({ kind: "scheduled", update })).toBe(expected);
  });

  test("hides release notes when notes is null, empty, or whitespace", () => {
    expect(visibleUpdateNotes({ kind: "available", update: makeUpdate({ notes: null }) })).toBe("");
    expect(visibleUpdateNotes({ kind: "available", update: makeUpdate({ notes: "   \n  " }) })).toBe("");
  });
});

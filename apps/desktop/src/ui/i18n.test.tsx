import { describe, expect, test } from "nsh/test";
import {
  en,
  languagePref,
  locale,
  normalizeLanguagePref,
  normalizeLocale,
  resolveLocalized,
  systemLocale,
  t,
  zhCN,
  type MessageKey,
} from "./i18n";
import { CORE_WORDED_CODES, diffKind, errorCopy, windowTitle } from "./logic";
import { filterShortcutGroups } from "./ShortcutsModal";
import { matchesSlashCommand, SLASH_COMMANDS, slashSnippetFor } from "./studio/SlashPopover";

function extractParams(text: string): string[] {
  const names = new Set<string>();
  const re = /\{(\w+)\}/g;
  let m: RegExpExecArray | null;
  while ((m = re.exec(text)) !== null) {
    names.add(m[1]);
  }
  return [...names].sort();
}

function resetLocale() {
  systemLocale.set("zh-CN");
  languagePref.set("system");
}

describe("i18n dictionaries (zh-CN & en)", () => {
  test("zh-CN and en have identical key sets", () => {
    const zhKeys = Object.keys(zhCN).sort();
    const enKeys = Object.keys(en).sort();
    expect(enKeys).toEqual(zhKeys);
    expect(zhKeys.length).toBeGreaterThan(100);
  });

  test("every key has identical {param} placeholder sets in zh-CN and en", () => {
    for (const key of Object.keys(zhCN) as MessageKey[]) {
      const zhParams = extractParams(zhCN[key]);
      const enParams = extractParams(en[key]);
      expect(enParams).toEqual(zhParams);
    }
  });

  test("all values are non-empty, English copy avoids exclamation marks, and Chinese build copy avoids internal jargon", () => {
    for (const key of Object.keys(zhCN) as MessageKey[]) {
      expect(zhCN[key].trim().length).toBeGreaterThan(0);
      expect(en[key].trim().length).toBeGreaterThan(0);
      expect(en[key].includes("!")).toBe(false);
      expect(zhCN[key].includes("门禁")).toBe(false);
      if (key.startsWith("build.") || key.startsWith("inspector.package.")) {
        expect(zhCN[key].includes("矩阵")).toBe(false);
      }
    }
  });
});

describe("locale normalization & reactive hot-switching", () => {
  test("normalizeLocale maps zh* to zh-CN and everything else to en", () => {
    expect(normalizeLocale("zh-CN")).toBe("zh-CN");
    expect(normalizeLocale("zh_TW.UTF-8")).toBe("zh-CN");
    expect(normalizeLocale("ZH")).toBe("zh-CN");
    expect(normalizeLocale("en-US")).toBe("en");
    expect(normalizeLocale("C")).toBe("en");
    expect(normalizeLocale("POSIX")).toBe("en");
    expect(normalizeLocale("")).toBe("en");
    expect(normalizeLocale(null)).toBe("en");
  });

  test("normalizeLanguagePref accepts system | zh-CN | en and defaults to system", () => {
    expect(normalizeLanguagePref("system")).toBe("system");
    expect(normalizeLanguagePref("zh-CN")).toBe("zh-CN");
    expect(normalizeLanguagePref("en")).toBe("en");
    expect(normalizeLanguagePref("ja")).toBe("system");
    expect(normalizeLanguagePref(undefined)).toBe("system");
  });

  test("switching languagePref updates locale(), t(), windowTitle(), errorCopy(), diffKind(), and shortcuts reactively", () => {
    try {
      systemLocale.set("zh-CN");
      languagePref.set("system");
      expect(locale()).toBe("zh-CN");
      expect(t("space.problems")).toBe("题库");
      expect(windowTitle({ workspace: true, workspaceName: "ws", space: "contests" })).toBe("比赛 — ws");
      expect(errorCopy("contest_frozen").message).toBe("比赛已定稿，内容只读");
      expect(diffKind("statement_changed")).toBe("题面");

      languagePref.set("en");
      expect(locale()).toBe("en");
      expect(t("space.problems")).toBe("Problems");
      expect(t("space.contests")).toBe("Contests");
      expect(t("space.settings")).toBe("Settings");
      expect(windowTitle({ workspace: true, workspaceName: "ws", space: "contests" })).toBe("Contests — ws");
      expect(errorCopy("contest_frozen").message).toBe("Contest is finalized and read-only");
      expect(diffKind("statement_changed")).toBe("Statement");
      expect(filterShortcutGroups("Properties").flatMap((g) => g.items).some((it) => it.id === "inspector")).toBe(true);

      // System preference follows systemLocale signal.
      languagePref.set("system");
      systemLocale.set("en");
      expect(locale()).toBe("en");
      expect(t("export.pdf")).toBe("Export PDF");
    } finally {
      resetLocale();
    }
  });
});

describe("resolveLocalized 6-step fallback chain", () => {
  test("1. plain string returns as-is", () => {
    expect(resolveLocalized("My Custom Template", "en")).toBe("My Custom Template");
    expect(resolveLocalized("我的模板", "zh-CN")).toBe("我的模板");
  });

  test("2a. exact match is case-insensitive and treats _ as -", () => {
    expect(resolveLocalized({ "zh_cn": "简体", en: "English" }, "zh-CN")).toBe("简体");
    expect(resolveLocalized({ "EN-us": "US English", zh: "中文" }, "en_US")).toBe("US English");
  });

  test("2b. primary subtag match prefers exact primary tag, then first sorted same-primary key (e.g. zh-Hans)", () => {
    expect(resolveLocalized({ zh: "中文", en: "English" }, "zh-CN")).toBe("中文");
    expect(resolveLocalized({ "zh-Hans": "简中", "zh-Hant": "繁中", en: "English" }, "zh-CN")).toBe("简中");
    expect(resolveLocalized({ "en-GB": "British", fr: "Français" }, "en")).toBe("British");
  });

  test("2c. falls back to en when target locale has no match", () => {
    expect(resolveLocalized({ en: "English", fr: "Français" }, "ja")).toBe("English");
    expect(resolveLocalized({ "en-US": "US", de: "Deutsch" }, "zh-CN")).toBe("US");
  });

  test("2d. falls back to zh / zh-CN when neither target nor en matches", () => {
    expect(resolveLocalized({ zh: "中文", fr: "Français" }, "ja")).toBe("中文");
    expect(resolveLocalized({ "zh-CN": "简体中文", de: "Deutsch" }, "ja")).toBe("简体中文");
  });

  test("2e. falls back to first non-empty value sorted lexicographically by key", () => {
    expect(resolveLocalized({ ja: "日本語", de: "Deutsch", fr: "Français" }, "ko")).toBe("Deutsch");
    expect(resolveLocalized({ a: "   ", b: "Bravo", c: "Charlie" }, "ko")).toBe("Bravo");
  });

  test("2f. empty dict, whitespace-only values, null, and undefined return empty string", () => {
    expect(resolveLocalized({}, "zh-CN")).toBe("");
    expect(resolveLocalized({ en: "", zh: "  " }, "en")).toBe("");
    expect(resolveLocalized(null, "en")).toBe("");
    expect(resolveLocalized(undefined, "zh-CN")).toBe("");
  });
});

describe("Slash command snippets follow publishing language with UI locale fallback", () => {
  test("explicit chinese publishing language emits Chinese snippets regardless of UI locale", () => {
    try {
      languagePref.set("en");
      expect(slashSnippetFor("hint", "chinese")?.template).toBe("## 提示\n\n");
      expect(slashSnippetFor("interaction", "chinese")?.template).toBe("## 交互方式\n\n");
      expect(slashSnippetFor("table", "chinese")?.template).toBe("| 列 1 | 列 2 |\n| --- | --- |\n|  |  |");
      expect(slashSnippetFor("subtasks", "chinese")?.template).toBe(
        "| 子任务 | 分值 | 特殊性质 |\n| :---: | :---: | --- |\n| 1 | 20 | $n \\le 10$ |\n| 2 | 30 | $n \\le 1000$ |\n| 3 | 50 | 无特殊限制 |",
      );
    } finally {
      resetLocale();
    }
  });

  test("explicit english publishing language emits exact spec §4.3.3 English snippets regardless of UI locale", () => {
    try {
      languagePref.set("zh-CN");
      expect(slashSnippetFor("hint", "english")?.template).toBe("## Note\n\n");
      expect(slashSnippetFor("interaction", "english")?.template).toBe("## Interaction\n\n");
      expect(slashSnippetFor("table", "english")?.template).toBe("| Column 1 | Column 2 |\n| --- | --- |\n|  |  |");
      expect(slashSnippetFor("subtasks", "english")?.template).toBe(
        "| Subtask | Score | Constraints |\n| :---: | :---: | --- |\n| 1 | 20 | $n \\le 10$ |\n| 2 | 30 | $n \\le 1000$ |\n| 3 | 50 | No additional constraints |",
      );
    } finally {
      resetLocale();
    }
  });

  test("unspecified publishing language falls back to active UI locale", () => {
    try {
      languagePref.set("en");
      expect(slashSnippetFor("hint", undefined)?.template).toBe("## Note\n\n");
      expect(slashSnippetFor("interaction", null)?.template).toBe("## Interaction\n\n");
      expect(slashSnippetFor("table")?.template).toBe("| Column 1 | Column 2 |\n| --- | --- |\n|  |  |");
      expect(slashSnippetFor("subtasks")?.template).toContain("No additional constraints");

      languagePref.set("zh-CN");
      expect(slashSnippetFor("hint", undefined)?.template).toBe("## 提示\n\n");
      expect(slashSnippetFor("interaction", null)?.template).toBe("## 交互方式\n\n");
      expect(slashSnippetFor("table")?.template).toBe("| 列 1 | 列 2 |\n| --- | --- |\n|  |  |");
      expect(slashSnippetFor("subtasks")?.template).toContain("无特殊限制");
    } finally {
      resetLocale();
    }
  });

  test("bilingual and pinyin keywords match in both zh-CN and en UI locales", () => {
    try {
      languagePref.set("en");
      expect(SLASH_COMMANDS.filter((c) => matchesSlashCommand(c, "提示")).map((c) => c.id)).toContain("hint");
      expect(SLASH_COMMANDS.filter((c) => matchesSlashCommand(c, "tishi")).map((c) => c.id)).toContain("hint");
      expect(SLASH_COMMANDS.filter((c) => matchesSlashCommand(c, "note")).map((c) => c.id)).toContain("hint");

      languagePref.set("zh-CN");
      expect(SLASH_COMMANDS.filter((c) => matchesSlashCommand(c, "note")).map((c) => c.id)).toContain("hint");
      expect(SLASH_COMMANDS.filter((c) => matchesSlashCommand(c, "subtask")).map((c) => c.id)).toContain("subtasks");
    } finally {
      resetLocale();
    }
  });
});

describe("Execution errors never fall back to the generic line (authoring-ui-refactor §6.2)", () => {
  test("judge codes show the Core human message verbatim", () => {
    resetLocale();
    const required = [
      "no_compiler",
      "compiler_not_found",
      "no_main_solution",
      "multiple_main_solutions",
      "unknown_checker",
      "test_not_found",
      "generator_not_found",
      "solution_not_found",
      "invalid_limit",
      "invalid_config",
    ];
    const generic = t("error.generic");
    for (const code of required) {
      expect(CORE_WORDED_CODES.has(code)).toBe(true);
      const raw = `core message for ${code}`;
      const copy = errorCopy(code, raw);
      expect(copy.message).toBe(raw);
      expect(copy.message).not.toBe(generic);
    }
  });

  test("unknown codes with Chinese Core text keep that text", () => {
    resetLocale();
    expect(errorCopy("something_new", "交互器编译失败。").message).toBe("交互器编译失败。");
    expect(errorCopy("something_new", "boom").message).toBe(t("error.generic"));
  });
});

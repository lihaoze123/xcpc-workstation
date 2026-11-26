// Document-style floating slash command popover anchored directly below the editor caret.
// When the query is empty, renders a compact single-row 6-icon "基础" strip above two vertical
// groups ("常用" and "题面与进阶") with low-saturation semantic tinted icon badges. Once the
// user types a filter query, collapses into a unified vertical result list.

import {
  cleanup,
  colorScheme,
  effect,
  Overlay,
  reduceMotion,
  Scroll,
  signal,
  Text,
  untracked,
  View,
  type Bounds,
  type InputRef,
  type ScrollRef,
} from "nsh";
import { Input } from "nsh/base";
import { Icon, type IconName } from "../Icon";
import { locale, t, type Locale, type MessageKey } from "../i18n";
import { Kbd, tip } from "../kit";
import { colorTransition, overlayTransition } from "../motion";
import { popoverShadow } from "../theme";

export type SlashCommandGroup = "basic" | "common" | "advanced";

export type SlashTint = "neutral" | "ochre" | "amber" | "emerald" | "indigo" | "teal" | "violet" | "sky";

export type PublishLanguage = "chinese" | "english";

export interface SlashCommand {
  readonly id: string;
  readonly group: SlashCommandGroup;
  readonly labelKey: MessageKey;
  readonly descKey: MessageKey;
  readonly keywords: readonly string[];
  readonly shortcut?: string;
  readonly icon?: IconName;
  readonly badge?: string;
  readonly tint: SlashTint;
  readonly kind?: "image";
  readonly template?: string;
  readonly templateEn?: string;
  readonly caret?: { readonly line: number; readonly column: number };
}

export function slashCommandLabel(cmd: SlashCommand): string {
  return t(cmd.labelKey);
}

export function slashCommandDescription(cmd: SlashCommand): string {
  return t(cmd.descKey);
}

/** Resolves effective publishing language for natural-language slash snippets (`hint`, `interaction`, `table`, `subtasks`). */
export function resolveSnippetLanguage(
  publishLanguage?: PublishLanguage | string | null,
  uiLocale?: Locale,
): PublishLanguage {
  if (publishLanguage === "english" || publishLanguage === "chinese") return publishLanguage;
  return (uiLocale ?? locale()) === "en" ? "english" : "chinese";
}

/** Returns the Markdown template and caret offset for `cmd` (or command id) under the given publishing/UI language. */
export function slashSnippetFor(
  cmdOrId: SlashCommand | string,
  publishLanguage?: PublishLanguage | string | null,
  uiLocale?: Locale,
): { template: string; caret?: { readonly line: number; readonly column: number } } | undefined {
  const cmd = typeof cmdOrId === "string" ? SLASH_COMMANDS.find((c) => c.id === cmdOrId) : cmdOrId;
  if (!cmd || cmd.template === undefined) return undefined;
  const lang = resolveSnippetLanguage(publishLanguage, uiLocale);
  const template = lang === "english" && cmd.templateEn !== undefined ? cmd.templateEn : cmd.template;
  return { template, caret: cmd.caret };
}

export const SLASH_BASIC_COMMANDS: readonly SlashCommand[] = [
  {
    id: "h2",
    group: "basic",
    labelKey: "slash.cmd.h2.label",
    descKey: "slash.cmd.h2.desc",
    keywords: ["h2", "heading", "title", "section", "二级标题", "标题", "章节", "biaoti", "bt", "zhangjie", "zj"],
    shortcut: "mod+2",
    badge: "H2",
    tint: "neutral",
    template: "## ",
    caret: { line: 0, column: 3 },
  },
  {
    id: "h3",
    group: "basic",
    labelKey: "slash.cmd.h3.label",
    descKey: "slash.cmd.h3.desc",
    keywords: ["h3", "subheading", "heading", "三级标题", "子标题", "zibiaoti", "zbt"],
    shortcut: "mod+3",
    badge: "H3",
    tint: "neutral",
    template: "### ",
    caret: { line: 0, column: 4 },
  },
  {
    id: "ul",
    group: "basic",
    labelKey: "slash.cmd.ul.label",
    descKey: "slash.cmd.ul.desc",
    keywords: ["ul", "list", "bullet", "bulleted", "无序列表", "列表", "liebiao", "lb"],
    shortcut: "mod+shift+8",
    icon: "list",
    tint: "neutral",
    template: "- ",
    caret: { line: 0, column: 2 },
  },
  {
    id: "ol",
    group: "basic",
    labelKey: "slash.cmd.ol.label",
    descKey: "slash.cmd.ol.desc",
    keywords: ["ol", "numbered", "ordered", "list", "有序列表", "有序", "youxu", "yx"],
    shortcut: "mod+shift+7",
    icon: "listOrdered",
    tint: "neutral",
    template: "1. ",
    caret: { line: 0, column: 3 },
  },
  {
    id: "quote",
    group: "basic",
    labelKey: "slash.cmd.quote.label",
    descKey: "slash.cmd.quote.desc",
    keywords: ["quote", "blockquote", "引用块", "引用", "yinyong", "yy"],
    shortcut: "mod+shift+.",
    badge: "❝",
    tint: "neutral",
    template: "> ",
    caret: { line: 0, column: 2 },
  },
  {
    id: "inline-math",
    group: "basic",
    labelKey: "slash.cmd.inlineMath.label",
    descKey: "slash.cmd.inlineMath.desc",
    keywords: ["inline", "math", "formula", "$", "行内公式", "公式", "hangnei", "hn", "gongshi", "gs"],
    shortcut: "mod+m",
    badge: "$",
    tint: "ochre",
    template: "$$",
    caret: { line: 0, column: 1 },
  },
];

export const SLASH_COMMON_COMMANDS: readonly SlashCommand[] = [
  {
    id: "display-math",
    group: "common",
    labelKey: "slash.cmd.displayMath.label",
    descKey: "slash.cmd.displayMath.desc",
    keywords: ["math", "formula", "equation", "display", "行间公式", "公式块", "gongshi", "gs", "$$"],
    shortcut: "mod+shift+m",
    icon: "sigma",
    tint: "ochre",
    template: "$$\n\n$$",
    caret: { line: 1, column: 0 },
  },
  {
    id: "image",
    group: "common",
    labelKey: "slash.cmd.image.label",
    descKey: "slash.cmd.image.desc",
    keywords: ["image", "img", "pic", "picture", "figure", "插入图片", "图片", "tupian", "tp"],
    icon: "image",
    tint: "amber",
    kind: "image",
  },
  {
    id: "table",
    group: "common",
    labelKey: "slash.cmd.table.label",
    descKey: "slash.cmd.table.desc",
    keywords: ["table", "grid", "markdown", "表格", "biaoge", "bg"],
    icon: "table",
    tint: "emerald",
    template: "| 列 1 | 列 2 |\n| --- | --- |\n|  |  |",
    templateEn: "| Column 1 | Column 2 |\n| --- | --- |\n|  |  |",
    caret: { line: 2, column: 2 },
  },
  {
    id: "cpp",
    group: "common",
    labelKey: "slash.cmd.cpp.label",
    descKey: "slash.cmd.cpp.desc",
    keywords: ["cpp", "c++", "code", "source", "代码块", "代码", "daima", "dm"],
    icon: "squareCode",
    tint: "indigo",
    template: "```cpp\n\n```",
    caret: { line: 1, column: 0 },
  },
];

export const SLASH_ADVANCED_COMMANDS: readonly SlashCommand[] = [
  {
    id: "cases",
    group: "advanced",
    labelKey: "slash.cmd.cases.label",
    descKey: "slash.cmd.cases.desc",
    keywords: ["cases", "piecewise", "分段函数", "分段", "fenduan", "fd", "math"],
    badge: "{",
    tint: "ochre",
    template: "$$\nf(x) = \\begin{cases}\n  , & x \\le 0 \\\\\n  , & x > 0\n\\end{cases}\n$$",
    caret: { line: 2, column: 2 },
  },
  {
    id: "matrix",
    group: "advanced",
    labelKey: "slash.cmd.matrix.label",
    descKey: "slash.cmd.matrix.desc",
    keywords: ["matrix", "bmatrix", "矩阵", "juzhen", "jz", "math"],
    badge: "[ ]",
    tint: "ochre",
    template: "$$\n\\begin{bmatrix}\n  a & b \\\\\n  c & d\n\\end{bmatrix}\n$$",
    caret: { line: 2, column: 2 },
  },
  {
    id: "subtasks",
    group: "advanced",
    labelKey: "slash.cmd.subtasks.label",
    descKey: "slash.cmd.subtasks.desc",
    keywords: ["subtask", "subtasks", "score", "子任务评分表", "子任务", "zirenwu", "zrw", "table"],
    icon: "listPlus",
    tint: "teal",
    template:
      "| 子任务 | 分值 | 特殊性质 |\n| :---: | :---: | --- |\n| 1 | 20 | $n \\le 10$ |\n| 2 | 30 | $n \\le 1000$ |\n| 3 | 50 | 无特殊限制 |",
    templateEn:
      "| Subtask | Score | Constraints |\n| :---: | :---: | --- |\n| 1 | 20 | $n \\le 10$ |\n| 2 | 30 | $n \\le 1000$ |\n| 3 | 50 | No additional constraints |",
    caret: { line: 2, column: 15 },
  },
  {
    id: "hint",
    group: "advanced",
    labelKey: "slash.cmd.hint.label",
    descKey: "slash.cmd.hint.desc",
    keywords: ["hint", "note", "提示章节", "提示", "tishi", "ts", "tszj", "h2", "section"],
    icon: "info",
    tint: "teal",
    template: "## 提示\n\n",
    templateEn: "## Note\n\n",
    caret: { line: 2, column: 0 },
  },
  {
    id: "interaction",
    group: "advanced",
    labelKey: "slash.cmd.interaction.label",
    descKey: "slash.cmd.interaction.desc",
    keywords: ["interaction", "interactive", "protocol", "交互章节", "交互", "jiaohu", "jh", "jhzj", "h2", "section"],
    icon: "bot",
    tint: "violet",
    template: "## 交互方式\n\n",
    templateEn: "## Interaction\n\n",
    caret: { line: 2, column: 0 },
  },
  {
    id: "typst",
    group: "advanced",
    labelKey: "slash.cmd.typst.label",
    descKey: "slash.cmd.typst.desc",
    keywords: ["typst", "raw", "vector", "diagram", "原始", "tu", "yst"],
    icon: "code",
    tint: "sky",
    template: "```typst\n\n```",
    caret: { line: 1, column: 0 },
  },
];

export const SLASH_COMMANDS: readonly SlashCommand[] = [
  ...SLASH_BASIC_COMMANDS,
  ...SLASH_COMMON_COMMANDS,
  ...SLASH_ADVANCED_COMMANDS,
];

export const SLASH_BASIC_COUNT = SLASH_BASIC_COMMANDS.length;

/**
 * Parses a slash-command trigger immediately before `column` on `lineStr`.
 * Triggers only when `/` is at column 0 or preceded by whitespace, and the query
 * contains no whitespace or `/`.
 */
export function matchSlashTrigger(
  lineStr: string,
  column: number,
): { startColumn: number; endColumn: number; query: string } | undefined {
  if (column <= 0 || column > lineStr.length) return undefined;
  const before = lineStr.slice(0, column);
  const m = before.match(/(?:^|\s)\/([^\s/]*)$/);
  if (!m) return undefined;
  const query = m[1];
  const slashCol = column - query.length - 1;
  return { startColumn: slashCol, endColumn: column, query };
}

/**
 * Strict prefix matching against command `id`, localized `label` (or its whitespace-separated tokens),
 * and `keywords` (bilingual keywords + pinyin / pinyin-initial prefixes).
 */
export function matchesSlashCommand(cmd: SlashCommand, query: string): boolean {
  const q = query.trim().toLowerCase();
  if (!q) return true;
  if (cmd.id.toLowerCase().startsWith(q)) return true;
  const labelLower = slashCommandLabel(cmd).toLowerCase();
  if (labelLower.startsWith(q)) return true;
  for (const part of labelLower.split(/\s+/)) {
    if (part.startsWith(q)) return true;
  }
  for (const kw of cmd.keywords) {
    if (kw.toLowerCase().startsWith(q)) return true;
  }
  return false;
}

interface TintPalette {
  bg: string;
  fg: string;
}

function tintPalette(tint: SlashTint, dark: boolean): TintPalette {
  switch (tint) {
    case "ochre":
      return dark
        ? { bg: "rgba(217, 155, 76, 0.16)", fg: "#e2ae66" }
        : { bg: "rgba(163, 106, 40, 0.10)", fg: "#9a6324" };
    case "amber":
      return dark
        ? { bg: "rgba(232, 168, 56, 0.16)", fg: "#e8b44c" }
        : { bg: "rgba(194, 120, 3, 0.10)", fg: "#b4690e" };
    case "emerald":
      return dark
        ? { bg: "rgba(74, 186, 134, 0.16)", fg: "#5cc494" }
        : { bg: "rgba(22, 128, 86, 0.10)", fg: "#1a7a54" };
    case "indigo":
      return dark
        ? { bg: "rgba(124, 146, 224, 0.18)", fg: "#90a4e8" }
        : { bg: "rgba(67, 88, 178, 0.10)", fg: "#4458a8" };
    case "teal":
      return dark
        ? { bg: "rgba(68, 178, 178, 0.18)", fg: "#63c4c4" }
        : { bg: "rgba(24, 118, 124, 0.10)", fg: "#187076" };
    case "violet":
      return dark
        ? { bg: "rgba(164, 132, 224, 0.18)", fg: "#b596ec" }
        : { bg: "rgba(112, 74, 176, 0.10)", fg: "#6b48a8" };
    case "sky":
      return dark
        ? { bg: "rgba(86, 168, 224, 0.18)", fg: "#6db8eb" }
        : { bg: "rgba(28, 114, 178, 0.10)", fg: "#1c6da8" };
    default:
      return dark
        ? { bg: "rgba(255, 255, 255, 0.07)", fg: "#d1d5db" }
        : { bg: "rgba(15, 23, 42, 0.055)", fg: "#475569" };
  }
}

const ROW_HEIGHT = 42;
const ROW_GAP = 1;
const GROUP_GAP = 2;
const BASIC_HEADER_H = 22; // lineHeight 14 + paddingTop 4 + paddingBottom 4
const BASIC_STRIP_H = 36; // height 32 + paddingBottom 4
const COMMON_HEADER_H = 23; // lineHeight 14 + paddingTop 6 + paddingBottom 3
const ADVANCED_HEADER_H = 25; // lineHeight 14 + paddingTop 8 + paddingBottom 3
const MAX_LIST_HEIGHT = 340;
const SCROLL_VIEWPORT_H = MAX_LIST_HEIGHT - 12;

export function SlashPopover(props: {
  anchor: Bounds;
  query: string;
  items: readonly SlashCommand[];
  activeIndex: number;
  navSeq: number;
  onQueryChange: (query: string) => void;
  onActiveChange: (index: number) => void;
  onSelect: (cmd: SlashCommand) => void;
  onSubmit: () => void;
  onCancel: () => void;
  onEmptyBackspace: () => void;
  onLeadingSpace: () => void;
  onPointerInteract: () => void;
  onBlurOutside: () => void;
}) {
  const shown = signal(false);
  let scrollRef: ScrollRef | undefined;
  let inputRef: InputRef | undefined;
  let raf: number | undefined;
  let focusRaf: number | undefined;
  let blurTimer: number | undefined;
  let pointerInteractAt = 0;
  let scrollY = 0;
  let viewportH = SCROLL_VIEWPORT_H;
  let lastMousePage: { x: number; y: number } | undefined;
  let ignoreHoverUntilMouseMove = false;

  function handlePointerHover(index: number, pagePosition: { x: number; y: number }) {
    if (ignoreHoverUntilMouseMove) {
      if (
        lastMousePage &&
        Math.abs(pagePosition.x - lastMousePage.x) <= 1 &&
        Math.abs(pagePosition.y - lastMousePage.y) <= 1
      ) {
        return;
      }
      ignoreHoverUntilMouseMove = false;
    }
    lastMousePage = { x: pagePosition.x, y: pagePosition.y };
    props.onActiveChange(index);
  }

  // Trigger warm spring reveal and take keyboard focus so arrow/Enter/Tab keys are never
  // intercepted by the underlying Editor's native keymap.
  raf = requestAnimationFrame(() => {
    raf = undefined;
    shown.set(true);
  });
  focusRaf = requestAnimationFrame(() => {
    focusRaf = undefined;
    inputRef?.focus();
  });
  cleanup(() => {
    if (raf !== undefined) cancelAnimationFrame(raf);
    if (focusRaf !== undefined) cancelAnimationFrame(focusRaf);
    if (blurTimer !== undefined) clearTimeout(blurTimer);
  });

  // Scroll only on keyboard navigation or query changes (`navSeq`), never on mouse hover,
  // and lock synthetic hover events caused by rows scrolling under a stationary cursor.
  effect(() => {
    const seq = props.navSeq;
    if (seq === 0) return;
    ignoreHoverUntilMouseMove = true;
    untracked(() => {
      if (!scrollRef) return;
      const idx = props.activeIndex;
      const q = props.query.trim();
      if (!q && idx < SLASH_BASIC_COUNT) {
        if (scrollY > 0) {
          scrollY = 0;
          scrollRef.scrollTo({ x: 0, y: 0, animated: false });
        }
        return;
      }
      let itemTop = 0;
      if (!q) {
        const listIdx = idx - SLASH_BASIC_COUNT;
        const commonCount = SLASH_COMMON_COMMANDS.length;
        const commonStart = BASIC_HEADER_H + GROUP_GAP + BASIC_STRIP_H + GROUP_GAP + COMMON_HEADER_H + GROUP_GAP;
        if (listIdx < commonCount) {
          itemTop = commonStart + listIdx * (ROW_HEIGHT + ROW_GAP);
        } else {
          const commonGroupH = commonCount * ROW_HEIGHT + Math.max(0, commonCount - 1) * ROW_GAP;
          const advStart = commonStart + commonGroupH + GROUP_GAP + ADVANCED_HEADER_H + GROUP_GAP;
          itemTop = advStart + (listIdx - commonCount) * (ROW_HEIGHT + ROW_GAP);
        }
      } else {
        itemTop = idx * (ROW_HEIGHT + ROW_GAP);
      }
      const itemBottom = itemTop + ROW_HEIGHT;
      const margin = 6;
      if (itemTop < scrollY + margin) {
        const nextY = Math.max(0, itemTop - margin);
        scrollY = nextY;
        scrollRef.scrollTo({ x: 0, y: nextY, animated: false });
      } else if (itemBottom > scrollY + viewportH - margin) {
        const nextY = Math.max(0, itemBottom - viewportH + margin);
        scrollY = nextY;
        scrollRef.scrollTo({ x: 0, y: nextY, animated: false });
      }
    });
  });

  const isFiltering = () => props.query.trim().length > 0;
  const dark = () => colorScheme() === "dark";

  const renderGlyph = (cmd: SlashCommand, color: string, iconSize: number, badgeSize: number) => {
    if (cmd.icon) {
      return <Icon name={cmd.icon} size={iconSize} color={color} />;
    }
    return (
      <Text
        style={{
          fontSize: cmd.badge && cmd.badge.length >= 3 ? badgeSize - 1.5 : badgeSize,
          fontWeight: 600,
          color,
          lineHeight: badgeSize + 3,
        }}
      >
        {cmd.badge ?? ""}
      </Text>
    );
  };

  const renderRow = (cmd: SlashCommand, index: () => number) => {
    const active = () => props.activeIndex === index();
    const palette = () => tintPalette(cmd.tint, dark());
    return (
      <View
        testID={`slash-item-${cmd.id}`}
        onPress={() => props.onSelect(cmd)}
        onMouseEnter={(e) => handlePointerHover(index(), e.pagePosition)}
        class={active() ? "bg-accent" : "hover:bg-muted/70"}
        style={{
          height: ROW_HEIGHT,
          paddingLeft: 8,
          paddingRight: 10,
          borderRadius: 8,
          flexDirection: "row",
          alignItems: "center",
          gap: 10,
          ...colorTransition(110),
        }}
      >
        <View
          style={{
            width: 26,
            height: 26,
            borderRadius: 7,
            backgroundColor: palette().bg,
            alignItems: "center",
            justifyContent: "center",
            flexShrink: 0,
          }}
        >
          {renderGlyph(cmd, palette().fg, 14, 12.5)}
        </View>
        <View style={{ flex: 1, minWidth: 0, gap: 1 }}>
          <Text
            style={{ fontSize: 12.5, fontWeight: 500, lineHeight: 16, numberOfLines: 1, textOverflow: "ellipsis" }}
            class="text-foreground"
          >
            {slashCommandLabel(cmd)}
          </Text>
          <Text
            style={{ fontSize: 11, lineHeight: 14, numberOfLines: 1, textOverflow: "ellipsis" }}
            class="text-muted-foreground"
          >
            {slashCommandDescription(cmd)}
          </Text>
        </View>
        <If when={Boolean(cmd.shortcut)}>
          <View style={{ flexShrink: 0 }}>
            <Kbd keys={cmd.shortcut!} faint />
          </View>
        </If>
      </View>
    );
  };

  return (
    <Overlay anchor={props.anchor} placement="bottom" align="start" offset={6} margin={10}>
      <View
        testID="slash-popover"
        onMouseMove={(e) => {
          if (
            !lastMousePage ||
            Math.abs(e.pagePosition.x - lastMousePage.x) > 1 ||
            Math.abs(e.pagePosition.y - lastMousePage.y) > 1
          ) {
            lastMousePage = { x: e.pagePosition.x, y: e.pagePosition.y };
            ignoreHoverUntilMouseMove = false;
          }
        }}
        onResponderGrant={() => {
          pointerInteractAt = Date.now();
          props.onPointerInteract();
          return false;
        }}
        class="bg-popover border border-border"
        style={{
          width: 288,
          maxHeight: MAX_LIST_HEIGHT + 36,
          borderRadius: 12,
          padding: 6,
          overflow: "hidden",
          boxShadow: popoverShadow(),
          opacity: shown() ? 1 : 0,
          transform: reduceMotion()
            ? undefined
            : shown()
              ? "translateY(0px) scale(1)"
              : "translateY(-4px) scale(0.985)",
          ...overlayTransition(),
        }}
      >
        {/* Borderless quiet filter header; receives focus so arrow/Enter/Tab never fight Editor */}
        <View
          class="border-b border-border/70"
          style={{
            flexDirection: "row",
            alignItems: "center",
            height: 30,
            paddingLeft: 8,
            paddingRight: 6,
            marginBottom: 4,
            gap: 4,
          }}
        >
          <Text style={{ fontSize: 12.5, fontWeight: 600 }} class="text-primary">
            /
          </Text>
          <Input
            ref={(r) => {
              inputRef = r;
            }}
            testID="slash-filter-input"
            value={props.query}
            placeholder={t("slash.placeholder")}
            autoCapitalize="none"
            autoCorrect={false}
            submitBehavior="submit"
            onChange={(e) => {
              if (e.value === " ") {
                props.onLeadingSpace();
                return;
              }
              props.onQueryChange(e.value);
            }}
            onSubmit={() => props.onSubmit()}
            onKeyPress={(e) => {
              if (e.key === "Escape") {
                props.onCancel();
              } else if (e.key === "Backspace" && props.query.length === 0) {
                props.onEmptyBackspace();
              }
            }}
            onBlur={() => {
              if (blurTimer !== undefined) clearTimeout(blurTimer);
              const delay = Date.now() - pointerInteractAt < 250 ? 150 : 60;
              blurTimer = setTimeout(() => {
                blurTimer = undefined;
                props.onBlurOutside();
              }, delay) as unknown as number;
            }}
            class="flex-1 h-6 bg-transparent text-[12.5px] text-foreground placeholder:text-muted-foreground/65"
            style={{ borderWidth: 0, paddingLeft: 2, paddingRight: 4 }}
          />
        </View>
        <Scroll
          ref={(r) => {
            scrollRef = r;
          }}
          style={{ maxHeight: SCROLL_VIEWPORT_H }}
          onScroll={(e) => {
            scrollY = e.offset.y;
            if (e.size.height > 0) viewportH = e.size.height;
            ignoreHoverUntilMouseMove = true;
          }}
        >
          <If
            when={!isFiltering()}
            else={
              <If
                when={props.items.length > 0}
                else={
                  <View style={{ paddingTop: 14, paddingBottom: 14, alignItems: "center" }}>
                    <Text style={{ fontSize: 12 }} class="text-muted-foreground">
                      {t("slash.noMatch")}
                    </Text>
                  </View>
                }
              >
                <View style={{ gap: ROW_GAP }}>
                  <Each of={props.items} key={(cmd) => cmd.id}>
                    {(cmd, idx) => renderRow(cmd(), idx)}
                  </Each>
                </View>
              </If>
            }
          >
            <View style={{ gap: GROUP_GAP }}>
              {/* Top compact 6-icon "基础" strip */}
              <Text
                style={{
                  fontSize: 11,
                  fontWeight: 500,
                  lineHeight: 14,
                  paddingLeft: 8,
                  paddingTop: 4,
                  paddingBottom: 4,
                }}
                class="text-muted-foreground"
              >
                {t("slash.group.basic")}
              </Text>
              <View style={{ flexDirection: "row", gap: 4, paddingLeft: 2, paddingRight: 2, paddingBottom: 4 }}>
                <Each of={SLASH_BASIC_COMMANDS} key={(cmd) => cmd.id}>
                  {(cmd, idx) => {
                    const active = () => props.activeIndex === idx();
                    return (
                      <View
                        testID={`slash-item-${cmd().id}`}
                        onPress={() => props.onSelect(cmd())}
                        onMouseEnter={(e) => handlePointerHover(idx(), e.pagePosition)}
                        tooltip={tip(slashCommandLabel(cmd()), cmd().shortcut)}
                        class={active() ? "bg-accent" : "hover:bg-muted/75"}
                        style={{
                          flex: 1,
                          height: 32,
                          borderRadius: 7,
                          alignItems: "center",
                          justifyContent: "center",
                          ...colorTransition(110),
                        }}
                      >
                        {renderGlyph(
                          cmd(),
                          dark() ? "#e5e7eb" : "#334155",
                          14.5,
                          12.5,
                        )}
                      </View>
                    );
                  }}
                </Each>
              </View>

              {/* Common section */}
              <Text
                style={{
                  fontSize: 11,
                  fontWeight: 500,
                  lineHeight: 14,
                  paddingLeft: 8,
                  paddingTop: 6,
                  paddingBottom: 3,
                }}
                class="text-muted-foreground"
              >
                {t("slash.group.common")}
              </Text>
              <View style={{ gap: 1 }}>
                <Each of={SLASH_COMMON_COMMANDS} key={(cmd) => cmd.id}>
                  {(cmd, idx) => renderRow(cmd(), () => SLASH_BASIC_COUNT + idx())}
                </Each>
              </View>

              {/* Advanced / statement structure section */}
              <Text
                style={{
                  fontSize: 11,
                  fontWeight: 500,
                  lineHeight: 14,
                  paddingLeft: 8,
                  paddingTop: 8,
                  paddingBottom: 3,
                }}
                class="text-muted-foreground"
              >
                {t("slash.group.advanced")}
              </Text>
              <View style={{ gap: 1 }}>
                <Each of={SLASH_ADVANCED_COMMANDS} key={(cmd) => cmd.id}>
                  {(cmd, idx) =>
                    renderRow(cmd(), () => SLASH_BASIC_COUNT + SLASH_COMMON_COMMANDS.length + idx())
                  }
                </Each>
              </View>
            </View>
          </If>
        </Scroll>
      </View>
    </Overlay>
  );
}

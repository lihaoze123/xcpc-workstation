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
import { tip } from "../kit";
import { colorTransition, overlayTransition } from "../motion";
import { popoverShadow } from "../theme";

export type SlashCommandGroup = "basic" | "common" | "advanced";

export type SlashTint = "neutral" | "ochre" | "amber" | "emerald" | "indigo" | "teal" | "violet" | "sky";

export interface SlashCommand {
  readonly id: string;
  readonly group: SlashCommandGroup;
  readonly label: string;
  readonly description: string;
  readonly keywords: readonly string[];
  readonly icon?: IconName;
  readonly badge?: string;
  readonly tint: SlashTint;
  readonly kind?: "image";
  readonly template?: string;
  readonly caret?: { readonly line: number; readonly column: number };
}

export const SLASH_BASIC_COMMANDS: readonly SlashCommand[] = [
  {
    id: "h2",
    group: "basic",
    label: "二级标题",
    description: "新增题面或题解章节",
    keywords: ["h2", "heading", "title", "section", "biaoti", "bt", "zhangjie", "zj"],
    badge: "H2",
    tint: "neutral",
    template: "## ",
    caret: { line: 0, column: 3 },
  },
  {
    id: "h3",
    group: "basic",
    label: "三级标题",
    description: "章节下的子标题",
    keywords: ["h3", "subheading", "zibiaoti", "zbt"],
    badge: "H3",
    tint: "neutral",
    template: "### ",
    caret: { line: 0, column: 4 },
  },
  {
    id: "ul",
    group: "basic",
    label: "无序列表",
    description: "项目符号列表",
    keywords: ["ul", "list", "bullet", "liebiao", "lb"],
    icon: "list",
    tint: "neutral",
    template: "- ",
    caret: { line: 0, column: 2 },
  },
  {
    id: "ol",
    group: "basic",
    label: "有序列表",
    description: "数字编号列表",
    keywords: ["ol", "numbered", "ordered", "youxu", "yx"],
    icon: "listOrdered",
    tint: "neutral",
    template: "1. ",
    caret: { line: 0, column: 3 },
  },
  {
    id: "quote",
    group: "basic",
    label: "引用块",
    description: "引用说明或摘录",
    keywords: ["quote", "blockquote", "yinyong", "yy"],
    badge: "❝",
    tint: "neutral",
    template: "> ",
    caret: { line: 0, column: 2 },
  },
  {
    id: "inline-math",
    group: "basic",
    label: "行内公式",
    description: "正文行内 LaTeX 公式",
    keywords: ["inline", "math", "formula", "$", "hangnei", "hn", "gongshi", "gs"],
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
    label: "行间公式",
    description: "居中显示的独立公式块",
    keywords: ["math", "formula", "equation", "display", "gongshi", "gs", "$$"],
    icon: "sigma",
    tint: "ochre",
    template: "$$\n\n$$",
    caret: { line: 1, column: 0 },
  },
  {
    id: "image",
    group: "common",
    label: "插入图片",
    description: "从素材库选择或导入",
    keywords: ["image", "img", "pic", "picture", "figure", "tupian", "tp"],
    icon: "image",
    tint: "amber",
    kind: "image",
  },
  {
    id: "table",
    group: "common",
    label: "Markdown 表格",
    description: "标准数据表格",
    keywords: ["table", "grid", "markdown", "biaoge", "bg"],
    icon: "table",
    tint: "emerald",
    template: "| 列 1 | 列 2 |\n| --- | --- |\n|  |  |",
    caret: { line: 2, column: 2 },
  },
  {
    id: "cpp",
    group: "common",
    label: "C++ 代码块",
    description: "C++ 源代码块",
    keywords: ["cpp", "c++", "code", "source", "daima", "dm"],
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
    label: "分段函数",
    description: "cases 分段公式",
    keywords: ["cases", "piecewise", "fenduan", "fd", "math"],
    badge: "{",
    tint: "ochre",
    template: "$$\nf(x) = \\begin{cases}\n  , & x \\le 0 \\\\\n  , & x > 0\n\\end{cases}\n$$",
    caret: { line: 2, column: 2 },
  },
  {
    id: "matrix",
    group: "advanced",
    label: "矩阵",
    description: "bmatrix 方括号矩阵",
    keywords: ["matrix", "bmatrix", "juzhen", "jz", "math"],
    badge: "[ ]",
    tint: "ochre",
    template: "$$\n\\begin{bmatrix}\n  a & b \\\\\n  c & d\n\\end{bmatrix}\n$$",
    caret: { line: 2, column: 2 },
  },
  {
    id: "subtasks",
    group: "advanced",
    label: "子任务评分表",
    description: "Subtask 分值与特殊性质",
    keywords: ["subtask", "subtasks", "score", "zirenwu", "zrw", "table"],
    icon: "listPlus",
    tint: "teal",
    template:
      "| 子任务 | 分值 | 特殊性质 |\n| :---: | :---: | --- |\n| 1 | 20 | $n \\le 10$ |\n| 2 | 30 | $n \\le 1000$ |\n| 3 | 50 | 无特殊限制 |",
    caret: { line: 2, column: 15 },
  },
  {
    id: "hint",
    group: "advanced",
    label: "提示章节",
    description: "提示与数据范围章节",
    keywords: ["hint", "note", "tishi", "ts", "tszj", "h2", "section"],
    icon: "info",
    tint: "teal",
    template: "## 提示\n\n",
    caret: { line: 2, column: 0 },
  },
  {
    id: "interaction",
    group: "advanced",
    label: "交互章节",
    description: "交互题协议说明章节",
    keywords: ["interaction", "interactive", "protocol", "jiaohu", "jh", "jhzj", "h2", "section"],
    icon: "bot",
    tint: "violet",
    template: "## 交互方式\n\n",
    caret: { line: 2, column: 0 },
  },
  {
    id: "typst",
    group: "advanced",
    label: "原始 Typst 块",
    description: "直接绘图或排版",
    keywords: ["typst", "raw", "vector", "diagram", "tu", "yst"],
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
 * Strict prefix matching against command `id`, `label` (or its whitespace-separated tokens),
 * and `keywords` (English keywords + pinyin / pinyin-initial prefixes).
 */
export function matchesSlashCommand(cmd: SlashCommand, query: string): boolean {
  const q = query.trim().toLowerCase();
  if (!q) return true;
  if (cmd.id.toLowerCase().startsWith(q)) return true;
  const labelLower = cmd.label.toLowerCase();
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
const SECTION_HEADER_HEIGHT = 26;
const BASIC_SECTION_HEIGHT = SECTION_HEADER_HEIGHT + 36;
const MAX_LIST_HEIGHT = 340;

export function SlashPopover(props: {
  anchor: Bounds;
  query: string;
  items: readonly SlashCommand[];
  activeIndex: number;
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

  // Keep the keyboard-selected row smoothly visible inside the scroll container.
  effect(() => {
    const idx = props.activeIndex;
    const q = props.query.trim();
    untracked(() => {
      if (!scrollRef) return;
      if (!q && idx < SLASH_BASIC_COUNT) {
        scrollRef.scrollTo({ x: 0, y: 0, animated: false });
        return;
      }
      let itemTop = 0;
      if (!q) {
        const listIdx = idx - SLASH_BASIC_COUNT;
        const commonCount = SLASH_COMMON_COMMANDS.length;
        if (listIdx < commonCount) {
          itemTop = BASIC_SECTION_HEIGHT + SECTION_HEADER_HEIGHT + listIdx * ROW_HEIGHT;
        } else {
          itemTop =
            BASIC_SECTION_HEIGHT +
            SECTION_HEADER_HEIGHT +
            commonCount * ROW_HEIGHT +
            SECTION_HEADER_HEIGHT +
            (listIdx - commonCount) * ROW_HEIGHT;
        }
      } else {
        itemTop = idx * ROW_HEIGHT;
      }
      const itemBottom = itemTop + ROW_HEIGHT;
      const visibleH = MAX_LIST_HEIGHT - 12;
      if (itemBottom > visibleH) {
        scrollRef.scrollTo({ x: 0, y: Math.max(0, itemBottom - visibleH + 8), animated: false });
      } else if (itemTop < 48) {
        scrollRef.scrollTo({ x: 0, y: 0, animated: false });
      }
    });
  });

  const isFiltering = () => props.query.trim().length > 0;
  const dark = () => colorScheme() === "dark";

  const renderGlyph = (cmd: SlashCommand, color: string, iconSize: number, badgeSize: number) => {
    if (cmd.icon) {
      return <Icon name={cmd.icon} size={iconSize} style={{ tintColor: color }} />;
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
        onMouseEnter={() => props.onActiveChange(index())}
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
            {cmd.label}
          </Text>
          <Text
            style={{ fontSize: 11, lineHeight: 14, numberOfLines: 1, textOverflow: "ellipsis" }}
            class="text-muted-foreground"
          >
            {cmd.description}
          </Text>
        </View>
      </View>
    );
  };

  return (
    <Overlay anchor={props.anchor} placement="bottom" align="start" offset={6} margin={10}>
      <View
        testID="slash-popover"
        onResponderGrant={() => {
          pointerInteractAt = Date.now();
          props.onPointerInteract();
          return false;
        }}
        class="bg-popover border border-border"
        style={{
          width: 276,
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
            placeholder="筛选命令…"
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
          style={{ maxHeight: MAX_LIST_HEIGHT - 12 }}
        >
          <If
            when={!isFiltering()}
            else={
              <If
                when={props.items.length > 0}
                else={
                  <View style={{ paddingTop: 14, paddingBottom: 14, alignItems: "center" }}>
                    <Text style={{ fontSize: 12 }} class="text-muted-foreground">
                      无匹配命令
                    </Text>
                  </View>
                }
              >
                <View style={{ gap: 1 }}>
                  <Each of={props.items} key={(cmd) => cmd.id}>
                    {(cmd, idx) => renderRow(cmd(), idx)}
                  </Each>
                </View>
              </If>
            }
          >
            <View style={{ gap: 2 }}>
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
                基础
              </Text>
              <View style={{ flexDirection: "row", gap: 4, paddingLeft: 2, paddingRight: 2, paddingBottom: 4 }}>
                <Each of={SLASH_BASIC_COMMANDS} key={(cmd) => cmd.id}>
                  {(cmd, idx) => {
                    const active = () => props.activeIndex === idx();
                    return (
                      <View
                        testID={`slash-item-${cmd().id}`}
                        onPress={() => props.onSelect(cmd())}
                        onMouseEnter={() => props.onActiveChange(idx())}
                        tooltip={tip(cmd().label)}
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
                常用
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
                题面与进阶
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

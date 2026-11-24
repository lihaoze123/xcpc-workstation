// Read-only keyboard shortcut reference (§9.6, §9.1).
// Exports reusable data (`SHORTCUT_GROUPS`, `filterShortcutGroups`), the embeddable
// `ShortcutsList` (for Settings → 快捷键), and `ShortcutsModal` (global Ctrl+/ overlay).

import { computed, platform, Scroll, signal, Text, View } from "nsh";
import { Input } from "nsh/base";
import { t, type MessageKey } from "./i18n";
import { Icon } from "./Icon";
import { IconBtn, Kbd, keyLabel } from "./kit";
import { Modal } from "./Modal";
import { cardShadow } from "./theme";

export interface ShortcutItem {
  readonly id: string;
  readonly label: string;
  /** Optional context badge to disambiguate identical shortcuts in different scopes (e.g. `Ctrl+F`). */
  readonly context?: string;
  /** Standard NSH keystroke string (formatted via `keymap.label`). */
  readonly keys?: string;
  /** Non-keystroke gesture label (e.g. Ctrl+点击 / Cmd+Click) when `keys` is not applicable. */
  readonly gesture?: { readonly mac: string; readonly other: string };
  readonly keywords?: readonly string[];
}

export interface ShortcutGroup {
  readonly id: string;
  readonly title: string;
  readonly items: readonly ShortcutItem[];
}

interface ShortcutItemDef {
  readonly id: string;
  readonly labelKey: MessageKey;
  readonly contextKey?: MessageKey;
  readonly contextLiteral?: string;
  readonly keys?: string;
  readonly gestureKeys?: { readonly mac: MessageKey; readonly other: MessageKey };
  readonly gestureLiteral?: { readonly mac: string; readonly other: string };
  readonly keywords?: readonly string[];
}

interface ShortcutGroupDef {
  readonly id: string;
  readonly titleKey: MessageKey;
  readonly items: readonly ShortcutItemDef[];
}

const SHORTCUT_GROUP_DEFS: readonly ShortcutGroupDef[] = [
  {
    id: "global",
    titleKey: "shortcuts.group.global",
    items: [
      { id: "palette", labelKey: "shortcuts.item.palette", keys: "mod+k", keywords: ["command", "search", "jump", "命令", "跳转"] },
      { id: "shortcuts", labelKey: "shortcuts.item.shortcuts", keys: "mod+/", keywords: ["shortcut", "help", "key", "帮助", "快捷键"] },
      { id: "space-problems", labelKey: "shortcuts.item.spaceProblems", contextKey: "shortcuts.ctx.home", keys: "mod+1", keywords: ["library", "problems", "题库"] },
      { id: "space-contests", labelKey: "shortcuts.item.spaceContests", contextKey: "shortcuts.ctx.home", keys: "mod+2", keywords: ["contests", "比赛"] },
      { id: "space-training", labelKey: "shortcuts.item.spaceTraining", contextKey: "shortcuts.ctx.home", keys: "mod+3", keywords: ["training", "dashboard", "训练", "看板"] },
      { id: "space-settings", labelKey: "shortcuts.item.spaceSettings", contextKey: "shortcuts.ctx.home", keys: "mod+4", keywords: ["settings", "设置"] },
      { id: "open-workspace", labelKey: "shortcuts.item.openWorkspace", keys: "mod+o", keywords: ["workspace", "folder", "工作区"] },
      { id: "escape", labelKey: "shortcuts.item.escape", keys: "escape", keywords: ["esc", "close", "back", "退出", "关闭"] },
    ],
  },
  {
    id: "workspace",
    titleKey: "shortcuts.group.workspace",
    items: [
      { id: "new-problem", labelKey: "shortcuts.item.newProblem", keys: "mod+n", keywords: ["problem", "create", "新建", "题目"] },
      { id: "new-contest", labelKey: "shortcuts.item.newContest", keys: "mod+shift+n", keywords: ["contest", "create", "新建", "比赛"] },
      { id: "list-search", labelKey: "shortcuts.item.listSearch", contextKey: "shortcuts.ctx.lists", keys: "mod+f", keywords: ["find", "filter", "搜索", "筛选"] },
      { id: "list-next", labelKey: "shortcuts.item.listNext", keys: "j", keywords: ["down", "next", "下移"] },
      { id: "list-prev", labelKey: "shortcuts.item.listPrev", keys: "k", keywords: ["up", "prev", "上移"] },
    ],
  },
  {
    id: "editor",
    titleKey: "shortcuts.group.editor",
    items: [
      { id: "tab-statement", labelKey: "shortcuts.item.tabStatement", keys: "alt+1", keywords: ["statement", "题面"] },
      { id: "tab-samples", labelKey: "shortcuts.item.tabSamples", keys: "alt+2", keywords: ["samples", "样例"] },
      { id: "tab-editorial", labelKey: "shortcuts.item.tabEditorial", keys: "alt+3", keywords: ["editorial", "题解"] },
      { id: "build-verify", labelKey: "shortcuts.item.buildVerify", contextKey: "shortcuts.ctx.buildTab", keys: "mod+b", keywords: ["build", "verify", "judge", "构建", "验证"] },
      { id: "inspector", labelKey: "shortcuts.item.inspector", keys: "mod+i", keywords: ["inspector", "properties", "属性"] },
      { id: "focus-mode", labelKey: "shortcuts.item.focusMode", keys: "mod+shift+f", keywords: ["focus", "zen", "专注"] },
      { id: "save", labelKey: "shortcuts.item.save", keys: "mod+s", keywords: ["save", "保存"] },
      { id: "export-pdf", labelKey: "shortcuts.item.exportPdf", keys: "mod+e", keywords: ["pdf", "export", "导出"] },
      { id: "editor-find", labelKey: "shortcuts.item.editorFind", contextKey: "shortcuts.ctx.sourceEditor", keys: "mod+f", keywords: ["find", "replace", "查找", "替换"] },
      { id: "heading-1", labelKey: "shortcuts.item.heading1", contextKey: "shortcuts.ctx.sourceEditor", keys: "mod+1", keywords: ["h1", "heading", "title", "标题"] },
      { id: "heading-2", labelKey: "shortcuts.item.heading2", contextKey: "shortcuts.ctx.sourceEditor", keys: "mod+2", keywords: ["h2", "heading", "section", "标题", "章节"] },
      { id: "heading-3", labelKey: "shortcuts.item.heading3", contextKey: "shortcuts.ctx.sourceEditor", keys: "mod+3", keywords: ["h3", "heading", "标题"] },
      { id: "heading-4", labelKey: "shortcuts.item.heading4", contextKey: "shortcuts.ctx.sourceEditor", keys: "mod+4", keywords: ["h4", "heading", "标题"] },
      { id: "bold", labelKey: "shortcuts.item.bold", keys: "mod+b", keywords: ["bold", "markdown", "粗体", "加粗"] },
      { id: "italic", labelKey: "shortcuts.item.italic", keys: "mod+shift+i", keywords: ["italic", "markdown", "斜体"] },
      { id: "inline-code", labelKey: "shortcuts.item.inlineCode", keys: "mod+`", keywords: ["code", "inline", "backtick", "代码"] },
      { id: "math-inline", labelKey: "shortcuts.item.mathInline", keys: "mod+m", keywords: ["math", "latex", "tex", "公式"] },
      { id: "math-block", labelKey: "shortcuts.item.mathBlock", keys: "mod+shift+m", keywords: ["math", "block", "latex", "公式块"] },
      { id: "list-ul", labelKey: "shortcuts.item.listUl", keys: "mod+shift+8", keywords: ["ul", "list", "bullet", "无序", "列表"] },
      { id: "list-ol", labelKey: "shortcuts.item.listOl", keys: "mod+shift+7", keywords: ["ol", "list", "ordered", "有序", "列表"] },
      { id: "blockquote", labelKey: "shortcuts.item.blockquote", keys: "mod+shift+.", keywords: ["quote", "blockquote", "引用"] },
    ],
  },
  {
    id: "preview",
    titleKey: "shortcuts.group.preview",
    items: [
      { id: "zoom-in", labelKey: "shortcuts.item.zoomIn", keys: "mod+plus", keywords: ["zoom", "scale", "放大"] },
      { id: "zoom-out", labelKey: "shortcuts.item.zoomOut", keys: "mod+minus", keywords: ["zoom", "scale", "缩小"] },
      { id: "fit-width", labelKey: "shortcuts.item.fitWidth", keys: "mod+0", keywords: ["fit", "reset", "适合宽度"] },
      { id: "locate-preview", labelKey: "shortcuts.item.locatePreview", keys: "mod+shift+l", keywords: ["sync", "locate", "定位"] },
      {
        id: "locate-source",
        labelKey: "shortcuts.item.locateSource",
        gestureKeys: { mac: "shortcuts.gesture.locateSourceMac", other: "shortcuts.gesture.locateSourceOther" },
        keywords: ["click", "source", "jump", "跳回源码"],
      },
    ],
  },
  {
    id: "vim",
    titleKey: "shortcuts.group.vim",
    items: [
      {
        id: "vim-escape",
        labelKey: "shortcuts.item.vimEscape",
        contextLiteral: "Insert / Visual",
        gestureLiteral: { mac: "Esc / Ctrl+[", other: "Esc / Ctrl+[" },
        keywords: ["vim", "normal", "escape", "ctrl+["],
      },
      {
        id: "vim-motion",
        labelKey: "shortcuts.item.vimMotion",
        contextLiteral: "Normal / Visual",
        gestureLiteral: { mac: "h j k l / w b e / 0 $ / gg G", other: "h j k l / w b e / 0 $ / gg G" },
        keywords: ["vim", "motion", "hjkl", "word", "移动"],
      },
      {
        id: "vim-insert",
        labelKey: "shortcuts.item.vimInsert",
        contextLiteral: "Normal",
        gestureLiteral: { mac: "i a I A o O", other: "i a I A o O" },
        keywords: ["vim", "insert", "插入"],
      },
      {
        id: "vim-edit",
        labelKey: "shortcuts.item.vimEdit",
        contextLiteral: "Normal",
        gestureLiteral: { mac: "x / dd / yy / p P", other: "x / dd / yy / p P" },
        keywords: ["vim", "delete", "yank", "paste", "dd", "yy"],
      },
      {
        id: "vim-undo",
        labelKey: "shortcuts.item.vimUndo",
        contextLiteral: "Normal",
        gestureLiteral: { mac: "u / Ctrl+r", other: "u / Ctrl+r" },
        keywords: ["vim", "undo", "redo", "撤销", "重做"],
      },
      {
        id: "vim-visual",
        labelKey: "shortcuts.item.vimVisual",
        contextLiteral: "Normal",
        gestureKeys: { mac: "shortcuts.gesture.vimVisual", other: "shortcuts.gesture.vimVisual" },
        keywords: ["vim", "visual", "select", "选择"],
      },
      {
        id: "vim-text-obj",
        labelKey: "shortcuts.item.vimTextObj",
        contextLiteral: "Normal / Visual",
        gestureLiteral: { mac: "vi / ci / di / yi + w $ () \"\" '' ``", other: "vi / ci / di / yi + w $ () \"\" '' ``" },
        keywords: ["vim", "text object", "viw", "ciw", "ci$", "文本对象"],
      },
    ],
  },
];

/** Resolves all shortcut groups in the active `locale()`. */
export function getShortcutGroups(): ShortcutGroup[] {
  return SHORTCUT_GROUP_DEFS.map((g) => ({
    id: g.id,
    title: t(g.titleKey),
    items: g.items.map((it) => ({
      id: it.id,
      label: t(it.labelKey),
      context: it.contextKey ? t(it.contextKey) : it.contextLiteral,
      keys: it.keys,
      gesture: it.gestureKeys
        ? { mac: t(it.gestureKeys.mac), other: t(it.gestureKeys.other) }
        : it.gestureLiteral,
      keywords: it.keywords,
    })),
  }));
}

/** Reactive open state for the global `Ctrl+/` shortcuts overlay. */
export const shortcutsOpen = signal(false);

function formatGesture(gesture: { readonly mac: string; readonly other: string }): string {
  return platform.os === "macos" ? gesture.mac : gesture.other;
}

function itemKeyText(item: ShortcutItem): string {
  if (item.keys) return keyLabel(item.keys);
  if (item.gesture) return formatGesture(item.gesture);
  return "";
}

/** Filters shortcut groups by label, group title, formatted shortcut, or keywords. */
export function filterShortcutGroups(query: string): ShortcutGroup[] {
  const all = getShortcutGroups();
  const q = query.trim().toLowerCase();
  if (!q) return all;
  const out: ShortcutGroup[] = [];
  for (const group of all) {
    const groupMatch = group.title.toLowerCase().includes(q);
    const items = groupMatch
      ? group.items
      : group.items.filter((item) => {
          if (item.label.toLowerCase().includes(q)) return true;
          if (item.context && item.context.toLowerCase().includes(q)) return true;
          if (item.keys && item.keys.toLowerCase().includes(q)) return true;
          if (itemKeyText(item).toLowerCase().includes(q)) return true;
          return (item.keywords ?? []).some((k) => k.toLowerCase().includes(q));
        });
    if (items.length > 0) out.push({ ...group, items });
  }
  return out;
}

function GestureKbd(props: { label: string }) {
  return (
    <View
      style={{ height: 20, paddingLeft: 6, paddingRight: 6, borderRadius: 5, borderWidth: 1, justifyContent: "center" }}
      class="border-border bg-muted"
    >
      <Text style={{ fontSize: 11, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
        {props.label}
      </Text>
    </View>
  );
}

/**
 * Reusable shortcut reference list with top search filter and
 * grouped rounded cards. Embeds cleanly in Settings → 快捷键 or inside `ShortcutsModal`.
 */
export function ShortcutsList(props: {
  query?: string;
  onQueryChange?: (q: string) => void;
  autoFocus?: boolean;
  maxHeight?: number;
}) {
  const internalQuery = signal("");
  const query = () => props.query ?? internalQuery();
  const setQuery = (next: string) => {
    internalQuery.set(next);
    props.onQueryChange?.(next);
  };

  const groups = computed(() => filterShortcutGroups(query()));

  const listContent = (
    <View style={{ gap: 16 }}>
      <If
        when={groups().length > 0}
        else={
          <View style={{ height: 96, alignItems: "center", justifyContent: "center", gap: 6 }}>
            <Icon name="searchX" size={18} tone="muted-foreground" />
            <Text style={{ fontSize: 13 }} class="text-muted-foreground">
              {t("common.searchNotFound", { query: query().trim() })}
            </Text>
          </View>
        }
      >
        <Each of={groups()} key={(g) => g.id}>
          {(group) => (
            <View style={{ gap: 6 }}>
              <Text style={{ fontSize: 12, fontWeight: 500, paddingLeft: 4 }} class="text-muted-foreground">
                {group().title}
              </Text>
              <View
                style={{
                  borderRadius: 10,
                  borderWidth: 1,
                  overflow: "hidden",
                  boxShadow: cardShadow(),
                }}
                class="bg-card border-border"
              >
                <Each of={group().items} key={(it) => it.id}>
                  {(item, idx) => (
                    <View>
                      <If when={idx() > 0}>
                        <View style={{ height: 1, marginLeft: 14, marginRight: 14 }} class="bg-border" />
                      </If>
                      <View
                        style={{
                          height: 38,
                          paddingLeft: 14,
                          paddingRight: 14,
                          flexDirection: "row",
                          alignItems: "center",
                          justifyContent: "space-between",
                          gap: 12,
                        }}
                      >
                        <View style={{ flexDirection: "row", alignItems: "center", gap: 8, flexShrink: 1, minWidth: 0 }}>
                          <Text style={{ fontSize: 13, lineHeight: 18, flexShrink: 1, numberOfLines: 1, textOverflow: "ellipsis" }} class="text-foreground">
                            {item().label}
                          </Text>
                          {item().context && (
                            <View
                              style={{ height: 18, paddingLeft: 6, paddingRight: 6, borderRadius: 4, justifyContent: "center", flexShrink: 0 }}
                              class="bg-muted"
                            >
                              <Text style={{ fontSize: 11, lineHeight: 14 }} class="text-muted-foreground">
                                {item().context}
                              </Text>
                            </View>
                          )}
                        </View>
                        <View style={{ flexShrink: 0 }}>
                          {item().keys ? (
                            <Kbd keys={item().keys!} />
                          ) : item().gesture ? (
                            <GestureKbd label={formatGesture(item().gesture!)} />
                          ) : null}
                        </View>
                      </View>
                    </View>
                  )}
                </Each>
              </View>
            </View>
          )}
        </Each>
      </If>
    </View>
  );

  return (
    <View style={{ gap: 14 }}>
      <View
        class="bg-muted border border-border focus-within:border-input"
        style={{
          height: 34,
          borderRadius: 8,
          paddingLeft: 10,
          paddingRight: 8,
          flexDirection: "row",
          alignItems: "center",
          gap: 8,
        }}
      >
        <Icon name="search" size={14} tone="muted-foreground" />
        <Input
          testID="shortcuts-search"
          class="bg-transparent placeholder:text-muted-foreground"
          style={{ flex: 1, height: 28, fontSize: 13 }}
          value={query()}
          placeholder={t("shortcuts.searchPlaceholder")}
          autoCapitalize="none"
          autoCorrect={false}
          autoFocus={props.autoFocus}
          onChange={(e) => setQuery(e.value)}
        />
        <If when={query().length > 0}>
          <IconBtn icon="x" size={22} iconSize={13} tone="muted-foreground" onPress={() => setQuery("")} />
        </If>
      </View>

      {props.maxHeight ? (
        <Scroll style={{ maxHeight: props.maxHeight }} contentStyle={{ paddingBottom: 4 }}>
          {listContent}
        </Scroll>
      ) : (
        listContent
      )}
    </View>
  );
}

/** Global `Ctrl+/` keyboard shortcuts overlay (`Esc` closes). */
export function ShortcutsModal(props?: { open?: boolean; onClose?: () => void }) {
  const isOpen = () => props?.open ?? shortcutsOpen();
  const close = () => {
    if (props?.onClose) props.onClose();
    else shortcutsOpen.set(false);
  };

  return (
    <Modal open={isOpen()} onClose={close} width={560} testID="shortcuts-modal">
      <View style={{ padding: 18, gap: 14 }}>
        <View style={{ flexDirection: "row", alignItems: "center", justifyContent: "space-between", gap: 12 }}>
          <View style={{ flexDirection: "row", alignItems: "center", gap: 10 }}>
            <Text style={{ fontSize: 15, lineHeight: 20, fontWeight: 600 }} class="text-foreground">
              {t("shortcuts.title")}
            </Text>
            <Kbd keys="mod+/" faint />
          </View>
          <IconBtn icon="x" size={26} iconSize={15} tone="muted-foreground" onPress={close} />
        </View>
        <ShortcutsList autoFocus maxHeight={460} />
      </View>
    </Modal>
  );
}

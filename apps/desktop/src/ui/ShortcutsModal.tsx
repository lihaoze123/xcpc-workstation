// Read-only keyboard shortcut reference (§9.6, §9.1).
// Exports reusable data (`SHORTCUT_GROUPS`, `filterShortcutGroups`), the embeddable
// `ShortcutsList` (for Settings → 快捷键), and `ShortcutsModal` (global Ctrl+/ overlay).

import { computed, platform, Scroll, signal, Text, View } from "nsh";
import { Input } from "nsh/base";
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
  /** Non-keystroke gesture label (e.g. Ctrl+点击 / Cmd+点击) when `keys` is not applicable. */
  readonly gesture?: { readonly mac: string; readonly other: string };
  readonly keywords?: readonly string[];
}

export interface ShortcutGroup {
  readonly id: string;
  readonly title: string;
  readonly items: readonly ShortcutItem[];
}

export const SHORTCUT_GROUPS: readonly ShortcutGroup[] = [
  {
    id: "global",
    title: "全局与导航",
    items: [
      { id: "palette", label: "命令面板", keys: "mod+k", keywords: ["command", "search", "jump", "命令", "跳转"] },
      { id: "shortcuts", label: "快捷键速查", keys: "mod+/", keywords: ["shortcut", "help", "key", "帮助"] },
      { id: "space-problems", label: "切换到题库", keys: "mod+1", keywords: ["library", "problems", "题库"] },
      { id: "space-contests", label: "切换到比赛", keys: "mod+2", keywords: ["contests", "比赛"] },
      { id: "space-settings", label: "切换到设置", keys: "mod+3", keywords: ["settings", "设置"] },
      { id: "open-workspace", label: "打开已有工作区", keys: "mod+o", keywords: ["workspace", "folder", "工作区"] },
      { id: "escape", label: "关闭浮层或返回上级", keys: "escape", keywords: ["esc", "close", "back", "退出"] },
    ],
  },
  {
    id: "workspace",
    title: "题库与比赛",
    items: [
      { id: "new-problem", label: "新建题目", keys: "mod+n", keywords: ["problem", "create", "新建"] },
      { id: "new-contest", label: "新建比赛", keys: "mod+shift+n", keywords: ["contest", "create", "新建"] },
      { id: "list-search", label: "聚焦搜索框", context: "题库与比赛列表", keys: "mod+f", keywords: ["find", "filter", "搜索", "筛选"] },
      { id: "list-next", label: "列表下一项", keys: "j", keywords: ["down", "next", "下移"] },
      { id: "list-prev", label: "列表上一项", keys: "k", keywords: ["up", "prev", "上移"] },
    ],
  },
  {
    id: "editor",
    title: "出版编辑",
    items: [
      { id: "tab-statement", label: "切到题面", keys: "alt+1", keywords: ["statement", "题面"] },
      { id: "tab-samples", label: "切到样例", keys: "alt+2", keywords: ["samples", "样例"] },
      { id: "tab-editorial", label: "切到题解", keys: "alt+3", keywords: ["editorial", "题解"] },
      { id: "inspector", label: "开关属性面板", keys: "mod+i", keywords: ["inspector", "properties", "属性"] },
      { id: "focus-mode", label: "专注模式", keys: "mod+shift+f", keywords: ["focus", "zen", "专注"] },
      { id: "save", label: "保存当前文件", keys: "mod+s", keywords: ["save", "保存"] },
      { id: "export-pdf", label: "导出 PDF", keys: "mod+e", keywords: ["pdf", "export", "导出"] },
      { id: "editor-find", label: "页内查找与替换", context: "源码编辑器", keys: "mod+f", keywords: ["find", "replace", "查找", "替换"] },
      { id: "bold", label: "加粗", keys: "mod+b", keywords: ["bold", "markdown", "粗体"] },
      { id: "italic", label: "斜体", keys: "mod+shift+i", keywords: ["italic", "markdown", "斜体"] },
      { id: "math-inline", label: "插入行内公式", keys: "mod+m", keywords: ["math", "latex", "tex", "公式"] },
      { id: "math-block", label: "插入块级公式", keys: "mod+shift+m", keywords: ["math", "block", "latex", "公式块"] },
    ],
  },
  {
    id: "preview",
    title: "预览画布",
    items: [
      { id: "zoom-in", label: "放大预览", keys: "mod+plus", keywords: ["zoom", "scale", "放大"] },
      { id: "zoom-out", label: "缩小预览", keys: "mod+minus", keywords: ["zoom", "scale", "缩小"] },
      { id: "fit-width", label: "适合画布宽度", keys: "mod+0", keywords: ["fit", "reset", "适合宽度"] },
      { id: "locate-preview", label: "从光标定位到预览页", keys: "mod+shift+l", keywords: ["sync", "locate", "定位"] },
      {
        id: "locate-source",
        label: "从预览页跳回源码",
        gesture: { mac: "⌘+点击", other: "Ctrl+点击" },
        keywords: ["click", "source", "jump", "跳回源码"],
      },
    ],
  },
];

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

/** Filters `SHORTCUT_GROUPS` by label, group title, formatted shortcut, or keywords. */
export function filterShortcutGroups(query: string): ShortcutGroup[] {
  const q = query.trim().toLowerCase();
  if (!q) return [...SHORTCUT_GROUPS];
  const out: ShortcutGroup[] = [];
  for (const group of SHORTCUT_GROUPS) {
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
              {`没有找到「${query().trim()}」`}
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
          placeholder="搜索快捷键或操作"
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
              快捷键
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

// Cmd+K: jump anywhere (the Core's fuzzy match ranked by frecency, the open contest's
// problems by label first) and run what the context offers. It opens where the title
// bar's search pill is (both centered on the window), its field over the pill; every
// row has its icon on the left and its shortcut flush right. Jumps are recorded as visits.

import { Clipboard, computed, signal, Text, View } from "nsh";
import { Command, type CommandEntry, type CommandItem } from "nsh/base";
import { Kbd } from "./kit";
import { Icon, type IconName } from "./Icon";
import { TITLE_BAR } from "./TitleBar";
import { Modal } from "./Modal";
import { shortcutsOpen } from "./ShortcutsModal";
import { MONO } from "./theme";
import {
  archivedExpanded,
  dialog,
  goSettings,
  goSpace,
  hasWorkspace,
  info,
  openStudio,
  paletteOpen,
  pickWorkspace,
  preview,
  requestNewWorkspace,
  selectedContest,
  selectedProblem,
  studio,
  studioActions,
  unfreezeFor,
} from "./store";

type Action = { id: string; label: string; icon: IconName; keywords?: string[]; shortcut?: string; run: () => void; when?: () => boolean };

function actions(): Action[] {
  const s = studioActions();
  const target = studio();
  const inStudio = !!target && !!s;
  const contest = inStudio && s!.isContest();
  const frozen = contest && s!.frozen();
  const archived = contest && s!.archived();
  const list: Action[] = [
    { id: "a:export-statements", icon: "fileDown", label: "导出题面册", keywords: ["pdf", "export"], run: () => s!.exportBook("statements"), when: () => inStudio },
    { id: "a:export-editorial", icon: "fileDown", label: "导出题解册", keywords: ["pdf", "export"], run: () => s!.exportBook("editorial"), when: () => inStudio },
    {
      id: "a:copy-markdown-samples",
      icon: "copy",
      label: "复制带样例的 Markdown",
      keywords: ["copy", "markdown", "md", "samples", "复制", "样例"],
      run: () => {
        if (inStudio) {
          s!.copyMarkdownWithSamples();
          return;
        }
        const id = selectedProblem();
        if (!id) return;
        try {
          void Clipboard.writeText(preview.exportMarkdownWithSamples(id, null));
        } catch {
          // Ignore clipboard/load errors outside studio.
        }
      },
      when: () => (inStudio ? s!.hasAssets() : !!selectedProblem()),
    },
    { id: "a:reveal", icon: "folderOpen", label: "在文件夹中显示", keywords: ["reveal", "folder"], run: () => s!.reveal(), when: () => inStudio },
    { id: "a:tab-statement", icon: "fileText", label: "切到题面", shortcut: "alt+1", run: () => s!.setTab("statement"), when: () => inStudio },
    { id: "a:tab-samples", icon: "table", label: "切到样例", shortcut: "alt+2", run: () => s!.setTab("samples"), when: () => inStudio },
    { id: "a:tab-editorial", icon: "filePenLine", label: "切到题解", shortcut: "alt+3", run: () => s!.setTab("editorial"), when: () => inStudio },
    { id: "a:inspector", icon: "panelRight", label: "属性", shortcut: "mod+i", run: () => s!.toggleInspector(), when: () => inStudio },
    { id: "a:assets", icon: "image", label: "素材库", keywords: ["assets", "image", "gallery", "pic", "素材", "图片"], run: () => s!.openAssets(), when: () => inStudio && s!.hasAssets() },
    { id: "a:freeze", icon: "stamp", label: "定稿比赛", keywords: ["freeze"], run: () => s!.freeze(), when: () => contest && !frozen && !archived },
    { id: "a:unfreeze", icon: "lockOpen", label: "重新编辑已定稿的比赛", keywords: ["unfreeze"], run: () => unfreezeFor.set(target!.id), when: () => contest && !!frozen && !archived },
    {
      id: "a:add-to-contest",
      icon: "listPlus",
      label: "加入比赛",
      run: () => {
        const id = target?.target === "problem" ? target.id : selectedProblem();
        if (id) dialog.set({ kind: "addToContest", problemId: id });
      },
      when: () => (target ? target.target === "problem" : !!selectedProblem()),
    },
    {
      id: "a:new-problem",
      icon: "plus",
      label: "新建题目",
      shortcut: "mod+n",
      run: () => dialog.set({ kind: "newProblem", addTo: contest && !frozen && !archived ? target!.id : undefined }),
      when: hasWorkspace,
    },
    { id: "a:new-contest", icon: "plus", label: "新建比赛", shortcut: "mod+shift+n", run: () => dialog.set({ kind: "newContest" }), when: hasWorkspace },
    { id: "a:space-problems", icon: "libraryBig", label: "题库", shortcut: inStudio ? undefined : "mod+1", run: () => goSpace("problems"), when: hasWorkspace },
    { id: "a:space-contests", icon: "trophy", label: "比赛", shortcut: inStudio ? undefined : "mod+2", run: () => goSpace("contests"), when: hasWorkspace },
    { id: "a:space-settings", icon: "settings", label: "设置", shortcut: inStudio ? undefined : "mod+3", run: () => goSpace("settings"), when: hasWorkspace },
    { id: "a:appearance", icon: "pencil", label: "外观设置", keywords: ["appearance", "theme", "dark", "light", "font", "外观", "主题", "字号"], run: () => goSettings("appearance"), when: hasWorkspace },
    { id: "a:shortcuts", icon: "code", label: "快捷键速查", keywords: ["shortcut", "key", "help", "快捷键"], shortcut: "mod+/", run: () => shortcutsOpen.set(true) },
    { id: "a:about", icon: "fileText", label: "关于", keywords: ["about", "version", "update", "关于", "版本", "更新"], run: () => goSettings("about"), when: hasWorkspace },
    { id: "a:open-workspace", icon: "folderOpen", label: "切换工作区", shortcut: "mod+o", run: () => void pickWorkspace() },
    { id: "a:new-workspace", icon: "folderPlus", label: "新建工作区", run: () => requestNewWorkspace() },
  ];
  return list.filter((a) => !a.when || a.when());
}

function matches(a: Action, q: string): boolean {
  if (!q) return true;
  const needle = q.toLowerCase();
  return a.label.toLowerCase().includes(needle) || (a.keywords ?? []).some((k) => k.includes(needle));
}

/** Top breathing room below the title bar so the palette never touches the window's top edge. */
const SEARCH_ROW = 40;
const PALETTE_TOP = TITLE_BAR + 16;
const PALETTE_WIDTH = 600;

export function CommandPalette() {
  return (
    <Modal open={paletteOpen()} onClose={() => paletteOpen.set(false)} width={PALETTE_WIDTH} topOffset={PALETTE_TOP} testID="command-palette">
      <PaletteBody />
    </Modal>
  );
}

function PaletteBody() {
  const query = signal("");
  const panelWidth = signal(PALETTE_WIDTH - 2);
  // `Command`'s virtualized list sizes each rendered row to its content, so the row
  // declares the panel's live inner width (minus list `px-1.5` and row `px-2.5`) so
  // shortcuts align flush right at every window width without clipping on narrow windows.
  const rowWidth = () => Math.max(200, Math.floor(panelWidth()) - 12 - 20);
  const close = () => paletteOpen.set(false);

  const activeContest = () => {
    const t = studio();
    return t?.target === "contest" ? t.id : null;
  };

  const jumps = computed(() => {
    if (!hasWorkspace()) return [];
    void info();
    return preview.fuzzyJump(query().trim(), activeContest()).slice(0, 30);
  });
  const acts = computed(() => actions().filter((a) => matches(a, query().trim())));

  const items = computed<CommandItem[]>(() => {
    const q = query().trim();
    // Ranked within each section; the sections in a fixed order so each shows once.
    const order = (kind: string) => (kind === "contest_item" ? 0 : kind === "problem" ? 1 : 2);
    const ranked = jumps()
      .map((h, i) => ({ h, i }))
      .sort((a, b) => order(a.h.kind) - order(b.h.kind) || a.i - b.i)
      .map((x) => x.h);
    const j: CommandItem[] = ranked.map((h) => ({
      id: `j:${h.visitKey}`,
      label: h.title,
      keywords: [h.id],
      group: h.kind === "contest" ? "比赛" : h.kind === "contest_item" ? "本场比赛" : "题目",
    }));
    const a: CommandItem[] = acts().map((x) => ({ id: x.id, label: x.label, group: "操作", shortcut: x.shortcut ?? null }));
    // Objects first (by frecency without a query), then what can be done here.
    return [...j, ...a];
  });

  function select(id: string) {
    close();
    if (id.startsWith("a:")) {
      const a = actions().find((x) => x.id === id);
      setTimeout(() => a?.run(), 0);
      return;
    }
    const hit = jumps().find((h) => `j:${h.visitKey}` === id);
    if (!hit) return;
    if (hit.kind === "problem") openStudio({ target: "problem", id: hit.id });
    else if (hit.kind === "contest") {
      if (hit.archived) archivedExpanded.set(true);
      selectedContest.set(hit.id);
      openStudio({ target: "contest", id: hit.id });
    } else if (hit.contestId) openStudio({ target: "contest", id: hit.contestId, label: hit.label ?? undefined });
  }

  const hitOf = (id: string) => jumps().find((h) => `j:${h.visitKey}` === id);
  const meta = (id: string) => {
    const hit = hitOf(id);
    return hit && hit.kind !== "contest_item" ? hit.id : undefined;
  };
  const lead = (id: string) => {
    if (id.startsWith("a:")) return <Icon name={actions().find((x) => x.id === id)?.icon ?? "arrowRight"} size={16} />;
    const hit = hitOf(id);
    if (hit?.kind === "contest_item")
      return (
        <View style={{ width: 18, height: 18, borderRadius: 4, alignItems: "center", justifyContent: "center" }} class="bg-muted">
          <Text style={{ fontSize: 11, fontWeight: 600, fontVariant: ["tabular-nums"] }} class="text-secondary-foreground">
            {hit.label ?? ""}
          </Text>
        </View>
      );
    return <Icon name={hit?.kind === "contest" ? "trophy" : "fileText"} size={16} />;
  };

  return (
    <Command
      onLayout={(e) => {
        if (e.bounds.size.width > 0) panelWidth.set(e.bounds.size.width);
      }}
      items={items()}
      filterable={false}
      query={query()}
      onQueryChange={(e) => query.set(e.query)}
      onSelect={(e) => select(e.id)}
      onCancel={close}
      autoFocus
      placeholder="跳转到题目或比赛，或输入操作"
      searchPrefix={<Icon name="search" size={16} tone="muted-foreground" />}
      searchSuffix={<Kbd keys="escape" faint />}
      searchBarClass="px-3.5 flex-row items-center gap-2.5 border-b border-border"
      searchBarStyle={{ height: 44 }}
      searchClass="flex-1 text-[14px] bg-transparent placeholder:text-muted-foreground"
      searchStyle={{ height: SEARCH_ROW, fontSize: 14 }}
      listClass="max-h-[440px] px-1.5 py-1.5"
      itemClass="h-9 px-2.5 rounded-[6px] flex-row items-center focus:bg-accent hover:bg-muted transition-colors"
      groupClass="px-2.5 pt-2.5 pb-1 text-[11.5px] text-muted-foreground"
      separatorClass="h-px mx-1 my-1 bg-border"
      renderItem={(entry: CommandEntry) => (
        <View style={{ width: rowWidth(), flexDirection: "row", alignItems: "center", gap: 10 }}>
          <View style={{ width: 18, alignItems: "center", flexShrink: 0 }}>{lead(entry.id)}</View>
          <Text style={{ numberOfLines: 1, textOverflow: "ellipsis", flexShrink: 1, fontSize: 13.5, lineHeight: 20 }}>
            {entry.label}
          </Text>
          {meta(entry.id) && (
            <Text style={{ numberOfLines: 1, textOverflow: "ellipsis", flexShrink: 1, fontSize: 11.5, lineHeight: 20, fontFamily: MONO }} class="text-muted-foreground">
              {meta(entry.id)!}
            </Text>
          )}
          {hitOf(entry.id)?.archived && (
            <View style={{ height: 18, paddingLeft: 6, paddingRight: 6, borderRadius: 4, justifyContent: "center", flexShrink: 0 }} class="bg-muted">
              <Text style={{ fontSize: 11 }} class="text-muted-foreground">
                已归档
              </Text>
            </View>
          )}
          <View style={{ flex: 1 }} />
          {entry.shortcut && <Kbd keys={entry.shortcut} faint />}
        </View>
      )}
      empty={
        <View style={{ height: 72, alignItems: "center", justifyContent: "center", gap: 6 }}>
          <Icon name="searchX" size={18} tone="muted-foreground" />
          <Text style={{ fontSize: 13 }} class="text-muted-foreground">
            {`没有找到「${query().trim()}」`}
          </Text>
        </View>
      }
    />
  );
}

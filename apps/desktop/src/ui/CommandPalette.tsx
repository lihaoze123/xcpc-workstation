// Cmd+K: jump anywhere (the Core's fuzzy match ranked by frecency, the open contest's
// problems by label first) and run what the context offers. It opens where the title
// bar's search pill is (both centered on the window), its field over the pill; every
// row has its icon on the left and its shortcut flush right. Jumps are recorded as visits.

import { cleanup, Clipboard, computed, effect, signal, Text, View } from "nsh";
import { Command, type CommandEntry, type CommandItem, type CommandRef } from "nsh/base";
import { t } from "./i18n";
import { Kbd } from "./kit";
import { Icon, type IconName } from "./Icon";
import { buildPaletteItems, shortcutForSpace } from "./logic";
import { TITLE_BAR } from "./TitleBar";
import { backdropTransition, overlayTransform, overlayTransition } from "./motion";
import { openDiagnosticsWindow } from "./o11y/windowManager";
import { shortcutsOpen } from "./ShortcutsModal";
import { modalBackdrop, modalShadow, ms } from "./theme";
import { showToast } from "./Toast";
import { parseJsonPayload } from "./snippets/snippetLogic";
import type { SnippetListPayload, TrdSummaryItem } from "./snippets/types";
import { runSnippetVerifyJob } from "./snippets/verifyJob";
import {
  parseJsonReport,
  type PluginCommandAction,
  type RegisteredPluginCommand,
} from "./tools/types";
import {
  activeSnippetStudio,
  activeTrdStudio,
  archivedExpanded,
  bundlerModalOpen,
  dialog,
  goSettings,
  goSpace,
  hasWorkspace,
  importPolygon,
  info,
  librarySidebarOpen,
  newTrdModalOpen,
  openNewSnippetModal,
  openSnippetStudio,
  openStudio,
  openToolPlugin,
  openTrdStudio,
  paletteOpen,
  pickWorkspace,
  pluginsRevision,
  preview,
  requestNewWorkspace,
  selectedContest,
  selectedProblem,
  snippetsRevision,
  snippetsSubTab,
  space,
  studio,
  studioActions,
  toggleLibrarySidebar,
  unfreezeFor,
  workspaceVersion,
} from "./store";

import { FONT_SIZE, FONT_WEIGHT, LINE_HEIGHT, RADIUS, SPACE } from "./tokens";
let cachedPluginCmdsRev = -1;
const cachedPluginCmdsByContext = new Map<string, RegisteredPluginCommand[]>();

function getCachedPluginCommands(rev: number, cmdContext: string): RegisteredPluginCommand[] {
  if (rev !== cachedPluginCmdsRev) {
    cachedPluginCmdsRev = rev;
    cachedPluginCmdsByContext.clear();
  }
  const hit = cachedPluginCmdsByContext.get(cmdContext);
  if (hit) return hit;
  const parsed = parseJsonReport<RegisteredPluginCommand[]>(
    typeof preview.pluginListCommands === "function" ? preview.pluginListCommands(cmdContext) : undefined,
    [],
  );
  cachedPluginCmdsByContext.set(cmdContext, parsed);
  return parsed;
}

let cachedSnippetCatalogKey = "";
let cachedSnippetCatalogValue: { snips: SnippetListPayload["snippets"]; trds: TrdSummaryItem[] } = {
  snips: [],
  trds: [],
};

function getCachedSnippetCatalog(snipRev: number, wsVer: number) {
  const key = `${snipRev}:${wsVer}`;
  if (key === cachedSnippetCatalogKey) return cachedSnippetCatalogValue;
  const snips = parseJsonPayload<SnippetListPayload>(preview.listSnippetsJson())?.snippets ?? [];
  const trds = parseJsonPayload<{ trds: TrdSummaryItem[] }>(preview.listTrdsJson())?.trds ?? [];
  cachedSnippetCatalogKey = key;
  cachedSnippetCatalogValue = { snips, trds };
  return cachedSnippetCatalogValue;
}

type Action = { id: string; label: string; icon: IconName; keywords?: string[]; shortcut?: string; run: () => void; when?: () => boolean };

function actions(): Action[] {
  const s = studioActions();
  const target = studio();
  const inStudio = !!target && !!s;
  const contest = inStudio && s!.isContest();
  const frozen = contest && s!.frozen();
  const archived = contest && s!.archived();
  const list: Action[] = [
    { id: "a:export-statements", icon: "fileDown", label: t("palette.action.exportStatements"), keywords: ["pdf", "export", "statement", "statements", "导出", "题面册", "daochu", "timian"], run: () => s!.exportBook("statements"), when: () => inStudio },
    { id: "a:export-editorial", icon: "fileDown", label: t("palette.action.exportEditorial"), keywords: ["pdf", "export", "editorial", "solution", "导出", "题解册", "daochu", "tijie"], run: () => s!.exportBook("editorial"), when: () => inStudio },
    { id: "a:export-domjudge", icon: "database", label: t("palette.action.exportDomjudge"), keywords: ["export", "domjudge", "dj", "oj", "package", "zip", "导出", "daochu"], run: () => s!.exportOjPackage("domjudge"), when: () => inStudio },
    { id: "a:export-hydro", icon: "database", label: t("palette.action.exportHydro"), keywords: ["export", "hydro", "hydrooj", "oj", "package", "zip", "导出", "daochu"], run: () => s!.exportOjPackage("hydro"), when: () => inStudio },
    { id: "a:import-polygon", icon: "folderOpen", label: t("palette.action.importPolygon"), keywords: ["import", "polygon", "zip", "package", "导入", "压缩包", "daoru"], run: () => void importPolygon(), when: () => hasWorkspace() && !info().readOnly },
    {
      id: "a:copy-markdown-samples",
      icon: "copy",
      label: t("palette.action.copyMarkdownWithSamples"),
      keywords: ["copy", "markdown", "md", "samples", "复制", "样例", "fuzhi", "yangli"],
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
    { id: "a:reveal", icon: "folderOpen", label: t("palette.action.reveal"), keywords: ["reveal", "folder", "finder", "explorer", "文件夹", "显示", "wenjianjia"], run: () => s!.reveal(), when: () => inStudio },
    { id: "a:tab-statement", icon: "fileText", label: t("palette.action.tabStatement"), keywords: ["statement", "tab", "题面", "timian"], shortcut: "alt+1", run: () => s!.setTab("statement"), when: () => inStudio },
    { id: "a:tab-samples", icon: "table", label: t("palette.action.tabSamples"), keywords: ["samples", "sample", "tab", "样例", "yangli"], shortcut: "alt+2", run: () => s!.setTab("samples"), when: () => inStudio },
    { id: "a:tab-editorial", icon: "filePenLine", label: t("palette.action.tabEditorial"), keywords: ["editorial", "solution", "tab", "题解", "tijie"], shortcut: "alt+3", run: () => s!.setTab("editorial"), when: () => inStudio },
    { id: "a:tab-build", icon: "database", label: t("palette.action.tabBuild"), keywords: ["build", "judge", "package", "verify", "authoring", "出题", "构建", "chuti", "goujian", "yanshou"], shortcut: "alt+4", run: () => s!.setTab("build"), when: () => inStudio },
    { id: "a:sanitize-problem", icon: "shieldCheck", label: t("palette.action.sanitizeProblem"), keywords: ["sanitize", "asan", "ubsan", "ub", "体检", "tijian"], run: () => s!.runSanitize(), when: () => inStudio && !contest && s!.hasPackage() && !info().readOnly },
    {
      id: "a:contest-check",
      icon: "shieldCheck",
      label: t("palette.action.contestCheck"),
      keywords: ["check", "audit", "contest", "verify", "检查", "比赛", "jiancha"],
      run: () => {
        if (contest) s!.checkContest();
        else {
          const cid = selectedContest();
          if (cid) openStudio({ target: "contest", id: cid });
        }
      },
      when: () => contest || (!inStudio && !!selectedContest()),
    },
    { id: "a:init-package", icon: "plus", label: t("palette.action.initPackage"), keywords: ["package", "init", "scaffold", "polygon", "题目包", "脚手架", "初始化", "chushihua"], run: () => s!.initPackage(), when: () => inStudio && !contest && !s!.hasPackage() && !info().readOnly },
    { id: "a:inspector", icon: "panelRight", label: t("palette.action.properties"), keywords: ["properties", "inspector", "layout", "属性", "版式", "shuxing"], shortcut: "mod+i", run: () => s!.toggleInspector(), when: () => inStudio },
    { id: "a:assets", icon: "image", label: t("palette.action.assets"), keywords: ["assets", "image", "gallery", "pic", "素材", "图片", "sucai", "tupian"], run: () => s!.openAssets(), when: () => inStudio && s!.hasAssets() },
    { id: "a:freeze", icon: "stamp", label: t("palette.action.finalize"), keywords: ["finalize", "freeze", "lock", "定稿", "冻结", "dinggao"], run: () => s!.freeze(), when: () => contest && !frozen && !archived },
    { id: "a:unfreeze", icon: "lockOpen", label: t("palette.action.reopen"), keywords: ["reopen", "unfreeze", "unlock", "重新编辑", "解冻", "chongxin"], run: () => unfreezeFor.set(target!.id), when: () => contest && !!frozen && !archived },
    {
      id: "a:add-to-contest",
      icon: "listPlus",
      label: t("palette.action.addToContest"),
      keywords: ["add", "contest", "加入", "比赛", "jiaru", "bisai"],
      run: () => {
        const id = target?.target === "problem" ? target.id : selectedProblem();
        if (id) dialog.set({ kind: "addToContest", problemId: id });
      },
      when: () => (target ? target.target === "problem" : !!selectedProblem()),
    },
    {
      id: "a:new-problem",
      icon: "plus",
      label: t("palette.action.newProblem"),
      keywords: ["new", "problem", "create", "新建", "题目", "xinjian", "timu"],
      shortcut: "mod+n",
      run: () => dialog.set({ kind: "newProblem", addTo: contest && !frozen && !archived ? target!.id : undefined }),
      when: hasWorkspace,
    },
    { id: "a:new-contest", icon: "plus", label: t("palette.action.newContest"), keywords: ["new", "contest", "create", "新建", "比赛", "xinjian", "bisai"], shortcut: "mod+shift+n", run: () => dialog.set({ kind: "newContest" }), when: hasWorkspace },
    { id: "a:new-group", icon: "folderPlus", label: t("palette.action.newGroup"), keywords: ["new", "group", "folder", "create", "新建", "分组", "xinjian", "fenzu"], run: () => dialog.set({ kind: "newGroup" }), when: () => hasWorkspace() && !info().readOnly },
    {
      id: "a:toggle-group-sidebar",
      icon: "panelLeft",
      label: librarySidebarOpen() ? t("palette.action.hideGroupSidebar") : t("palette.action.showGroupSidebar"),
      keywords: ["group", "sidebar", "panel", "toggle", "分组", "侧栏", "显示", "隐藏", "fenzu", "celan"],
      run: () => toggleLibrarySidebar(),
      when: () => hasWorkspace() && !inStudio && space() === "problems",
    },
    { id: "a:space-problems", icon: "libraryBig", label: t("space.problems"), keywords: ["problems", "library", "题库", "tiku"], shortcut: inStudio || activeSnippetStudio() || activeTrdStudio() ? undefined : shortcutForSpace("problems", { hasWorkspace: hasWorkspace(), snippetsEnabled: info().snippetsEnabled, trainingEnabled: info().trainingEnabled }), run: () => goSpace("problems"), when: hasWorkspace },
    { id: "a:space-contests", icon: "trophy", label: t("space.contests"), keywords: ["contests", "比赛", "bisai"], shortcut: inStudio || activeSnippetStudio() || activeTrdStudio() ? undefined : shortcutForSpace("contests", { hasWorkspace: hasWorkspace(), snippetsEnabled: info().snippetsEnabled, trainingEnabled: info().trainingEnabled }), run: () => goSpace("contests"), when: hasWorkspace },
    { id: "a:space-snippets", icon: "bookOpen", label: t("space.snippets"), keywords: ["snippets", "trd", "templates", "模板", "算法库", "moban"], shortcut: inStudio || activeSnippetStudio() || activeTrdStudio() ? undefined : shortcutForSpace("snippets", { hasWorkspace: hasWorkspace(), snippetsEnabled: info().snippetsEnabled, trainingEnabled: info().trainingEnabled }), run: () => goSpace("snippets"), when: () => hasWorkspace() && info().snippetsEnabled },
    { id: "a:bundler", icon: "fileCode2", label: t("snippets.bundler"), keywords: ["bundle", "expand", "cpp", "single", "展开", "提交代码", "zhankai"], run: () => bundlerModalOpen.set(true), when: () => hasWorkspace() && info().snippetsEnabled },
    { id: "a:new-snippet", icon: "plus", label: t("snippets.newSnippet"), keywords: ["new", "snippet", "algorithm", "新建", "算法", "模板", "moban"], run: () => { goSpace("snippets"); snippetsSubTab.set("library"); openNewSnippetModal(); }, when: () => hasWorkspace() && info().snippetsEnabled && !info().readOnly },
    { id: "a:new-trd", icon: "plus", label: t("snippets.newTrd"), keywords: ["new", "trd", "handbook", "新建", "模板册", "mobance"], run: () => { goSpace("snippets"); snippetsSubTab.set("trds"); newTrdModalOpen.set(true); }, when: () => hasWorkspace() && info().snippetsEnabled && !info().readOnly },
    {
      id: "a:verify-all-snippets",
      icon: "shieldCheck",
      label: t("snippets.verifyAll"),
      keywords: ["verify", "snippet", "all", "验板", "验证", "全部模板"],
      run: () => {
        if (runSnippetVerifyJob({ mode: "all", slug: null, force: false })) {
          showToast({ tone: "info", text: t("snippets.action.verifying") });
        }
      },
      when: () => hasWorkspace() && info().snippetsEnabled,
    },
    { id: "a:space-training", icon: "crown", label: t("space.training"), keywords: ["training", "dashboard", "calendar", "submissions", "训练", "看板", "xunlian"], shortcut: inStudio || activeSnippetStudio() || activeTrdStudio() ? undefined : shortcutForSpace("training", { hasWorkspace: hasWorkspace(), snippetsEnabled: info().snippetsEnabled, trainingEnabled: info().trainingEnabled }), run: () => goSpace("training"), when: () => hasWorkspace() && info().trainingEnabled },
    { id: "a:space-tools", icon: "squareCode", label: t("space.tools"), keywords: ["tools", "toolbox", "plugin", "graph", "math", "工具", "画板", "插件", "gongju"], shortcut: inStudio || activeSnippetStudio() || activeTrdStudio() ? undefined : shortcutForSpace("tools", { hasWorkspace: hasWorkspace(), snippetsEnabled: info().snippetsEnabled, trainingEnabled: info().trainingEnabled }), run: () => goSpace("tools") },
    { id: "a:space-settings", icon: "settings", label: t("space.settings"), keywords: ["settings", "preferences", "设置", "shezhi"], shortcut: inStudio || activeSnippetStudio() || activeTrdStudio() ? undefined : shortcutForSpace("settings", { hasWorkspace: hasWorkspace(), snippetsEnabled: info().snippetsEnabled, trainingEnabled: info().trainingEnabled }), run: () => goSpace("settings") },
    { id: "a:appearance", icon: "palette", label: t("palette.action.appearance"), keywords: ["appearance", "theme", "dark", "light", "font", "language", "外观", "主题", "字号", "语言", "waiguan", "yuyan"], run: () => goSettings("appearance") },
    { id: "a:toolchain", icon: "squareCode", label: t("palette.action.toolchain"), keywords: ["toolchain", "compiler", "g++", "clang++", "cxx", "工具链", "编译器", "gongjulian"], run: () => goSettings("toolchain") },
    { id: "a:shortcuts", icon: "code", label: t("palette.action.shortcuts"), keywords: ["shortcut", "shortcuts", "key", "help", "快捷键", "kuaijiejian"], shortcut: "mod+/", run: () => shortcutsOpen.set(true) },
    { id: "a:about", icon: "fileText", label: t("palette.action.about"), keywords: ["about", "version", "update", "关于", "版本", "更新", "guanyu"], run: () => goSettings("about") },
    { id: "a:diagnostics", icon: "activity", label: t("palette.action.diagnostics"), keywords: ["o11y", "diagnostics", "perf", "trace", "metrics", "logs", "诊断", "性能", "监控", "日志", "zhenduan", "xingneng"], shortcut: "ctrl+shift+d", run: () => openDiagnosticsWindow() },
    { id: "a:open-workspace", icon: "folderOpen", label: t("palette.action.switchWorkspace"), keywords: ["workspace", "open", "switch", "切换", "打开", "工作区", "gongzuoqu"], shortcut: "mod+o", run: () => void pickWorkspace() },
    { id: "a:new-workspace", icon: "folderPlus", label: t("palette.action.newWorkspace"), keywords: ["workspace", "new", "create", "新建", "工作区", "gongzuoqu"], run: () => requestNewWorkspace() },
  ];

  // Dynamically collect enabled plugin commands matching the current context (cached per revision).
  const rev = pluginsRevision();
  const inTools = !inStudio && space() === "tools";
  const cmdContext = target?.target === "problem" ? "problem" : target?.target === "contest" ? "contest" : "global";
  const pluginCmds = getCachedPluginCommands(rev, cmdContext);
  const pluginActions: Action[] = pluginCmds.map((reg) => ({
    id: `a:plugin:${reg.plugin_id}:${reg.command.id}`,
    icon: "sparkles",
    label: reg.command.title,
    keywords: [reg.plugin_id, reg.plugin_name, reg.command.id, "plugin", "tool", "插件", "工具"],
    run: () => {
      const ctxPayload: Record<string, unknown> = { context: cmdContext };
      if (target?.target === "problem") ctxPayload.problem_id = target.id;
      else if (target?.target === "contest") ctxPayload.contest_id = target.id;
      else if (selectedProblem()) ctxPayload.problem_id = selectedProblem();
      void preview
        .pluginRunCommand(reg.plugin_id, reg.command.id, JSON.stringify(ctxPayload))
        .then((res) => {
          if (!res.ok) {
            showToast({ tone: "issues", text: res.error ?? t("tools.commandFailed") });
            return;
          }
          const action = parseJsonReport<PluginCommandAction>(res, { type: "none" });
          if (action.type === "open_tool") {
            openToolPlugin(action.plugin_id, {
              toolId: action.tool_id ?? undefined,
              payload: action.initial_inputs,
            });
          } else if (action.type === "message") {
            showToast({ tone: "info", text: action.text });
          } else if (action.type === "tool_result") {
            showToast({ tone: "success", text: action.result.summary ?? reg.command.title });
          }
        });
    },
  }));

  const builtins = list.filter((a) => !a.when || a.when());
  // In the Snippets space, prioritize snippet/TRD actions at the top of the action list.
  const inSnippets = !inStudio && space() === "snippets";
  const orderedBuiltins = inSnippets
    ? [
        ...builtins.filter((a) => a.id.includes("snippet") || a.id.includes("trd") || a.id.includes("bundler")),
        ...builtins.filter((a) => !a.id.includes("snippet") && !a.id.includes("trd") && !a.id.includes("bundler")),
      ]
    : builtins;
  // In the Tools space, sort tool/plugin commands to the very top of the palette.
  return inTools ? [...pluginActions, ...orderedBuiltins] : [...orderedBuiltins, ...pluginActions];
}

/** Top breathing room below the title bar so the palette never touches the window's top edge. */
const SEARCH_ROW = 40;
const PALETTE_TOP = TITLE_BAR + 16;
const PALETTE_WIDTH = 600;

export function CommandPalette() {
  // Mount lazily on first open and keep mounted afterward (display: "none" when hidden)
  // so repeated Cmd+K toggles skip rebuilding the `<Command>` virtual list (~18-31ms -> <5ms).
  const hasOpened = signal(paletteOpen());
  const visible = signal(paletteOpen());
  const shown = signal(false);
  let raf: number | undefined;
  let closeTimer: number | undefined;

  effect(() => {
    const isOpen = paletteOpen();
    if (raf !== undefined) {
      cancelAnimationFrame(raf);
      raf = undefined;
    }
    clearTimeout(closeTimer);
    if (isOpen) {
      if (!hasOpened()) hasOpened.set(true);
      visible.set(true);
      raf = requestAnimationFrame(() => {
        raf = undefined;
        shown.set(true);
      });
    } else if (visible()) {
      shown.set(false);
      const dur = ms(140);
      if (dur === 0) visible.set(false);
      else closeTimer = setTimeout(() => visible.set(false), dur);
    }
  });
  cleanup(() => {
    if (raf !== undefined) cancelAnimationFrame(raf);
    clearTimeout(closeTimer);
  });

  const close = () => paletteOpen.set(false);

  return (
    <If when={hasOpened()}>
      <View
        style={{
          position: "absolute",
          top: 0,
          left: 0,
          right: 0,
          bottom: 0,
          display: visible() ? "flex" : "none",
          zIndex: 90,
        }}
      >
        <View
          focusable={false}
          style={{
            position: "absolute",
            top: 0,
            left: 0,
            right: 0,
            bottom: 0,
            backgroundColor: modalBackdrop(),
            opacity: shown() ? 1 : 0,
            pointerEvents: paletteOpen() ? "auto" : "none",
            ...backdropTransition(),
          }}
          onPress={() => {
            if (paletteOpen()) close();
          }}
        />
        <View desktop={{ windowDrag: true }} style={{ position: "absolute", top: 0, left: 0, right: 0, height: TITLE_BAR }} />
        <View
          style={{
            position: "absolute",
            top: PALETTE_TOP,
            left: 0,
            right: 0,
            bottom: 0,
            alignItems: "center",
            justifyContent: "flex-start",
            pointerEvents: "box-none",
          }}
        >
          <View
            testID="command-palette"
            style={{
              width: PALETTE_WIDTH,
              maxWidth: "90%",
              maxHeight: "85%",
              borderRadius: RADIUS.lg,
              borderWidth: 1,
              overflow: "hidden",
              boxShadow: modalShadow(),
              opacity: shown() ? 1 : 0,
              transform: overlayTransform(shown()),
              ...overlayTransition(),
              pointerEvents: paletteOpen() ? "auto" : "none",
            }}
            class="bg-card border-border"
          >
            <PaletteBody />
          </View>
        </View>
      </View>
    </If>
  );
}

function PaletteBody() {
  const query = signal("");
  const panelWidth = signal(PALETTE_WIDTH - 2);
  let cmdRef: CommandRef | null = null;
  let focusRaf: number | undefined;
  // Reset query and focus the search input whenever the kept-mounted palette opens.
  effect(() => {
    const isOpen = paletteOpen();
    if (focusRaf !== undefined) {
      cancelAnimationFrame(focusRaf);
      focusRaf = undefined;
    }
    if (isOpen) {
      query.set("");
      focusRaf = requestAnimationFrame(() => {
        focusRaf = undefined;
        cmdRef?.focus();
      });
    }
  });
  cleanup(() => {
    if (focusRaf !== undefined) cancelAnimationFrame(focusRaf);
  });
  // `Command`'s virtualized list sizes each rendered row to its content, so the row
  // declares the panel's live inner width (minus list `px-1.5` and row `px-2.5`) so
  // shortcuts align flush right at every window width without clipping on narrow windows.
  const rowWidth = () => Math.max(200, Math.floor(panelWidth()) - 12 - 20);
  const close = () => paletteOpen.set(false);

  const activeContest = () => {
    const target = studio();
    return target?.target === "contest" ? target.id : null;
  };

  const actionMode = () => query().trimStart().startsWith(">");

  const jumps = computed(() => {
    if (!paletteOpen() || !hasWorkspace() || actionMode()) return [];
    void info();
    return preview.fuzzyJump(query().trim(), activeContest());
  });

  const snippetCatalog = computed(() => {
    if (!paletteOpen() || !hasWorkspace() || !info().snippetsEnabled) {
      return { snips: [] as SnippetListPayload["snippets"], trds: [] as TrdSummaryItem[] };
    }
    return getCachedSnippetCatalog(snippetsRevision(), workspaceVersion());
  });

  const snippetHits = computed(() => {
    if (!paletteOpen() || !hasWorkspace() || !info().snippetsEnabled || actionMode()) return [];
    const { snips, trds } = snippetCatalog();
    const q = query().trim().toLowerCase();
    const out: { id: string; kind: "snippet" | "trd"; slug: string; label: string; group: string; hasCode?: boolean }[] = [];
    for (const s of snips) {
      const hay = `${s.title} ${s.slug} ${(s.tags ?? []).join(" ")}`.toLowerCase();
      if (!q || hay.includes(q)) {
        out.push({
          id: `s:${s.slug}`,
          kind: "snippet",
          slug: s.slug,
          label: s.title,
          group: t("snippets.tab.library"),
          hasCode: s.has_code,
        });
      }
    }
    for (const trd of trds) {
      const hay = `${trd.name} ${trd.id}`.toLowerCase();
      if (!q || hay.includes(q)) {
        out.push({
          id: `t:${trd.id}`,
          kind: "trd",
          slug: trd.id,
          label: trd.name,
          group: t("snippets.tab.trds"),
        });
      }
    }
    return out.slice(0, q ? 12 : space() === "snippets" ? 8 : 0);
  });

  const allActions = computed(() => (paletteOpen() ? actions() : []));
  const actionById = computed(() => new Map(allActions().map((a) => [a.id, a])));
  const jumpById = computed(() => new Map(jumps().map((h) => [`j:${h.visitKey}`, h])));
  const snipHitById = computed(() => new Map(snippetHits().map((h) => [h.id, h])));

  const built = computed(() =>
    buildPaletteItems(query(), jumps(), allActions(), activeContest(), {
      prioritizeActions: !studio() && (space() === "tools" || space() === "snippets"),
    }),
  );

  const items = computed<CommandItem[]>(() => {
    const snipItems: CommandItem[] = snippetHits().map((x) => ({
      id: x.id,
      label: x.label,
      group: x.group,
      shortcut: null,
    }));
    const baseItems: CommandItem[] = built().items.map((x) => ({
      id: x.id,
      label: x.label,
      group: x.groupLabel,
      shortcut: x.shortcut ?? null,
    }));
    return space() === "snippets" || query().trim().length > 0
      ? [...snipItems, ...baseItems]
      : baseItems;
  });

  function select(id: string) {
    close();
    if (id.startsWith("s:")) {
      openSnippetStudio(id.slice(2));
      return;
    }
    if (id.startsWith("t:")) {
      openTrdStudio(id.slice(2));
      return;
    }
    if (id.startsWith("a:")) {
      const a = actionById().get(id);
      setTimeout(() => a?.run(), 0);
      return;
    }
    const hit = jumpById().get(id);
    if (!hit) return;
    if (hit.kind === "problem") openStudio({ target: "problem", id: hit.id });
    else if (hit.kind === "contest") {
      if (hit.archived) archivedExpanded.set(true);
      selectedContest.set(hit.id);
      openStudio({ target: "contest", id: hit.id });
    } else if (hit.contestId) openStudio({ target: "contest", id: hit.contestId, label: hit.label ?? undefined });
  }

  const hitOf = (id: string) => jumpById().get(id);
  const snipHitOf = (id: string) => snipHitById().get(id);
  const meta = (id: string) => {
    const sh = snipHitOf(id);
    if (sh) return sh.slug;
    const hit = hitOf(id);
    return hit && hit.kind !== "contest_item" ? hit.id : undefined;
  };
  const lead = (id: string) => {
    const sh = snipHitOf(id);
    if (sh) {
      return <Icon name={sh.kind === "trd" ? "bookOpen" : sh.hasCode === false ? "sigma" : "code"} size="md" />;
    }
    if (id.startsWith("a:")) return <Icon name={actionById().get(id)?.icon ?? "arrowRight"} size="md" />;
    const hit = hitOf(id);
    if (hit?.kind === "contest_item")
      return (
        // token-exempt: compact inline icon box
        <View style={{ width: 18, height: 18, borderRadius: RADIUS.xs, alignItems: "center", justifyContent: "center" }} class="bg-muted">
          <Text style={{ fontSize: FONT_SIZE.micro, fontWeight: FONT_WEIGHT.semibold, fontVariant: ["tabular-nums"] }} class="text-secondary-foreground">
            {hit.label ?? ""}
          </Text>
        </View>
      );
    return <Icon name={hit?.kind === "contest" ? "trophy" : "fileText"} size="md" />;
  };

  return (
    <Command
      ref={(r) => {
        cmdRef = r;
      }}
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
      placeholder={!studio() && space() === "snippets" ? t("palette.placeholderSnippets") : t("palette.placeholder")}
      searchPrefix={<Icon name="search" size="md" tone={actionMode() ? "primary" : "muted-foreground"} />}
      searchSuffix={<Kbd keys="escape" faint />}
      searchBarClass="px-3.5 flex-row items-center gap-2.5 border-b border-border"
      searchBarStyle={{ height: 44 }}
      searchClass="flex-1 text-[14px] bg-transparent placeholder:text-muted-foreground"
      searchStyle={{ height: SEARCH_ROW, fontSize: FONT_SIZE.strong }}
      listClass="max-h-[440px] px-1.5 py-1.5"
      itemClass="h-9 px-2.5 rounded-[6px] flex-row items-center focus:bg-accent hover:bg-muted transition-colors"
      groupClass="px-2.5 pt-2.5 pb-1 text-[11.5px] text-muted-foreground"
      separatorClass="h-px mx-1 my-1 bg-border"
      renderItem={(entry: CommandEntry) => (
        <View style={{ width: rowWidth(), flexDirection: "row", alignItems: "center", gap: SPACE[8] }}>
          <View style={{ width: 18, alignItems: "center", flexShrink: 0 }}>{lead(entry.id)}</View>
          <Text style={{ numberOfLines: 1, textOverflow: "ellipsis", flexShrink: 1, fontSize: FONT_SIZE.body, lineHeight: LINE_HEIGHT.strong }}>
            {entry.label}
          </Text>
          {meta(entry.id) && (
            <Text style={{ numberOfLines: 1, textOverflow: "ellipsis", flexShrink: 1, fontSize: FONT_SIZE.caption, lineHeight: LINE_HEIGHT.strong, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
              {meta(entry.id)!}
            </Text>
          )}
          {hitOf(entry.id)?.archived && (
            // token-exempt: compact inline badge height
            <View style={{ height: 18, paddingLeft: SPACE[6], paddingRight: SPACE[6], borderRadius: RADIUS.xs, justifyContent: "center", flexShrink: 0 }} class="bg-muted">
              <Text style={{ fontSize: FONT_SIZE.micro }} class="text-muted-foreground">
                {t("contests.archived")}
              </Text>
            </View>
          )}
          <View style={{ flex: 1 }} />
          {entry.shortcut && <Kbd keys={entry.shortcut} faint />}
        </View>
      )}
      empty={
        <View style={{ height: 72, alignItems: "center", justifyContent: "center", gap: SPACE[6] }}>
          <Icon name="searchX" size="lg" tone="muted-foreground" />
          <Text style={{ fontSize: FONT_SIZE.body }} class="text-muted-foreground">
            {t("common.searchNotFound", { query: built().query || query().trim() })}
          </Text>
        </View>
      }
    />
  );
}

// Cmd+1: the problem library. Collapsible persistent group sidebar on the left,
// multi-dimensional filters & dynamic group-by folding over a virtualized table
// (40 pt rows: select, title + inline ID, rating, tags, 3-slot status, source),
// multi-select floating batch bar, and right-hand ProblemInspector / BatchInspector.

import { batch, cleanup, Clipboard, computed, effect, keymap, signal, Text, View, type InputRef } from "nsh";
import {
  DataTable,
  Editor,
  Input,
  ResizablePanel,
  ResizablePanelGroup,
  type DataTableRef,
  type DataTableSort,
} from "nsh/base";
import { Menu as DesktopMenu } from "nsh/desktop";
import type { PreviewRequest } from "xcpc-workstation/native";
import { Btn, Chevron, Chip, IconBtn, RatingBadge, Segmented, tip } from "./kit";
import { locale, t } from "./i18n";
import { Icon } from "./Icon";
import { AppMenu, MoreTrigger, type MenuEntry } from "./Menu";
import { DetailHeader, EmptyState, Spacer, Toolbar } from "./layout";
import {
  applyRowSelection,
  computeLibraryColumns,
  EMPTY_SELECTION,
  groupProblems,
  headerCheckboxState,
  TIER_RANGES,
  type LibraryGroupBy,
  type SelectionState,
  type TierName,
} from "./logic";
import { EASE_STANDARD, MONO, monoFont, ms } from "./theme";
import { PreviewCanvas, type CanvasController } from "./studio/PreviewCanvas";
import {
  batchTask,
  dialog,
  importPolygon,
  importingPolygon,
  info,
  jumpToContest,
  libraryGroupBy,
  librarySidebarOpen,
  openStudio,
  preview,
  refresh,
  revision,
  selectedProblem,
  setLibraryGroupBy,
  setLibrarySidebarOpen,
  startBatchBuildTask,
  workspaceVersion,
} from "./store";
import { GroupSidebar, type SidebarGroupSelection } from "./library/GroupSidebar";
import {
  cellPadding,
  renderLibraryCellContent,
  renderLibraryHeaderContent,
  ROW_LINE,
  type LibraryRow,
} from "./library/cells";
import { GroupedProblemList } from "./library/GroupedProblemList";
import { BatchFloatingBar } from "./library/BatchFloatingBar";
import { BatchInspector } from "./library/BatchInspector";

const SEARCH_DEBOUNCE_MS = 120;
const AUTO_COLLAPSE_WIDTH = 1040;

function FilterMenu(props: { label: string; active: boolean; entries: MenuEntry[]; multi?: boolean; onSelect: (id: string) => void }) {
  return (
    <AppMenu
      entries={props.entries}
      stayOpen={props.multi}
      onSelect={props.onSelect}
      trigger={
        <View
          class={
            props.active
              ? "bg-accent border border-border hover:bg-accent/80 active:bg-accent focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-ring"
              : "border border-transparent hover:bg-muted active:bg-accent focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-ring"
          }
          style={{
            height: 28,
            maxWidth: 112,
            paddingLeft: 8,
            paddingRight: 6,
            borderRadius: 6,
            flexDirection: "row",
            alignItems: "center",
            gap: 4,
            flexShrink: 0,
            transitionProperty: "colors",
            transitionDuration: ms(120),
            transitionTimingFunction: EASE_STANDARD,
          }}
        >
          <Text
            style={{ fontSize: 13, lineHeight: ROW_LINE, flexShrink: 1, minWidth: 0, numberOfLines: 1, textOverflow: "ellipsis" }}
            class={props.active ? "text-foreground" : "text-secondary-foreground"}
          >
            {props.label}
          </Text>
          <Chevron size={13} tone={props.active ? "foreground" : "muted-foreground"} />
        </View>
      }
    />
  );
}

export function LibraryView(props: { active: boolean }) {
  const inputText = signal("");
  const queryText = signal("");
  const tags = signal<string[]>([]);
  const tier = signal<TierName | undefined>(undefined);
  const platform = signal<string | undefined>(undefined);
  const trainingStatusFilter = signal<string | undefined>(undefined);
  const sidebarGroup = signal<SidebarGroupSelection>({ kind: "all" });
  const sort = signal<DataTableSort | null>(null);
  const selection = signal<SelectionState>(EMPTY_SELECTION);

  const outerWidth = signal(1280);
  const narrowSidebarOverride = signal<boolean | undefined>(undefined);
  const width = signal(0); // table container width
  let table: DataTableRef | undefined;
  let search: InputRef | undefined;
  let searchTimer: number | undefined;
  let widthTimer: number | undefined;
  cleanup(() => {
    clearTimeout(searchTimer);
    clearTimeout(widthTimer);
  });

  const isNarrowWindow = () => outerWidth() > 0 && outerWidth() < AUTO_COLLAPSE_WIDTH;
  const effectiveSidebarOpen = () => {
    if (isNarrowWindow()) return narrowSidebarOverride() ?? false;
    return librarySidebarOpen();
  };

  const toggleSidebar = () => {
    if (isNarrowWindow()) {
      narrowSidebarOverride.set(!effectiveSidebarOpen());
    } else {
      setLibrarySidebarOpen(!librarySidebarOpen());
    }
  };

  // Reset session narrow override when returning to wide window.
  effect(() => {
    if (!isNarrowWindow() && narrowSidebarOverride() !== undefined) {
      narrowSidebarOverride.set(undefined);
    }
  });

  const setSearch = (val: string) => {
    inputText.set(val);
    clearTimeout(searchTimer);
    if (val === "") {
      queryText.set("");
      return;
    }
    searchTimer = setTimeout(() => queryText.set(val), SEARCH_DEBOUNCE_MS);
  };

  const clearFilters = () => {
    clearTimeout(searchTimer);
    batch(() => {
      inputText.set("");
      queryText.set("");
      tags.set([]);
      tier.set(undefined);
      platform.set(undefined);
      trainingStatusFilter.set(undefined);
      sidebarGroup.set({ kind: "all" });
    });
  };

  // Clear stale filters, sort, and selection whenever the user switches workspaces.
  effect(() => {
    void workspaceVersion();
    clearFilters();
    sort.set(null);
    selection.set(EMPTY_SELECTION);
  });

  const readOnly = () => info().readOnly;
  const trainingEnabled = () => info().trainingEnabled;
  const totalCount = () => info().problems.length;

  effect(() => {
    if (!trainingEnabled() && trainingStatusFilter() !== undefined) {
      trainingStatusFilter.set(undefined);
    }
  });

  const facets = computed(() => {
    void revision();
    return preview.workspaceFacets();
  });

  // If the selected named group was deleted externally, fall back to "all".
  effect(() => {
    const sel = sidebarGroup();
    if (sel.kind === "named" && !facets().groups.some((g) => g.name === sel.name)) {
      sidebarGroup.set({ kind: "all" });
    }
  });

  const rows = computed<LibraryRow[]>(() => {
    void revision();
    const range = TIER_RANGES.find((r) => r.tier === tier());
    const sg = sidebarGroup();
    const found = preview.searchProblems({
      text: queryText().trim() || null,
      tags: tags(),
      ratingMin: range ? range.min : null,
      ratingMax: range && range.max !== null ? range.max - 1 : null,
      platform: platform() ?? null,
      group: sg.kind === "named" ? sg.name : null,
      ungroupedOnly: sg.kind === "ungrouped",
    });
    const activeTrainingFilter = trainingEnabled() ? trainingStatusFilter() : undefined;
    const list: LibraryRow[] = found
      .filter((p) => {
        if (!activeTrainingFilter) return true;
        return (p.trainingStatus || "none") === activeTrainingFilter;
      })
      .map((p) => ({
        ...p,
        ratingText: p.rating == null ? "" : String(p.rating),
        tagsText: p.tags.join(" "),
        source: p.sources.map((s) => `${s.platform} ${s.id}`).join("  "),
        status: `${p.packageStatus ?? ""}:${p.hasEditorial ? "e" : ""}:${p.hasSolutions ? "c" : ""}`,
      }));
    const s = sort();
    if (s) {
      const dir = s.direction === "descending" ? -1 : 1;
      list.sort((a, b) => {
        if (s.key === "ratingText") {
          if (a.rating == null && b.rating == null) return a.id.localeCompare(b.id);
          if (a.rating == null) return 1; // keep unrated at the bottom
          if (b.rating == null) return -1;
          return (a.rating - b.rating) * dir;
        }
        if (s.key === "title") return a.title.localeCompare(b.title, locale()) * dir;
        return a.id.localeCompare(b.id) * dir;
      });
    }
    return list;
  });

  const visibleIds = computed(() => rows().map((r) => r.id));

  const sections = computed(() =>
    groupProblems(
      rows(),
      libraryGroupBy(),
      facets().groups.map((g) => g.name),
      info().contests,
    ),
  );

  // Ordered visible IDs (follows section order when grouped so Shift+Click range matches visual order).
  const orderedVisibleIds = computed(() => {
    if (libraryGroupBy() === "none") return visibleIds();
    const out: string[] = [];
    for (const sec of sections()) {
      for (const r of sec.rows) out.push(r.id);
    }
    return out;
  });

  const colLayout = computed(() => computeLibraryColumns(width()));
  const columns = computed(() => colLayout().columns);

  const filtered = () =>
    inputText().trim() !== "" ||
    queryText().trim() !== "" ||
    tags().length > 0 ||
    tier() !== undefined ||
    platform() !== undefined ||
    (trainingEnabled() && trainingStatusFilter() !== undefined) ||
    sidebarGroup().kind !== "all";

  const visibleSelected = computed(() => {
    const id = selectedProblem();
    return id && rows().some((r) => r.id === id) ? id : undefined;
  });

  const selectedRow = computed(() => {
    const id = visibleSelected();
    return id ? rows().findIndex((r) => r.id === id) : -1;
  });

  const headerTriState = computed(() => headerCheckboxState(selection().selectedIds, orderedVisibleIds()));

  const selectedProblemsList = computed(() => {
    const sel = selection().selectedIds;
    if (sel.size === 0) return [];
    const byId = new Map(rows().map((r) => [r.id, r]));
    const out: LibraryRow[] = [];
    for (const id of sel) {
      const r = byId.get(id);
      if (r) out.push(r);
    }
    return out;
  });

  const selectedIdArray = () => [...selection().selectedIds];

  // Prune multi-selection whenever visible rows change (prevents acting on hidden rows).
  effect(() => {
    const ids = orderedVisibleIds();
    const cur = selection();
    const next = applyRowSelection(cur, ids, { kind: "prune" });
    if (next !== cur) queueMicrotask(() => selection.set(next));
  });

  let lastSelectedId: string | undefined = selectedProblem();
  effect(() => {
    const list = rows();
    const id = selectedProblem();
    const selectionChanged = id !== lastSelectedId;
    lastSelectedId = id;
    if (list.length === 0) {
      if (id !== undefined) {
        if (selectionChanged && filtered() && info().problems.some((p) => p.id === id)) {
          queueMicrotask(clearFilters);
          return;
        }
        queueMicrotask(() => selectedProblem.set(undefined));
      }
    } else if (id === undefined || !list.some((r) => r.id === id)) {
      if (selectionChanged && id !== undefined && filtered() && info().problems.some((p) => p.id === id)) {
        queueMicrotask(clearFilters);
        return;
      }
      queueMicrotask(() => selectedProblem.set(list[0].id));
    }
  });

  effect(() => {
    const row = selectedRow();
    if (row >= 0 && libraryGroupBy() === "none") table?.scrollToRow(row, false);
  });

  const applySel = (action: Parameters<typeof applyRowSelection>[2]) => {
    const next = applyRowSelection(
      { ...selection(), focusedId: selection().focusedId ?? selectedProblem() },
      orderedVisibleIds(),
      action,
    );
    batch(() => {
      selection.set(next);
      if (next.focusedId && next.focusedId !== selectedProblem()) {
        selectedProblem.set(next.focusedId);
      }
    });
  };

  function move(delta: number) {
    const ids = orderedVisibleIds();
    if (ids.length === 0) return;
    const curId = selectedProblem();
    const cur = curId ? ids.indexOf(curId) : -1;
    const i = cur < 0 ? 0 : Math.min(ids.length - 1, Math.max(0, cur + delta));
    const nextId = ids[i];
    batch(() => {
      selectedProblem.set(nextId);
      selection.set({ selectedIds: new Set(), anchorId: nextId, focusedId: nextId });
    });
    if (libraryGroupBy() === "none") table?.scrollToRow(i, false);
  }

  function open() {
    const id = visibleSelected();
    if (id) openStudio({ target: "problem", id });
  }

  keymap.bind({ j: () => props.active && move(1), k: () => props.active && move(-1) }, { when: "notEditing" });
  keymap.bind("mod+a", () => {
    if (!props.active) return;
    applySel({ kind: "selectAll" });
  }, { when: "notEditing" });
  keymap.bind("escape", () => {
    if (!props.active) return;
    if (selection().selectedIds.size > 0) {
      applySel({ kind: "clear" });
    } else if (inputText().length > 0) {
      setSearch("");
    }
  }, { when: "notEditing" });
  keymap.bind("mod+n", () => props.active && !readOnly() && dialog.set({ kind: "newProblem" }));
  keymap.bind("mod+f", () => props.active && search?.focus());

  const moveIdsToGroup = (ids: string[], group: string | null) => {
    if (ids.length === 0 || readOnly()) return;
    const res = preview.batchSetGroup(ids, group);
    if (res.ok || res.succeeded > 0) refresh();
  };

  const openContextMenuForRow = (row: LibraryRow, x: number, y: number, onClose?: () => void) => {
    const ro = readOnly();
    const sel = selection().selectedIds;
    if (sel.has(row.id) && sel.size >= 2) {
      const ids = [...sel];
      const groupItems = [
        ...facets().groups.map((g) => ({
          label: g.name,
          disabled: ro,
          onSelect: () => moveIdsToGroup(ids, g.name),
        })),
        ...(facets().groups.length > 0 ? [{ type: "separator" as const }] : []),
        { label: t("library.menu.ungroup"), disabled: ro, onSelect: () => moveIdsToGroup(ids, null) },
        { label: t("library.sidebar.newGroup"), disabled: ro, onSelect: () => dialog.set({ kind: "newGroup" }) },
      ];
      void DesktopMenu.popup(
        [
          { label: t("library.batch.selectedCount", { count: ids.length }), disabled: true },
          { type: "separator" },
          { label: t("library.batch.addToContest"), disabled: ro, onSelect: () => dialog.set({ kind: "batchAddToContest", problemIds: ids }) },
          { label: t("library.batch.moveToGroup"), disabled: ro, submenu: groupItems },
          { label: t("library.batch.editTags"), disabled: ro, onSelect: () => dialog.set({ kind: "batchTags", problemIds: ids }) },
          { label: t("library.batch.setRating"), disabled: ro, onSelect: () => dialog.set({ kind: "batchRating", problemIds: ids }) },
          { type: "separator" },
          { label: t("library.batch.buildVerify"), disabled: ro, onSelect: () => { startBatchBuildTask(ids, "verify"); } },
          { label: t("library.batch.buildDebug"), disabled: ro, onSelect: () => { startBatchBuildTask(ids, "debug"); } },
          { label: t("library.batch.export"), disabled: ro, onSelect: () => dialog.set({ kind: "batchExport", problemIds: ids }) },
          { type: "separator" },
          { label: t("library.batch.delete"), disabled: ro, destructive: true, onSelect: () => dialog.set({ kind: "batchDelete", problemIds: ids }) },
        ],
        { x, y },
      ).finally(() => onClose?.());
      return;
    }

    selectedProblem.set(row.id);
    const singleGroupSubmenu = [
      ...facets().groups.map((g) => ({
        label: g.name,
        checked: row.group === g.name,
        disabled: ro,
        onSelect: () => moveIdsToGroup([row.id], g.name),
      })),
      ...(facets().groups.length > 0 ? [{ type: "separator" as const }] : []),
      { label: t("library.menu.ungroup"), checked: !row.group, disabled: ro, onSelect: () => moveIdsToGroup([row.id], null) },
      { label: t("library.sidebar.newGroup"), disabled: ro, onSelect: () => dialog.set({ kind: "newGroup" }) },
    ];

    const setProblemTrainingStatus = (problemId: string, status: string) => {
      if (ro) return;
      const res = preview.setTrainingStatus(problemId, status);
      if (res.ok) refresh();
    };

    const trainingSubmenu = [
      {
        label: t("training.status.todo"),
        checked: row.trainingStatus === "todo",
        disabled: ro,
        onSelect: () => setProblemTrainingStatus(row.id, "todo"),
      },
      {
        label: t("training.status.attempted"),
        checked: row.trainingStatus === "attempted",
        disabled: ro,
        onSelect: () => setProblemTrainingStatus(row.id, "attempted"),
      },
      {
        label: t("training.status.ac"),
        checked: row.trainingStatus === "ac",
        disabled: ro,
        onSelect: () => setProblemTrainingStatus(row.id, "ac"),
      },
      {
        label: t("training.status.upsolved"),
        checked: row.trainingStatus === "upsolved",
        disabled: ro,
        onSelect: () => setProblemTrainingStatus(row.id, "upsolved"),
      },
      { type: "separator" as const },
      {
        label: t("training.status.clearManual"),
        disabled: ro,
        onSelect: () => setProblemTrainingStatus(row.id, "clear"),
      },
    ];

    void DesktopMenu.popup(
      [
        { label: ro ? t("common.view") : t("common.edit"), onSelect: () => openStudio({ target: "problem", id: row.id }) },
        {
          label: t("library.menu.copyMarkdownWithSamples"),
          onSelect: () => {
            try {
              void Clipboard.writeText(preview.exportMarkdownWithSamples(row.id, null));
            } catch {
              // Ignore errors when problem files are unreadable.
            }
          },
        },
        { label: t("library.menu.duplicate"), disabled: ro, onSelect: () => dialog.set({ kind: "duplicateProblem", problemId: row.id }) },
        {
          label: t("library.menu.createPackage"),
          disabled: ro || preview.inspectPackage(row.id, null).hasPackage,
          onSelect: () => {
            const res = preview.initPackage(row.id);
            if (res.ok) {
              refresh();
              openStudio({ target: "problem", id: row.id, initialTab: "build" });
            }
          },
        },
        ...(trainingEnabled()
          ? [{ label: t("library.menu.trainingStatus"), disabled: ro, submenu: trainingSubmenu }]
          : []),
        { label: t("library.menu.moveToGroup"), disabled: ro, submenu: singleGroupSubmenu },
        { label: t("library.menu.addToContest"), disabled: ro, onSelect: () => dialog.set({ kind: "addToContest", problemId: row.id }) },
        { label: t("common.revealInFolder"), onSelect: () => void preview.revealInFolder(`problems/${row.id}`) },
        { type: "separator" },
        { label: t("library.menu.delete"), disabled: ro, destructive: true, onSelect: () => dialog.set({ kind: "deleteProblem", problemId: row.id }) },
      ],
      { x, y },
    ).finally(() => onClose?.());
  };

  const tagItems = (): MenuEntry[] => facets().tags.map((tg) => ({ id: tg, label: tg, checked: tags().includes(tg) }));
  const tierItems = (): MenuEntry[] => [
    { id: "", label: t("library.filter.allRatings"), checked: tier() === undefined },
    ...TIER_RANGES.map((r) => ({ id: r.tier, label: r.label, checked: tier() === r.tier })),
  ];
  const platformItems = (): MenuEntry[] =>
    facets().platforms.length > 0
      ? [
          { id: "", label: t("library.filter.allSources"), checked: platform() === undefined },
          ...facets().platforms.map((p) => ({ id: p, label: p, checked: platform() === p })),
        ]
      : [{ id: "__empty__", label: t("library.filter.noSources"), disabled: true }];

  const trainingFilterItems = (): MenuEntry[] => [
    { id: "", label: t("training.filter.all"), checked: trainingStatusFilter() === undefined },
    { id: "none", label: t("training.status.none"), checked: trainingStatusFilter() === "none" },
    { id: "todo", label: t("training.status.todo"), checked: trainingStatusFilter() === "todo" },
    { id: "attempted", label: t("training.status.attempted"), checked: trainingStatusFilter() === "attempted" },
    { id: "ac", label: t("training.status.ac"), checked: trainingStatusFilter() === "ac" },
    { id: "upsolved", label: t("training.status.upsolved"), checked: trainingStatusFilter() === "upsolved" },
  ];

  const trainingFilterLabel = () => {
    const st = trainingStatusFilter();
    if (!st) return t("training.filter.label");
    if (st === "none") return t("training.status.none");
    if (st === "todo") return t("training.status.todo");
    if (st === "attempted") return t("training.status.attempted");
    if (st === "ac") return t("training.status.ac");
    return t("training.status.upsolved");
  };

  const groupByItems = (): MenuEntry[] => [
    { id: "none", label: t("library.groupBy.none"), checked: libraryGroupBy() === "none" },
    { id: "group", label: t("library.groupBy.group"), checked: libraryGroupBy() === "group" },
    { id: "rating", label: t("library.groupBy.rating"), checked: libraryGroupBy() === "rating" },
    { id: "platform", label: t("library.groupBy.platform"), checked: libraryGroupBy() === "platform" },
    { id: "contest", label: t("library.groupBy.contest"), checked: libraryGroupBy() === "contest" },
    { id: "package_status", label: t("library.groupBy.packageStatus"), checked: libraryGroupBy() === "package_status" },
  ];

  const groupByLabel = () => {
    const mode = libraryGroupBy();
    if (mode === "none") return t("library.groupBy.label");
    if (mode === "group") return t("library.groupBy.group");
    if (mode === "rating") return t("library.groupBy.rating");
    if (mode === "platform") return t("library.groupBy.platform");
    if (mode === "contest") return t("library.groupBy.contest");
    return t("library.groupBy.packageStatus");
  };

  const rowClasses = computed(() => {
    const sel = selection().selectedIds;
    if (sel.size === 0) return [];
    return rows().map((r) => (sel.has(r.id) ? "bg-accent" : ""));
  });

  const showBatchInspector = () => selection().selectedIds.size >= 2 || batchTask() !== undefined;
  const rightPanelVisible = () => showBatchInspector() || visibleSelected() !== undefined;

  const emptyElement = () =>
    filtered() ? (
      <EmptyState
        icon="searchX"
        title={t("library.emptyFilteredTitle")}
        hint={t("library.emptyFilteredHint")}
        action={<Btn label={t("library.filter.clear")} icon="filterX" onPress={clearFilters} />}
      />
    ) : (
      <EmptyState
        icon="libraryBig"
        title={t("library.emptyTitle")}
        hint={readOnly() ? t("common.readOnlyWorkspaceHint") : t("library.emptyHint")}
        action={
          readOnly() ? undefined : (
            <Btn
              label={t("library.newProblem")}
              icon="plus"
              variant="primary"
              onPress={() => dialog.set({ kind: "newProblem" })}
              tooltip={tip(t("library.newProblem"), "mod+n")}
            />
          )
        }
      />
    );

  return (
    <View
      style={{ flex: 1, flexDirection: "row" }}
      class="bg-background"
      onLayout={(e) => {
        const w = Math.round(e.bounds.size.width);
        if (w > 0 && Math.abs(w - outerWidth()) >= 2) outerWidth.set(w);
      }}
    >
      <If when={effectiveSidebarOpen()}>
        <GroupSidebar
          selection={sidebarGroup()}
          onSelect={(sel) => sidebarGroup.set(sel)}
          totalCount={facets().totalCount}
          ungroupedCount={facets().ungroupedCount}
          groups={facets().groups}
          readOnly={readOnly()}
          onNewGroup={() => dialog.set({ kind: "newGroup" })}
          onRenameGroup={(name) => dialog.set({ kind: "renameGroup", name })}
          onDeleteGroup={(name) => dialog.set({ kind: "deleteGroup", name })}
        />
      </If>

      <View style={{ flex: 1, minWidth: 0 }}>
        <ResizablePanelGroup handleClass="bg-border w-px group-hover:bg-primary/50 group-active:bg-primary transition-colors">
          <ResizablePanel minSize={320}>
            <View
              style={{ flex: 1, minWidth: 0, position: "relative" }}
              onLayout={(e) => {
                const nextW = Math.round(e.bounds.size.width);
                if (nextW <= 0) return;
                if (width() === 0) {
                  width.set(nextW);
                  return;
                }
                if (Math.abs(nextW - width()) < 2) return;
                clearTimeout(widthTimer);
                widthTimer = setTimeout(() => {
                  widthTimer = undefined;
                  width.set(nextW);
                }, 80);
              }}
            >
              <Toolbar>
                <IconBtn
                  testID="library-sidebar-toggle"
                  icon="panelLeft"
                  tone={effectiveSidebarOpen() ? "foreground" : "muted-foreground"}
                  tooltip={tip(t("library.sidebar.toggle"))}
                  onPress={toggleSidebar}
                  style={{ flexShrink: 0 }}
                />
                <View
                  style={{
                    flexBasis: 200,
                    flexShrink: 1,
                    minWidth: 100,
                    height: 28,
                    borderRadius: 6,
                    borderWidth: 1,
                    flexDirection: "row",
                    alignItems: "center",
                    paddingLeft: 8,
                    paddingRight: 4,
                    gap: 6,
                  }}
                  class="border-border bg-card focus-within:border-primary focus-within:ring-1 focus-within:ring-inset focus-within:ring-ring transition-colors"
                  captureKeys={["up", "down", "enter", "escape"]}
                  onKeyDown={(e) => {
                    if (e.key === "ArrowDown") move(1);
                    else if (e.key === "ArrowUp") move(-1);
                    else if (e.key === "Enter") open();
                    else if (e.key === "Escape") {
                      if (selection().selectedIds.size > 0) applySel({ kind: "clear" });
                      else if (inputText().length > 0) setSearch("");
                    }
                  }}
                >
                  <Icon name="search" size={14} tone="muted-foreground" />
                  <Input
                    ref={(r) => (search = r)}
                    testID="library-search"
                    class="bg-transparent placeholder:text-muted-foreground"
                    style={{ flex: 1, height: 26, fontSize: 13 }}
                    value={inputText()}
                    placeholder={t("library.searchPlaceholder")}
                    autoCapitalize="none"
                    autoCorrect={false}
                    autoFocus
                    onChange={(e) => setSearch(e.value)}
                  />
                  <If when={inputText().length > 0}>
                    <View
                      testID="library-search-clear"
                      onPress={() => {
                        setSearch("");
                        search?.focus();
                      }}
                      tooltip={tip(t("library.clearSearch"), "escape")}
                      class="hover:bg-muted active:bg-accent"
                      style={{
                        width: 20,
                        height: 20,
                        borderRadius: 4,
                        alignItems: "center",
                        justifyContent: "center",
                        flexShrink: 0,
                        transitionProperty: "colors",
                        transitionDuration: ms(120),
                        transitionTimingFunction: EASE_STANDARD,
                      }}
                    >
                      <Icon name="x" size={12} tone="muted-foreground" />
                    </View>
                  </If>
                </View>
                <FilterMenu
                  label={tags().length === 0 ? t("library.filter.tags") : tags().length === 1 ? tags()[0] : t("library.filter.tagsCount", { count: tags().length })}
                  active={tags().length > 0}
                  multi
                  entries={tagItems().length > 0 ? tagItems() : [{ id: "__empty__", label: t("library.filter.noTags"), disabled: true }]}
                  onSelect={(id) => id && id !== "__empty__" && tags.set(tags().includes(id) ? tags().filter((x) => x !== id) : [...tags(), id])}
                />
                <FilterMenu
                  label={TIER_RANGES.find((r) => r.tier === tier())?.label ?? t("library.filter.rating")}
                  active={tier() !== undefined}
                  entries={tierItems()}
                  onSelect={(id) => tier.set(id ? (id as TierName) : undefined)}
                />
                <FilterMenu
                  label={platform() ?? t("library.filter.source")}
                  active={platform() !== undefined}
                  entries={platformItems()}
                  onSelect={(id) => id !== "__empty__" && platform.set(id || undefined)}
                />
                <If when={trainingEnabled()}>
                  <FilterMenu
                    label={trainingFilterLabel()}
                    active={trainingStatusFilter() !== undefined}
                    entries={trainingFilterItems()}
                    onSelect={(id) => trainingStatusFilter.set(id || undefined)}
                  />
                </If>
                <FilterMenu
                  label={groupByLabel()}
                  active={libraryGroupBy() !== "none"}
                  entries={groupByItems()}
                  onSelect={(id) => setLibraryGroupBy(id as LibraryGroupBy)}
                />
                <If when={filtered()}>
                  <Btn label={width() >= 660 ? t("common.clear") : undefined} icon="filterX" variant="ghost" tooltip={tip(t("library.filter.clear"))} onPress={clearFilters} style={{ flexShrink: 0 }} />
                </If>
                <Spacer />
                <If when={totalCount() > 0 && width() >= 540}>
                  <Text style={{ fontSize: 12, fontVariant: ["tabular-nums"], flexShrink: 0 }} class="text-muted-foreground">
                    {filtered() ? `${rows().length} / ${totalCount()}` : t("library.count", { count: totalCount() })}
                  </Text>
                </If>
                <Btn
                  testID="library-import-polygon"
                  label={width() >= 820 ? t("packages.importPolygon") : undefined}
                  icon="folderOpen"
                  variant="outline"
                  disabled={readOnly() || importingPolygon()}
                  tooltip={tip(readOnly() ? t("packages.readOnlyCannotImport") : t("packages.importPolygonTooltip"))}
                  onPress={() => void importPolygon()}
                  style={{ flexShrink: 0 }}
                />
                <Btn
                  label={width() >= 700 ? t("library.newProblem") : undefined}
                  icon="plus"
                  disabled={readOnly()}
                  tooltip={tip(readOnly() ? t("library.readOnlyCannotCreate") : t("library.newProblem"), readOnly() ? undefined : "mod+n")}
                  onPress={() => dialog.set({ kind: "newProblem" })}
                  style={{ flexShrink: 0 }}
                />
              </Toolbar>

              <View
                style={{ flex: 1, minHeight: 0 }}
                captureKeys={["up", "down", "enter"]}
                onKeyDown={(e) => {
                  if (e.key === "ArrowDown") move(1);
                  else if (e.key === "ArrowUp") move(-1);
                  else if (e.key === "Enter") open();
                }}
              >
                <If
                  when={libraryGroupBy() === "none"}
                  else={
                    <GroupedProblemList
                      sections={sections()}
                      columns={columns()}
                      showInlineId={colLayout().showInlineId}
                      maxTags={colLayout().maxTags}
                      tagsWidth={colLayout().tagsWidth}
                      trainingEnabled={trainingEnabled()}
                      sort={sort()}
                      onSortChange={(s) => sort.set(s)}
                      headerTriState={headerTriState()}
                      onToggleAll={() => applySel({ kind: "toggleAll" })}
                      selectedIds={selection().selectedIds}
                      focusedId={visibleSelected()}
                      onRowClick={(id, opts) => applySel({ kind: "click", id, ...opts })}
                      onRowDoubleClick={(id) => openStudio({ target: "problem", id })}
                      onToggleGroupSelect={(ids) => applySel({ kind: "toggleGroup", ids })}
                      onRowContextMenu={(row, x, y) => openContextMenuForRow(row, x, y)}
                      empty={emptyElement()}
                    />
                  }
                >
                  <If when={width() > 0}>
                    <DataTable<LibraryRow>
                      ref={(r) => (table = r)}
                      testID="library-table"
                      style={{ flex: 1 }}
                      columns={columns()}
                      data={rows()}
                      rowClasses={rowClasses()}
                      sort={sort()}
                      onSortChange={(e) => sort.set(e.sort)}
                      rowHeight={40}
                      headerHeight={34}
                      fontSize={13.5}
                      columnSelectable={false}
                      movableColumns={false}
                      resizableColumns={false}
                      selectOnModifierPress={false}
                      selection={selection().selectedIds.size === 0 && selectedRow() >= 0 ? { row: selectedRow() } : null}
                      onRowPress={(e) => {
                        const row = rows()[e.row];
                        if (!row) return;
                        const isCheckbox = e.column === 0;
                        const mod = e.control || e.platform;
                        applySel({
                          kind: "click",
                          id: row.id,
                          shift: e.shift,
                          mod,
                          checkbox: isCheckbox,
                        });
                      }}
                      onDoublePress={(e) => {
                        if (e.target.column === 0) return;
                        const row = rows()[e.target.row ?? -1];
                        if (row) openStudio({ target: "problem", id: row.id });
                      }}
                      onContextMenu={(e) => {
                        const row = e.row != null ? rows()[e.row] : undefined;
                        if (!row) return;
                        openContextMenuForRow(row, e.x, e.y, () => table?.clearMenuTarget());
                      }}
                      renderCell={(c) => {
                        const r = rows()[c.row];
                        const pad = cellPadding(c.key);
                        return (
                          <View
                            style={{
                              flex: 1,
                              minWidth: 0,
                              flexDirection: "row",
                              alignItems: "center",
                              justifyContent: pad.justifyContent,
                              paddingLeft: pad.paddingLeft,
                              paddingRight: pad.paddingRight,
                              overflow: "hidden",
                            }}
                          >
                            {renderLibraryCellContent(c.key, r, {
                              selected: r ? selection().selectedIds.has(r.id) : false,
                              showInlineId: colLayout().showInlineId,
                              maxTags: colLayout().maxTags,
                              tagsWidth: colLayout().tagsWidth,
                              trainingEnabled: trainingEnabled(),
                            })}
                          </View>
                        );
                      }}
                      renderHeaderCell={(h) =>
                        renderLibraryHeaderContent({
                          key: h.key,
                          title: h.title ?? "",
                          sortDir: h.sort,
                          triState: headerTriState,
                          onToggleAll: () => applySel({ kind: "toggleAll" }),
                          nativeSortIndicator: true,
                        })
                      }
                      classes={{
                        root: "bg-background",
                        header: "bg-background border-b border-border",
                        row: "bg-background",
                        rowOdd: "bg-background",
                        rowHover: "bg-muted",
                        rowSelected: "bg-accent",
                      }}
                      empty={emptyElement()}
                    />
                  </If>
                </If>
              </View>

              <If when={selection().selectedIds.size >= 1}>
                <BatchFloatingBar
                  count={selection().selectedIds.size}
                  tableWidth={width()}
                  readOnly={readOnly()}
                  groups={facets().groups}
                  onAddToContest={() => dialog.set({ kind: "batchAddToContest", problemIds: selectedIdArray() })}
                  onMoveToGroup={(g) => moveIdsToGroup(selectedIdArray(), g)}
                  onNewGroupAndMove={() => dialog.set({ kind: "newGroup" })}
                  onEditTags={() => dialog.set({ kind: "batchTags", problemIds: selectedIdArray() })}
                  onSetRating={() => dialog.set({ kind: "batchRating", problemIds: selectedIdArray() })}
                  onStartBuild={(mode) => {
                    startBatchBuildTask(selectedIdArray(), mode);
                  }}
                  onExport={() => dialog.set({ kind: "batchExport", problemIds: selectedIdArray() })}
                  onDelete={() => dialog.set({ kind: "batchDelete", problemIds: selectedIdArray() })}
                  onClear={() => applySel({ kind: "clear" })}
                />
              </If>
            </View>
          </ResizablePanel>

          <ResizablePanel
            size={380}
            minSize={300}
            maxSize={560}
            fixed
            visible={rightPanelVisible()}
          >
            <If
              when={showBatchInspector()}
              else={<If when={visibleSelected()}>{(id) => <ProblemInspector id={id()} active={props.active} groups={facets().groups} />}</If>}
            >
              <BatchInspector
                selectedProblems={selectedProblemsList()}
                groups={facets().groups}
                readOnly={readOnly()}
                onClearSelection={() => applySel({ kind: "clear" })}
                onAddToContest={() => dialog.set({ kind: "batchAddToContest", problemIds: selectedIdArray() })}
                onMoveToGroup={(g) => moveIdsToGroup(selectedIdArray(), g)}
                onNewGroupAndMove={() => dialog.set({ kind: "newGroup" })}
                onEditTags={() => dialog.set({ kind: "batchTags", problemIds: selectedIdArray() })}
                onSetRating={() => dialog.set({ kind: "batchRating", problemIds: selectedIdArray() })}
                onStartBuild={(mode) => {
                  startBatchBuildTask(selectedIdArray(), mode);
                }}
                onExport={() => dialog.set({ kind: "batchExport", problemIds: selectedIdArray() })}
                onDelete={() => dialog.set({ kind: "batchDelete", problemIds: selectedIdArray() })}
              />
            </If>
          </ResizablePanel>
        </ResizablePanelGroup>
      </View>
    </View>
  );
}

type Tab = "statements" | "editorial" | "code";

/** The selected problem as it prints, with its sources, referencing contests, and actions. */
function ProblemInspector(props: { id: string; active: boolean; groups: readonly { name: string; count: number }[] }) {
  const tab = signal<Tab>("statements");
  const solution = signal<string | undefined>(undefined);
  let canvas: CanvasController | undefined;

  const detail = computed(() => {
    void revision();
    return preview.problemDetail(props.id);
  });

  const referencingContests = computed(() => {
    const ids = detail().referencingContests;
    const byId = new Map(info().contests.map((c) => [c.id, c]));
    return ids.map((id) => byId.get(id) ?? { id, name: id, frozen: false });
  });

  effect(() => {
    void props.id;
    batch(() => {
      if (tab() === "code" && detail().solutions.length === 0) tab.set("statements");
      if (tab() === "editorial" && !detail().hasEditorial) tab.set("statements");
      solution.set(undefined);
    });
  });

  // Recompile when workspace revision bumps while this inspector is visible.
  let lastRev = revision();
  effect(() => {
    const rev = revision();
    if (props.active && rev !== lastRev) {
      lastRev = rev;
      if (tab() !== "code") canvas?.compile();
    }
  });

  const tabs = computed(() => [
    { value: "statements" as Tab, label: t("library.inspector.statement") },
    ...(detail().hasEditorial ? [{ value: "editorial" as Tab, label: t("library.inspector.editorial") }] : []),
    ...(detail().solutions.length > 0 ? [{ value: "code" as Tab, label: t("library.inspector.code") }] : []),
  ]);

  const request = (): PreviewRequest | undefined => {
    const cur = tab();
    if (cur === "code") return undefined;
    return { target: "problem", id: props.id, book: cur, path: "", text: "" };
  };
  const currentSolution = () => solution() ?? detail().solutions[0];
  const code = computed(() => {
    const name = currentSolution();
    return tab() === "code" && name ? preview.readSolution(props.id, name).text : "";
  });

  const hasMeta = () =>
    !!detail().group || detail().tags.length > 0 || detail().sources.length > 0 || referencingContests().length > 0;
  const readOnly = () => info().readOnly;

  const jumpContest = (contestId: string) => {
    jumpToContest(contestId);
  };

  const moreEntries = (): MenuEntry[] => {
    const ro = readOnly();
    const curGroup = detail().group ?? null;
    const groupEntries: MenuEntry[] = [
      ...props.groups.map((g) => ({
        id: `group:${g.name}`,
        label: `${t("library.menu.moveToGroup")}: ${g.name}`,
        icon: "folder" as const,
        checked: curGroup === g.name,
        disabled: ro,
      })),
      ...(curGroup
        ? [{ id: "group:__clear__", label: t("library.menu.ungroup"), icon: "x" as const, disabled: ro }]
        : []),
    ];
    return [
      { id: "copyMd", label: t("library.menu.copyMarkdownWithSamples"), icon: "fileText" },
      ...(ro || preview.inspectPackage(props.id, null).hasPackage
        ? []
        : [{ id: "createPackage", label: t("library.menu.createPackage"), icon: "code" as const }]),
      { id: "duplicate", label: t("library.menu.duplicate"), icon: "copy", disabled: ro },
      { id: "add", label: t("library.menu.addToContest"), icon: "listPlus", disabled: ro },
      ...(groupEntries.length > 0 ? [{ separator: true as const, id: "sep-grp" }, ...groupEntries] : []),
      { separator: true, id: "sep-rev" },
      { id: "reveal", label: t("common.revealInFolder"), icon: "folderOpen" },
      { separator: true, id: "sep-del" },
      { id: "delete", label: t("library.menu.delete"), icon: "trash", destructive: true, disabled: ro },
    ];
  };

  return (
    <View style={{ flex: 1, minWidth: 0 }} class="bg-card">
      <View style={{ borderBottomWidth: 1 }} class="border-border">
        <DetailHeader
          title={detail().title}
          badge={detail().rating != null ? <RatingBadge rating={detail().rating} /> : undefined}
          second={
            hasMeta() ? (
              <>
                <If when={detail().group}>
                  {(g) => (
                    <View
                      style={{
                        height: 20,
                        maxWidth: 160,
                        minWidth: 0,
                        overflow: "hidden",
                        paddingLeft: 6,
                        paddingRight: 6,
                        borderRadius: 5,
                        flexDirection: "row",
                        alignItems: "center",
                        gap: 4,
                      }}
                      class="bg-muted"
                    >
                      <Icon name="folder" size={11} tone="muted-foreground" />
                      <Text
                        style={{ fontSize: 11.5, flexShrink: 1, minWidth: 0, numberOfLines: 1, textOverflow: "ellipsis" }}
                        class="text-secondary-foreground"
                      >
                        {g()}
                      </Text>
                    </View>
                  )}
                </If>
                <Each of={detail().tags} key={(tg) => tg}>
                  {(tg) => <Chip label={tg()} />}
                </Each>
                <Each of={detail().sources} key={(s) => `${s.platform}:${s.id}`}>
                  {(s) => (
                    <View
                      tooltip={tip(`${s().platform} ${s().id}`)}
                      style={{
                        height: 20,
                        maxWidth: 180,
                        minWidth: 0,
                        overflow: "hidden",
                        paddingLeft: 6,
                        paddingRight: 6,
                        borderRadius: 5,
                        flexDirection: "row",
                        alignItems: "center",
                        gap: 4,
                      }}
                      class="bg-muted"
                    >
                      <Icon name="link" size={11} tone="muted-foreground" />
                      <Text
                        style={{
                          fontSize: 11.5,
                          fontFamily: MONO,
                          flexShrink: 1,
                          minWidth: 0,
                          numberOfLines: 1,
                          textOverflow: "ellipsis",
                        }}
                        class="text-secondary-foreground"
                      >
                        {`${s().platform} ${s().id}`}
                      </Text>
                    </View>
                  )}
                </Each>
                <Each of={referencingContests()} key={(c) => c.id}>
                  {(c) => (
                    <View
                      onPress={() => jumpContest(c().id)}
                      tooltip={tip(t("library.inspector.jumpContest", { name: c().name }))}
                      class="bg-muted hover:bg-accent active:bg-accent focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-ring"
                      style={{
                        height: 20,
                        maxWidth: 168,
                        minWidth: 0,
                        overflow: "hidden",
                        paddingLeft: 6,
                        paddingRight: 6,
                        borderRadius: 5,
                        flexDirection: "row",
                        alignItems: "center",
                        gap: 4,
                        transitionProperty: "colors",
                        transitionDuration: ms(120),
                        transitionTimingFunction: EASE_STANDARD,
                      }}
                    >
                      <Icon name="trophy" size={11} tone="muted-foreground" />
                      <Text
                        style={{
                          fontSize: 11.5,
                          flexShrink: 1,
                          minWidth: 0,
                          numberOfLines: 1,
                          textOverflow: "ellipsis",
                        }}
                        class="text-secondary-foreground"
                      >
                        {c().name}
                      </Text>
                    </View>
                  )}
                </Each>
              </>
            ) : undefined
          }
          actions={
            <>
              <Btn
                variant="primary"
                icon={readOnly() ? "fileText" : "pencil"}
                label={readOnly() ? t("common.view") : t("common.edit")}
                tooltip={tip(readOnly() ? t("library.inspector.viewTooltip") : t("library.inspector.editTooltip"), "enter")}
                onPress={() => openStudio({ target: "problem", id: props.id })}
              />
              <AppMenu
                align="end"
                entries={moreEntries()}
                onSelect={(id) => {
                  if (id === "copyMd") {
                    try {
                      void Clipboard.writeText(preview.exportMarkdownWithSamples(props.id, null));
                    } catch {
                      // Ignore errors when problem files are unreadable.
                    }
                  } else if (id === "createPackage") {
                    const res = preview.initPackage(props.id);
                    if (res.ok) {
                      refresh();
                      openStudio({ target: "problem", id: props.id, initialTab: "build" });
                    }
                  } else if (id === "duplicate") dialog.set({ kind: "duplicateProblem", problemId: props.id });
                  else if (id === "add") dialog.set({ kind: "addToContest", problemId: props.id });
                  else if (id === "group:__clear__") {
                    const res = preview.batchSetGroup([props.id], null);
                    if (res.ok || res.succeeded > 0) refresh();
                  } else if (id.startsWith("group:")) {
                    const res = preview.batchSetGroup([props.id], id.slice(6));
                    if (res.ok || res.succeeded > 0) refresh();
                  } else if (id === "reveal") void preview.revealInFolder(`problems/${props.id}`);
                  else if (id === "delete") dialog.set({ kind: "deleteProblem", problemId: props.id });
                }}
                trigger={<MoreTrigger testID="problem-more" />}
              />
            </>
          }
        />
      </View>
      <If when={tabs().length > 1}>
        <View style={{ height: 36, flexDirection: "row", alignItems: "stretch", paddingLeft: 12, paddingRight: 12, gap: 8, borderBottomWidth: 1 }} class="border-border">
          <Segmented items={tabs()} value={tab()} onChange={(v) => tab.set(v)} itemWidth={72} underline />
          <View style={{ flex: 1 }} />
          <If when={tab() === "code" && detail().solutions.length > 1}>
            <View style={{ justifyContent: "center" }}>
              <AppMenu
                align="end"
                entries={detail().solutions.map((s) => ({ id: s, label: s, checked: s === currentSolution() }))}
                onSelect={(id) => solution.set(id)}
                trigger={
                  <View
                    class="hover:bg-muted active:bg-accent focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-ring"
                    style={{
                      height: 26,
                      paddingLeft: 8,
                      paddingRight: 6,
                      borderRadius: 6,
                      flexDirection: "row",
                      alignItems: "center",
                      gap: 4,
                      transitionProperty: "colors",
                      transitionDuration: ms(120),
                      transitionTimingFunction: EASE_STANDARD,
                    }}
                  >
                    <Text style={{ fontSize: 12, fontFamily: MONO }} class="text-secondary-foreground">
                      {currentSolution() ?? ""}
                    </Text>
                    <Chevron size={13} />
                  </View>
                }
              />
            </View>
          </If>
        </View>
      </If>
      <If
        when={tab() === "code"}
        else={<PreviewCanvas request={request} controller={(c) => (canvas = c)} hud={false} active={props.active} />}
      >
        <Editor
          style={{ flex: 1 }}
          value={code()}
          language={currentSolution() ?? "cpp"}
          readOnly
          fontFamily={monoFont()}
          fontSize={12.5}
          lineHeight={20}
          activeLine={false}
          scrollBeyondLastLine={false}
          class="bg-card"
          classes={{ gutter: "pl-2 pr-1", content: "pl-2 pt-2" }}
        />
      </If>
    </View>
  );
}

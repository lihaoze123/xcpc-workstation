// Cmd+1: the problem library (Phase 6D §8.1).
// Collapsible persistent group sidebar on the left, fixed ~440pt headerless problem list
// (40pt single-line rows, unified filter+sort popover, 28pt active filter chip bar, `+` split menu),
// multi-select floating batch bar, and right-hand reader-mode ProblemInspector / BatchInspector.

import { batch, cleanup, Clipboard, computed, effect, keymap, Scroll, signal, Text, View, type InputRef, type ScrollRef } from "nsh";
import {
  Editor,
  Input,
  Popover,
  ResizablePanel,
  ResizablePanelGroup,
} from "nsh/base";
import { Menu as DesktopMenu } from "nsh/desktop";
import type { PreviewRequest } from "xcpc-workstation/native";
import { Btn, Chevron, Chip, IconBtn, RatingBadge, Segmented, tip } from "./kit";
import { locale, t } from "./i18n";
import { Icon } from "./Icon";
import { AppMenu, MoreTrigger, type MenuEntry } from "./Menu";
import { DetailHeader, EmptyState, Toolbar } from "./layout";
import {
  applyRowSelection,
  EMPTY_SELECTION,
  groupProblems,
  TIER_RANGES,
  type LibraryGroupBy,
  type SelectionState,
  type TierName,
} from "./logic";
import { colorTransition, pressActiveStyle, pressTransition } from "./motion";
import { EASE_STANDARD, MONO, monoFont, ms, popoverShadow } from "./theme";
import { PreviewCanvas, type CanvasController } from "./studio/PreviewCanvas";
import {
  activeRevision,
  batchTask,
  dialog,
  importPolygon,
  importingPolygon,
  info,
  jumpToContest,
  libraryGroupBy,
  librarySidebarOpen,
  openNewSnippetModal,
  openStudio,
  pluginsRevision,
  preview,
  refresh,
  selectedProblem,
  setLibraryGroupBy,
  setLibrarySidebarOpen,
  startBatchBuildTask,
  workspaceVersion,
} from "./store";
import { showToast } from "./Toast";
import {
  parseJsonReport,
  type ImportAppliedReport,
  type RegisteredImporter,
} from "./tools/types";
import { GroupSidebar, type SidebarGroupSelection } from "./library/GroupSidebar";
import {
  DEFAULT_LIBRARY_SORT,
  LibraryListRow,
  sortLibraryRows,
  type LibraryRow,
  type LibrarySortKey,
} from "./library/cells";
import { GroupedProblemList } from "./library/GroupedProblemList";
import { BatchFloatingBar } from "./library/BatchFloatingBar";
import { BatchInspector } from "./library/BatchInspector";
import { CONTROL, FONT_SIZE, FONT_WEIGHT, LINE_HEIGHT, RADIUS, ROW, SPACE, TYPE } from "./tokens";

const SEARCH_DEBOUNCE_MS = 120;
const AUTO_COLLAPSE_WIDTH = 1040;

interface ActiveFilterChip {
  id: string;
  label: string;
  onRemove: () => void;
}

/** Compact selectable pill used inside the grouped Filter popover. */
function FilterOptionPill(props: { label: string; active: boolean; onPress: () => void }) {
  return (
    <View
      onPress={props.onPress}
      class={
        props.active
          ? "bg-accent border border-primary/40 text-foreground"
          : "bg-muted/60 border border-transparent hover:bg-muted text-secondary-foreground"
      }
      style={{
        height: CONTROL.sm,
        paddingLeft: SPACE[8],
        paddingRight: SPACE[8],
        borderRadius: RADIUS.sm,
        alignItems: "center",
        justifyContent: "center",
        ...colorTransition(120),
      }}
    >
      <Text
        style={{ fontSize: FONT_SIZE.caption, lineHeight: LINE_HEIGHT.caption, numberOfLines: 1, textOverflow: "ellipsis" }}
        class={props.active ? "text-foreground" : "text-secondary-foreground"}
      >
        {props.label}
      </Text>
    </View>
  );
}

/** Radio option row for the bottom Sort section of the Filter popover. */
function SortRadioRow(props: { label: string; selected: boolean; onPress: () => void; testID?: string }) {
  return (
    <View
      testID={props.testID}
      onPress={props.onPress}
      class="hover:bg-muted active:bg-accent"
      style={{
        height: CONTROL.lg,
        paddingLeft: SPACE[8],
        paddingRight: SPACE[8],
        borderRadius: RADIUS.sm,
        flexDirection: "row",
        alignItems: "center",
        gap: SPACE[8],
        ...colorTransition(120),
      }}
    >
      <View
        class={props.selected ? "border border-primary" : "border border-border"}
        style={{
          width: 14,
          height: 14,
          borderRadius: RADIUS.full,
          alignItems: "center",
          justifyContent: "center",
          flexShrink: 0,
        }}
      >
        <If when={props.selected}>
          <View style={{ width: 6, height: 6, borderRadius: RADIUS.full }} class="bg-primary" />
        </If>
      </View>
      <Text style={{ ...TYPE.body, flex: 1 }} class="text-foreground">
        {props.label}
      </Text>
    </View>
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
  const sortKey = signal<LibrarySortKey>(DEFAULT_LIBRARY_SORT);
  const filterOpen = signal(false);
  const selection = signal<SelectionState>(EMPTY_SELECTION);

  const outerWidth = signal(1280);
  const narrowSidebarOverride = signal<boolean | undefined>(undefined);
  const width = signal(440); // list container width
  let listScroll: ScrollRef | undefined;
  let listHeight = 0;
  let listScrollY = 0;
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

  const clearStructuredFilters = () => {
    batch(() => {
      tags.set([]);
      tier.set(undefined);
      platform.set(undefined);
      trainingStatusFilter.set(undefined);
      sidebarGroup.set({ kind: "all" });
      setLibraryGroupBy("none");
    });
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
    sortKey.set(DEFAULT_LIBRARY_SORT);
    selection.set(EMPTY_SELECTION);
  });

  const readOnly = () => info().readOnly;
  const trainingEnabled = () => info().trainingEnabled;

  effect(() => {
    if (!trainingEnabled() && trainingStatusFilter() !== undefined) {
      trainingStatusFilter.set(undefined);
    }
  });

  const rev = activeRevision(() => props.active);

  const facets = computed(() => {
    void rev();
    return preview.workspaceFacets();
  });

  // If the selected named group was deleted externally, fall back to "all".
  effect(() => {
    const sel = sidebarGroup();
    if (sel.kind === "named" && !facets().groups.some((g) => g.name === sel.name)) {
      sidebarGroup.set({ kind: "all" });
    }
  });

  const recentRank = computed<Map<string, number>>(() => {
    if (sortKey() !== "recent") return new Map();
    void rev();
    const map = new Map<string, number>();
    try {
      const hits = typeof preview.fuzzyJump === "function" ? preview.fuzzyJump("", null) : [];
      let idx = 0;
      for (const h of hits) {
        if (h.kind === "problem" && !map.has(h.id)) {
          map.set(h.id, idx++);
        }
      }
    } catch {
      // Fall back to deterministic ID order when fuzzyJump is unavailable.
    }
    return map;
  });

  const rows = computed<LibraryRow[]>(() => {
    void rev();
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
    return sortLibraryRows(list, sortKey(), locale(), recentRank());
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

  // Workspace-wide slot presence (§6.2): evaluated across ALL problems in the workspace
  // (never the current filtered subset) so toggling filters never causes slots to pop in/out.
  const allWorkspaceProblems = computed(() => {
    void rev();
    return preview.searchProblems({
      text: null,
      tags: [],
      ratingMin: null,
      ratingMax: null,
      platform: null,
      group: null,
      ungroupedOnly: false,
    });
  });

  const workspaceColPresence = computed(() => {
    const all = allWorkspaceProblems();
    const trEnabled = trainingEnabled();
    let hasRating = false;
    let hasStatus = false;
    for (const p of all) {
      if (!hasRating && p.rating != null) hasRating = true;
      if (
        !hasStatus &&
        (p.packageStatus != null ||
          (trEnabled && !!p.trainingStatus && p.trainingStatus !== "none") ||
          p.hasEditorial ||
          p.hasSolutions)
      ) {
        hasStatus = true;
      }
      if (hasRating && hasStatus) break;
    }
    return { hasRating, hasStatus };
  });

  const trainingStatusLabel = (st: string) => {
    if (st === "none") return t("training.status.none");
    if (st === "todo") return t("training.status.todo");
    if (st === "attempted") return t("training.status.attempted");
    if (st === "ac") return t("training.status.ac");
    return t("training.status.upsolved");
  };

  const groupByOptionLabel = (mode: LibraryGroupBy) => {
    if (mode === "none") return t("library.groupBy.none");
    if (mode === "group") return t("library.groupBy.group");
    if (mode === "rating") return t("library.groupBy.rating");
    if (mode === "platform") return t("library.groupBy.platform");
    if (mode === "contest") return t("library.groupBy.contest");
    return t("library.groupBy.packageStatus");
  };

  const activeFilterChips = computed<ActiveFilterChip[]>(() => {
    const chips: ActiveFilterChip[] = [];
    const curTier = tier();
    if (curTier !== undefined) {
      const range = TIER_RANGES.find((r) => r.tier === curTier);
      if (range) {
        chips.push({
          id: `tier:${curTier}`,
          label: `${t("library.filter.rating")} ${range.label}`,
          onRemove: () => tier.set(undefined),
        });
      }
    }
    for (const tg of tags()) {
      chips.push({
        id: `tag:${tg}`,
        label: tg,
        onRemove: () => tags.set(tags().filter((x) => x !== tg)),
      });
    }
    const curPlatform = platform();
    if (curPlatform !== undefined) {
      chips.push({
        id: `platform:${curPlatform}`,
        label: `${t("library.filter.source")} ${curPlatform}`,
        onRemove: () => platform.set(undefined),
      });
    }
    const curTr = trainingEnabled() ? trainingStatusFilter() : undefined;
    if (curTr !== undefined) {
      chips.push({
        id: `training:${curTr}`,
        label: `${t("training.filter.label")} ${trainingStatusLabel(curTr)}`,
        onRemove: () => trainingStatusFilter.set(undefined),
      });
    }
    const curGroupBy = libraryGroupBy();
    if (curGroupBy !== "none") {
      chips.push({
        id: `groupBy:${curGroupBy}`,
        label: `${t("library.groupBy.label")} ${groupByOptionLabel(curGroupBy)}`,
        onRemove: () => setLibraryGroupBy("none"),
      });
    }
    const sg = sidebarGroup();
    if (sg.kind === "named") {
      chips.push({
        id: `sidebar:${sg.name}`,
        label: sg.name,
        onRemove: () => sidebarGroup.set({ kind: "all" }),
      });
    } else if (sg.kind === "ungrouped") {
      chips.push({
        id: "sidebar:__ungrouped__",
        label: t("library.sidebar.ungrouped"),
        onRemove: () => sidebarGroup.set({ kind: "all" }),
      });
    }
    return chips;
  });

  const hasFilterMenuActive = () =>
    tags().length > 0 ||
    tier() !== undefined ||
    platform() !== undefined ||
    (trainingEnabled() && trainingStatusFilter() !== undefined) ||
    libraryGroupBy() !== "none" ||
    sortKey() !== DEFAULT_LIBRARY_SORT;

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

  function ensureRowVisible(index: number) {
    if (!listScroll || listHeight <= 0 || index < 0) return;
    const rowTop = index * ROW.list;
    const rowBottom = rowTop + ROW.list;
    if (rowTop < listScrollY) {
      listScroll.scrollTo({ y: Math.max(0, rowTop), animated: false });
    } else if (rowBottom > listScrollY + listHeight) {
      listScroll.scrollTo({ y: Math.max(0, rowBottom - listHeight), animated: false });
    }
  }

  effect(() => {
    const row = selectedRow();
    if (row >= 0 && libraryGroupBy() === "none") ensureRowVisible(row);
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
    if (libraryGroupBy() === "none") ensureRowVisible(i);
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
    if (filterOpen()) {
      filterOpen.set(false);
    } else if (selection().selectedIds.size > 0) {
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
          disabled: ro || row.packageStatus != null,
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

  const showBatchInspector = () => selection().selectedIds.size >= 2 || batchTask() !== undefined;
  const rightPanelVisible = () => showBatchInspector() || visibleSelected() !== undefined;

  const pluginImporters = computed<RegisteredImporter[]>(() => {
    void pluginsRevision();
    return parseJsonReport<RegisteredImporter[]>(
      typeof preview.pluginListImporters === "function" ? preview.pluginListImporters() : undefined,
      [],
    );
  });

  const onSelectImporter = (key: string) => {
    const idx = key.indexOf(":");
    if (idx < 0) return;
    const pid = key.slice(0, idx);
    const iid = key.slice(idx + 1);
    void preview
      .pluginRunImporter(pid, iid, {
        title: t("tools.importDialogTitle"),
        buttonLabel: t("tools.importAction"),
      })
      .then((res) => {
        if (!res.ok) {
          if (res.code !== "cancelled") {
            showToast({ tone: "issues", text: res.error ?? t("tools.importFailed") });
          }
          return;
        }
        const report = parseJsonReport<ImportAppliedReport>(res, {
          kind: "problem",
          created_ids: [],
          skipped_ids: [],
        });
        refresh();
        showToast({
          tone: "success",
          text: t("tools.importSuccess", { count: report.created_ids.length }),
        });
      });
  };

  const addMenuEntries = computed<MenuEntry[]>(() => {
    const ro = readOnly();
    const list: MenuEntry[] = [
      { id: "newProblem", label: t("library.newProblem"), icon: "plus", keys: "mod+n", disabled: ro },
      {
        id: "importPolygon",
        label: t("packages.importPolygon"),
        icon: "folderOpen",
        disabled: ro || importingPolygon(),
      },
    ];
    const importers = pluginImporters();
    if (importers.length > 0) {
      list.push({ separator: true, id: "sep-importers" });
      for (const imp of importers) {
        list.push({
          id: `imp:${imp.plugin_id}:${imp.importer.id}`,
          label: imp.importer.name,
          note: imp.plugin_name,
          icon: "sparkles",
          disabled: ro,
        });
      }
    }
    return list;
  });

  const handleAddMenuSelect = (id: string) => {
    if (id === "newProblem") {
      if (!readOnly()) dialog.set({ kind: "newProblem" });
    } else if (id === "importPolygon") {
      if (!readOnly() && !importingPolygon()) void importPolygon();
    } else if (id.startsWith("imp:")) {
      if (!readOnly()) onSelectImporter(id.slice(4));
    }
  };

  const sortSectionTitle = () => (locale() === "en" ? "Sort" : "排序");
  const sortRecentLabel = () => (locale() === "en" ? "Recently Modified" : "最近修改");

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
      class="bg-card"
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
          <ResizablePanel size={440} minSize={340} maxSize={580} fixed={rightPanelVisible()}>
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
              {/* 52pt Toolbar with ONLY 4 elements (§8.1): sidebar toggle, flex search, unified Filter, `+` split menu */}
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
                    flex: 1,
                    minWidth: 80,
                    height: CONTROL.md,
                    borderRadius: RADIUS.sm,
                    borderWidth: 1,
                    flexDirection: "row",
                    alignItems: "center",
                    paddingLeft: SPACE[8],
                    paddingRight: SPACE[4],
                    gap: SPACE[6],
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
                  <Icon name="search" size="sm" tone="muted-foreground" />
                  <Input
                    ref={(r) => (search = r)}
                    testID="library-search"
                    class="bg-transparent placeholder:text-muted-foreground"
                    style={{ flex: 1, height: CONTROL.sm, fontSize: FONT_SIZE.body }}
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
                        height: 20, // token-exempt: compact inline icon box
                        borderRadius: RADIUS.xs,
                        alignItems: "center",
                        justifyContent: "center",
                        flexShrink: 0,
                        transitionProperty: "colors",
                        transitionDuration: ms(120),
                        transitionTimingFunction: EASE_STANDARD,
                      }}
                    >
                      <Icon name="x" size="sm" tone="muted-foreground" />
                    </View>
                  </If>
                </View>

                <Popover
                  open={filterOpen()}
                  onOpenChange={(e) => filterOpen.set(e.open)}
                  placement="bottom"
                  align="end"
                  offset={4}
                  popupClass="bg-popover border border-border"
                  popupStyle={{
                    width: 296,
                    maxHeight: 440,
                    padding: SPACE[12],
                    borderRadius: RADIUS.lg,
                    boxShadow: popoverShadow(),
                  }}
                  trigger={
                    <View
                      testID="library-filter-trigger"
                      class={
                        hasFilterMenuActive()
                          ? "bg-accent border border-border hover:bg-accent/80 active:bg-accent focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-ring"
                          : "border border-border bg-card hover:bg-muted active:bg-accent focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-ring"
                      }
                      style={{
                        height: CONTROL.md,
                        paddingLeft: SPACE[8],
                        paddingRight: SPACE[6],
                        borderRadius: RADIUS.sm,
                        flexDirection: "row",
                        alignItems: "center",
                        gap: SPACE[4],
                        flexShrink: 0,
                        ...colorTransition(120),
                      }}
                    >
                      <Text
                        style={{ fontSize: FONT_SIZE.body, lineHeight: LINE_HEIGHT.body }}
                        class={hasFilterMenuActive() ? "text-foreground" : "text-secondary-foreground"}
                      >
                        {t("library.filter.more")}
                      </Text>
                      <If when={hasFilterMenuActive()}>
                        <View
                          testID="library-filter-dot"
                          style={{ width: 6, height: 6, borderRadius: RADIUS.full, flexShrink: 0 }}
                          class="bg-primary"
                        />
                      </If>
                      <Chevron size="sm" tone={hasFilterMenuActive() ? "foreground" : "muted-foreground"} />
                    </View>
                  }
                >
                  <Scroll style={{ maxHeight: 416 }}>
                    <View style={{ gap: SPACE[12] }}>
                      {/* Group 1: Difficulty */}
                      <View style={{ gap: SPACE[6] }}>
                        <Text style={{ fontSize: FONT_SIZE.caption, fontWeight: FONT_WEIGHT.semibold }} class="text-muted-foreground">
                          {t("library.filter.rating")}
                        </Text>
                        <View style={{ flexDirection: "row", flexWrap: "wrap", gap: SPACE[4] }}>
                          <FilterOptionPill
                            label={t("library.filter.allRatings")}
                            active={tier() === undefined}
                            onPress={() => tier.set(undefined)}
                          />
                          <Each of={TIER_RANGES} key={(r) => r.tier}>
                            {(r) => (
                              <FilterOptionPill
                                label={r().label}
                                active={tier() === r().tier}
                                onPress={() => tier.set(tier() === r().tier ? undefined : r().tier)}
                              />
                            )}
                          </Each>
                        </View>
                      </View>

                      {/* Group 2: Tags */}
                      <If when={facets().tags.length > 0}>
                        <View style={{ gap: SPACE[6] }}>
                          <Text style={{ fontSize: FONT_SIZE.caption, fontWeight: FONT_WEIGHT.semibold }} class="text-muted-foreground">
                            {t("library.filter.tags")}
                          </Text>
                          <View style={{ flexDirection: "row", flexWrap: "wrap", gap: SPACE[4] }}>
                            <Each of={facets().tags} key={(tg) => tg}>
                              {(tg) => (
                                <FilterOptionPill
                                  label={tg()}
                                  active={tags().includes(tg())}
                                  onPress={() =>
                                    tags.set(
                                      tags().includes(tg()) ? tags().filter((x) => x !== tg()) : [...tags(), tg()],
                                    )
                                  }
                                />
                              )}
                            </Each>
                          </View>
                        </View>
                      </If>

                      {/* Group 3: Source platform */}
                      <If when={facets().platforms.length > 0}>
                        <View style={{ gap: SPACE[6] }}>
                          <Text style={{ fontSize: FONT_SIZE.caption, fontWeight: FONT_WEIGHT.semibold }} class="text-muted-foreground">
                            {t("library.filter.source")}
                          </Text>
                          <View style={{ flexDirection: "row", flexWrap: "wrap", gap: SPACE[4] }}>
                            <FilterOptionPill
                              label={t("library.filter.allSources")}
                              active={platform() === undefined}
                              onPress={() => platform.set(undefined)}
                            />
                            <Each of={facets().platforms} key={(p) => p}>
                              {(p) => (
                                <FilterOptionPill
                                  label={p()}
                                  active={platform() === p()}
                                  onPress={() => platform.set(platform() === p() ? undefined : p())}
                                />
                              )}
                            </Each>
                          </View>
                        </View>
                      </If>

                      {/* Training status (when enabled) */}
                      <If when={trainingEnabled()}>
                        <View style={{ gap: SPACE[6] }}>
                          <Text style={{ fontSize: FONT_SIZE.caption, fontWeight: FONT_WEIGHT.semibold }} class="text-muted-foreground">
                            {t("training.filter.label")}
                          </Text>
                          <View style={{ flexDirection: "row", flexWrap: "wrap", gap: SPACE[4] }}>
                            <FilterOptionPill
                              label={t("training.filter.all")}
                              active={trainingStatusFilter() === undefined}
                              onPress={() => trainingStatusFilter.set(undefined)}
                            />
                            <Each of={["todo", "attempted", "ac", "upsolved", "none"] as const} key={(st) => st}>
                              {(st) => (
                                <FilterOptionPill
                                  label={trainingStatusLabel(st())}
                                  active={trainingStatusFilter() === st()}
                                  onPress={() =>
                                    trainingStatusFilter.set(trainingStatusFilter() === st() ? undefined : st())
                                  }
                                />
                              )}
                            </Each>
                          </View>
                        </View>
                      </If>

                      <View style={{ height: 1 }} class="bg-border" />

                      {/* Group-by */}
                      <View style={{ gap: SPACE[6] }}>
                        <Text style={{ fontSize: FONT_SIZE.caption, fontWeight: FONT_WEIGHT.semibold }} class="text-muted-foreground">
                          {t("library.groupBy.label")}
                        </Text>
                        <View style={{ flexDirection: "row", flexWrap: "wrap", gap: SPACE[4] }}>
                          <Each
                            of={["none", "group", "rating", "platform", "contest", "package_status"] as LibraryGroupBy[]}
                            key={(m) => m}
                          >
                            {(m) => (
                              <FilterOptionPill
                                label={groupByOptionLabel(m())}
                                active={libraryGroupBy() === m()}
                                onPress={() => setLibraryGroupBy(m())}
                              />
                            )}
                          </Each>
                        </View>
                      </View>

                      <View style={{ height: 1 }} class="bg-border" />

                      {/* Sort radio group at the bottom (§8.1) */}
                      <View style={{ gap: SPACE[2] }}>
                        <Text style={{ fontSize: FONT_SIZE.caption, fontWeight: FONT_WEIGHT.semibold, paddingBottom: SPACE[2] }} class="text-muted-foreground">
                          {sortSectionTitle()}
                        </Text>
                        <SortRadioRow
                          testID="library-sort-title"
                          label={t("library.col.title")}
                          selected={sortKey() === "title"}
                          onPress={() => sortKey.set("title")}
                        />
                        <SortRadioRow
                          testID="library-sort-rating"
                          label={t("library.col.rating")}
                          selected={sortKey() === "rating"}
                          onPress={() => sortKey.set("rating")}
                        />
                        <SortRadioRow
                          testID="library-sort-recent"
                          label={sortRecentLabel()}
                          selected={sortKey() === "recent"}
                          onPress={() => sortKey.set("recent")}
                        />
                      </View>
                    </View>
                  </Scroll>
                </Popover>

                {/* Right `+` split button: click `+` for New Problem, `▾` for New Problem / Import Polygon (§8.1) */}
                <View
                  testID="library-add-split"
                  style={{
                    height: CONTROL.md,
                    borderRadius: RADIUS.sm,
                    flexDirection: "row",
                    alignItems: "center",
                    overflow: "hidden",
                    flexShrink: 0,
                    opacity: readOnly() ? 0.6 : 1,
                  }}
                  class="bg-primary"
                >
                  <View
                    testID="library-new-problem"
                    disabled={readOnly()}
                    onPress={() => !readOnly() && dialog.set({ kind: "newProblem" })}
                    tooltip={tip(readOnly() ? t("library.readOnlyCannotCreate") : t("library.newProblem"), readOnly() ? undefined : "mod+n")}
                    class={readOnly() ? "" : "hover:bg-sidebar-primary active:opacity-90 focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-ring"}
                    activeStyle={pressActiveStyle(readOnly())}
                    style={{
                      width: CONTROL.md,
                      height: CONTROL.md,
                      alignItems: "center",
                      justifyContent: "center",
                      transform: "scale(1)",
                      ...pressTransition(140),
                    }}
                  >
                    <Icon name="plus" size="md" tone="primary-foreground" />
                  </View>
                  <View style={{ width: 1, height: 14 }} class="bg-primary-foreground/25" />
                  <AppMenu
                    align="end"
                    entries={addMenuEntries()}
                    onSelect={handleAddMenuSelect}
                    trigger={
                      <View
                        testID="library-add-menu-trigger"
                        disabled={readOnly()}
                        tooltip={tip(readOnly() ? t("library.readOnlyCannotCreate") : t("packages.importPolygon"))}
                        class={readOnly() ? "" : "hover:bg-sidebar-primary active:opacity-90 focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-ring"}
                        style={{
                          width: 20,
                          height: CONTROL.md,
                          alignItems: "center",
                          justifyContent: "center",
                          ...colorTransition(140),
                        }}
                      >
                        <Chevron size="sm" tone="primary-foreground" />
                      </View>
                    }
                  />
                </View>
              </Toolbar>

              {/* 28pt active filter chip bar — rendered ONLY when at least one filter is active (§8.1) */}
              <If when={activeFilterChips().length > 0}>
                <View
                  testID="library-active-filters"
                  style={{
                    height: CONTROL.md,
                    paddingLeft: SPACE[16],
                    paddingRight: SPACE[12],
                    flexDirection: "row",
                    alignItems: "center",
                    gap: SPACE[6],
                    borderBottomWidth: 1,
                    flexShrink: 0,
                  }}
                  class="border-border bg-card"
                >
                  <View style={{ flex: 1, minWidth: 0, flexDirection: "row", alignItems: "center", gap: SPACE[4], overflow: "hidden" }}>
                    <Each of={activeFilterChips()} key={(c) => c.id}>
                      {(chip) => (
                        <View
                          onPress={() => chip().onRemove()}
                          class="bg-muted hover:bg-accent active:bg-accent"
                          style={{
                            height: 20, // token-exempt: compact inline filter chip height
                            maxWidth: 160,
                            paddingLeft: SPACE[6],
                            paddingRight: SPACE[4],
                            borderRadius: RADIUS.xs,
                            flexDirection: "row",
                            alignItems: "center",
                            gap: SPACE[4],
                            flexShrink: 0,
                            ...colorTransition(120),
                          }}
                        >
                          <Text
                            style={{ fontSize: FONT_SIZE.caption, numberOfLines: 1, textOverflow: "ellipsis", flexShrink: 1, minWidth: 0 }}
                            class="text-secondary-foreground"
                          >
                            {chip().label}
                          </Text>
                          <Icon name="x" size="sm" tone="muted-foreground" />
                        </View>
                      )}
                    </Each>
                  </View>
                  <View
                    testID="library-active-filters-clear"
                    onPress={clearStructuredFilters}
                    class="hover:bg-muted active:bg-accent"
                    style={{
                      height: CONTROL.sm,
                      paddingLeft: SPACE[6],
                      paddingRight: SPACE[6],
                      borderRadius: RADIUS.sm,
                      alignItems: "center",
                      justifyContent: "center",
                      flexShrink: 0,
                      ...colorTransition(120),
                    }}
                  >
                    <Text style={{ fontSize: FONT_SIZE.caption }} class="text-muted-foreground">
                      {t("common.clear")}
                    </Text>
                  </View>
                </View>
              </If>

              {/* Headerless 40pt problem list (flat or grouped) */}
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
                      showRatingCol={workspaceColPresence().hasRating}
                      showStatusCol={workspaceColPresence().hasStatus}
                      trainingEnabled={trainingEnabled()}
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
                  <If when={rows().length > 0} else={<View style={{ flex: 1 }}>{emptyElement()}</View>}>
                    <Scroll
                      ref={(r) => (listScroll = r)}
                      testID="library-list"
                      style={{ flex: 1 }}
                      onLayout={(e) => {
                        listHeight = e.bounds.size.height;
                      }}
                      onScroll={(e) => {
                        listScrollY = e.offset.y;
                      }}
                    >
                      {/* token-exempt: clearance above batch floating bar */}
                      <View style={{ paddingBottom: 72 }}>
                        <Each of={rows()} key={(r) => r.id}>
                          {(row) => (
                            <LibraryListRow
                              row={row()}
                              showRatingCol={workspaceColPresence().hasRating}
                              showStatusCol={workspaceColPresence().hasStatus}
                              trainingEnabled={trainingEnabled()}
                              selected={selection().selectedIds.has(row().id)}
                              anySelected={selection().selectedIds.size > 0}
                              focused={visibleSelected() === row().id && selection().selectedIds.size === 0}
                              onRowClick={(id, opts) => applySel({ kind: "click", id, ...opts })}
                              onRowDoubleClick={(id) => openStudio({ target: "problem", id })}
                              onRowContextMenu={(r, x, y) => openContextMenuForRow(r, x, y)}
                            />
                          )}
                        </Each>
                      </View>
                    </Scroll>
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

          <ResizablePanel minSize={360} visible={rightPanelVisible()}>
            <If
              when={showBatchInspector()}
              else={
                <If when={visibleSelected()}>
                  {(id) => (
                    <ProblemInspector
                      id={id()}
                      active={props.active}
                      groups={facets().groups}
                      trainingStatus={rows().find((r) => r.id === id())?.trainingStatus}
                    />
                  )}
                </If>
              }
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

/** The selected problem in reader mode, with its sources, referencing contests, and actions. */
function ProblemInspector(props: {
  id: string;
  active: boolean;
  groups: readonly { name: string; count: number }[];
  trainingStatus?: string | null;
}) {
  const tab = signal<Tab>("statements");
  const solution = signal<string | undefined>(undefined);
  let canvas: CanvasController | undefined;

  const rev = activeRevision(() => props.active);

  const detail = computed(() => {
    void rev();
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
  let lastRev = rev();
  effect(() => {
    const cur = rev();
    if (props.active && cur !== lastRev) {
      lastRev = cur;
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
    const curTraining = props.trainingStatus ?? "none";
    const trainingEntries: MenuEntry[] = info().trainingEnabled
      ? [
          { separator: true as const, id: "sep-training" },
          {
            id: "training:ac",
            label: t("library.menu.markAc"),
            checked: curTraining === "ac",
            disabled: ro,
          },
          {
            id: "training:todo",
            label: t("library.menu.markTodo"),
            checked: curTraining === "todo",
            disabled: ro,
          },
          {
            id: "training:clear",
            label: t("training.status.clearManual"),
            icon: "x" as const,
            disabled: ro || curTraining === "none",
          },
        ]
      : [];
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
      ...(ro || preview.hasPackage(props.id)
        ? []
        : [{ id: "createPackage", label: t("library.menu.createPackage"), icon: "code" as const }]),
      { id: "duplicate", label: t("library.menu.duplicate"), icon: "copy", disabled: ro },
      { id: "add", label: t("library.menu.addToContest"), icon: "listPlus", disabled: ro },
      ...trainingEntries,
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
                        height: 20, // token-exempt: compact inline tag chip height
                        maxWidth: 160,
                        minWidth: 0,
                        overflow: "hidden",
                        paddingLeft: SPACE[6],
                        paddingRight: SPACE[6],
                        borderRadius: RADIUS.xs,
                        flexDirection: "row",
                        alignItems: "center",
                        gap: SPACE[4],
                      }}
                      class="bg-muted"
                    >
                      <Icon name="folder" size="sm" tone="muted-foreground" />
                      <Text
                        style={{ fontSize: FONT_SIZE.caption, flexShrink: 1, minWidth: 0, numberOfLines: 1, textOverflow: "ellipsis" }}
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
                        height: 20, // token-exempt: compact inline source chip height
                        maxWidth: 180,
                        minWidth: 0,
                        overflow: "hidden",
                        paddingLeft: SPACE[6],
                        paddingRight: SPACE[6],
                        borderRadius: RADIUS.xs,
                        flexDirection: "row",
                        alignItems: "center",
                        gap: SPACE[4],
                      }}
                      class="bg-muted"
                    >
                      <Icon name="link" size="sm" tone="muted-foreground" />
                      <Text
                        style={{
                          fontSize: FONT_SIZE.caption,
                          fontVariant: ["tabular-nums"],
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
                        height: 20, // token-exempt: compact inline tag chip height
                        maxWidth: 168,
                        minWidth: 0,
                        overflow: "hidden",
                        paddingLeft: SPACE[6],
                        paddingRight: SPACE[6],
                        borderRadius: RADIUS.xs,
                        flexDirection: "row",
                        alignItems: "center",
                        gap: SPACE[4],
                        transitionProperty: "colors",
                        transitionDuration: ms(120),
                        transitionTimingFunction: EASE_STANDARD,
                      }}
                    >
                      <Icon name="trophy" size="sm" tone="muted-foreground" />
                      <Text
                        style={{
                          fontSize: FONT_SIZE.caption,
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
                  else if (id.startsWith("training:")) {
                    if (!readOnly()) {
                      const res = preview.setTrainingStatus(props.id, id.slice("training:".length));
                      if (res.ok) refresh();
                    }
                  } else if (id === "group:__clear__") {
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
        <View style={{ height: ROW.table, flexDirection: "row", alignItems: "stretch", paddingLeft: SPACE[20], paddingRight: SPACE[20], gap: SPACE[8], borderBottomWidth: 1 }} class="border-border">
          <Segmented items={tabs()} value={tab()} onChange={(v) => tab.set(v)} itemWidth={72} underline />
          <View style={{ flex: 1 }} />
          <If when={tab() === "code" && info().snippetsEnabled && !info().readOnly}>
            <View style={{ justifyContent: "center" }}>
              <Btn
                testID="library-promote-snippet"
                variant="outline"
                icon="bookOpen"
                label={t("snippets.action.promoteToSnippet")}
                onPress={() =>
                  openNewSnippetModal({
                    code: code(),
                    sourceProblem: props.id,
                    title: detail().title,
                  })
                }
              />
            </View>
          </If>
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
                      height: CONTROL.sm,
                      paddingLeft: SPACE[8],
                      paddingRight: SPACE[6],
                      borderRadius: RADIUS.sm,
                      flexDirection: "row",
                      alignItems: "center",
                      gap: SPACE[4],
                      transitionProperty: "colors",
                      transitionDuration: ms(120),
                      transitionTimingFunction: EASE_STANDARD,
                    }}
                  >
                    <Text style={{ fontSize: FONT_SIZE.caption, fontFamily: MONO }} class="text-secondary-foreground">
                      {currentSolution() ?? ""}
                    </Text>
                    <Chevron size="sm" />
                  </View>
                }
              />
            </View>
          </If>
        </View>
      </If>
      <If
        when={tab() === "code"}
        else={<PreviewCanvas request={request} controller={(c) => (canvas = c)} readerMode active={props.active} />}
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

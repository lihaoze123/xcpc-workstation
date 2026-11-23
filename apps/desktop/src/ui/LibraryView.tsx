// Cmd+1: the problem library. Filters (keywords, tags, a rating tier, a platform) over a
// virtualized table (40 pt rows: ID, title, rating, tags, source, editorial, code; no
// training state) that fills its width, and on the right the selected problem as it
// prints (the paper preview of its statement or editorial) or its `solutions/` read-only,
// with its sources, referencing contests, and actions.

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
import type { PreviewRequest, ProblemSummaryJs } from "xcpc-workstation/native";
import { Btn, Chevron, Chip, RatingBadge, Segmented, tip } from "./kit";
import { locale, t } from "./i18n";
import { Icon } from "./Icon";
import { AppMenu, MoreTrigger, type MenuEntry } from "./Menu";
import { DetailHeader, EmptyState, Spacer, Toolbar } from "./layout";
import { TIER_RANGES, type TierName } from "./logic";
import { EASE_STANDARD, MONO, monoFont, ms } from "./theme";
import { PreviewCanvas, type CanvasController } from "./studio/PreviewCanvas";
import {
  dialog,
  goSpace,
  info,
  jumpToContest,
  openStudio,
  preview,
  refresh,
  revision,
  selectedContest,
  selectedProblem,
  workspaceVersion,
} from "./store";

type Row = ProblemSummaryJs & { tagsText: string; source: string; editorialMark: string; codeMark: string; ratingText: string };

const FIXED = { id: 168, rating: 72, tags: 240, source: 150, mark: 52 };
const LINE = 20; // one line height for every cell, so a monospaced ID and a title share a center
const SEARCH_DEBOUNCE_MS = 120;

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
              : "hover:bg-muted active:bg-accent focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-ring"
          }
          style={{
            height: 28,
            maxWidth: 108,
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
            style={{ fontSize: 13, lineHeight: LINE, flexShrink: 1, numberOfLines: 1, textOverflow: "ellipsis" }}
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
  const sort = signal<DataTableSort | null>(null);
  const width = signal(0); // the table's
  let table: DataTableRef | undefined;
  let search: InputRef | undefined;
  let searchTimer: number | undefined;
  let widthTimer: number | undefined;
  cleanup(() => {
    clearTimeout(searchTimer);
    clearTimeout(widthTimer);
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

  const clear = () => {
    clearTimeout(searchTimer);
    batch(() => {
      inputText.set("");
      queryText.set("");
      tags.set([]);
      tier.set(undefined);
      platform.set(undefined);
    });
  };

  // Clear stale filters & sort whenever the user switches workspaces.
  effect(() => {
    void workspaceVersion();
    clear();
    sort.set(null);
  });

  const readOnly = () => info().readOnly;
  const totalCount = () => info().problems.length;

  const facets = computed(() => {
    void revision();
    return preview.workspaceFacets();
  });

  const rows = computed<Row[]>(() => {
    void revision();
    const range = TIER_RANGES.find((r) => r.tier === tier());
    const found = preview.searchProblems({
      text: queryText().trim() || null,
      tags: tags(),
      ratingMin: range ? range.min : null,
      ratingMax: range && range.max !== null ? range.max - 1 : null,
      platform: platform() ?? null,
    });
    const list: Row[] = found.map((p) => ({
      ...p,
      ratingText: p.rating == null ? "" : String(p.rating),
      tagsText: p.tags.join(" "),
      source: p.sources.map((s) => `${s.platform} ${s.id}`).join("  "),
      editorialMark: p.hasEditorial ? "1" : "",
      codeMark: p.hasSolutions ? "1" : "",
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

  // The title takes what the other columns leave, so the rows reach the right edge; a
  // narrow table drops the columns that matter least (marks, then source, then tags)
  // rather than scroll sideways.
  const columns = computed(() => {
    const w = width();
    const marks = w >= 760;
    const source = w >= 660;
    const tagsW = w >= 560 ? FIXED.tags : 0;
    const used = FIXED.id + FIXED.rating + tagsW + (source ? FIXED.source : 0) + (marks ? 2 * FIXED.mark : 0);
    return [
      { key: "id", title: t("library.col.id"), width: FIXED.id, resizable: false, sortable: true },
      { key: "title", title: t("library.col.title"), width: Math.max(140, w - used - 2), resizable: false, sortable: true },
      { key: "ratingText", title: t("library.col.rating"), width: FIXED.rating, align: "end" as const, resizable: false, sortable: true },
      ...(tagsW ? [{ key: "tagsText", title: t("library.col.tags"), width: tagsW, resizable: false }] : []),
      ...(source ? [{ key: "source", title: t("library.col.source"), width: FIXED.source, resizable: false }] : []),
      ...(marks
        ? [
            { key: "editorialMark", title: t("library.col.editorial"), width: FIXED.mark, align: "center" as const, resizable: false },
            { key: "codeMark", title: t("library.col.code"), width: FIXED.mark, align: "center" as const, resizable: false },
          ]
        : []),
    ];
  });

  const filtered = () => inputText().trim() !== "" || queryText().trim() !== "" || tags().length > 0 || tier() !== undefined || platform() !== undefined;
  const visibleSelected = computed(() => {
    const id = selectedProblem();
    return id && rows().some((r) => r.id === id) ? id : undefined;
  });
  const selectedRow = computed(() => {
    const id = visibleSelected();
    return id ? rows().findIndex((r) => r.id === id) : -1;
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
          queueMicrotask(clear);
          return;
        }
        queueMicrotask(() => selectedProblem.set(undefined));
      }
    } else if (id === undefined || !list.some((r) => r.id === id)) {
      if (selectionChanged && id !== undefined && filtered() && info().problems.some((p) => p.id === id)) {
        queueMicrotask(clear);
        return;
      }
      queueMicrotask(() => selectedProblem.set(list[0].id));
    }
  });

  effect(() => {
    const row = selectedRow();
    if (row >= 0) table?.scrollToRow(row, false);
  });

  function move(delta: number) {
    const list = rows();
    if (list.length === 0) return;
    const cur = selectedRow();
    const i = cur < 0 ? 0 : Math.min(list.length - 1, Math.max(0, cur + delta));
    selectedProblem.set(list[i].id);
    table?.scrollToRow(i, false);
  }

  function open() {
    const id = visibleSelected();
    if (id) openStudio({ target: "problem", id });
  }

  keymap.bind({ j: () => props.active && move(1), k: () => props.active && move(-1) }, { when: "notEditing" });
  keymap.bind("mod+n", () => props.active && !readOnly() && dialog.set({ kind: "newProblem" }));
  keymap.bind("mod+f", () => props.active && search?.focus());

  const tagItems = (): MenuEntry[] => facets().tags.map((t) => ({ id: t, label: t, checked: tags().includes(t) }));
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

  const cell = (key: string, row: number, value: string) => {
    const r = rows()[row];
    if (key === "ratingText") return value ? <RatingBadge rating={Number(value)} /> : null;
    if (key === "tagsText") {
      return (
        <View style={{ flexDirection: "row", gap: 4, overflow: "hidden" }}>
          <Each of={r?.tags ?? []} key={(t) => t}>
            {(t) => <Chip label={t()} />}
          </Each>
        </View>
      );
    }
    if (key === "editorialMark" || key === "codeMark") {
      return value ? <View style={{ width: 6, height: 6, borderRadius: 3 }} class="bg-primary" /> : null;
    }
    if (key === "title") {
      const st = r?.packageStatus;
      const badgeLabel =
        st === "unverified"
          ? t("library.package.unverified")
          : st === "stale"
            ? t("library.package.stale")
            : st === "failed"
              ? t("library.package.failed")
              : undefined;
      return (
        <View style={{ flex: 1, flexDirection: "row", alignItems: "center", gap: 6, minWidth: 0 }}>
          <Text
            style={{ numberOfLines: 1, textOverflow: "ellipsis", fontSize: 13.5, lineHeight: LINE, flexShrink: 1 }}
            class="text-foreground"
          >
            {value}
          </Text>
          {badgeLabel && (
            <Text
              style={{ fontSize: 11, flexShrink: 0 }}
              class={st === "failed" ? "text-destructive" : "text-muted-foreground"}
            >
              {badgeLabel}
            </Text>
          )}
        </View>
      );
    }
    return (
      <Text
        style={{
          numberOfLines: 1,
          textOverflow: "ellipsis",
          fontSize: key === "id" ? 12.5 : 13.5,
          lineHeight: LINE,
          fontFamily: key === "id" ? MONO : undefined,
        }}
        class={key === "id" ? "text-muted-foreground" : "text-secondary-foreground"}
      >
        {value}
      </Text>
    );
  };

  return (
    <View style={{ flex: 1 }} class="bg-background">
      <ResizablePanelGroup handleClass="bg-border w-px group-hover:bg-primary/50 group-active:bg-primary transition-colors">
        <ResizablePanel minSize={320}>
          <View
            style={{ flex: 1, minWidth: 0 }}
            onLayout={(e) => {
              const nextW = Math.round(e.bounds.size.width);
              if (nextW <= 0) return;
              if (width() === 0) {
                width.set(nextW);
                return;
              }
              if (Math.abs(nextW - width()) < 2) return;
              // Debounce column width recomputation during continuous splitter drag so
              // DataTable doesn't rebuild column metadata on every mouse-move pixel.
              clearTimeout(widthTimer);
              widthTimer = setTimeout(() => {
                widthTimer = undefined;
                width.set(nextW);
              }, 80);
            }}
          >
            <Toolbar>
              <View
                style={{
                  flexBasis: 220,
                  flexShrink: 1,
                  minWidth: 110,
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
                captureKeys={["up", "down", "escape"]}
                onKeyDown={(e) => {
                  if (e.key === "ArrowDown") move(1);
                  else if (e.key === "ArrowUp") move(-1);
                  else if (e.key === "Escape" && inputText().length > 0) setSearch("");
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
                onSelect={(id) => id && id !== "__empty__" && tags.set(tags().includes(id) ? tags().filter((t) => t !== id) : [...tags(), id])}
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
              <If when={filtered()}>
                <Btn label={width() >= 580 ? t("common.clear") : undefined} icon="filterX" variant="ghost" tooltip={tip(t("library.filter.clear"))} onPress={clear} style={{ flexShrink: 0 }} />
              </If>
              <Spacer />
              <If when={totalCount() > 0 && width() >= 500}>
                <Text style={{ fontSize: 12, fontVariant: ["tabular-nums"], flexShrink: 0 }} class="text-muted-foreground">
                  {filtered() ? `${rows().length} / ${totalCount()}` : t("library.count", { count: totalCount() })}
                </Text>
              </If>
              <Btn
                label={width() >= 660 ? t("library.newProblem") : undefined}
                icon="plus"
                disabled={readOnly()}
                tooltip={tip(readOnly() ? t("library.readOnlyCannotCreate") : t("library.newProblem"), readOnly() ? undefined : "mod+n")}
                onPress={() => dialog.set({ kind: "newProblem" })}
                style={{ flexShrink: 0 }}
              />
            </Toolbar>
            <View
              style={{ flex: 1 }}
              captureKeys={["up", "down", "enter"]}
              onKeyDown={(e) => {
                if (e.key === "ArrowDown") move(1);
                else if (e.key === "ArrowUp") move(-1);
                else if (e.key === "Enter") open();
              }}
            >
              <DataTable<Row>
                ref={(r) => (table = r)}
                testID="library-table"
                style={{ flex: 1 }}
                columns={columns()}
                data={rows()}
                sort={sort()}
                onSortChange={(e) => sort.set(e.sort)}
                rowHeight={40}
                headerHeight={34}
                fontSize={13.5}
                columnSelectable={false}
                movableColumns={false}
                resizableColumns={false}
                selection={selectedRow() >= 0 ? { row: selectedRow() } : null}
                onSelectionChange={(e) => {
                  const row = e.selection?.row;
                  if (row != null && rows()[row]) selectedProblem.set(rows()[row].id);
                }}
                onDoublePress={(e) => {
                  const row = rows()[e.target.row ?? -1];
                  if (row) openStudio({ target: "problem", id: row.id });
                }}
                onContextMenu={(e) => {
                  const row = e.row != null ? rows()[e.row] : undefined;
                  if (!row) return;
                  selectedProblem.set(row.id);
                  const ro = readOnly();
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
                        disabled: ro || preview.inspectPackage(row.id).hasPackage,
                        onSelect: () => {
                          const res = preview.initPackage(row.id);
                          if (res.ok) {
                            refresh();
                            openStudio({ target: "problem", id: row.id, initialTab: "build" });
                          }
                        },
                      },
                      { label: t("library.menu.addToContest"), disabled: ro, onSelect: () => dialog.set({ kind: "addToContest", problemId: row.id }) },
                      { label: t("common.revealInFolder"), onSelect: () => void preview.revealInFolder(`problems/${row.id}`) },
                      { type: "separator" },
                      { label: t("library.menu.delete"), disabled: ro, destructive: true, onSelect: () => dialog.set({ kind: "deleteProblem", problemId: row.id }) },
                    ],
                    { x: e.x, y: e.y },
                  ).finally(() => table?.clearMenuTarget());
                }}
                renderCell={(c) => (
                  <View
                    style={{
                      flex: 1,
                      flexDirection: "row",
                      alignItems: "center",
                      justifyContent: c.key === "ratingText" ? "flex-end" : c.key.endsWith("Mark") ? "center" : "flex-start",
                      paddingLeft: c.key === "id" ? 16 : 12,
                      paddingRight: 12,
                      overflow: "hidden",
                    }}
                  >
                    {cell(c.key, c.row, c.text)}
                  </View>
                )}
                renderHeaderCell={(h) => {
                  const sortable = h.key === "id" || h.key === "title" || h.key === "ratingText";
                  return (
                    <View
                      class={sortable ? "hover:bg-muted/60 transition-colors" : undefined}
                      style={{
                        flex: 1,
                        flexDirection: "row",
                        alignItems: "center",
                        justifyContent: h.key === "ratingText" ? "flex-end" : h.key.endsWith("Mark") ? "center" : "flex-start",
                        paddingLeft: h.key === "id" ? 16 : 12,
                        paddingRight: 12,
                        gap: 4,
                      }}
                    >
                      <Text style={{ fontSize: 12, lineHeight: LINE }} class={h.sort ? "text-foreground" : "text-muted-foreground"}>
                        {h.title ?? ""}
                      </Text>
                      {h.sort && (
                        <Icon
                          name="chevronDown"
                          size={12}
                          tone="secondary-foreground"
                          style={{ transform: h.sort === "ascending" ? "rotate(180deg)" : "rotate(0deg)" }}
                        />
                      )}
                    </View>
                  );
                }}
                classes={{
                  root: "bg-background",
                  header: "bg-background border-b border-border",
                  row: "bg-background",
                  rowOdd: "bg-background",
                  rowHover: "bg-muted",
                  rowSelected: "bg-accent",
                }}
                empty={
                  filtered() ? (
                    <EmptyState icon="searchX" title={t("library.emptyFilteredTitle")} hint={t("library.emptyFilteredHint")} action={<Btn label={t("library.filter.clear")} icon="filterX" onPress={clear} />} />
                  ) : (
                    <EmptyState
                      icon="libraryBig"
                      title={t("library.emptyTitle")}
                      hint={readOnly() ? t("common.readOnlyWorkspaceHint") : t("library.emptyHint")}
                      action={
                        readOnly() ? undefined : (
                          <Btn label={t("library.newProblem")} icon="plus" variant="primary" onPress={() => dialog.set({ kind: "newProblem" })} tooltip={tip(t("library.newProblem"), "mod+n")} />
                        )
                      }
                    />
                  )
                }
              />
            </View>
          </View>
        </ResizablePanel>
        <ResizablePanel
          size={380}
          minSize={300}
          maxSize={560}
          fixed
          visible={visibleSelected() !== undefined}
        >
          <If when={visibleSelected()}>{(id) => <ProblemInspector id={id()} active={props.active} />}</If>
        </ResizablePanel>
      </ResizablePanelGroup>
    </View>
  );
}

type Tab = "statements" | "editorial" | "code";

/** The selected problem as it prints, with its sources, referencing contests, and actions. */
function ProblemInspector(props: { id: string; active: boolean }) {
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
    const t = tab();
    if (t === "code") return undefined;
    return { target: "problem", id: props.id, book: t, path: "", text: "" };
  };
  const currentSolution = () => solution() ?? detail().solutions[0];
  const code = computed(() => {
    const name = currentSolution();
    return tab() === "code" && name ? preview.readSolution(props.id, name).text : "";
  });

  const hasMeta = () => detail().tags.length > 0 || detail().sources.length > 0 || referencingContests().length > 0;
  const readOnly = () => info().readOnly;

  const jumpContest = (contestId: string) => {
    jumpToContest(contestId);
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
                <Each of={detail().tags} key={(t) => t}>
                  {(t) => <Chip label={t()} />}
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
                entries={[
                  { id: "copyMd", label: t("library.menu.copyMarkdownWithSamples"), icon: "fileText" },
                  ...(readOnly() || preview.inspectPackage(props.id).hasPackage
                    ? []
                    : [{ id: "createPackage", label: t("library.menu.createPackage"), icon: "code" as const }]),
                  { id: "duplicate", label: t("library.menu.duplicate"), icon: "copy", disabled: readOnly() },
                  { id: "add", label: t("library.menu.addToContest"), icon: "listPlus", disabled: readOnly() },
                  { id: "reveal", label: t("common.revealInFolder"), icon: "folderOpen" },
                  { separator: true, id: "sep" },
                  { id: "delete", label: t("library.menu.delete"), icon: "trash", destructive: true, disabled: readOnly() },
                ]}
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
                  else if (id === "reveal") void preview.revealInFolder(`problems/${props.id}`);
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

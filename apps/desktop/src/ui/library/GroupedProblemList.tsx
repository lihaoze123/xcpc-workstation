// Grouped folding view for LibraryView (§5.3):
// Shared sticky header matching <DataTable> pixel-for-pixel + <Scroll>
// rendering collapsible GroupHeader (32pt) & ProblemRow (40pt) sections in normal flow.

import { reduceMotion, Scroll, signal, Text, View, type JSX } from "nsh";
import type { DataTableSort } from "nsh/base";
import { Icon } from "../Icon";
import {
  headerCheckboxState,
  type CheckboxTriState,
  type LibraryColumnSpec,
  type ProblemGroupSection,
} from "../logic";
import { colorTransition } from "../motion";
import { EASE_STANDARD, ms } from "../theme";
import {
  cellPadding,
  renderLibraryCellContent,
  renderLibraryHeaderContent,
  TriCheckbox,
  type LibraryRow,
} from "./cells";

import { FONT_SIZE, FONT_WEIGHT, SPACE } from "../tokens";
const HEADER_H = 34;
const GROUP_H = 32;
const ROW_H = 40;

function GroupedRow(props: {
  row: LibraryRow;
  columns: readonly LibraryColumnSpec[];
  showInlineId: boolean;
  maxTags: number;
  tagsWidth: number;
  trainingEnabled?: boolean;
  selected: boolean;
  anySelected: boolean;
  focused: boolean;
  onRowClick: (id: string, opts: { shift: boolean; mod: boolean; checkbox: boolean }) => void;
  onRowDoubleClick: (id: string) => void;
  onRowContextMenu: (row: LibraryRow, x: number, y: number) => void;
}) {
  const hovered = signal(false);
  const colWidth = (key: string) => props.columns.find((c) => c.key === key)?.width;
  const renderCol = (key: "select" | "title" | "ratingText" | "status" | "tagsText" | "source") => {
    const w = colWidth(key);
    if (w == null) return null;
    const pad = cellPadding(key);
    return (
      <View
        style={{
          width: w,
          flexShrink: 0,
          minWidth: 0,
          flexDirection: "row",
          alignItems: "center",
          justifyContent: pad.justifyContent,
          paddingLeft: pad.paddingLeft,
          paddingRight: pad.paddingRight,
          overflow: "hidden",
        }}
      >
        {renderLibraryCellContent(key, props.row, {
          selected: props.selected,
          showCheckbox: () => hovered() || props.selected || props.anySelected,
          showInlineId: props.showInlineId,
          maxTags: props.maxTags,
          tagsWidth: props.tagsWidth,
          trainingEnabled: props.trainingEnabled,
        })}
      </View>
    );
  };

  return (
    <View
      onMouseEnter={() => hovered.set(true)}
      onMouseLeave={() => hovered.set(false)}
      onPress={(e) => {
        const mod = e.modifiers.control || e.modifiers.platform;
        const shift = e.modifiers.shift;
        const isCheckboxCol = e.position.x <= 36;
        if (e.clickCount >= 2 && !isCheckboxCol && !mod && !shift) {
          props.onRowDoubleClick(props.row.id);
          return;
        }
        props.onRowClick(props.row.id, { shift, mod, checkbox: isCheckboxCol });
      }}
      onContextMenu={(e) => props.onRowContextMenu(props.row, e.pagePosition.x, e.pagePosition.y)}
      class={props.selected || props.focused ? "bg-accent" : "bg-card hover:bg-muted"}
      style={{
        height: ROW_H,
        flexDirection: "row",
        alignItems: "stretch",
        flexShrink: 0,
        ...colorTransition(100),
      }}
    >
      {renderCol("select")}
      {renderCol("title")}
      {renderCol("ratingText")}
      {renderCol("status")}
      {renderCol("tagsText")}
      {renderCol("source")}
    </View>
  );
}

export function GroupedProblemList(props: {
  sections: readonly ProblemGroupSection<LibraryRow>[];
  columns: readonly LibraryColumnSpec[];
  showInlineId: boolean;
  maxTags: number;
  tagsWidth: number;
  trainingEnabled?: boolean;
  sort: DataTableSort | null;
  onSortChange: (sort: DataTableSort | null) => void;
  headerTriState: CheckboxTriState;
  onToggleAll: () => void;
  selectedIds: ReadonlySet<string>;
  focusedId: string | undefined;
  onRowClick: (id: string, opts: { shift: boolean; mod: boolean; checkbox: boolean }) => void;
  onRowDoubleClick: (id: string) => void;
  onToggleGroupSelect: (ids: readonly string[]) => void;
  onRowContextMenu: (row: LibraryRow, x: number, y: number) => void;
  empty: JSX.Element;
}) {
  const collapsed = signal<ReadonlySet<string>>(new Set());
  const headerHovered = signal(false);

  const toggleCollapse = (key: string) => {
    const next = new Set(collapsed());
    if (next.has(key)) next.delete(key);
    else next.add(key);
    collapsed.set(next);
  };

  const cycleSort = (key: string) => {
    if (key !== "title" && key !== "ratingText") return;
    const cur = props.sort;
    if (!cur || cur.key !== key) props.onSortChange({ key, direction: "ascending" });
    else if (cur.direction === "ascending") props.onSortChange({ key, direction: "descending" });
    else props.onSortChange(null);
  };

  return (
    <View style={{ flex: 1, minWidth: 0 }} class="bg-card">
      {/* Shared sticky header matching <DataTable> */}
      <View
        onMouseEnter={() => headerHovered.set(true)}
        onMouseLeave={() => headerHovered.set(false)}
        style={{
          height: HEADER_H,
          flexDirection: "row",
          alignItems: "stretch",
          borderBottomWidth: 1,
          flexShrink: 0,
        }}
        class="bg-card border-border"
      >
        <Each of={props.columns} key={(c) => c.key}>
          {(col) => {
            const sortDir = () => (props.sort?.key === col().key ? props.sort.direction : null);
            return (
              <View
                style={{ width: col().width, flexShrink: 0, flexDirection: "row", alignItems: "stretch" }}
                onPress={col().sortable ? () => cycleSort(col().key) : undefined}
              >
                {renderLibraryHeaderContent({
                  key: col().key,
                  title: col().title,
                  sortDir: sortDir(),
                  triState: () => props.headerTriState,
                  showCheckbox: () => headerHovered() || props.selectedIds.size > 0,
                  onToggleAll: props.onToggleAll,
                })}
              </View>
            );
          }}
        </Each>
      </View>

      <If when={props.sections.length > 0} else={<View style={{ flex: 1 }}>{props.empty}</View>}>
        <Scroll style={{ flex: 1 }}>
          {/* token-exempt: clearance above batch floating bar */}
          <View style={{ paddingBottom: 72 }}>
            <Each of={props.sections} key={(s) => s.key}>
              {(sec) => {
                const isCollapsed = () => collapsed().has(sec().key);
                const rowIds = () => sec().rows.map((r) => r.id);
                const groupTri = () => headerCheckboxState(props.selectedIds, rowIds());
                const groupHovered = signal(false);
                return (
                  <View style={{ flexShrink: 0 }}>
                    <View
                      onMouseEnter={() => groupHovered.set(true)}
                      onMouseLeave={() => groupHovered.set(false)}
                      style={{
                        height: GROUP_H,
                        flexDirection: "row",
                        alignItems: "center",
                        borderBottomWidth: 1,
                      }}
                      class="bg-muted/40 border-border"
                    >
                      <View
                        style={{ width: 36, height: GROUP_H, alignItems: "center", justifyContent: "center", flexShrink: 0 }}
                        onPress={() => props.onToggleGroupSelect(rowIds())}
                      >
                        <TriCheckbox
                          state={groupTri()}
                          visible={() => groupHovered() || groupTri() !== "none" || props.selectedIds.size > 0}
                        />
                      </View>
                      <View
                        onPress={() => toggleCollapse(sec().key)}
                        class="hover:bg-muted/70"
                        style={{
                          flex: 1,
                          height: GROUP_H,
                          paddingLeft: SPACE[4],
                          paddingRight: SPACE[12],
                          flexDirection: "row",
                          alignItems: "center",
                          gap: SPACE[6],
                          minWidth: 0,
                          ...colorTransition(120),
                        }}
                      >
                        <Icon
                          name="chevronRight"
                          size="sm"
                          tone="muted-foreground"
                          style={{
                            transform: isCollapsed() ? "rotate(0deg)" : "rotate(90deg)",
                            ...(reduceMotion()
                              ? {}
                              : {
                                  transitionProperty: "transform",
                                  transitionDuration: ms(120),
                                  transitionTimingFunction: EASE_STANDARD,
                                }),
                          }}
                        />
                        <Text
                          style={{
                            fontSize: FONT_SIZE.caption,
                            fontWeight: FONT_WEIGHT.semibold,
                            flexShrink: 1,
                            minWidth: 0,
                            numberOfLines: 1,
                            textOverflow: "ellipsis",
                          }}
                          class="text-foreground"
                        >
                          {sec().label}
                        </Text>
                        <Text
                          style={{ fontSize: FONT_SIZE.caption, fontVariant: ["tabular-nums"], flexShrink: 0 }}
                          class="text-muted-foreground"
                        >
                          {String(sec().rows.length)}
                        </Text>
                      </View>
                    </View>

                    <If when={!isCollapsed()}>
                      <View style={{ flexShrink: 0 }}>
                        <Each of={sec().rows} key={(r) => r.id}>
                          {(row) => (
                            <GroupedRow
                              row={row()}
                              columns={props.columns}
                              showInlineId={props.showInlineId}
                              maxTags={props.maxTags}
                              tagsWidth={props.tagsWidth}
                              trainingEnabled={props.trainingEnabled}
                              selected={props.selectedIds.has(row().id)}
                              anySelected={props.selectedIds.size > 0}
                              focused={props.focusedId === row().id && props.selectedIds.size === 0}
                              onRowClick={props.onRowClick}
                              onRowDoubleClick={props.onRowDoubleClick}
                              onRowContextMenu={props.onRowContextMenu}
                            />
                          )}
                        </Each>
                      </View>
                    </If>
                  </View>
                );
              }}
            </Each>
          </View>
        </Scroll>
      </If>
    </View>
  );
}

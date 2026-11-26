// Grouped folding list view for LibraryView (Phase 6D §8.1):
// Headerless scrollable list rendering collapsible GroupHeader (32pt) & LibraryListRow (40pt) sections.

import { Scroll, signal, Text, View, type JSX } from "nsh";
import { Icon } from "../Icon";
import { FOCUS_RING } from "../kit";
import { headerCheckboxState, type ProblemGroupSection } from "../logic";
import { chevronTransition, colorTransition } from "../tokens";
import { LibraryListRow, TriCheckbox, type LibraryRow } from "./cells";
import { CONTROL, FONT_SIZE, FONT_WEIGHT, SPACE } from "../tokens";

export function GroupedProblemList(props: {
  sections: readonly ProblemGroupSection<LibraryRow>[];
  showRatingCol: boolean;
  showStatusCol: boolean;
  trainingEnabled?: boolean;
  selectedIds: ReadonlySet<string>;
  focusedId: string | undefined;
  onRowClick: (id: string, opts: { shift: boolean; mod: boolean; checkbox: boolean }) => void;
  onRowDoubleClick: (id: string) => void;
  onToggleGroupSelect: (ids: readonly string[]) => void;
  onRowContextMenu: (row: LibraryRow, x: number, y: number) => void;
  empty: JSX.Element;
}) {
  const collapsed = signal<ReadonlySet<string>>(new Set());

  const toggleCollapse = (key: string) => {
    const next = new Set(collapsed());
    if (next.has(key)) next.delete(key);
    else next.add(key);
    collapsed.set(next);
  };

  return (
    <View style={{ flex: 1, minWidth: 0 }} class="bg-card">
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
                        height: CONTROL.lg,
                        paddingLeft: SPACE[8],
                        flexDirection: "row",
                        alignItems: "center",
                        borderBottomWidth: 1,
                      }}
                      class="bg-muted/40 border-border"
                    >
                      <View
                        class={FOCUS_RING}
                        style={{ width: 24, height: CONTROL.lg, alignItems: "center", justifyContent: "center", flexShrink: 0 }}
                        onPress={() => props.onToggleGroupSelect(rowIds())}
                      >
                        <TriCheckbox
                          state={groupTri()}
                          visible={() => groupHovered() || groupTri() !== "none" || props.selectedIds.size > 0}
                        />
                      </View>
                      <View
                        onPress={() => toggleCollapse(sec().key)}
                        class={`hover:bg-muted/70 ${FOCUS_RING}`}
                        style={{
                          flex: 1,
                          height: CONTROL.lg,
                          paddingLeft: SPACE[8],
                          paddingRight: SPACE[12],
                          flexDirection: "row",
                          alignItems: "center",
                          gap: SPACE[6],
                          minWidth: 0,
                          ...colorTransition(),
                        }}
                      >
                        <Icon
                          name="chevronRight"
                          size="sm"
                          tone="muted-foreground"
                          style={{
                            transform: isCollapsed() ? "rotate(0deg)" : "rotate(90deg)",
                            ...chevronTransition(),
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
                            <LibraryListRow
                              row={row()}
                              showRatingCol={props.showRatingCol}
                              showStatusCol={props.showStatusCol}
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

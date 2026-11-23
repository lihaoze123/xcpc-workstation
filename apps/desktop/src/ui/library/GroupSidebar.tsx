// Collapsible persistent group sidebar for LibraryView (§5.1):
// 0 ms outer width toggle (handled by parent <If>) to prevent DataTable reflow jank,
// with subtle 120 ms inner fade/slide that respects `prefers-reduced-motion`.

import { effect, reduceMotion, Scroll, signal, Text, View, type ContextMenuEvent } from "nsh";
import { Menu as DesktopMenu } from "nsh/desktop";
import type { GroupSummaryJs } from "xcpc-workstation/native";
import { t } from "../i18n";
import { Icon, type IconName } from "../Icon";
import { Btn, tip } from "../kit";
import { colorTransition } from "../motion";
import { EASE_STANDARD, ms } from "../theme";

export type SidebarGroupSelection =
  | { kind: "all" }
  | { kind: "ungrouped" }
  | { kind: "named"; name: string };

function SidebarRow(props: {
  icon?: IconName;
  label: string;
  count: number;
  active: boolean;
  onPress: () => void;
  onContextMenu?: (e: ContextMenuEvent) => void;
  testID?: string;
}) {
  return (
    <View
      testID={props.testID}
      onPress={props.onPress}
      onContextMenu={props.onContextMenu}
      class={
        props.active
          ? "bg-accent focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-ring"
          : "hover:bg-muted active:bg-accent focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-ring"
      }
      style={{
        height: 30,
        paddingLeft: 8,
        paddingRight: 8,
        borderRadius: 6,
        flexDirection: "row",
        alignItems: "center",
        gap: 7,
        minWidth: 0,
        ...colorTransition(120),
      }}
    >
      {props.icon && (
        <Icon
          name={props.icon}
          size={14}
          tone={props.active ? "foreground" : "muted-foreground"}
        />
      )}
      <Text
        style={{
          flex: 1,
          minWidth: 0,
          fontSize: 13,
          lineHeight: 18,
          numberOfLines: 1,
          textOverflow: "ellipsis",
        }}
        class={props.active ? "text-foreground" : "text-secondary-foreground"}
      >
        {props.label}
      </Text>
      <Text
        style={{ fontSize: 11.5, fontVariant: ["tabular-nums"], flexShrink: 0 }}
        class={props.active ? "text-foreground" : "text-muted-foreground"}
      >
        {String(props.count)}
      </Text>
    </View>
  );
}

export function GroupSidebar(props: {
  selection: SidebarGroupSelection;
  onSelect: (sel: SidebarGroupSelection) => void;
  totalCount: number;
  ungroupedCount: number;
  groups: readonly GroupSummaryJs[];
  readOnly: boolean;
  onNewGroup: () => void;
  onRenameGroup: (name: string) => void;
  onDeleteGroup: (name: string) => void;
}) {
  const revealed = signal(reduceMotion());
  effect(() => {
    if (reduceMotion()) {
      revealed.set(true);
      return;
    }
    const raf = requestAnimationFrame(() => revealed.set(true));
    return () => cancelAnimationFrame(raf);
  });

  const openGroupMenu = (name: string, x: number, y: number) => {
    const ro = props.readOnly;
    void DesktopMenu.popup(
      [
        {
          label: t("library.sidebar.renameGroup"),
          disabled: ro,
          onSelect: () => props.onRenameGroup(name),
        },
        {
          label: t("library.sidebar.deleteGroup"),
          disabled: ro,
          destructive: true,
          onSelect: () => props.onDeleteGroup(name),
        },
      ],
      { x, y },
    );
  };

  return (
    <View
      testID="library-group-sidebar"
      style={{
        width: 180,
        flexShrink: 0,
        borderRightWidth: 1,
        overflow: "hidden",
      }}
      class="bg-background border-border"
    >
      <View
        style={{
          flex: 1,
          padding: 8,
          gap: 2,
          opacity: revealed() ? 1 : 0,
          transform: reduceMotion() ? undefined : revealed() ? "translateX(0px)" : "translateX(-4px)",
          transitionProperty: "opacity, transform",
          transitionDuration: ms(120),
          transitionTimingFunction: EASE_STANDARD,
        }}
      >
        <SidebarRow
          testID="group-item-all"
          icon="libraryBig"
          label={t("library.sidebar.all")}
          count={props.totalCount}
          active={props.selection.kind === "all"}
          onPress={() => props.onSelect({ kind: "all" })}
        />
        <SidebarRow
          testID="group-item-ungrouped"
          icon="folder"
          label={t("library.sidebar.ungrouped")}
          count={props.ungroupedCount}
          active={props.selection.kind === "ungrouped"}
          onPress={() => props.onSelect({ kind: "ungrouped" })}
        />
        <View style={{ height: 1, marginTop: 6, marginBottom: 6 }} class="bg-border" />
        <Scroll style={{ flex: 1 }}>
          <View style={{ gap: 2 }}>
            <Each of={props.groups} key={(g) => g.name}>
              {(g) => (
                <SidebarRow
                  label={g().name}
                  count={g().count}
                  active={props.selection.kind === "named" && props.selection.name === g().name}
                  onPress={() => props.onSelect({ kind: "named", name: g().name })}
                  onContextMenu={(e) => openGroupMenu(g().name, e.pagePosition.x, e.pagePosition.y)}
                />
              )}
            </Each>
          </View>
        </Scroll>
        <View style={{ paddingTop: 6, borderTopWidth: 1 }} class="border-border">
          <Btn
            testID="group-sidebar-new"
            variant="ghost"
            icon="plus"
            label={t("library.sidebar.newGroup")}
            disabled={props.readOnly}
            tooltip={tip(props.readOnly ? t("library.readOnlyCannotCreate") : t("library.sidebar.newGroup"))}
            onPress={props.onNewGroup}
            style={{ justifyContent: "flex-start", paddingLeft: 8 }}
          />
        </View>
      </View>
    </View>
  );
}

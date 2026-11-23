// Floating bottom batch action bar (§5.4):
// Appears centered at the bottom of the library table when `selectedIds.size >= 1`.

import { reduceMotion, Text, View } from "nsh";
import type { GroupSummaryJs } from "xcpc-workstation/native";
import { t } from "../i18n";
import { Btn, IconBtn, tip } from "../kit";
import { AppMenu, type MenuEntry } from "../Menu";
import { overlayTransition } from "../motion";
import { modalShadow } from "../theme";

export function BatchFloatingBar(props: {
  count: number;
  tableWidth: number;
  readOnly: boolean;
  groups: readonly GroupSummaryJs[];
  onAddToContest: () => void;
  onMoveToGroup: (group: string | null) => void;
  onNewGroupAndMove: () => void;
  onEditTags: () => void;
  onSetRating: () => void;
  onStartBuild: (mode: "verify" | "debug") => void;
  onExport: () => void;
  onDelete: () => void;
  onClear: () => void;
}) {
  const showLabels = () => props.tableWidth >= 680;

  const groupMenuEntries = (): MenuEntry[] => [
    ...props.groups.map((g) => ({
      id: `g:${g.name}`,
      label: g.name,
      icon: "folder" as const,
      note: String(g.count),
    })),
    ...(props.groups.length > 0 ? [{ separator: true as const, id: "sep-ungroup" }] : []),
    { id: "__ungroup__", label: t("library.menu.ungroup"), icon: "x" as const },
    { separator: true as const, id: "sep-new" },
    { id: "__new__", label: t("library.sidebar.newGroup"), icon: "plus" as const },
  ];

  const buildMenuEntries = (): MenuEntry[] => [
    { id: "verify", label: t("library.batch.buildVerify"), icon: "shieldCheck" },
    { id: "debug", label: t("library.batch.buildDebug"), icon: "play" },
  ];

  return (
    <View
      testID="library-batch-bar"
      style={{
        position: "absolute",
        left: 16,
        right: 16,
        bottom: 16,
        alignItems: "center",
        pointerEvents: "box-none",
      }}
    >
      <View
        class="bg-card border border-border"
        style={{
          height: 44,
          borderRadius: 12,
          paddingLeft: 12,
          paddingRight: 8,
          flexDirection: "row",
          alignItems: "center",
          gap: 4,
          boxShadow: modalShadow(),
          opacity: 1,
          transform: reduceMotion() ? undefined : "translateY(0px) scale(1)",
          ...overlayTransition(),
        }}
      >
        <Text
          style={{
            fontSize: 13,
            fontWeight: 500,
            fontVariant: ["tabular-nums"],
            paddingRight: 6,
            flexShrink: 0,
          }}
          class="text-foreground"
        >
          {t("library.batch.selectedCount", { count: props.count })}
        </Text>
        <View style={{ width: 1, height: 18, marginRight: 2 }} class="bg-border" />

        <Btn
          variant="ghost"
          icon="listPlus"
          label={showLabels() ? t("library.batch.addToContest") : undefined}
          disabled={props.readOnly}
          tooltip={tip(t("library.batch.addToContest"))}
          onPress={props.onAddToContest}
        />

        <AppMenu
          entries={groupMenuEntries()}
          onSelect={(id) => {
            if (id === "__ungroup__") props.onMoveToGroup(null);
            else if (id === "__new__") props.onNewGroupAndMove();
            else if (id.startsWith("g:")) props.onMoveToGroup(id.slice(2));
          }}
          trigger={
            <Btn
              variant="ghost"
              icon="folder"
              label={showLabels() ? t("library.batch.moveToGroup") : undefined}
              disabled={props.readOnly}
              tooltip={tip(t("library.batch.moveToGroup"))}
            />
          }
        />

        <Btn
          variant="ghost"
          icon="pencil"
          label={showLabels() ? t("library.batch.tags") : undefined}
          disabled={props.readOnly}
          tooltip={tip(t("library.batch.editTags"))}
          onPress={props.onEditTags}
        />

        <Btn
          variant="ghost"
          icon="scale"
          label={showLabels() ? t("library.batch.rating") : undefined}
          disabled={props.readOnly}
          tooltip={tip(t("library.batch.setRating"))}
          onPress={props.onSetRating}
        />

        <AppMenu
          entries={buildMenuEntries()}
          onSelect={(id) => {
            if (id === "verify" || id === "debug") props.onStartBuild(id);
          }}
          trigger={
            <Btn
              variant="ghost"
              icon="play"
              label={showLabels() ? t("library.batch.build") : undefined}
              disabled={props.readOnly}
              tooltip={tip(t("library.batch.build"))}
            />
          }
        />

        <Btn
          variant="ghost"
          icon="fileDown"
          label={showLabels() ? t("library.batch.export") : undefined}
          disabled={props.readOnly}
          tooltip={tip(t("library.batch.export"))}
          onPress={props.onExport}
        />

        <Btn
          variant="ghost"
          icon="trash"
          label={showLabels() ? t("library.batch.delete") : undefined}
          disabled={props.readOnly}
          tooltip={tip(t("library.batch.delete"))}
          onPress={props.onDelete}
          textStyle={{ color: "var(--destructive)" }}
        />

        <View style={{ width: 1, height: 18, marginLeft: 2, marginRight: 2 }} class="bg-border" />
        <IconBtn
          icon="x"
          size={28}
          iconSize={15}
          tooltip={tip(t("library.batch.clearSelection"), "escape")}
          onPress={props.onClear}
        />
      </View>
    </View>
  );
}

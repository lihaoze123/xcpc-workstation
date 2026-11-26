// Right-hand BatchInspector panel (§5.4):
// Replaces ProblemInspector inside the fixed 380pt right ResizablePanel when
// `selectedIds.size >= 2` OR a background batch task is running / showing results.

import { computed, Scroll, Text, View } from "nsh";
import { Progress } from "nsh/base";
import type { BatchTaskItemJs, BatchTaskSnapshotJs, GroupSummaryJs, ProblemSummaryJs } from "xcpc-workstation/native";
import { t } from "../i18n";
import { Icon, type IconName, type IconTone } from "../Icon";
import { Btn, DelayedSpinner, tip } from "../kit";
import { ErrorDisplay } from "../ErrorDisplay";
import { DetailHeader } from "../layout";
import { errorCopy } from "../logic";
import { AppMenu, type MenuEntry } from "../Menu";
import { colorTransition } from "../motion";
import {
  batchTask,
  cancelBatchTask,
  dismissBatchTask,
  info,
  openStudio,
  preview,
} from "../store";

import { CONTROL, FONT_SIZE, FONT_WEIGHT, RADIUS, ROW, SPACE } from "../tokens";
function formatElapsed(msVal: number): string {
  const rounded = Math.max(0, Math.round(msVal));
  if (rounded < 1000) return `${rounded} ms`;
  return `${(rounded / 1000).toFixed(1)} s`;
}

function taskHeadline(snap: BatchTaskSnapshotJs): string {
  if (snap.running) {
    if (snap.kind === "build_verify") return t("library.batch.runningVerify", { completed: snap.completed, total: snap.total });
    if (snap.kind === "build_debug") return t("library.batch.runningDebug", { completed: snap.completed, total: snap.total });
    if (snap.kind === "export_domjudge") return t("library.batch.runningExportDomjudge", { completed: snap.completed, total: snap.total });
    if (snap.kind === "export_hydro") return t("library.batch.runningExportHydro", { completed: snap.completed, total: snap.total });
    return t("library.batch.runningExportPdf");
  }
  if (snap.cancelled) return t("library.batch.cancelledTitle");
  if (snap.kind === "build_verify") return t("library.batch.finishedVerify");
  if (snap.kind === "build_debug") return t("library.batch.finishedDebug");
  return t("library.batch.finishedExport");
}

function itemVisual(item: BatchTaskItemJs): { icon?: IconName; tone: IconTone; label: string; clickable: boolean } {
  switch (item.status) {
    case "passed":
      return { icon: "check", tone: "primary", label: t("library.batch.statusPassed"), clickable: false };
    case "exported":
      return { icon: "check", tone: "primary", label: t("library.batch.statusExported"), clickable: false };
    case "failed":
      return { icon: "triangleAlert", tone: "destructive", label: t("library.batch.statusFailed"), clickable: true };
    case "error":
      return { icon: "triangleAlert", tone: "destructive", label: t("library.batch.statusError"), clickable: true };
    case "skipped":
      return { icon: "minus", tone: "muted-foreground", label: t("library.batch.statusSkipped"), clickable: false };
    case "cancelled":
      return { icon: "x", tone: "muted-foreground", label: t("library.batch.statusCancelled"), clickable: false };
    case "running":
      return { tone: "primary", label: t("library.batch.statusRunning"), clickable: false };
    default:
      return { icon: "minus", tone: "muted-foreground", label: t("library.batch.statusPending"), clickable: false };
  }
}

function ActionRow(props: {
  icon: IconName;
  label: string;
  destructive?: boolean;
  disabled?: boolean;
  onPress?: () => void;
}) {
  return (
    <View
      disabled={props.disabled}
      onPress={() => {
        if (!props.disabled) props.onPress?.();
      }}
      class={
        props.disabled
          ? ""
          : "hover:bg-muted active:bg-accent focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-ring"
      }
      style={{
        height: ROW.table,
        paddingLeft: SPACE[12],
        paddingRight: SPACE[12],
        borderRadius: RADIUS.sm,
        flexDirection: "row",
        alignItems: "center",
        gap: SPACE[8],
        opacity: props.disabled ? 0.5 : 1,
        ...colorTransition(120),
      }}
    >
      <Icon
        name={props.icon}
        size="md"
        tone={props.destructive ? "destructive" : "secondary-foreground"}
      />
      <Text
        style={{ fontSize: FONT_SIZE.body, flex: 1, numberOfLines: 1, textOverflow: "ellipsis" }}
        class={props.destructive ? "text-destructive" : "text-foreground"}
      >
        {props.label}
      </Text>
    </View>
  );
}

export function BatchInspector(props: {
  selectedProblems: readonly ProblemSummaryJs[];
  groups: readonly GroupSummaryJs[];
  readOnly: boolean;
  onClearSelection: () => void;
  onAddToContest: () => void;
  onMoveToGroup: (group: string | null) => void;
  onNewGroupAndMove: () => void;
  onEditTags: () => void;
  onSetRating: () => void;
  onStartBuild: (mode: "verify" | "debug") => void;
  onExport: () => void;
  onDelete: () => void;
}) {
  const task = () => batchTask();
  const titleById = computed(() => new Map(info().problems.map((p) => [p.id, p.title])));

  const summary = computed(() => {
    const list = props.selectedProblems;
    const ratings = list.map((p) => p.rating).filter((r): r is number => r != null);
    let ratingText = t("library.batch.unratedSummary");
    if (ratings.length > 0) {
      const min = Math.min(...ratings);
      const max = Math.max(...ratings);
      const avg = Math.round(ratings.reduce((a, b) => a + b, 0) / ratings.length);
      ratingText =
        min === max
          ? t("library.batch.ratingSingle", { val: min })
          : t("library.batch.ratingSummary", { min, max, avg });
    }
    let packages = 0;
    let verified = 0;
    for (const p of list) {
      if (p.packageStatus) {
        packages += 1;
        if (p.packageStatus === "verified_fresh") verified += 1;
      }
    }
    return {
      ratingText,
      packageText: t("library.batch.packageSummary", { verified, packages, total: list.length }),
    };
  });

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

  const headerTitle = () => {
    const cur = task();
    if (props.selectedProblems.length > 0) {
      return t("library.batch.selectedCount", { count: props.selectedProblems.length });
    }
    return cur ? taskHeadline(cur) : "";
  };

  return (
    <View testID="library-batch-inspector" style={{ flex: 1, minWidth: 0 }} class="bg-card">
      <View style={{ borderBottomWidth: 1 }} class="border-border">
        <DetailHeader
          title={headerTitle()}
          actions={
            props.selectedProblems.length > 0 ? (
              <Btn
                variant="ghost"
                label={t("library.batch.clearSelection")}
                onPress={props.onClearSelection}
              />
            ) : undefined
          }
        />
      </View>

      <If when={props.selectedProblems.length > 0}>
        <View
          style={{
            paddingLeft: SPACE[16],
            paddingRight: SPACE[16],
            paddingTop: SPACE[12],
            paddingBottom: SPACE[12],
            gap: SPACE[4],
            borderBottomWidth: 1,
          }}
          class="border-border bg-muted/30"
        >
          <Text style={{ fontSize: FONT_SIZE.caption, fontVariant: ["tabular-nums"] }} class="text-secondary-foreground">
            {summary().ratingText}
          </Text>
          <Text style={{ fontSize: FONT_SIZE.caption, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
            {summary().packageText}
          </Text>
        </View>
      </If>

      <If
        when={task()}
        else={
          <Scroll style={{ flex: 1 }}>
            <View style={{ padding: SPACE[12], gap: SPACE[2] }}>
              <ActionRow
                icon="listPlus"
                label={t("library.batch.addToContest")}
                disabled={props.readOnly}
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
                  <ActionRow
                    icon="folder"
                    label={t("library.batch.moveToGroup")}
                    disabled={props.readOnly}
                  />
                }
              />
              <ActionRow
                icon="pencil"
                label={t("library.batch.editTags")}
                disabled={props.readOnly}
                onPress={props.onEditTags}
              />
              <ActionRow
                icon="scale"
                label={t("library.batch.setRating")}
                disabled={props.readOnly}
                onPress={props.onSetRating}
              />
              <View style={{ height: 1, marginTop: SPACE[6], marginBottom: SPACE[6] }} class="bg-border" />
              <ActionRow
                icon="shieldCheck"
                label={t("library.batch.buildVerify")}
                disabled={props.readOnly}
                onPress={() => props.onStartBuild("verify")}
              />
              <ActionRow
                icon="play"
                label={t("library.batch.buildDebug")}
                disabled={props.readOnly}
                onPress={() => props.onStartBuild("debug")}
              />
              <ActionRow
                icon="fileDown"
                label={t("library.batch.export")}
                disabled={props.readOnly}
                onPress={props.onExport}
              />
              <View style={{ height: 1, marginTop: SPACE[6], marginBottom: SPACE[6] }} class="bg-border" />
              <ActionRow
                icon="trash"
                label={t("library.batch.delete")}
                destructive
                disabled={props.readOnly}
                onPress={props.onDelete}
              />
            </View>
          </Scroll>
        }
      >
        {(snap) => {
          const pct = () => {
            const s = snap();
            if (s.total <= 0) return s.running ? 0 : 100;
            return Math.min(100, Math.round((s.completed / s.total) * 100));
          };
          const isExport = () => snap().kind.startsWith("export_");
          return (
            <View style={{ flex: 1, minHeight: 0 }}>
              {/* Live status header + Progress bar */}
              <View style={{ padding: SPACE[12], gap: SPACE[8], borderBottomWidth: 1 }} class="border-border">
                <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[8] }}>
                  <Text
                    style={{ flex: 1, fontSize: FONT_SIZE.body, fontWeight: FONT_WEIGHT.semibold, fontVariant: ["tabular-nums"], numberOfLines: 1, textOverflow: "ellipsis" }}
                    class="text-foreground"
                  >
                    {taskHeadline(snap())}
                  </Text>
                  <Text style={{ fontSize: FONT_SIZE.caption, fontVariant: ["tabular-nums"], flexShrink: 0 }} class="text-muted-foreground">
                    {formatElapsed(snap().elapsedMs)}
                  </Text>
                  <If when={snap().running}>
                    <Btn variant="outline" label={t("common.cancel")} onPress={cancelBatchTask} />
                  </If>
                </View>
                <Progress
                  value={pct()}
                  indeterminate={snap().running && snap().kind === "export_merged_pdf"}
                  trackClass="h-1.5 rounded-full bg-muted overflow-hidden"
                  indicatorClass="h-full rounded-full bg-primary"
                />
                <ErrorDisplay
                  error={
                    snap().error
                      ? { code: snap().code ?? undefined, error: snap().error ?? undefined }
                      : undefined
                  }
                  slot
                />
              </View>

              {/* Per-problem live results list */}
              <Scroll style={{ flex: 1 }}>
                <View style={{ padding: SPACE[8], gap: SPACE[2] }}>
                  <Each of={snap().items} key={(it) => it.problemId}>
                    {(it) => {
                      const vis = () => itemVisual(it());
                      const title = () => titleById().get(it().problemId) ?? it().problemId;
                      const detailText = () => {
                        const item = it();
                        if (item.status === "running" && item.phase) return item.phase;
                        if (item.code) return errorCopy(item.code, item.message).message;
                        if (item.message) return item.message;
                        if (item.elapsedMs > 0) return formatElapsed(item.elapsedMs);
                        return vis().label;
                      };
                      return (
                        <View
                          onPress={() => {
                            if (vis().clickable) openStudio({ target: "problem", id: it().problemId, initialTab: "build" });
                          }}
                          tooltip={vis().clickable ? tip(t("library.batch.openBuildTooltip")) : undefined}
                          class={vis().clickable ? "hover:bg-muted active:bg-accent" : undefined}
                          style={{
                            height: CONTROL.lg,
                            paddingLeft: SPACE[8],
                            paddingRight: SPACE[8],
                            borderRadius: RADIUS.sm,
                            flexDirection: "row",
                            alignItems: "center",
                            gap: SPACE[8],
                            minWidth: 0,
                            ...colorTransition(120),
                          }}
                        >
                          <View style={{ width: 16, alignItems: "center", justifyContent: "center", flexShrink: 0 }}>
                            {it().status === "running" ? (
                              <DelayedSpinner active size={12} />
                            ) : vis().icon ? (
                              <Icon name={vis().icon!} size="sm" tone={vis().tone} />
                            ) : null}
                          </View>
                          <View style={{ flex: 1, minWidth: 0, overflow: "hidden", flexDirection: "row", alignItems: "center", gap: SPACE[6] }}>
                            <Text
                              style={{ fontSize: FONT_SIZE.caption, flexShrink: 1, minWidth: 0, numberOfLines: 1, textOverflow: "ellipsis" }}
                              class="text-foreground"
                            >
                              {title()}
                            </Text>
                            <Text
                              style={{
                                fontSize: FONT_SIZE.micro,
                                fontVariant: ["tabular-nums"],
                                maxWidth: 96,
                                flexShrink: 2,
                                minWidth: 0,
                                numberOfLines: 1,
                                textOverflow: "ellipsis",
                              }}
                              class="text-muted-foreground"
                            >
                              {it().problemId}
                            </Text>
                          </View>
                          <Text
                            style={{
                              fontSize: FONT_SIZE.caption,
                              fontVariant: ["tabular-nums"],
                              maxWidth: 120,
                              flexShrink: 0,
                              numberOfLines: 1,
                              textOverflow: "ellipsis",
                            }}
                            class={
                              it().status === "failed" || it().status === "error"
                                ? "text-destructive"
                                : "text-muted-foreground"
                            }
                          >
                            {detailText()}
                          </Text>
                        </View>
                      );
                    }}
                  </Each>
                </View>
              </Scroll>

              {/* Footer summary & actions when finished */}
              <If when={!snap().running}>
                <View
                  style={{
                    padding: SPACE[12],
                    borderTopWidth: 1,
                    gap: SPACE[8],
                  }}
                  class="border-border bg-background/60"
                >
                  <Text style={{ fontSize: FONT_SIZE.caption, fontVariant: ["tabular-nums"] }} class="text-secondary-foreground">
                    {isExport()
                      ? t("library.batch.summaryExportLine", {
                          succeeded: snap().succeeded,
                          failed: snap().failed,
                          skipped: snap().skipped,
                        })
                      : t("library.batch.summaryLine", {
                          succeeded: snap().succeeded,
                          failed: snap().failed,
                          skipped: snap().skipped,
                        })}
                  </Text>
                  <View style={{ flexDirection: "row", gap: SPACE[8], justifyContent: "flex-end" }}>
                    <If when={!!snap().outputPath}>
                      <Btn
                        variant="outline"
                        icon="folderOpen"
                        label={t("common.revealInFolder")}
                        onPress={() => {
                          const out = snap().outputPath;
                          if (out) void preview.revealInFolder(out);
                        }}
                      />
                    </If>
                    <Btn
                      variant="ghost"
                      label={t("library.batch.dismissResult")}
                      onPress={dismissBatchTask}
                    />
                  </View>
                </View>
              </If>
            </View>
          );
        }}
      </If>
    </View>
  );
}

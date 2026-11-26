// Cmd+2: the local contests (composition, finalized copies, and collapsible archived list).
// The list on the left (active contests + collapsible 已归档 section, 新建 at its toolbar's end);
// on the right the selected one's head (language, date, place; 编辑/查看, 定稿 / 重新编辑 / 取消归档, ⋯),
// its overview (the difficulty bar and tags) and its problems.

import { computed, effect, keymap, Scroll, signal, Text, View, type ScrollRef, type ViewRef } from "nsh";
import { Menu as DesktopMenu } from "nsh/desktop";
import type { WorkspaceContest } from "xcpc-workstation/native";
import { Btn, DelayedSpinner, FOCUS_RING, RatingBadge, tip } from "./kit";
import { t } from "./i18n";
import { Icon } from "./Icon";
import { AppMenu, MoreTrigger } from "./Menu";
import { ErrorDisplay } from "./ErrorDisplay";
import { ContestStats, createFreeze, freezeBlockedCopy } from "./Contest";
import { DetailHeader, EmptyState, GUTTER, Spacer, Toolbar } from "./layout";
import { labels, moveItem, partitionContests } from "./logic";
import { EASE_STANDARD, ms, popoverShadow } from "./theme";
import {
  activeRevision,
  archivedExpanded,
  dialog,
  errorOf,
  importPolygon,
  importingPolygon,
  info,
  openStudio,
  preview,
  refresh,
  selectedContest,
  unfreezeFor,
  workspaceVersion,
  type UiError,
} from "./store";
import { createReorder } from "./studio/reorder";
import { StatusMark } from "./studio/StatusSlot";
import { markForPackageStatus } from "./studio/statusLogic";

import { CONTROL, FONT_SIZE, FONT_WEIGHT, RADIUS, ROW, SPACE, TOOLBAR, TYPE } from "./tokens";
const ROW_STRIDE = 54; // 52pt row + 2pt gap

function toggleContestArchive(
  id: string,
  archive: boolean,
  onError?: (err: UiError) => void,
) {
  const before = info().contests;
  const res = preview.setContestArchived(id, archive);
  if (!res.ok) {
    onError?.(errorOf(res, archive ? t("ctx.archiveContest") : t("ctx.unarchiveContest"))!);
    return;
  }
  if (archive) {
    if (selectedContest() === id) {
      const remainingActive = before.filter((c) => !c.archived && c.id !== id);
      selectedContest.set(remainingActive[0]?.id);
    }
  } else {
    selectedContest.set(id);
  }
  refresh();
}

export function ContestsView(props: { active: boolean }) {
  const rev = activeRevision(() => props.active);
  const contests = () => info().contests;
  const contestDates = computed(() => {
    void rev();
    const map = new Map<string, string>();
    for (const c of contests()) {
      const date = preview.contestDetail(c.id).date?.trim();
      if (date) map.set(c.id, date);
    }
    return map;
  });
  const split = computed(() => partitionContests(contests()));
  const activeContests = () => split().active;
  const archivedContests = () => split().archived;
  const visibleContests = computed(() =>
    archivedExpanded() ? [...activeContests(), ...archivedContests()] : activeContests(),
  );
  const readOnly = () => info().readOnly;
  const listError = signal<UiError | undefined>(undefined);
  let listScroll: ScrollRef | undefined;
  let listHeight = 0;
  let listScrollY = 0;
  let archivedScroll: ScrollRef | undefined;
  let archivedHeight = 0;
  let archivedScrollY = 0;

  effect(() => {
    void workspaceVersion();
    listError.set(undefined);
  });

  effect(() => {
    const all = contests();
    const active = activeContests();
    const id = selectedContest();
    if (all.length === 0) {
      if (id !== undefined) queueMicrotask(() => selectedContest.set(undefined));
    } else if (id === undefined || !all.some((c) => c.id === id)) {
      const next = active[0]?.id ?? (archivedExpanded() ? archivedContests()[0]?.id : undefined);
      if (next !== id) queueMicrotask(() => selectedContest.set(next));
    }
  });

  function ensureActiveVisible(index: number) {
    if (!listScroll || listHeight <= 0 || index < 0) return;
    const rowTop = 8 + index * ROW_STRIDE;
    const rowBottom = rowTop + 52;
    if (rowTop < listScrollY + 8) {
      listScroll.scrollTo({ y: Math.max(0, rowTop - 8), animated: false });
    } else if (rowBottom > listScrollY + listHeight - 8) {
      listScroll.scrollTo({ y: Math.max(0, rowBottom - listHeight + 8), animated: false });
    }
  }

  function ensureArchivedVisible(index: number) {
    if (!archivedScroll || archivedHeight <= 0 || index < 0) return;
    const rowTop = 4 + index * ROW_STRIDE;
    const rowBottom = rowTop + 52;
    if (rowTop < archivedScrollY + 4) {
      archivedScroll.scrollTo({ y: Math.max(0, rowTop - 4), animated: false });
    } else if (rowBottom > archivedScrollY + archivedHeight - 4) {
      archivedScroll.scrollTo({ y: Math.max(0, rowBottom - archivedHeight + 4), animated: false });
    }
  }

  effect(() => {
    const id = selectedContest();
    if (!id) return;
    const activeIdx = activeContests().findIndex((c) => c.id === id);
    if (activeIdx >= 0) {
      ensureActiveVisible(activeIdx);
      return;
    }
    if (archivedExpanded()) {
      const archIdx = archivedContests().findIndex((c) => c.id === id);
      if (archIdx >= 0) {
        requestAnimationFrame(() => ensureArchivedVisible(archIdx));
      }
    }
  });

  function move(delta: number) {
    const list = visibleContests();
    if (list.length === 0) return;
    const cur = list.findIndex((c) => c.id === selectedContest());
    const i = cur < 0 ? 0 : Math.min(list.length - 1, Math.max(0, cur + delta));
    const target = list[i];
    selectedContest.set(target.id);
    if (target.archived) {
      const archIdx = archivedContests().findIndex((c) => c.id === target.id);
      ensureArchivedVisible(archIdx);
    } else {
      ensureActiveVisible(i);
    }
  }

  function openSelected() {
    const id = selectedContest();
    if (id && contests().some((c) => c.id === id)) openStudio({ target: "contest", id });
  }

  keymap.bind({ j: () => props.active && move(1), k: () => props.active && move(-1) }, { when: "notEditing" });
  keymap.bind("mod+shift+n", () => props.active && !readOnly() && dialog.set({ kind: "newContest" }));

  const renderContestRow = (c: () => WorkspaceContest, muted = false) => {
    const selected = () => c().id === selectedContest();
    const date = () => contestDates().get(c().id);
    const hasSecondLine = () => Boolean(date()) || c().frozen;
    return (
      <View
        testID={`contest-${c().id}`}
        onPress={(e) => {
          selectedContest.set(c().id);
          if (e.clickCount >= 2) openStudio({ target: "contest", id: c().id });
        }}
        onContextMenu={(e) => {
          const cur = c();
          selectedContest.set(cur.id);
          const ro = readOnly();
          const locked = cur.frozen || cur.archived || ro;
          void DesktopMenu.popup(
            [
              { label: locked ? t("common.view") : t("common.edit"), onSelect: () => openStudio({ target: "contest", id: cur.id }) },
              {
                label: t("contests.pickProblemsEllipsis"),
                disabled: locked,
                onSelect: () => dialog.set({ kind: "pickProblems", contestId: cur.id }),
              },
              {
                label: t("contests.newProblemAddEllipsis"),
                disabled: locked,
                onSelect: () => dialog.set({ kind: "newProblem", addTo: cur.id }),
              },
              { type: "separator" },
              {
                label: cur.archived ? t("contests.unarchive") : t("contests.archive"),
                disabled: ro,
                onSelect: () => toggleContestArchive(cur.id, !cur.archived, (err) => listError.set(err)),
              },
              { label: t("common.revealInFolder"), onSelect: () => void preview.revealInFolder(`contests/${cur.id}`) },
            ],
            e.pagePosition,
          );
        }}
        class={selected() ? `bg-accent ${FOCUS_RING}` : `hover:bg-muted active:bg-accent ${FOCUS_RING}`}
        style={{
          height: TOOLBAR,
          paddingLeft: SPACE[12],
          paddingRight: SPACE[12],
          borderRadius: RADIUS.md,
          flexDirection: "row",
          alignItems: "center",
          gap: SPACE[8],
          opacity: muted && !selected() ? 0.78 : 1,
          transitionProperty: "colors",
          transitionDuration: ms(120),
          transitionTimingFunction: EASE_STANDARD,
        }}
      >
        <View style={{ flex: 1, minWidth: 0, gap: SPACE[2] }}>
          <Text style={{ ...TYPE.body, numberOfLines: 1, textOverflow: "ellipsis" }} class="text-foreground">
            {c().name}
          </Text>
          <If when={hasSecondLine()}>
            <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[6] }}>
              <If when={Boolean(date())}>
                <Text style={{ ...TYPE.caption, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
                  {date()}
                </Text>
              </If>
              <If when={c().frozen}>
                <Icon name="lock" size="sm" tone="muted-foreground" />
              </If>
            </View>
          </If>
        </View>
      </View>
    );
  };

  let rootRef: ViewRef | undefined;
  effect(() => {
    if (props.active) {
      rootRef?.focus();
    }
  });

  return (
    <View
      ref={(r) => (rootRef = r)}
      focusable
      style={{ flex: 1, flexDirection: "row" }}
      class="bg-card"
      captureKeys={["up", "down", "enter"]}
      onKeyDown={(e) => {
        if (e.key === "ArrowDown") move(1);
        else if (e.key === "ArrowUp") move(-1);
        else if (e.key === "Enter") openSelected();
      }}
    >
      <View style={{ width: 280, borderRightWidth: 1 }} class="border-border bg-card">
        <Toolbar>
          <Spacer />
          <Btn
            testID="contests-import-polygon"
            icon="folderOpen"
            variant="outline"
            disabled={readOnly() || importingPolygon()}
            tooltip={tip(readOnly() ? t("packages.readOnlyCannotImport") : t("packages.importPolygonTooltip"))}
            onPress={() => void importPolygon()}
            style={{ flexShrink: 0 }}
          />
          <Btn
            label={t("contests.newContest")}
            icon="plus"
            disabled={readOnly()}
            tooltip={tip(readOnly() ? t("library.readOnlyCannotCreate") : t("contests.newContest"), readOnly() ? undefined : "mod+shift+n")}
            onPress={() => dialog.set({ kind: "newContest" })}
          />
        </Toolbar>
        <Scroll
          ref={(r) => (listScroll = r)}
          style={{ flex: 1 }}
          onLayout={(e) => (listHeight = e.bounds.size.height)}
          onScroll={(e) => (listScrollY = e.offset.y)}
        >
          <View style={{ padding: SPACE[8], gap: SPACE[2] }}>
            <Each of={activeContests()} key={(c) => c.id}>
              {(c) => renderContestRow(c, false)}
            </Each>
          </View>
        </Scroll>
        <If when={listError()}>
          <View style={{ padding: SPACE[8], borderTopWidth: 1 }} class="border-border">
            <ErrorDisplay error={listError()} onDismiss={() => listError.set(undefined)} />
          </View>
        </If>
        <If when={archivedContests().length > 0}>
          <View style={{ borderTopWidth: 1, flexShrink: 0 }} class="border-border bg-card">
            <View
              testID="archived-contests-toggle"
              onPress={() => archivedExpanded.set(!archivedExpanded())}
              class={`hover:bg-muted active:bg-accent ${FOCUS_RING}`}
              style={{
                height: ROW.table,
                paddingLeft: SPACE[12],
                paddingRight: SPACE[12],
                flexDirection: "row",
                alignItems: "center",
                gap: SPACE[6],
                transitionProperty: "colors",
                transitionDuration: ms(120),
                transitionTimingFunction: EASE_STANDARD,
              }}
            >
              <Icon
                name="chevronDown"
                size="sm"
                tone="muted-foreground"
                style={{
                  transform: archivedExpanded() ? "rotate(0deg)" : "rotate(-90deg)",
                  transitionProperty: "transform",
                  transitionDuration: ms(120),
                  transitionTimingFunction: EASE_STANDARD,
                }}
              />
              <Text style={{ fontSize: FONT_SIZE.caption, fontWeight: FONT_WEIGHT.semibold }} class="text-muted-foreground">
                {t("contests.archived")}
              </Text>
              <Text style={{ fontSize: FONT_SIZE.caption, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
                {String(archivedContests().length)}
              </Text>
            </View>
            <If when={archivedExpanded()}>
              <Scroll
                ref={(r) => (archivedScroll = r)}
                style={{ maxHeight: 220 }}
                onLayout={(e) => (archivedHeight = e.bounds.size.height)}
                onScroll={(e) => (archivedScrollY = e.offset.y)}
              >
                <View style={{ paddingLeft: SPACE[8], paddingRight: SPACE[8], paddingTop: SPACE[2], paddingBottom: SPACE[8], gap: SPACE[2] }}>
                  <Each of={archivedContests()} key={(c) => c.id}>
                    {(c) => renderContestRow(c, true)}
                  </Each>
                </View>
              </Scroll>
            </If>
          </View>
        </If>
      </View>
      <If
        when={contests().length > 0}
        else={
          <EmptyState
            icon="trophy"
            title={t("contests.emptyTitle")}
            hint={readOnly() ? t("common.readOnlyWorkspaceHint") : t("contests.emptyHint")}
            action={
              readOnly() ? undefined : (
                <Btn label={t("contests.newContest")} icon="plus" variant="primary" onPress={() => dialog.set({ kind: "newContest" })} tooltip={tip(t("contests.newContest"), "mod+shift+n")} />
              )
            }
          />
        }
      >
        <If
          when={selectedContest() && contests().some((c) => c.id === selectedContest())}
          else={
            <EmptyState
              icon="trophy"
              title={t("contests.noActiveTitle")}
              hint={t("contests.noActiveHint")}
              action={
                readOnly() ? undefined : (
                  <Btn label={t("contests.newContest")} icon="plus" variant="primary" onPress={() => dialog.set({ kind: "newContest" })} tooltip={tip(t("contests.newContest"), "mod+shift+n")} />
                )
              }
            />
          }
        >
          <ContestPanel id={selectedContest()!} active={props.active} />
        </If>
      </If>
    </View>
  );
}

function ContestPanel(props: { id: string; active: boolean }) {
  const rev = activeRevision(() => props.active);
  const detail = computed(() => {
    void rev();
    return preview.contestDetail(props.id);
  });
  const f = createFreeze();
  const actionError = signal<UiError | undefined>(undefined);
  const pendingOrder = signal<string[] | undefined>(undefined);
  const hoveredId = signal<string | undefined>(undefined);
  const open = (label?: string) => openStudio({ target: "contest", id: props.id, label });
  const readOnly = () => info().readOnly;
  const viewOnly = () => detail().frozen || detail().archived || readOnly();

  // Clear stale freeze/action errors and pending drag order whenever switching contests or workspaces.
  effect(() => {
    void props.id;
    void workspaceVersion();
    f.reset();
    actionError.set(undefined);
    pendingOrder.set(undefined);
    hoveredId.set(undefined);
  });

  effect(() => {
    const p = pendingOrder();
    if (p && detail().items.map((i) => i.id).join() === p.join()) {
      queueMicrotask(() => pendingOrder.set(undefined));
    }
  });

  const items = computed(() => {
    const base = detail().items;
    const p = pendingOrder();
    const ordered = p ? p.map((id) => base.find((i) => i.id === id)).filter((x) => x !== undefined) : base;
    const ls = labels(ordered.length);
    return ordered.map((item, i) => ({ item, label: ls[i] }));
  });

  const hasRated = computed(() => items().some((x) => x.item.rating != null));
  const canDrag = () => !viewOnly() && items().length > 1;
  const metaItems = computed(() =>
    [detail().language === "english" ? "English" : "中文", detail().date?.trim() ?? "", detail().location?.trim() ?? ""].filter(Boolean),
  );

  const reorder = createReorder((from, to) => {
    const ids = moveItem(
      items().map((x) => x.item.id),
      from,
      to,
    );
    pendingOrder.set(ids);
    const res = preview.composeContest(props.id, ids);
    if (!res.ok) {
      pendingOrder.set(undefined);
      actionError.set(errorOf(res, t("ctx.reorderContest")));
      return;
    }
    actionError.set(undefined);
    refresh();
  });

  effect(() => {
    reorder.prune(items().length);
  });

  const reveal = async () => {
    const res = await preview.revealInFolder(`contests/${props.id}`);
    actionError.set(res.ok ? undefined : errorOf(res));
  };

  return (
    <View style={{ flex: 1, minWidth: 0 }}>
      <DetailHeader
        bordered
        size="lg"
        title={detail().name}
        badge={
          detail().frozen || detail().archived ? (
            <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[4] }}>
              <If when={detail().frozen}>
                <View style={{ height: CONTROL.sm, paddingLeft: SPACE[6], paddingRight: SPACE[8], borderRadius: RADIUS.xs, flexDirection: "row", alignItems: "center", gap: SPACE[4] }} class="bg-muted">
                  <Icon name="lock" size="sm" />
                  <Text style={TYPE.caption} class="text-secondary-foreground">
                    {t("contests.finalized")}
                  </Text>
                </View>
              </If>
              <If when={detail().archived}>
                <View style={{ height: CONTROL.sm, paddingLeft: SPACE[8], paddingRight: SPACE[8], borderRadius: RADIUS.xs, flexDirection: "row", alignItems: "center" }} class="bg-muted">
                  <Text style={TYPE.caption} class="text-secondary-foreground">
                    {t("contests.archived")}
                  </Text>
                </View>
              </If>
            </View>
          ) : undefined
        }
        actions={
          <>
            <Btn
              variant="primary"
              icon={viewOnly() ? "fileText" : "pencil"}
              label={viewOnly() ? t("common.view") : t("common.edit")}
              tooltip={tip(
                detail().archived
                  ? t("contests.editTooltip.archived")
                  : detail().frozen
                    ? t("contests.editTooltip.finalized")
                    : readOnly()
                      ? t("contests.editTooltip.readOnly")
                      : t("contests.editTooltip.editable"),
                "enter",
              )}
              onPress={() => open()}
              testID="contest-edit"
            />
            <If
              when={detail().archived}
              else={
                <If
                  when={detail().frozen}
                  else={
                    <Btn
                      tooltip={tip(readOnly() ? t("contests.readOnlyCannotFinalize") : t("contests.finalizeTooltip"))}
                      disabled={f.busy() || readOnly() || detail().items.length === 0}
                      onPress={() => void f.freeze(props.id)}
                      style={{ minWidth: 76 }}
                    >
                      <View style={{ width: 15, height: 15, alignItems: "center", justifyContent: "center" }}>
                        <If when={f.busyVisible()} else={<Icon name="stamp" size="md" tone="foreground" />}>
                          <DelayedSpinner active={f.busy()} size={13} />
                        </If>
                      </View>
                      <Text class="text-foreground" style={{ fontSize: FONT_SIZE.body }}>
                        {t("contests.finalize")}
                      </Text>
                    </Btn>
                  }
                >
                  <Btn
                    icon="lockOpen"
                    label={t("contests.reopen")}
                    disabled={readOnly()}
                    tooltip={tip(readOnly() ? t("contests.readOnlyCannotReopen") : t("contests.reopenTooltip"))}
                    onPress={() => unfreezeFor.set(props.id)}
                  />
                </If>
              }
            >
              <Btn
                icon="rotateCcw"
                label={t("contests.unarchive")}
                disabled={readOnly()}
                tooltip={tip(readOnly() ? t("contests.readOnlyCannotUnarchive") : t("contests.unarchiveTooltip"))}
                onPress={() => toggleContestArchive(props.id, false, (e) => actionError.set(e))}
                testID="contest-unarchive"
              />
            </If>
            <AppMenu
              align="end"
              entries={[
                { id: "pickProblems", label: t("contests.pickProblemsEllipsis"), icon: "listPlus", disabled: viewOnly() },
                { id: "newProblem", label: t("contests.newProblemAddEllipsis"), icon: "plus", disabled: viewOnly() },
                { separator: true, id: "sep" },
                {
                  id: "archive",
                  label: detail().archived ? t("contests.unarchive") : t("contests.archive"),
                  icon: detail().archived ? "rotateCcw" : "folder",
                  disabled: readOnly(),
                },
                { id: "reveal", label: t("common.revealInFolder"), icon: "folderOpen" },
              ]}
              onSelect={(id) => {
                if (id === "pickProblems") dialog.set({ kind: "pickProblems", contestId: props.id });
                else if (id === "newProblem") dialog.set({ kind: "newProblem", addTo: props.id });
                else if (id === "archive") toggleContestArchive(props.id, !detail().archived, (e) => actionError.set(e));
                else if (id === "reveal") void reveal();
              }}
              trigger={<MoreTrigger testID="contest-more" />}
            />
          </>
        }
      />
      <If when={f.error() || actionError()}>
        <View style={{ paddingLeft: GUTTER, paddingRight: GUTTER, paddingTop: SPACE[12] }}>
          <ErrorDisplay
            error={f.error() ? { ...f.error()!, context: undefined } : actionError()}
            onDismiss={() => {
              f.reset();
              actionError.set(undefined);
            }}
            locate={
              f.error()
                ? f.findings().slice(0, 4).map((d) => ({
                    label: d.code?.startsWith("package_")
                      ? d.message
                      : d.file
                        ? `${d.file}${d.line ? `:${d.line}` : ""}`
                        : d.message,
                    onPress: () => {
                      if (d.code?.startsWith("package_") && d.file) {
                        const m = d.file.match(/^problems\/([^/]+)\//);
                        if (m) {
                          openStudio({ target: "problem", id: m[1], initialTab: "build" });
                          return;
                        }
                      }
                      open(detail().items.find((i) => d.file?.startsWith(`problems/${i.id}/`))?.label);
                    },
                  }))
                : undefined
            }
            action={
              f.error() && f.findings().length > 0 ? (
                <Text style={{ fontSize: FONT_SIZE.caption, fontVariant: ["tabular-nums"] }} class="text-secondary-foreground">
                  {f.findings().length > 4
                    ? t("contests.freezeBlockedMore", {
                        summary: freezeBlockedCopy(f.findings().length),
                        more: f.findings().length - 4,
                      })
                    : freezeBlockedCopy(f.findings().length)}
                </Text>
              ) : undefined
            }
          />
        </View>
      </If>
      <If
        when={detail().items.length > 0}
        else={
          <EmptyState
            icon="listPlus"
            title={t("contests.emptyProblemsTitle")}
            hint={
              detail().archived
                ? t("contests.emptyProblemsArchivedHint")
                : readOnly()
                  ? t("common.readOnlyWorkspaceHint")
                  : t("contests.emptyProblemsHint")
            }
            action={
              !viewOnly() ? (
                <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[8] }}>
                  <Btn
                    label={t("contests.pickProblems")}
                    icon="listPlus"
                    variant="primary"
                    onPress={() => dialog.set({ kind: "pickProblems", contestId: props.id })}
                  />
                  <Btn
                    label={t("contests.newProblemAdd")}
                    icon="plus"
                    variant="outline"
                    onPress={() => dialog.set({ kind: "newProblem", addTo: props.id })}
                  />
                </View>
              ) : undefined
            }
          />
        }
      >
        <Scroll
          smoothWheel
          style={{ flex: 1 }}
          onMouseMove={(e) => reorder.move(e)}
          onMouseUp={(e) => reorder.end(e)}
          onMouseLeave={(e) => {
            reorder.leave(e);
            hoveredId.set(undefined);
          }}
        >
          <View style={{ paddingTop: SPACE[12], paddingLeft: GUTTER, paddingRight: GUTTER, paddingBottom: SPACE[24], gap: SPACE[16] }}>
            <If when={metaItems().length > 0}>
              <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[12], paddingLeft: SPACE[8] }}>
                <Each of={metaItems()} key="index">
                  {(meta) => (
                    <Text style={{ ...TYPE.caption, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
                      {meta()}
                    </Text>
                  )}
                </Each>
              </View>
            </If>
            <View ref={(v) => reorder.container(v)}>
              <Each of={items()} key={(x) => x.item.id}>
                {(x, index) => {
                  const dragging = () => reorder.drag()?.from === index();
                  const showHandle = () => canDrag() && (hoveredId() === x().item.id || dragging());
                  return (
                    <View
                      ref={(v) => reorder.row(index(), v)}
                      onMouseEnter={() => hoveredId.set(x().item.id)}
                      onMouseLeave={() => {
                        if (hoveredId() === x().item.id) hoveredId.set(undefined);
                      }}
                      onPressDown={(e) => canDrag() && reorder.pressDown(index(), e, items().length)}
                      onPress={() => reorder.clicked() && !x().item.missing && open(x().label)}
                      tooltip={x().item.missing ? tip(t("contests.missingProblemTooltip")) : undefined}
                      class={
                        dragging()
                          ? "bg-card border border-border"
                          : x().item.missing
                            ? `border-border ${FOCUS_RING}`
                            : `hover:bg-muted active:bg-accent border-border ${FOCUS_RING}`
                      }
                      style={{
                        height: ROW.list,
                        paddingLeft: SPACE[8],
                        paddingRight: SPACE[12],
                        flexDirection: "row",
                        alignItems: "center",
                        gap: SPACE[8],
                        borderTopWidth: index() === 0 || dragging() ? 0 : 1,
                        borderRadius: dragging() ? RADIUS.sm : RADIUS.none,
                        zIndex: dragging() ? 2 : 0,
                        transform: dragging() ? `translateY(${reorder.offset()}px)` : "none",
                        boxShadow: dragging() ? popoverShadow() : "none",
                        cursor: canDrag() ? (dragging() ? "grabbing" : "grab") : "default",
                        opacity: x().item.missing ? 0.65 : 1,
                        transitionProperty: "colors",
                        transitionDuration: ms(120),
                        transitionTimingFunction: EASE_STANDARD,
                      }}
                    >
                      <View
                        style={{
                          width: 16,
                          alignItems: "center",
                          justifyContent: "center",
                          opacity: showHandle() ? 1 : 0,
                          transitionProperty: "opacity",
                          transitionDuration: ms(120),
                          transitionTimingFunction: EASE_STANDARD,
                        }}
                      >
                        <Icon name="gripVertical" size="sm" tone="muted-foreground" />
                      </View>
                      <Text
                        style={{ width: 28, ...TYPE.strong, fontVariant: ["tabular-nums"] }}
                        class="text-foreground"
                      >
                        {x().label}
                      </Text>
                      <Text
                        style={{ ...TYPE.body, flex: 1, minWidth: 0, numberOfLines: 1, textOverflow: "ellipsis" }}
                        class={x().item.missing ? "text-destructive" : "text-foreground"}
                      >
                        {x().item.missing ? t("contests.missingSuffix", { title: x().item.title }) : x().item.title}
                      </Text>
                      <View
                        tooltip={
                          x().item.missing
                            ? tip(t("contests.missingProblemTooltip"))
                            : x().item.packageStatus && x().item.packageStatus !== "verified_fresh"
                              ? tip(x().item.packageStatus === "failed" ? t("library.package.failed") : t("library.package.unverified"))
                              : undefined
                        }
                      >
                        <If when={x().item.missing} else={<StatusMark kind={markForPackageStatus(x().item.packageStatus)} />}>
                          <Icon name="triangleAlert" size="sm" tone="destructive" />
                        </If>
                      </View>
                      <Text
                        style={{ width: 110, ...TYPE.caption, textAlign: "right", fontVariant: ["tabular-nums"] }}
                        class="text-secondary-foreground"
                      >
                        {[x().item.limits.time, x().item.limits.memory].filter(Boolean).join(" / ")}
                      </Text>
                      <If when={hasRated()}>
                        <View style={{ width: 44, alignItems: "flex-end" }}>
                          <RatingBadge rating={x().item.rating} />
                        </View>
                      </If>
                    </View>
                  );
                }}
              </Each>
              <If when={reorder.line() !== undefined}>
                {/* token-exempt: 2px drag-drop insertion line */}
                <View style={{ position: "absolute", left: 8, right: 8, top: reorder.line()!, height: 2, borderRadius: 1 }} class="bg-primary" />
              </If>
              <If when={!viewOnly()}>
                <View
                  testID="contest-pick-problems"
                  onPress={() => dialog.set({ kind: "pickProblems", contestId: props.id })}
                  class={`hover:bg-muted active:bg-accent border-border ${FOCUS_RING}`}
                  style={{
                    height: ROW.list,
                    paddingLeft: SPACE[32],
                    paddingRight: SPACE[12],
                    borderTopWidth: 1,
                    flexDirection: "row",
                    alignItems: "center",
                    gap: SPACE[8],
                    transitionProperty: "colors",
                    transitionDuration: ms(120),
                    transitionTimingFunction: EASE_STANDARD,
                  }}
                >
                  <Icon name="plus" size="sm" tone="muted-foreground" />
                  <Text style={TYPE.body} class="text-muted-foreground">
                    {t("contests.pickProblems")}
                  </Text>
                </View>
              </If>
            </View>
            <ContestStats id={props.id} active={props.active} />
          </View>
        </Scroll>
      </If>
    </View>
  );
}

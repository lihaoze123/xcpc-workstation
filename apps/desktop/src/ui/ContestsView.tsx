// Cmd+2: the local contests (composition, finalized copies, and collapsible archived list).
// The list on the left (active contests + collapsible 已归档 section, 新建 at its toolbar's end);
// on the right the selected one's head (language, date, place; 编辑/查看, 定稿 / 重新编辑 / 取消归档, ⋯),
// its overview (the difficulty bar and tags) and its problems.

import { computed, effect, keymap, Scroll, signal, Text, View, type ScrollRef } from "nsh";
import { Menu as DesktopMenu } from "nsh/desktop";
import type { WorkspaceContest } from "xcpc-workstation/native";
import { Btn, DelayedSpinner, Lock, RatingBadge, tip } from "./kit";
import { t } from "./i18n";
import { Icon } from "./Icon";
import { AppMenu, MoreTrigger } from "./Menu";
import { ErrorDisplay } from "./ErrorDisplay";
import { ContestStats, createFreeze, freezeBlockedCopy } from "./Contest";
import { DetailHeader, EmptyState, GUTTER, Section, Spacer, Toolbar } from "./layout";
import { partitionContests } from "./logic";
import { EASE_STANDARD, ms } from "./theme";
import {
  archivedExpanded,
  dialog,
  errorOf,
  importPolygon,
  importingPolygon,
  info,
  openStudio,
  preview,
  refresh,
  revision,
  selectedContest,
  unfreezeFor,
  workspaceVersion,
  type UiError,
} from "./store";

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
  const contests = () => info().contests;
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
        class={selected() ? "bg-accent" : "hover:bg-muted active:bg-accent"}
        style={{
          height: 52,
          paddingLeft: 10,
          paddingRight: 10,
          borderRadius: 8,
          flexDirection: "row",
          alignItems: "center",
          gap: 10,
          opacity: muted && !selected() ? 0.78 : 1,
          transitionProperty: "colors",
          transitionDuration: ms(120),
          transitionTimingFunction: EASE_STANDARD,
        }}
      >
        <View style={{ flex: 1, minWidth: 0, gap: 3 }}>
          <Text style={{ numberOfLines: 1, textOverflow: "ellipsis", fontSize: 13.5, lineHeight: 18 }}>{c().name}</Text>
          <View style={{ flexDirection: "row", alignItems: "center", gap: 8 }}>
            <Text style={{ fontSize: 12, lineHeight: 16, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
              {t("contests.problemCount", { count: c().items.length })}
            </Text>
            <If when={c().frozen}>
              <Text style={{ fontSize: 12, lineHeight: 16 }} class="text-muted-foreground">
                {t("contests.finalized")}
              </Text>
            </If>
          </View>
        </View>
        <Lock closed={c().frozen} size={14} />
      </View>
    );
  };

  return (
    <View
      style={{ flex: 1, flexDirection: "row" }}
      class="bg-background"
      captureKeys={["up", "down", "enter"]}
      onKeyDown={(e) => {
        if (e.key === "ArrowDown") move(1);
        else if (e.key === "ArrowUp") move(-1);
        else if (e.key === "Enter") openSelected();
      }}
    >
      <View style={{ width: 280, borderRightWidth: 1 }} class="border-border">
        <Toolbar>
          <Text style={{ fontSize: 12.5, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
            {activeContests().length > 0 ? t("contests.count", { count: activeContests().length }) : ""}
          </Text>
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
          <View style={{ padding: 8, gap: 2 }}>
            <Each of={activeContests()} key={(c) => c.id}>
              {(c) => renderContestRow(c, false)}
            </Each>
          </View>
        </Scroll>
        <If when={listError()}>
          <View style={{ padding: 8, borderTopWidth: 1 }} class="border-border">
            <ErrorDisplay error={listError()} onDismiss={() => listError.set(undefined)} />
          </View>
        </If>
        <If when={archivedContests().length > 0}>
          <View style={{ borderTopWidth: 1, flexShrink: 0 }} class="border-border bg-background">
            <View
              testID="archived-contests-toggle"
              onPress={() => archivedExpanded.set(!archivedExpanded())}
              class="hover:bg-muted active:bg-accent"
              style={{
                height: 36,
                paddingLeft: 12,
                paddingRight: 12,
                flexDirection: "row",
                alignItems: "center",
                gap: 6,
                transitionProperty: "colors",
                transitionDuration: ms(120),
                transitionTimingFunction: EASE_STANDARD,
              }}
            >
              <Icon
                name="chevronDown"
                size={13}
                tone="muted-foreground"
                style={{
                  transform: archivedExpanded() ? "rotate(0deg)" : "rotate(-90deg)",
                  transitionProperty: "transform",
                  transitionDuration: ms(120),
                  transitionTimingFunction: EASE_STANDARD,
                }}
              />
              <Text style={{ fontSize: 12, fontWeight: 500 }} class="text-muted-foreground">
                {t("contests.archived")}
              </Text>
              <Text style={{ fontSize: 12, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
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
                <View style={{ paddingLeft: 8, paddingRight: 8, paddingTop: 2, paddingBottom: 8, gap: 2 }}>
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
          <ContestPanel id={selectedContest()!} />
        </If>
      </If>
    </View>
  );
}

function ContestPanel(props: { id: string }) {
  const detail = computed(() => {
    void revision();
    return preview.contestDetail(props.id);
  });
  const f = createFreeze();
  const actionError = signal<UiError | undefined>(undefined);
  const open = (label?: string) => openStudio({ target: "contest", id: props.id, label });
  const readOnly = () => info().readOnly;
  const viewOnly = () => detail().frozen || detail().archived || readOnly();

  // Clear stale freeze/action errors whenever switching contests or workspaces.
  effect(() => {
    void props.id;
    void workspaceVersion();
    f.reset();
    actionError.set(undefined);
  });

  const reveal = async () => {
    const res = await preview.revealInFolder(`contests/${props.id}`);
    actionError.set(res.ok ? undefined : errorOf(res));
  };

  return (
    <View style={{ flex: 1, minWidth: 0 }}>
      <DetailHeader
        size="lg"
        title={detail().name}
        badge={
          detail().frozen || detail().archived ? (
            <View style={{ flexDirection: "row", alignItems: "center", gap: 4 }}>
              <If when={detail().frozen}>
                <View style={{ height: 22, paddingLeft: 6, paddingRight: 7, borderRadius: 6, flexDirection: "row", alignItems: "center", gap: 4 }} class="bg-muted">
                  <Icon name="lock" size={12} />
                  <Text style={{ fontSize: 11.5 }} class="text-secondary-foreground">
                    {t("contests.finalized")}
                  </Text>
                </View>
              </If>
              <If when={detail().archived}>
                <View style={{ height: 22, paddingLeft: 7, paddingRight: 7, borderRadius: 6, flexDirection: "row", alignItems: "center" }} class="bg-muted">
                  <Text style={{ fontSize: 11.5 }} class="text-secondary-foreground">
                    {t("contests.archived")}
                  </Text>
                </View>
              </If>
            </View>
          ) : undefined
        }
        second={
          <View style={{ flexDirection: "row", alignItems: "center", gap: 16 }}>
            <Each of={[detail().language === "english" ? "English" : "中文", detail().date ?? "", detail().location ?? ""].filter(Boolean)} key="index">
              {(t) => (
                <Text style={{ fontSize: 12.5, fontVariant: ["tabular-nums"] }} class="text-secondary-foreground">
                  {t()}
                </Text>
              )}
            </Each>
          </View>
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
                        <If when={f.busyVisible()} else={<Icon name="stamp" size={15} tone="foreground" />}>
                          <DelayedSpinner active={f.busy()} size={13} />
                        </If>
                      </View>
                      <Text class="text-foreground" style={{ fontSize: 13 }}>
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
        <View style={{ paddingLeft: GUTTER, paddingRight: GUTTER, paddingBottom: 12 }}>
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
                <Text style={{ fontSize: 12, fontVariant: ["tabular-nums"] }} class="text-secondary-foreground">
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
                <View style={{ flexDirection: "row", alignItems: "center", gap: 8 }}>
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
        <Scroll style={{ flex: 1 }}>
          <View style={{ paddingLeft: GUTTER, paddingRight: GUTTER, paddingBottom: 24, gap: 20 }}>
            <Section
              title={t("contests.section.problems")}
              trailing={
                <View style={{ flexDirection: "row", alignItems: "center", gap: 10 }}>
                  <Text style={{ fontSize: 12, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
                    {t("contests.problemCount", { count: detail().items.length })}
                  </Text>
                  <If when={!viewOnly()}>
                    <Btn
                      label={t("contests.pickProblemsShort")}
                      icon="listPlus"
                      size="sm"
                      onPress={() => dialog.set({ kind: "pickProblems", contestId: props.id })}
                      testID="contest-pick-problems"
                    />
                  </If>
                </View>
              }
            >
              <View style={{ height: 32, paddingLeft: 14, paddingRight: 14, flexDirection: "row", alignItems: "center", gap: 12, borderBottomWidth: 1 }} class="border-border">
                <Text style={{ width: 28, fontSize: 12 }} class="text-muted-foreground">{t("contests.col.label")}</Text>
                <Text style={{ flex: 1, fontSize: 12 }} class="text-muted-foreground">{t("contests.col.title")}</Text>
                <Text style={{ width: 110, fontSize: 12, textAlign: "right" }} class="text-muted-foreground">{t("contests.col.limits")}</Text>
                <Text style={{ width: 44, fontSize: 12, textAlign: "right" }} class="text-muted-foreground">{t("contests.col.rating")}</Text>
              </View>
              <Each of={detail().items} key={(i) => i.label}>
                {(item, n) => (
                  <View
                    onPress={item().missing ? undefined : () => open(item().label)}
                    tooltip={item().missing ? tip(t("contests.missingProblemTooltip")) : undefined}
                    class={item().missing ? "border-border" : "hover:bg-muted active:bg-accent border-border"}
                    style={{
                      height: 40,
                      paddingLeft: 14,
                      paddingRight: 14,
                      flexDirection: "row",
                      alignItems: "center",
                      gap: 12,
                      borderTopWidth: n() === 0 ? 0 : 1,
                      opacity: item().missing ? 0.65 : 1,
                      transitionProperty: "colors",
                      transitionDuration: ms(120),
                      transitionTimingFunction: EASE_STANDARD,
                    }}
                  >
                    <Text style={{ width: 28, fontSize: 13.5, fontWeight: 600, fontVariant: ["tabular-nums"] }} class="text-foreground">{item().label}</Text>
                    <Text style={{ numberOfLines: 1, textOverflow: "ellipsis", flex: 1, fontSize: 13.5 }} class={item().missing ? "text-destructive" : "text-foreground"}>
                      {item().missing ? t("contests.missingSuffix", { title: item().title }) : item().title}
                    </Text>
                    <If when={item().packageStatus && item().packageStatus !== "verified_fresh"}>
                      <Text
                        style={{ fontSize: 11, flexShrink: 0 }}
                        class={item().packageStatus === "failed" ? "text-destructive" : "text-muted-foreground"}
                      >
                        {item().packageStatus === "failed"
                          ? t("library.package.failed")
                          : item().packageStatus === "stale"
                            ? t("library.package.stale")
                            : t("library.package.unverified")}
                      </Text>
                    </If>
                    <Text style={{ width: 110, fontSize: 12.5, textAlign: "right", fontVariant: ["tabular-nums"] }} class="text-secondary-foreground">
                      {[item().limits.time, item().limits.memory].filter(Boolean).join(" / ")}
                    </Text>
                    <View style={{ width: 44, alignItems: "flex-end" }}>
                      <RatingBadge rating={item().rating} />
                    </View>
                  </View>
                )}
              </Each>
            </Section>
            <Section title={t("contests.section.overview")}>
              <View style={{ padding: 14 }}>
                <ContestStats id={props.id} />
              </View>
            </Section>
          </View>
        </Scroll>
      </If>
    </View>
  );
}

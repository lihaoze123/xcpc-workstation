// Cmd+2: the local contests (composition, finalized copies, and collapsible archived list).
// The list on the left (active contests + collapsible 已归档 section, 新建 at its toolbar's end);
// on the right the selected one's head (language, date, place; 编辑/查看, 定稿 / 重新编辑 / 取消归档, ⋯),
// its overview (the difficulty bar and tags) and its problems.

import { computed, effect, keymap, Scroll, signal, Text, View, type ScrollRef } from "nsh";
import { Menu as DesktopMenu } from "nsh/desktop";
import type { WorkspaceContest } from "xcpc-workstation/native";
import { Btn, DelayedSpinner, Lock, RatingBadge, tip } from "./kit";
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

const FREEZE_TIP = "存一份定稿：之后题库里的修改不再影响这场比赛";
const ROW_STRIDE = 54; // 52pt row + 2pt gap

function toggleContestArchive(
  id: string,
  archive: boolean,
  onError?: (err: UiError) => void,
) {
  const before = info().contests;
  const res = preview.setContestArchived(id, archive);
  if (!res.ok) {
    onError?.(errorOf(res, archive ? "归档比赛" : "取消归档")!);
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
  let listScroll: ScrollRef | undefined;
  let listHeight = 0;
  let listScrollY = 0;

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

  function ensureVisible(index: number) {
    if (!listScroll || listHeight <= 0) return;
    const rowTop = 8 + index * ROW_STRIDE;
    const rowBottom = rowTop + 52;
    if (rowTop < listScrollY + 8) {
      listScroll.scrollTo({ y: Math.max(0, rowTop - 8), animated: false });
    } else if (rowBottom > listScrollY + listHeight - 8) {
      listScroll.scrollTo({ y: Math.max(0, rowBottom - listHeight + 8), animated: false });
    }
  }

  function move(delta: number) {
    const list = visibleContests();
    if (list.length === 0) return;
    const cur = list.findIndex((c) => c.id === selectedContest());
    const i = cur < 0 ? 0 : Math.min(list.length - 1, Math.max(0, cur + delta));
    selectedContest.set(list[i].id);
    ensureVisible(i);
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
              { label: locked ? "查看" : "编辑", onSelect: () => openStudio({ target: "contest", id: cur.id }) },
              {
                label: "从题库挑题…",
                disabled: locked,
                onSelect: () => dialog.set({ kind: "pickProblems", contestId: cur.id }),
              },
              {
                label: "新建题目加入…",
                disabled: locked,
                onSelect: () => dialog.set({ kind: "newProblem", addTo: cur.id }),
              },
              { type: "separator" },
              {
                label: cur.archived ? "取消归档" : "归档比赛",
                disabled: ro,
                onSelect: () => toggleContestArchive(cur.id, !cur.archived),
              },
              { label: "在文件夹中显示", onSelect: () => void preview.revealInFolder(`contests/${cur.id}`) },
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
              {`${c().items.length} 题`}
            </Text>
            <If when={c().frozen}>
              <Text style={{ fontSize: 12, lineHeight: 16 }} class="text-muted-foreground">
                已定稿
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
            {activeContests().length > 0 ? `${activeContests().length} 场比赛` : ""}
          </Text>
          <Spacer />
          <Btn
            label="新建比赛"
            icon="plus"
            disabled={readOnly()}
            tooltip={tip(readOnly() ? "只读工作区无法新建" : "新建比赛", readOnly() ? undefined : "mod+shift+n")}
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

            <If when={archivedContests().length > 0}>
              <View style={{ marginTop: activeContests().length > 0 ? 10 : 0, gap: 2 }}>
                <View
                  testID="archived-contests-toggle"
                  onPress={() => archivedExpanded.set(!archivedExpanded())}
                  class="hover:bg-muted active:bg-accent"
                  style={{
                    height: 30,
                    paddingLeft: 8,
                    paddingRight: 8,
                    borderRadius: 6,
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
                    已归档
                  </Text>
                  <Text style={{ fontSize: 12, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
                    {String(archivedContests().length)}
                  </Text>
                </View>
                <If when={archivedExpanded()}>
                  <View style={{ gap: 2 }}>
                    <Each of={archivedContests()} key={(c) => c.id}>
                      {(c) => renderContestRow(c, true)}
                    </Each>
                  </View>
                </If>
              </View>
            </If>
          </View>
        </Scroll>
      </View>
      <If
        when={contests().length > 0}
        else={
          <EmptyState
            icon="trophy"
            title="还没有比赛"
            hint={readOnly() ? "工作区以只读方式打开" : "新建一场比赛，再从题库里挑题加进来"}
            action={
              readOnly() ? undefined : (
                <Btn label="新建比赛" icon="plus" variant="primary" onPress={() => dialog.set({ kind: "newContest" })} tooltip={tip("新建比赛", "mod+shift+n")} />
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
              title="没有进行中的比赛"
              hint="已归档的比赛收纳在左侧底部，也可以新建一场比赛"
              action={
                readOnly() ? undefined : (
                  <Btn label="新建比赛" icon="plus" variant="primary" onPress={() => dialog.set({ kind: "newContest" })} tooltip={tip("新建比赛", "mod+shift+n")} />
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
            <View style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>
              <If when={detail().frozen}>
                <View style={{ height: 22, paddingLeft: 7, paddingRight: 8, borderRadius: 6, flexDirection: "row", alignItems: "center", gap: 4 }} class="bg-muted">
                  <Icon name="lock" size={12} />
                  <Text style={{ fontSize: 12 }} class="text-secondary-foreground">
                    已定稿
                  </Text>
                </View>
              </If>
              <If when={detail().archived}>
                <View style={{ height: 22, paddingLeft: 8, paddingRight: 8, borderRadius: 6, flexDirection: "row", alignItems: "center" }} class="bg-muted">
                  <Text style={{ fontSize: 12 }} class="text-secondary-foreground">
                    已归档
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
              label={viewOnly() ? "查看" : "编辑"}
              tooltip={tip(
                detail().archived
                  ? "查看已归档比赛的题面和预览"
                  : detail().frozen
                    ? "查看已定稿的题面和预览"
                    : readOnly()
                      ? "以只读方式查看比赛"
                      : "编辑题面、顺序和封面",
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
                      tooltip={tip(readOnly() ? "只读工作区无法定稿" : FREEZE_TIP)}
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
                        定稿
                      </Text>
                    </Btn>
                  }
                >
                  <Btn
                    icon="lockOpen"
                    label="重新编辑"
                    disabled={readOnly()}
                    tooltip={tip(readOnly() ? "只读工作区无法重新编辑" : "移除定稿副本，重新跟随题库")}
                    onPress={() => unfreezeFor.set(props.id)}
                  />
                </If>
              }
            >
              <Btn
                icon="rotateCcw"
                label="取消归档"
                disabled={readOnly()}
                tooltip={tip(readOnly() ? "只读工作区无法取消归档" : "恢复到进行中比赛列表")}
                onPress={() => toggleContestArchive(props.id, false, (e) => actionError.set(e))}
                testID="contest-unarchive"
              />
            </If>
            <AppMenu
              align="end"
              entries={[
                { id: "pickProblems", label: "从题库挑题…", icon: "listPlus", disabled: viewOnly() },
                { id: "newProblem", label: "新建题目加入…", icon: "plus", disabled: viewOnly() },
                { separator: true, id: "sep" },
                {
                  id: "archive",
                  label: detail().archived ? "取消归档" : "归档比赛",
                  icon: detail().archived ? "rotateCcw" : "folder",
                  disabled: readOnly(),
                },
                { id: "reveal", label: "在文件夹中显示", icon: "folderOpen" },
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
                    label: d.file ? `${d.file}${d.line ? `:${d.line}` : ""}` : d.message,
                    onPress: () => open(detail().items.find((i) => d.file?.startsWith(`problems/${i.id}/`))?.label),
                  }))
                : undefined
            }
            action={
              f.error() && f.findings().length > 0 ? (
                <Text style={{ fontSize: 12, fontVariant: ["tabular-nums"] }} class="text-secondary-foreground">
                  {f.findings().length > 4
                    ? `${freezeBlockedCopy(f.findings().length)}（另 ${f.findings().length - 4} 项）`
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
            title="这场比赛还没有题目"
            hint={
              detail().archived
                ? "比赛已归档，如需加题请先取消归档"
                : readOnly()
                  ? "工作区以只读方式打开"
                  : "从题库里挑题加进来，或者直接新建一道题"
            }
            action={
              !viewOnly() ? (
                <View style={{ flexDirection: "row", alignItems: "center", gap: 8 }}>
                  <Btn
                    label="从题库挑题"
                    icon="listPlus"
                    variant="primary"
                    onPress={() => dialog.set({ kind: "pickProblems", contestId: props.id })}
                  />
                  <Btn
                    label="新建题目加入"
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
              title="题目"
              trailing={
                <View style={{ flexDirection: "row", alignItems: "center", gap: 10 }}>
                  <Text style={{ fontSize: 12, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
                    {`${detail().items.length} 题`}
                  </Text>
                  <If when={!viewOnly()}>
                    <Btn
                      label="挑题"
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
                <Text style={{ width: 28, fontSize: 12 }} class="text-muted-foreground">题号</Text>
                <Text style={{ flex: 1, fontSize: 12 }} class="text-muted-foreground">标题</Text>
                <Text style={{ width: 110, fontSize: 12, textAlign: "right" }} class="text-muted-foreground">时间 / 内存</Text>
                <Text style={{ width: 44, fontSize: 12, textAlign: "right" }} class="text-muted-foreground">难度</Text>
              </View>
              <Each of={detail().items} key={(i) => i.label}>
                {(item, n) => (
                  <View
                    onPress={item().missing ? undefined : () => open(item().label)}
                    tooltip={item().missing ? tip("题库中找不到这道题，请先恢复或从比赛中移除") : undefined}
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
                    <Text style={{ width: 28, fontSize: 13.5, fontWeight: 600, fontVariant: ["tabular-nums"] }}>{item().label}</Text>
                    <Text style={{ numberOfLines: 1, textOverflow: "ellipsis", flex: 1, fontSize: 13.5 }} class={item().missing ? "text-destructive" : ""}>
                      {item().missing ? `${item().title}（缺失）` : item().title}
                    </Text>
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
            <Section title="概况">
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

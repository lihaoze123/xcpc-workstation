// Training space View 2: Aggregated OJ Contest Calendar.
// Groups contests into Today / This Week / Later (/ Finished), localizes relative times & durations,
// hides platform filter when only 1 platform has contests, and makes entire rows clickable with
// a subtle external-link icon on hover (no repetitive outlined button wall).

import { cleanup, computed, effect, Scroll, signal, Text, View } from "nsh";
import { Switch } from "nsh/base";
import { Preview } from "xcpc-workstation/native";
import { t } from "../i18n";
import { Icon } from "../Icon";
import { Chip, FOCUS_RING, tip, TruncText } from "../kit";
import { EmptyState, FilterEmpty } from "../layout";
import { colorTransition } from "../tokens";
import { cardShadow } from "../theme";
import { showToast } from "../Toast";
import { isEn, zhEn } from "./locale";
import {
  formatContestStartBrief,
  formatCountdown,
  formatDuration,
  formatUnixDateTime,
  groupContestsByTime,
  platformDisplayName,
  type ContestCalendarItem,
  type ContestTimeGroup,
} from "./types";

import { CONTROL, FONT_SIZE, FONT_WEIGHT, RADIUS, SPACE } from "../tokens";
function groupLabel(group: ContestTimeGroup): string {
  switch (group) {
    case "today":
      return zhEn("今天", "Today");
    case "thisWeek":
      return zhEn("本周", "This Week");
    case "later":
      return zhEn("以后", "Later");
    case "finished":
      return zhEn("已结束", "Finished");
  }
}

export function ContestsTab(props: {
  contests: readonly ContestCalendarItem[];
  platforms: readonly string[];
  platformFilter: string;
  includeFinished: boolean;
  onPlatformChange: (p: string) => void;
  onIncludeFinishedChange: (v: boolean) => void;
}) {
  const nowSec = signal(Math.floor(Date.now() / 1000));
  const hoveredKey = signal<string | undefined>(undefined);
  const timer = setInterval(() => nowSec.set(Math.floor(Date.now() / 1000)), 30_000);
  cleanup(() => clearInterval(timer));

  const seenPlatforms = signal<string[]>([]);
  effect(() => {
    const cur = seenPlatforms();
    const next = new Set<string>(cur);
    for (const p of props.platforms) {
      if (p) next.add(p);
    }
    for (const c of props.contests) {
      if (c.platform) next.add(c.platform);
    }
    if (props.platformFilter) next.add(props.platformFilter);
    if (next.size !== cur.length) {
      seenPlatforms.set([...next].sort());
    }
  });

  const platformChoices = computed(() => {
    const set = new Set<string>(seenPlatforms());
    for (const p of props.platforms) {
      if (p) set.add(p);
    }
    for (const c of props.contests) {
      if (c.platform) set.add(c.platform);
    }
    if (props.platformFilter) set.add(props.platformFilter);
    return [...set].sort();
  });

  const showPlatformFilter = computed(() => platformChoices().length > 1 || props.platformFilter !== "");
  const grouped = computed(() => groupContestsByTime(props.contests, nowSec()));

  const openContestUrl = (url: string | null | undefined) => {
    if (!url) return;
    const res = Preview.openExternalUrl(url);
    if (!res.ok && res.error) {
      showToast({ tone: "issues", text: res.error });
    }
  };

  const statusBadge = (item: ContestCalendarItem) => {
    const now = nowSec();
    const en = isEn();
    if (item.status === "running") {
      const rem = formatCountdown(item.end_time, now, en);
      return (
        <View
          style={{ height: 20, paddingLeft: SPACE[8], paddingRight: SPACE[8], borderRadius: RADIUS.xs, justifyContent: "center" }} // token-exempt: compact countdown badge height
          class="bg-chart-2/15"
        >
          <Text style={{ fontSize: FONT_SIZE.micro, fontWeight: FONT_WEIGHT.semibold, fontVariant: ["tabular-nums"] }} class="text-chart-2">
            {en ? `Running  ${rem} left` : `进行中  剩 ${rem}`}
          </Text>
        </View>
      );
    }
    if (item.status === "upcoming") {
      const rel = formatCountdown(item.start_time, now, en);
      const brief = formatContestStartBrief(item.start_time, en);
      const label = en
        ? brief ? `in ${rel}  ${brief}` : `in ${rel}`
        : brief ? `${rel}后  ${brief}` : `${rel}后`;
      return (
        <View
          testID={`contest-countdown-${item.platform}-${item.contest_id}`}
          style={{ height: 20, paddingLeft: SPACE[8], paddingRight: SPACE[8], borderRadius: RADIUS.xs, justifyContent: "center", flexShrink: 0 }} // token-exempt: compact countdown badge height
          class="bg-muted"
        >
          <Text style={{ fontSize: FONT_SIZE.micro, fontWeight: FONT_WEIGHT.semibold, fontVariant: ["tabular-nums"] }} class="text-secondary-foreground">
            {label}
          </Text>
        </View>
      );
    }
    return <Chip label={t("training.contests.finished")} />;
  };

  return (
    <Scroll
      testID="training-contests-scroll"
      style={{ flex: 1, minHeight: 0 }}
      contentStyle={{ padding: SPACE[20], gap: SPACE[12], paddingBottom: SPACE[32] }}
    >
      {/* Filter bar */}
      <View
        style={{
          flexDirection: "row",
          alignItems: "center",
          justifyContent: showPlatformFilter() ? "space-between" : "flex-end",
          flexWrap: "wrap",
          gap: SPACE[8],
        }}
      >
        <If when={showPlatformFilter()}>
          <View testID="contests-platform-bar" style={{ flexDirection: "row", alignItems: "center", flexWrap: "wrap", gap: SPACE[6] }}>
            <View
              testID="contests-platform-all"
              onPress={() => props.onPlatformChange("")}
              class={props.platformFilter === "" ? `bg-accent border border-foreground/30 ${FOCUS_RING}` : `bg-card border border-border hover:bg-muted ${FOCUS_RING}`}
              style={{ height: CONTROL.sm, paddingLeft: SPACE[12], paddingRight: SPACE[12], borderRadius: RADIUS.sm, justifyContent: "center" }}
            >
              <Text
                style={{ fontSize: FONT_SIZE.caption, fontWeight: FONT_WEIGHT.semibold }}
                class={props.platformFilter === "" ? "text-foreground" : "text-secondary-foreground"}
              >
                {t("training.contests.allPlatforms")}
              </Text>
            </View>
            <Each of={platformChoices()} key={(p) => p}>
              {(plat) => {
                const active = () => props.platformFilter === plat();
                return (
                  <View
                    testID={`contests-platform-${plat()}`}
                    onPress={() => props.onPlatformChange(plat())}
                    class={active() ? `bg-accent border border-foreground/30 ${FOCUS_RING}` : `bg-card border border-border hover:bg-muted ${FOCUS_RING}`}
                    style={{ height: CONTROL.sm, paddingLeft: SPACE[12], paddingRight: SPACE[12], borderRadius: RADIUS.sm, justifyContent: "center" }}
                  >
                    <Text
                      style={{ fontSize: FONT_SIZE.caption, fontWeight: FONT_WEIGHT.semibold }}
                      class={active() ? "text-foreground" : "text-secondary-foreground"}
                    >
                      {platformDisplayName(plat())}
                    </Text>
                  </View>
                );
              }}
            </Each>
          </View>
        </If>

        <View
          style={{ flexDirection: "row", alignItems: "center", gap: SPACE[8] }}
        >
          <Text
            onPress={() => props.onIncludeFinishedChange(!props.includeFinished)}
            style={{ fontSize: FONT_SIZE.caption, borderRadius: RADIUS.xs }}
            class={`text-secondary-foreground ${FOCUS_RING}`}
          >
            {t("training.contests.includeFinished")}
          </Text>
          <Switch
            testID="contests-include-finished"
            checked={props.includeFinished}
            onChange={(e) => props.onIncludeFinishedChange(e.checked)}
            trackClass="w-8 h-[18px] rounded-full bg-input checked:bg-primary transition-colors duration-[120ms]"
            thumbClass="size-[14px] m-[2px] rounded-full bg-white shadow-sm checked:translate-x-[14px] transition-transform duration-[120ms]"
          />
        </View>
      </View>

      {/* Grouped contest list */}
      <If
        when={grouped().length > 0}
        else={
          <View testID="training-contests-empty" style={{ flex: 1 }}>
            <If
              when={props.platformFilter !== ""}
              else={
                <EmptyState
                  icon="trophy"
                  title={t("training.contests.emptyTitle")}
                  hint={t("training.contests.emptyDesc")}
                />
              }
            >
              <FilterEmpty
                message={t("training.contests.emptyTitle")}
                onClear={() => props.onPlatformChange("")}
              />
            </If>
          </View>
        }
      >
        <View testID="training-contests-list" style={{ gap: SPACE[16] }}>
          <Each of={grouped()} key={(g) => g.group}>
            {(section) => (
              <View testID={`contests-group-${section().group}`} style={{ gap: SPACE[8] }}>
                <Text
                  style={{ fontSize: FONT_SIZE.caption, fontWeight: FONT_WEIGHT.semibold, paddingLeft: SPACE[2] }}
                  class="text-muted-foreground"
                >
                  {groupLabel(section().group)}
                </Text>
                <View
                  class="bg-card border border-border"
                  style={{ borderRadius: RADIUS.lg, overflow: "hidden", boxShadow: cardShadow() }}
                >
                  <Each of={section().items} key={(c) => `${c.platform}:${c.contest_id}`}>
                    {(item, idx) => {
                      const rowKey = () => `${item().platform}:${item().contest_id}`;
                      const isHovered = () => hoveredKey() === rowKey();
                      const clickable = () => Boolean(item().url);
                      return (
                        <View>
                          <If when={idx() > 0}>
                            <View style={{ height: 1, marginLeft: SPACE[16], marginRight: SPACE[16] }} class="bg-border" />
                          </If>
                          <View
                            testID={`contest-row-${item().platform}-${item().contest_id}`}
                            onPress={clickable() ? () => openContestUrl(item().url) : undefined}
                            onMouseEnter={() => hoveredKey.set(rowKey())}
                            onMouseLeave={() => {
                              if (hoveredKey() === rowKey()) hoveredKey.set(undefined);
                            }}
                            tooltip={clickable() ? tip(t("training.contests.openPage")) : undefined}
                            class={clickable() ? `hover:bg-muted/60 active:bg-accent/60 ${FOCUS_RING}` : undefined}
                            style={{
                              paddingTop: SPACE[12],
                              paddingBottom: SPACE[12],
                              paddingLeft: SPACE[16],
                              paddingRight: SPACE[16],
                              flexDirection: "row",
                              alignItems: "center",
                              justifyContent: "space-between",
                              gap: SPACE[12],
                              ...colorTransition(),
                            }}
                          >
                            <View style={{ flex: 1, minWidth: 0, gap: SPACE[4] }}>
                              <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[8] }}>
                                <If when={showPlatformFilter()}>
                                  <View
                                    testID={`contest-platform-badge-${item().platform}-${item().contest_id}`}
                                    class="bg-muted"
                                    style={{
                                      width: 82,
                                      height: 20, // token-exempt: compact platform badge height
                                      borderRadius: RADIUS.xs,
                                      paddingLeft: SPACE[6],
                                      paddingRight: SPACE[6],
                                      alignItems: "center",
                                      justifyContent: "center",
                                      flexShrink: 0,
                                    }}
                                  >
                                    <TruncText
                                      style={{ fontSize: FONT_SIZE.caption, fontWeight: FONT_WEIGHT.semibold, numberOfLines: 1, textOverflow: "ellipsis" }}
                                      class="text-secondary-foreground"
                                    >
                                      {platformDisplayName(item().platform)}
                                    </TruncText>
                                  </View>
                                </If>
                                <TruncText
                                  style={{
                                    fontSize: FONT_SIZE.body,
                                    fontWeight: FONT_WEIGHT.semibold,
                                    flexShrink: 1,
                                    minWidth: 0,
                                    numberOfLines: 1,
                                    textOverflow: "ellipsis",
                                  }}
                                  class="text-foreground"
                                >
                                  {item().name}
                                </TruncText>
                              </View>
                              <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[12] }}>
                                <Text style={{ fontSize: FONT_SIZE.caption, fontVariant: ["tabular-nums"] }} class="text-secondary-foreground">
                                  {formatUnixDateTime(item().start_time)}
                                </Text>
                                <Text style={{ fontSize: FONT_SIZE.caption, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
                                  {t("training.contests.duration", { duration: formatDuration(item().duration_sec, isEn()) })}
                                </Text>
                              </View>
                            </View>

                            <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[8], flexShrink: 0 }}>
                              {statusBadge(item())}
                              {/* Subtle hover external-link icon (fixed slot, zero layout shift) */}
                              <View
                                style={{
                                  width: 24,
                                  height: CONTROL.sm,
                                  alignItems: "center",
                                  justifyContent: "center",
                                  flexShrink: 0,
                                  opacity: clickable() && isHovered() ? 1 : 0,
                                }}
                              >
                                <Icon name="link" size="sm" tone="muted-foreground" />
                              </View>
                            </View>
                          </View>
                        </View>
                      );
                    }}
                  </Each>
                </View>
              </View>
            )}
          </Each>
        </View>
      </If>
    </Scroll>
  );
}

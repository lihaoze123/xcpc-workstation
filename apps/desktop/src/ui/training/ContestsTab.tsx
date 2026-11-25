// Training space View 2: Aggregated OJ Contest Calendar.
// Groups contests into Today / This Week / Later (/ Finished), localizes relative times & durations,
// hides platform filter when only 1 platform has contests, and makes entire rows clickable with
// a subtle external-link icon on hover (no repetitive outlined button wall).

import { cleanup, computed, effect, Scroll, signal, Text, View } from "nsh";
import { Switch } from "nsh/base";
import { Preview } from "xcpc-workstation/native";
import { t } from "../i18n";
import { Icon } from "../Icon";
import { Chip, tip } from "../kit";
import { colorTransition } from "../motion";
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
          style={{ height: 20, paddingLeft: 7, paddingRight: 7, borderRadius: 5, justifyContent: "center" }}
          class="bg-chart-2/15"
        >
          <Text style={{ fontSize: 11, fontWeight: 600, fontVariant: ["tabular-nums"] }} class="text-chart-2">
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
          style={{ height: 20, paddingLeft: 7, paddingRight: 7, borderRadius: 5, justifyContent: "center", flexShrink: 0 }}
          class="bg-muted"
        >
          <Text style={{ fontSize: 11, fontWeight: 500, fontVariant: ["tabular-nums"] }} class="text-secondary-foreground">
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
      contentStyle={{ padding: 20, gap: 14, paddingBottom: 36 }}
    >
      {/* Filter bar */}
      <View
        style={{
          flexDirection: "row",
          alignItems: "center",
          justifyContent: showPlatformFilter() ? "space-between" : "flex-end",
          flexWrap: "wrap",
          gap: 10,
        }}
      >
        <If when={showPlatformFilter()}>
          <View testID="contests-platform-bar" style={{ flexDirection: "row", alignItems: "center", flexWrap: "wrap", gap: 6 }}>
            <View
              testID="contests-platform-all"
              onPress={() => props.onPlatformChange("")}
              class={props.platformFilter === "" ? "bg-accent border border-foreground/30" : "bg-card border border-border hover:bg-muted"}
              style={{ height: 26, paddingLeft: 10, paddingRight: 10, borderRadius: 6, justifyContent: "center" }}
            >
              <Text
                style={{ fontSize: 12, fontWeight: 500 }}
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
                    class={active() ? "bg-accent border border-foreground/30" : "bg-card border border-border hover:bg-muted"}
                    style={{ height: 26, paddingLeft: 10, paddingRight: 10, borderRadius: 6, justifyContent: "center" }}
                  >
                    <Text
                      style={{ fontSize: 12, fontWeight: 500 }}
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
          style={{ flexDirection: "row", alignItems: "center", gap: 8 }}
        >
          <Text
            onPress={() => props.onIncludeFinishedChange(!props.includeFinished)}
            style={{ fontSize: 12 }}
            class="text-secondary-foreground"
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
          <View
            testID="training-contests-empty"
            class="bg-card border border-border"
            style={{
              borderRadius: 10,
              padding: 28,
              alignItems: "center",
              justifyContent: "center",
              gap: 6,
              boxShadow: cardShadow(),
            }}
          >
            <Text style={{ fontSize: 13.5, fontWeight: 500 }} class="text-foreground">
              {t("training.contests.emptyTitle")}
            </Text>
            <Text style={{ fontSize: 12 }} class="text-muted-foreground">
              {t("training.contests.emptyDesc")}
            </Text>
          </View>
        }
      >
        <View testID="training-contests-list" style={{ gap: 16 }}>
          <Each of={grouped()} key={(g) => g.group}>
            {(section) => (
              <View testID={`contests-group-${section().group}`} style={{ gap: 8 }}>
                <Text
                  style={{ fontSize: 12, fontWeight: 600, paddingLeft: 2 }}
                  class="text-muted-foreground"
                >
                  {groupLabel(section().group)}
                </Text>
                <View
                  class="bg-card border border-border"
                  style={{ borderRadius: 10, overflow: "hidden", boxShadow: cardShadow() }}
                >
                  <Each of={section().items} key={(c) => `${c.platform}:${c.contest_id}`}>
                    {(item, idx) => {
                      const rowKey = () => `${item().platform}:${item().contest_id}`;
                      const isHovered = () => hoveredKey() === rowKey();
                      const clickable = () => Boolean(item().url);
                      return (
                        <View>
                          <If when={idx() > 0}>
                            <View style={{ height: 1, marginLeft: 16, marginRight: 16 }} class="bg-border" />
                          </If>
                          <View
                            testID={`contest-row-${item().platform}-${item().contest_id}`}
                            onPress={clickable() ? () => openContestUrl(item().url) : undefined}
                            onMouseEnter={() => hoveredKey.set(rowKey())}
                            onMouseLeave={() => {
                              if (hoveredKey() === rowKey()) hoveredKey.set(undefined);
                            }}
                            tooltip={clickable() ? tip(t("training.contests.openPage")) : undefined}
                            class={clickable() ? "hover:bg-muted/60 active:bg-accent/60" : undefined}
                            style={{
                              paddingTop: 12,
                              paddingBottom: 12,
                              paddingLeft: 16,
                              paddingRight: 16,
                              flexDirection: "row",
                              alignItems: "center",
                              justifyContent: "space-between",
                              gap: 14,
                              ...colorTransition(120),
                            }}
                          >
                            <View style={{ flex: 1, minWidth: 0, gap: 4 }}>
                              <View style={{ flexDirection: "row", alignItems: "center", gap: 10 }}>
                                <If when={showPlatformFilter()}>
                                  <View
                                    testID={`contest-platform-badge-${item().platform}-${item().contest_id}`}
                                    class="bg-muted"
                                    style={{
                                      width: 82,
                                      height: 20,
                                      borderRadius: 5,
                                      paddingLeft: 6,
                                      paddingRight: 6,
                                      alignItems: "center",
                                      justifyContent: "center",
                                      flexShrink: 0,
                                    }}
                                  >
                                    <Text
                                      style={{ fontSize: 11.5, fontWeight: 500, numberOfLines: 1, textOverflow: "ellipsis" }}
                                      class="text-secondary-foreground"
                                    >
                                      {platformDisplayName(item().platform)}
                                    </Text>
                                  </View>
                                </If>
                                <Text
                                  style={{
                                    fontSize: 13.5,
                                    fontWeight: 500,
                                    flexShrink: 1,
                                    minWidth: 0,
                                    numberOfLines: 1,
                                    textOverflow: "ellipsis",
                                  }}
                                  class="text-foreground"
                                >
                                  {item().name}
                                </Text>
                              </View>
                              <View style={{ flexDirection: "row", alignItems: "center", gap: 14 }}>
                                <Text style={{ fontSize: 12, fontVariant: ["tabular-nums"] }} class="text-secondary-foreground">
                                  {formatUnixDateTime(item().start_time)}
                                </Text>
                                <Text style={{ fontSize: 12, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
                                  {t("training.contests.duration", { duration: formatDuration(item().duration_sec, isEn()) })}
                                </Text>
                              </View>
                            </View>

                            <View style={{ flexDirection: "row", alignItems: "center", gap: 8, flexShrink: 0 }}>
                              {statusBadge(item())}
                              {/* Subtle hover external-link icon (fixed slot, zero layout shift) */}
                              <View
                                style={{
                                  width: 24,
                                  height: 24,
                                  alignItems: "center",
                                  justifyContent: "center",
                                  flexShrink: 0,
                                  opacity: clickable() && isHovered() ? 1 : 0,
                                }}
                              >
                                <Icon name="link" size={14} tone="muted-foreground" />
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

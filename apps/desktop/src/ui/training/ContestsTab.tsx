// Training space View 2: Aggregated OJ Contest Calendar.
// Platform filter + upcoming/running (optional finished) contest list with local time,
// duration, live countdown, and one-click external browser open.

import { cleanup, computed, Scroll, signal, Text, View } from "nsh";
import { Switch } from "nsh/base";
import { Preview } from "xcpc-workstation/native";
import { t } from "../i18n";
import { Btn, Chip } from "../kit";
import { cardShadow } from "../theme";
import { showToast } from "../Toast";
import {
  formatCountdown,
  formatDuration,
  formatUnixDateTime,
  platformDisplayName,
  type ContestCalendarItem,
} from "./types";

export function ContestsTab(props: {
  contests: readonly ContestCalendarItem[];
  platforms: readonly string[];
  platformFilter: string;
  includeFinished: boolean;
  onPlatformChange: (p: string) => void;
  onIncludeFinishedChange: (v: boolean) => void;
}) {
  const nowSec = signal(Math.floor(Date.now() / 1000));
  const timer = setInterval(() => nowSec.set(Math.floor(Date.now() / 1000)), 30_000);
  cleanup(() => clearInterval(timer));

  const platformChoices = computed(() => {
    const set = new Set<string>(props.platforms);
    for (const c of props.contests) set.add(c.platform);
    return [...set].sort();
  });

  const openContestUrl = (url: string | null | undefined) => {
    if (!url) return;
    const res = Preview.openExternalUrl(url);
    if (!res.ok && res.error) {
      showToast({ tone: "issues", text: res.error });
    }
  };

  const statusBadge = (item: ContestCalendarItem) => {
    const now = nowSec();
    if (item.status === "running") {
      return (
        <View
          style={{ height: 20, paddingLeft: 7, paddingRight: 7, borderRadius: 5, justifyContent: "center" }}
          class="bg-chart-2/15"
        >
          <Text style={{ fontSize: 11, fontWeight: 600, fontVariant: ["tabular-nums"] }} class="text-chart-2">
            {t("training.contests.runningLeft", { time: formatCountdown(item.end_time, now) })}
          </Text>
        </View>
      );
    }
    if (item.status === "upcoming") {
      return (
        <View
          style={{ height: 20, paddingLeft: 7, paddingRight: 7, borderRadius: 5, justifyContent: "center" }}
          class="bg-primary/12"
        >
          <Text style={{ fontSize: 11, fontWeight: 500, fontVariant: ["tabular-nums"] }} class="text-primary">
            {t("training.contests.startsIn", { time: formatCountdown(item.start_time, now) })}
          </Text>
        </View>
      );
    }
    return <Chip label={t("training.contests.finished")} />;
  };

  return (
    <Scroll style={{ flex: 1 }} contentStyle={{ padding: 20, gap: 14, paddingBottom: 32 }}>
      {/* Filter bar */}
      <View style={{ flexDirection: "row", alignItems: "center", justifyContent: "space-between", flexWrap: "wrap", gap: 10 }}>
        <View style={{ flexDirection: "row", alignItems: "center", flexWrap: "wrap", gap: 6 }}>
          <View
            testID="contests-platform-all"
            onPress={() => props.onPlatformChange("")}
            class={props.platformFilter === "" ? "bg-accent border border-border" : "bg-card border border-border hover:bg-muted"}
            style={{ height: 26, paddingLeft: 10, paddingRight: 10, borderRadius: 6, justifyContent: "center" }}
          >
            <Text
              style={{ fontSize: 12, fontWeight: props.platformFilter === "" ? 600 : 400 }}
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
                  class={active() ? "bg-accent border border-border" : "bg-card border border-border hover:bg-muted"}
                  style={{ height: 26, paddingLeft: 10, paddingRight: 10, borderRadius: 6, justifyContent: "center" }}
                >
                  <Text
                    style={{ fontSize: 12, fontWeight: active() ? 600 : 400 }}
                    class={active() ? "text-foreground" : "text-secondary-foreground"}
                  >
                    {platformDisplayName(plat())}
                  </Text>
                </View>
              );
            }}
          </Each>
        </View>

        <View style={{ flexDirection: "row", alignItems: "center", gap: 8 }}>
          <Text style={{ fontSize: 12 }} class="text-secondary-foreground">
            {t("training.contests.includeFinished")}
          </Text>
          <Switch
            testID="contests-include-finished"
            checked={props.includeFinished}
            onChange={(e) => props.onIncludeFinishedChange(e.checked)}
          />
        </View>
      </View>

      {/* Contest list */}
      <If
        when={props.contests.length > 0}
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
        <View
          testID="training-contests-list"
          class="bg-card border border-border"
          style={{ borderRadius: 10, overflow: "hidden", boxShadow: cardShadow() }}
        >
          <Each of={props.contests} key={(c) => `${c.platform}:${c.contest_id}`}>
            {(item, idx) => (
              <View>
                <If when={idx() > 0}>
                  <View style={{ height: 1, marginLeft: 16, marginRight: 16 }} class="bg-border" />
                </If>
                <View
                  testID={`contest-row-${item().platform}-${item().contest_id}`}
                  style={{
                    paddingTop: 12,
                    paddingBottom: 12,
                    paddingLeft: 16,
                    paddingRight: 16,
                    flexDirection: "row",
                    alignItems: "center",
                    justifyContent: "space-between",
                    gap: 14,
                  }}
                >
                  <View style={{ flex: 1, minWidth: 0, gap: 4 }}>
                    <View style={{ flexDirection: "row", alignItems: "center", gap: 8 }}>
                      <Chip label={platformDisplayName(item().platform)} />
                      {statusBadge(item())}
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
                        {t("training.contests.duration", { duration: formatDuration(item().duration_sec) })}
                      </Text>
                    </View>
                  </View>

                  <If when={item().url}>
                    <Btn
                      testID={`contest-open-${item().contest_id}`}
                      label={t("training.contests.openPage")}
                      icon="link"
                      variant="outline"
                      onPress={() => openContestUrl(item().url)}
                    />
                  </If>
                </View>
              </View>
            )}
          </Each>
        </View>
      </If>
    </Scroll>
  );
}

// Training space View 1: Overview dashboard.
// Inline empty-state onboarding when unconfigured, global time window selector with scope caption,
// platform Rating cards + 3 focused KPI cards (Period Solved, Streak, Today),
// dual-mode annual heatmap, Rating trajectory, difficulty trend, and distributions with drilldown.

import { computed, Scroll, Text, View } from "nsh";
import { t } from "../i18n";
import { Segmented } from "../kit";
import { ratingTier } from "../logic";
import { cardShadow, tierColor } from "../theme";
import { DifficultyTrendCard } from "./DifficultyTrendCard";
import { DistributionsRow } from "./DistributionsRow";
import { HeatmapCard } from "./HeatmapCard";
import { zhEn } from "./locale";
import { OnboardingEmptyState, type OnboardingOjSource } from "./OnboardingEmptyState";
import { RatingChartCard } from "./RatingChartCard";
import {
  platformDisplayName,
  type DifficultyBucket,
  type TimeWindow,
  type TrainingDashboardData,
} from "./types";

function KpiCard(props: {
  label: string;
  windowBadge?: string;
  value: string;
  sub?: string;
  valueColor?: string;
  testID?: string;
}) {
  return (
    <View
      testID={props.testID}
      class="bg-card border border-border"
      style={{
        flex: 1,
        minWidth: 160,
        borderRadius: 10,
        paddingTop: 12,
        paddingBottom: 12,
        paddingLeft: 14,
        paddingRight: 14,
        gap: 4,
        boxShadow: cardShadow(),
      }}
    >
      <View style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>
        <Text style={{ fontSize: 11.5 }} class="text-muted-foreground">
          {props.label}
        </Text>
        <If when={props.windowBadge}>
          <View
            class="bg-muted"
            style={{ height: 17, paddingLeft: 5, paddingRight: 5, borderRadius: 4, justifyContent: "center" }}
          >
            <Text style={{ fontSize: 10.5 }} class="text-muted-foreground">
              {props.windowBadge!}
            </Text>
          </View>
        </If>
      </View>
      <Text
        style={{
          fontSize: 22,
          lineHeight: 28,
          fontWeight: 600,
          fontVariant: ["tabular-nums"],
          color: props.valueColor,
        }}
        class={props.valueColor ? undefined : "text-foreground"}
      >
        {props.value}
      </Text>
      <If when={props.sub}>
        <Text
          style={{ fontSize: 11.5, fontVariant: ["tabular-nums"], numberOfLines: 1, textOverflow: "ellipsis" }}
          class="text-secondary-foreground"
        >
          {props.sub!}
        </Text>
      </If>
    </View>
  );
}

export function OverviewTab(props: {
  data: TrainingDashboardData | undefined;
  ojSources?: readonly OnboardingOjSource[];
  hasConfiguredAccounts?: boolean;
  window: TimeWindow;
  tagMinRating: number | null;
  onWindowChange: (w: TimeWindow) => void;
  onYearChange: (year: number) => void;
  onTagMinRatingChange: (min: number | null) => void;
  onDrilldownBucket: (bucket: DifficultyBucket) => void;
  onDrilldownTag: (tag: string, minRating: number | null) => void;
  onAccountConfigured?: () => void;
  onOpenManualLog?: () => void;
}) {
  const d = () => props.data;
  const hasAnyData = () => {
    const cur = d();
    if (!cur) return false;
    return (
      cur.accounts.length > 0 ||
      Boolean(cur.has_local_records) ||
      cur.kpi.total_ac > 0 ||
      cur.kpi.total_submissions > 0 ||
      cur.rating_series.length > 0
    );
  };

  const hasRatingHistory = () => {
    const cur = d();
    if (!cur) return false;
    return cur.rating_series.some((s) => s.points.length > 0);
  };

  const hasDifficultyTrend = () => {
    const cur = d();
    if (!cur) return false;
    return cur.difficulty_trend.some((p) => p.p75 != null || p.p90 != null);
  };

  const windowItems = computed(() => [
    { value: "30d" as const, label: t("training.window.30d"), testID: "training-window-30d" },
    { value: "90d" as const, label: t("training.window.90d"), testID: "training-window-90d" },
    { value: "year" as const, label: t("training.window.year"), testID: "training-window-year" },
    { value: "all" as const, label: t("training.window.all"), testID: "training-window-all" },
  ]);

  const activeWindowLabel = () => {
    switch (props.window) {
      case "30d":
        return t("training.window.30d");
      case "90d":
        return t("training.window.90d");
      case "year":
        return t("training.window.year");
      case "all":
        return t("training.window.all");
    }
  };

  return (
    <Scroll
      smoothWheel
      testID="training-overview-scroll"
      style={{ flex: 1, minHeight: 0 }}
      contentStyle={{ padding: 20, gap: 16, paddingBottom: 36 }}
    >
      <If
        when={hasAnyData()}
        else={
          <OnboardingEmptyState
            sources={props.ojSources ?? []}
            onAccountConfigured={props.onAccountConfigured ?? (() => {})}
            onOpenManualLog={props.onOpenManualLog}
          />
        }
      >
        {/* Global time window selector */}
        <View style={{ flexDirection: "row", alignItems: "center" }}>
          <Segmented
            items={windowItems()}
            value={props.window}
            onChange={props.onWindowChange}
            itemWidth={68}
          />
        </View>

        {/* Platform Rating cards (when accounts exist) */}
        <If when={(d()?.accounts.length ?? 0) > 0}>
          <View style={{ flexDirection: "row", flexWrap: "wrap", gap: 12 }}>
            <Each of={d()?.accounts ?? []} key={(a) => a.platform}>
              {(acc) => {
                const curRating = () => acc().rating;
                const maxRating = () => acc().max_rating;
                return (
                  <View
                    testID={`training-account-${acc().platform}`}
                    class="bg-card border border-border"
                    style={{
                      flex: 1,
                      minWidth: 200,
                      borderRadius: 10,
                      paddingTop: 12,
                      paddingBottom: 12,
                      paddingLeft: 14,
                      paddingRight: 14,
                      gap: 4,
                      boxShadow: cardShadow(),
                    }}
                  >
                    <View style={{ flexDirection: "row", alignItems: "center", justifyContent: "space-between", gap: 8 }}>
                      <Text style={{ fontSize: 12, fontWeight: 500 }} class="text-secondary-foreground">
                        {platformDisplayName(acc().platform)}
                      </Text>
                      <Text style={{ fontSize: 11.5 }} class="text-muted-foreground">
                        {acc().handle || "—"}
                      </Text>
                    </View>
                    <View style={{ flexDirection: "row", alignItems: "baseline", gap: 8 }}>
                      <Text
                        style={{
                          fontSize: 22,
                          lineHeight: 28,
                          fontWeight: 600,
                          fontVariant: ["tabular-nums"],
                          color: curRating() != null ? tierColor(ratingTier(curRating())) : undefined,
                        }}
                        class={curRating() != null ? undefined : "text-muted-foreground"}
                      >
                        {curRating() != null ? String(curRating()) : "—"}
                      </Text>
                      <If when={maxRating() != null}>
                        <Text
                          style={{
                            fontSize: 11.5,
                            fontVariant: ["tabular-nums"],
                            color: tierColor(ratingTier(maxRating())),
                          }}
                        >
                          {t("training.kpi.maxRating", { rating: maxRating()! })}
                        </Text>
                      </If>
                    </View>
                    <Text style={{ fontSize: 11.5, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
                      {t("training.kpi.accountStats", {
                        ac: acc().solved_count,
                        subs: acc().submission_count,
                      })}
                    </Text>
                  </View>
                );
              }}
            </Each>
          </View>
        </If>

        {/* 3 focused KPI cards: Period Solved (follows window), Streak, Today */}
        <If when={d()}>
          {(cur) => (
            <View style={{ flexDirection: "row", flexWrap: "wrap", gap: 12 }}>
              <KpiCard
                testID="training-kpi-window"
                label={zhEn("周期内过题", "Period Solved")}
                windowBadge={activeWindowLabel()}
                value={String(cur().kpi.window_ac)}
                sub={t("training.kpi.subsCount", { count: cur().kpi.window_submissions })}
              />
              <KpiCard
                testID="training-kpi-streak"
                label={zhEn("连续天数", "Streak")}
                value={t("training.kpi.days", { count: cur().kpi.streak_days })}
                sub={t("training.kpi.longestStreak", { count: cur().kpi.longest_streak_days })}
              />
              <KpiCard
                testID="training-kpi-today"
                label={zhEn("今日", "Today")}
                value={String(cur().kpi.today_ac)}
                sub={t("training.kpi.subsCount", { count: cur().kpi.today_submissions })}
              />
            </View>
          )}
        </If>

        {/* Dual-mode annual heatmap */}
        <If when={d()}>
          {(cur) => (
            <HeatmapCard
              year={cur().heatmap_year}
              availableYears={cur().available_years}
              cells={cur().heatmap}
              onYearChange={props.onYearChange}
            />
          )}
        </If>

        {/* Rating curve (only when rating history exists) + Solved problem difficulty trend */}
        <If when={d() && (hasRatingHistory() || hasDifficultyTrend())}>
          <View style={{ flexDirection: "row", flexWrap: "wrap", gap: 14 }}>
            <If when={hasRatingHistory()}>
              <RatingChartCard series={d()!.rating_series} />
            </If>
            <If when={hasDifficultyTrend() || hasRatingHistory()}>
              <DifficultyTrendCard points={d()!.difficulty_trend} />
            </If>
          </View>
        </If>

        {/* Difficulty buckets + Tag distribution */}
        <If when={d()}>
          {(cur) => (
            <DistributionsRow
              buckets={cur().difficulty_buckets}
              tags={cur().tag_distribution}
              windowLabel={activeWindowLabel()}
              tagMinRating={props.tagMinRating}
              onTagMinRatingChange={props.onTagMinRatingChange}
              onSelectBucket={props.onDrilldownBucket}
              onSelectTag={props.onDrilldownTag}
            />
          )}
        </If>
      </If>
    </Scroll>
  );
}

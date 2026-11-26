// Training space View 1: Overview dashboard (Phase 6D §8.4).
// Inline empty-state onboarding when unconfigured, unified Overview Band (OJ Ratings + 3 KPIs in one card),
// rolling 12-month dual-mode heatmap, Rating trajectory, difficulty trend, and distributions with drilldown.

import { Scroll, Text, View } from "nsh";
import { t } from "../i18n";
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

import { RADIUS, SPACE, TYPE } from "../tokens";

function OverviewMetricCell(props: {
  label: string;
  value: string;
  dotColor?: string;
  sub?: string;
  testID?: string;
}) {
  return (
    <View
      testID={props.testID}
      style={{
        flex: 1,
        minWidth: 112,
        gap: SPACE[4],
      }}
    >
      <Text style={TYPE.caption} class="text-muted-foreground">
        {props.label}
      </Text>
      <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[6] }}>
        <Text style={TYPE.display} class="text-foreground">
          {props.value}
        </Text>
        <If when={props.dotColor}>
          <View
            style={{
              width: 6,
              height: 6,
              borderRadius: RADIUS.full,
              backgroundColor: props.dotColor!,
            }}
          />
        </If>
      </View>
      <If when={props.sub}>
        <Text
          style={[TYPE.caption, { fontVariant: ["tabular-nums"], numberOfLines: 1, textOverflow: "ellipsis" }]}
          class="text-muted-foreground"
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
      contentStyle={{ padding: SPACE[20], gap: SPACE[16], paddingBottom: SPACE[32] }}
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
        {/* Unified Overview Band: OJ Ratings (configured/present) + Solved / Streak / Today in one card */}
        <If when={d()}>
          {(cur) => (
            <View
              testID="training-overview-band"
              class="bg-card border border-border"
              style={{
                flexDirection: "row",
                flexWrap: "wrap",
                alignItems: "flex-start",
                borderRadius: RADIUS.lg,
                padding: SPACE[16],
                gap: SPACE[24],
                boxShadow: cardShadow(),
              }}
            >
              <Each of={cur().accounts} key={(a) => a.platform}>
                {(acc) => {
                  const curRating = () => acc().rating;
                  const maxRating = () => acc().max_rating;
                  return (
                    <OverviewMetricCell
                      testID={`training-account-${acc().platform}`}
                      label={platformDisplayName(acc().platform)}
                      value={curRating() != null ? String(curRating()) : String(acc().solved_count)}
                      dotColor={curRating() != null ? tierColor(ratingTier(curRating())) : undefined}
                      sub={
                        maxRating() != null
                          ? t("training.kpi.maxRating", { rating: maxRating()! })
                          : t("training.kpi.subsCount", { count: acc().submission_count })
                      }
                    />
                  );
                }}
              </Each>
              <OverviewMetricCell
                testID="training-kpi-window"
                label={zhEn("过题", "Solved")}
                value={String(cur().kpi.window_ac)}
                sub={t("training.kpi.subsCount", { count: cur().kpi.window_submissions })}
              />
              <OverviewMetricCell
                testID="training-kpi-streak"
                label={zhEn("连续", "Streak")}
                value={t("training.kpi.days", { count: cur().kpi.streak_days })}
                sub={t("training.kpi.longestStreak", { count: cur().kpi.longest_streak_days })}
              />
              <OverviewMetricCell
                testID="training-kpi-today"
                label={zhEn("今日", "Today")}
                value={String(cur().kpi.today_ac)}
                sub={t("training.kpi.subsCount", { count: cur().kpi.today_submissions })}
              />
            </View>
          )}
        </If>

        {/* Rolling 12-month dual-mode heatmap */}
        <If when={d()}>
          {(cur) => <HeatmapCard cells={cur().heatmap} />}
        </If>

        {/* Rating curve (only when rating history exists) + Solved problem difficulty trend */}
        <If when={d() && (hasRatingHistory() || hasDifficultyTrend())}>
          <View style={{ flexDirection: "row", flexWrap: "wrap", gap: SPACE[12] }}>
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

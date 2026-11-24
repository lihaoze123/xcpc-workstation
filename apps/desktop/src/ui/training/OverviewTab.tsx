// Training space View 1: Overview dashboard.
// Global time window selector, platform Rating cards, AC/submission/streak KPI cards,
// dual-mode annual heatmap, Rating trajectory, P75/P90 difficulty trend, and
// difficulty-bucket + high-difficulty tag distributions with click-to-drilldown.

import { Scroll, Text, View } from "nsh";
import { t } from "../i18n";
import { Btn } from "../kit";
import { ratingTier } from "../logic";
import { goSettings } from "../store";
import { cardShadow, tierColor } from "../theme";
import { DifficultyTrendCard } from "./DifficultyTrendCard";
import { DistributionsRow } from "./DistributionsRow";
import { HeatmapCard } from "./HeatmapCard";
import { RatingChartCard } from "./RatingChartCard";
import {
  platformDisplayName,
  type DifficultyBucket,
  type TimeWindow,
  type TrainingDashboardData,
} from "./types";

const WINDOWS: readonly { value: TimeWindow; labelKey: "30d" | "90d" | "year" | "all" }[] = [
  { value: "30d", labelKey: "30d" },
  { value: "90d", labelKey: "90d" },
  { value: "year", labelKey: "year" },
  { value: "all", labelKey: "all" },
];

function KpiCard(props: {
  label: string;
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
        minWidth: 148,
        borderRadius: 10,
        paddingTop: 12,
        paddingBottom: 12,
        paddingLeft: 14,
        paddingRight: 14,
        gap: 4,
        boxShadow: cardShadow(),
      }}
    >
      <Text style={{ fontSize: 11.5 }} class="text-muted-foreground">
        {props.label}
      </Text>
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
  window: TimeWindow;
  tagMinRating: number | null;
  onWindowChange: (w: TimeWindow) => void;
  onYearChange: (year: number) => void;
  onTagMinRatingChange: (min: number | null) => void;
  onDrilldownBucket: (bucket: DifficultyBucket) => void;
  onDrilldownTag: (tag: string, minRating: number | null) => void;
}) {
  const d = () => props.data;
  const hasAnyData = () => {
    const cur = d();
    if (!cur) return false;
    return cur.accounts.length > 0 || cur.kpi.total_submissions > 0 || cur.rating_series.length > 0;
  };

  return (
    <Scroll style={{ flex: 1 }} contentStyle={{ padding: 20, gap: 16, paddingBottom: 32 }}>
      <If
        when={hasAnyData()}
        else={
          <View
            testID="training-overview-empty"
            class="bg-card border border-border"
            style={{
              borderRadius: 10,
              padding: 28,
              alignItems: "center",
              justifyContent: "center",
              gap: 10,
              boxShadow: cardShadow(),
            }}
          >
            <Text style={{ fontSize: 14, fontWeight: 500 }} class="text-foreground">
              {t("training.empty.title")}
            </Text>
            <Text style={{ fontSize: 12.5, textAlign: "center", maxWidth: 420 }} class="text-muted-foreground">
              {t("training.empty.desc")}
            </Text>
            <Btn
              testID="training-empty-settings-btn"
              label={t("training.empty.openPlugins")}
              variant="primary"
              onPress={() => goSettings("plugins")}
            />
          </View>
        }
      >
        {/* Global time window selector */}
        <View style={{ flexDirection: "row", alignItems: "center", justifyContent: "space-between", flexWrap: "wrap", gap: 10 }}>
          <View style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>
            <Each of={WINDOWS} key={(w) => w.value}>
              {(item) => {
                const active = () => props.window === item().value;
                return (
                  <View
                    testID={`training-window-${item().value}`}
                    onPress={() => props.onWindowChange(item().value)}
                    class={active() ? "bg-accent border border-border" : "bg-card border border-border hover:bg-muted"}
                    style={{
                      height: 26,
                      paddingLeft: 10,
                      paddingRight: 10,
                      borderRadius: 6,
                      justifyContent: "center",
                    }}
                  >
                    <Text
                      style={{ fontSize: 12, fontWeight: active() ? 600 : 400 }}
                      class={active() ? "text-foreground" : "text-secondary-foreground"}
                    >
                      {t(`training.window.${item().labelKey}` as const)}
                    </Text>
                  </View>
                );
              }}
            </Each>
          </View>

          <If when={d()}>
            {(cur) => (
              <Text style={{ fontSize: 12, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
                {t("training.kpi.windowSummary", {
                  ac: cur().kpi.window_ac,
                  subs: cur().kpi.window_submissions,
                })}
              </Text>
            )}
          </If>
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

        {/* Core KPI cards: Today, Week, Month, Total, Streak */}
        <If when={d()}>
          {(cur) => (
            <View style={{ flexDirection: "row", flexWrap: "wrap", gap: 12 }}>
              <KpiCard
                testID="training-kpi-today"
                label={t("training.kpi.today")}
                value={String(cur().kpi.today_ac)}
                sub={t("training.kpi.subsCount", { count: cur().kpi.today_submissions })}
              />
              <KpiCard
                testID="training-kpi-week"
                label={t("training.kpi.week")}
                value={String(cur().kpi.week_ac)}
                sub={t("training.kpi.subsCount", { count: cur().kpi.week_submissions })}
              />
              <KpiCard
                testID="training-kpi-month"
                label={t("training.kpi.month")}
                value={String(cur().kpi.month_ac)}
                sub={t("training.kpi.subsCount", { count: cur().kpi.month_submissions })}
              />
              <KpiCard
                testID="training-kpi-total"
                label={t("training.kpi.total")}
                value={String(cur().kpi.total_ac)}
                sub={t("training.kpi.subsCount", { count: cur().kpi.total_submissions })}
              />
              <KpiCard
                testID="training-kpi-streak"
                label={t("training.kpi.streak")}
                value={t("training.kpi.days", { count: cur().kpi.streak_days })}
                sub={t("training.kpi.longestStreak", { count: cur().kpi.longest_streak_days })}
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

        {/* Rating curve + P75/P90 Difficulty breakthrough trend */}
        <If when={d()}>
          {(cur) => (
            <View style={{ flexDirection: "row", flexWrap: "wrap", gap: 14 }}>
              <RatingChartCard series={cur().rating_series} />
              <DifficultyTrendCard points={cur().difficulty_trend} />
            </View>
          )}
        </If>

        {/* Difficulty buckets + Tag distribution */}
        <If when={d()}>
          {(cur) => (
            <DistributionsRow
              buckets={cur().difficulty_buckets}
              tags={cur().tag_distribution}
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

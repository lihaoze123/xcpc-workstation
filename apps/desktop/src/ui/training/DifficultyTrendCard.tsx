// Solved problem difficulty trend: weekly / monthly AC difficulty P75 & P90 dual-line chart
// built on nsh-o11y <TimeSeriesChart>. Matches bottom numeric label colors to legend colors
// (P90 primary blue, P75 chart-2 green).

import { computed, signal, Text, View } from "nsh";
import {
  TimeSeriesChart,
  type ArrayData,
  type ChartColors,
  type SeriesOptions,
} from "nsh-o11y";
import { t } from "../i18n";
import { cardShadow, token } from "../theme";
import { zhEn } from "./locale";
import type { DifficultyTrendPoint } from "./types";

const CHART_H = 210;

/** Converts a trend period (`YYYY-MM`, `YYYY-Www`, or `YYYY-MM-DD`) into a local ms timestamp. */
function periodToTimestampMs(period: string, fallbackIndex: number): number {
  const weekMatch = /^(\d{4})-W(\d{2})$/.exec(period);
  if (weekMatch) {
    const year = Number(weekMatch[1]);
    const week = Number(weekMatch[2]);
    // ISO week 1 always contains Jan 4; compute Monday of that week in local time.
    const jan4 = new Date(year, 0, 4);
    const dow = (jan4.getDay() + 6) % 7; // Mon = 0 .. Sun = 6
    return new Date(year, 0, 4 - dow + (week - 1) * 7).getTime();
  }
  const ymMatch = /^(\d{4})-(\d{2})$/.exec(period);
  if (ymMatch) {
    return new Date(Number(ymMatch[1]), Number(ymMatch[2]) - 1, 1).getTime();
  }
  const ymdMatch = /^(\d{4})-(\d{2})-(\d{2})$/.exec(period);
  if (ymdMatch) {
    return new Date(Number(ymdMatch[1]), Number(ymdMatch[2]) - 1, Number(ymdMatch[3])).getTime();
  }
  const parsed = Date.parse(period);
  if (Number.isFinite(parsed)) return parsed;
  return Date.UTC(2020, 0, 1) + fallbackIndex * 86_400_000;
}

export function DifficultyTrendCard(props: { points: readonly DifficultyTrendPoint[] }) {
  const hoveredIdx = signal<number | undefined>(undefined);

  const ratedPoints = computed(() => props.points.filter((p) => p.p75 != null || p.p90 != null));

  const bounds = computed(() => {
    const pts = ratedPoints();
    if (pts.length === 0) return { minY: 800, maxY: 2200 };
    let lo = Infinity;
    let hi = -Infinity;
    for (const p of pts) {
      if (p.p75 != null) {
        lo = Math.min(lo, p.p75);
        hi = Math.max(hi, p.p75);
      }
      if (p.p90 != null) {
        lo = Math.min(lo, p.p90);
        hi = Math.max(hi, p.p90);
      }
    }
    const minY = Math.max(0, Math.floor((lo - 150) / 200) * 200);
    const maxY = Math.max(minY + 400, Math.ceil((hi + 150) / 200) * 200);
    return { minY, maxY };
  });

  const chartData = computed<ArrayData>(() => {
    const pts = ratedPoints();
    return {
      x: pts.map((p, i) => periodToTimestampMs(p.period, i)),
      ys: [
        pts.map((p) => (p.p90 != null ? p.p90 : NaN)),
        pts.map((p) => (p.p75 != null ? p.p75 : NaN)),
      ],
    };
  });

  const chartSeries = computed<SeriesOptions[]>(() => [
    {
      label: "P90",
      color: token("primary"),
      width: 2,
      spanGaps: true,
    },
    {
      label: "P75",
      color: token("chart-2"),
      width: 2,
      spanGaps: true,
    },
  ]);

  const chartColors = computed<ChartColors>(() => ({
    background: token("card"),
    text: token("foreground"),
    muted: token("muted-foreground"),
    grid: token("border"),
    cursor: token("muted-foreground"),
    selection: token("primary"),
    error: token("destructive"),
  }));

  const activePoint = () => {
    const pts = ratedPoints();
    if (pts.length === 0) return undefined;
    const idx = hoveredIdx();
    if (idx !== undefined && idx >= 0 && idx < pts.length) return pts[idx];
    return pts[pts.length - 1];
  };

  return (
    <View
      testID="training-trend-chart"
      class="bg-card border border-border"
      style={{
        flex: 1,
        minWidth: 320,
        borderRadius: 10,
        padding: 16,
        gap: 10,
        boxShadow: cardShadow(),
      }}
    >
      <View style={{ flexDirection: "row", alignItems: "center", justifyContent: "space-between", gap: 8 }}>
        <View style={{ flexDirection: "row", alignItems: "baseline", gap: 8 }}>
          <Text style={{ fontSize: 13.5, fontWeight: 600 }} class="text-foreground">
            {zhEn("做出的题有多难", "Solved Problem Difficulty")}
          </Text>
          <Text style={{ fontSize: 11.5 }} class="text-muted-foreground">
            {zhEn("75% / 90% 分位", "75th / 90th pct")}
          </Text>
        </View>
        <View style={{ flexDirection: "row", alignItems: "center", gap: 10 }}>
          <View style={{ flexDirection: "row", alignItems: "center", gap: 5 }}>
            <View style={{ width: 8, height: 8, borderRadius: 4 }} class="bg-primary" />
            <Text style={{ fontSize: 11.5 }} class="text-secondary-foreground">
              P90
            </Text>
          </View>
          <View style={{ flexDirection: "row", alignItems: "center", gap: 5 }}>
            <View style={{ width: 8, height: 8, borderRadius: 4 }} class="bg-chart-2" />
            <Text style={{ fontSize: 11.5 }} class="text-secondary-foreground">
              P75
            </Text>
          </View>
        </View>
      </View>

      <If
        when={ratedPoints().length > 0}
        else={
          <View style={{ height: CHART_H, alignItems: "center", justifyContent: "center" }}>
            <Text style={{ fontSize: 12.5 }} class="text-muted-foreground">
              {t("training.trend.empty")}
            </Text>
          </View>
        }
      >
        <TimeSeriesChart
          testID="training-trend-canvas"
          data={chartData()}
          series={chartSeries()}
          unit="none"
          yMin={bounds().minY}
          yMax={bounds().maxY}
          colors={chartColors()}
          fontSize={10.5}
          legend={false}
          onCursor={(info) => hoveredIdx.set(info ? info.index : undefined)}
          style={{ width: "100%", height: CHART_H }}
        />

        <If when={activePoint()}>
          {(pt) => (
            <View
              testID="training-trend-detail"
              style={{ flexDirection: "row", alignItems: "center", justifyContent: "space-between", gap: 10 }}
            >
              <Text style={{ fontSize: 12, fontWeight: 500, fontVariant: ["tabular-nums"] }} class="text-secondary-foreground">
                {pt().period}
              </Text>
              <View style={{ flexDirection: "row", alignItems: "center", gap: 10 }}>
                <If when={pt().p90 != null}>
                  <Text
                    testID="training-trend-p90-val"
                    style={{
                      fontSize: 12,
                      fontWeight: 600,
                      fontVariant: ["tabular-nums"],
                    }}
                    class="text-primary"
                  >
                    {`P90 ${pt().p90}`}
                  </Text>
                </If>
                <If when={pt().p75 != null}>
                  <Text
                    testID="training-trend-p75-val"
                    style={{
                      fontSize: 12,
                      fontWeight: 500,
                      fontVariant: ["tabular-nums"],
                    }}
                    class="text-chart-2"
                  >
                    {`P75 ${pt().p75}`}
                  </Text>
                </If>
                <Text style={{ fontSize: 11.5, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
                  {t("training.trend.ratedAc", { count: pt().rated_ac_count })}
                </Text>
              </View>
            </View>
          )}
        </If>
      </If>
    </View>
  );
}

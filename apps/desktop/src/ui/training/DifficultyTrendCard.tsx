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
import { SectionEmpty } from "../layout";
import { cardShadow, token } from "../theme";
import { zhEn } from "./locale";
import type { DifficultyTrendPoint } from "./types";

import { FONT_SIZE, FONT_WEIGHT, RADIUS, SPACE } from "../tokens";
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

/** Converts a trend period (`YYYY-MM`, `YYYY-Www`, or `YYYY-MM-DD`) into a `YYYY-MM` month key. */
function periodToMonthKey(period: string): string {
  const ymMatch = /^(\d{4})-(\d{2})$/.exec(period);
  if (ymMatch) return period;
  const ymdMatch = /^(\d{4})-(\d{2})-(\d{2})$/.exec(period);
  if (ymdMatch) return `${ymdMatch[1]}-${ymdMatch[2]}`;
  const weekMatch = /^(\d{4})-W(\d{2})$/.exec(period);
  if (weekMatch) {
    const year = Number(weekMatch[1]);
    const week = Number(weekMatch[2]);
    const jan4 = new Date(year, 0, 4);
    const dow = (jan4.getDay() + 6) % 7;
    const d = new Date(year, 0, 4 - dow + (week - 1) * 7);
    return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}`;
  }
  return period;
}

/** Aggregates weekly/daily points into monthly buckets so multi-year histories remain legible. */
function aggregateMonthly(points: readonly DifficultyTrendPoint[]): DifficultyTrendPoint[] {
  const buckets = new Map<
    string,
    { p75Sum: number; p75W: number; p90Sum: number; p90W: number; ac: number; totalAc: number }
  >();
  for (const p of points) {
    if (p.p75 == null && p.p90 == null) continue;
    const key = periodToMonthKey(p.period);
    let b = buckets.get(key);
    if (!b) {
      b = { p75Sum: 0, p75W: 0, p90Sum: 0, p90W: 0, ac: 0, totalAc: 0 };
      buckets.set(key, b);
    }
    const w = Math.max(1, p.rated_ac_count);
    if (p.p75 != null) {
      b.p75Sum += p.p75 * w;
      b.p75W += w;
    }
    if (p.p90 != null) {
      b.p90Sum += p.p90 * w;
      b.p90W += w;
    }
    b.ac += p.rated_ac_count;
    b.totalAc += p.ac_count;
  }
  return [...buckets.entries()]
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([period, b]) => ({
      period,
      p75: b.p75W > 0 ? Math.round(b.p75Sum / b.p75W) : null,
      p90: b.p90W > 0 ? Math.round(b.p90Sum / b.p90W) : null,
      ac_count: b.totalAc,
      rated_ac_count: b.ac,
    }));
}

/** Computes a 3-period moving average over nullable values (NaN for missing). */
function smooth3(values: readonly (number | null | undefined)[]): number[] {
  return values.map((v, idx) => {
    if (v == null) return NaN;
    let sum = 0;
    let count = 0;
    for (let j = Math.max(0, idx - 1); j <= Math.min(values.length - 1, idx + 1); j += 1) {
      const val = values[j];
      if (val != null) {
        sum += val;
        count += 1;
      }
    }
    return count > 0 ? Math.round(sum / count) : NaN;
  });
}

export function DifficultyTrendCard(props: { points: readonly DifficultyTrendPoint[] }) {
  const hoveredIdx = signal<number | undefined>(undefined);

  const ratedPoints = computed(() => aggregateMonthly(props.points));

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
        smooth3(pts.map((p) => p.p90)),
        smooth3(pts.map((p) => p.p75)),
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
        borderRadius: RADIUS.lg,
        padding: SPACE[16],
        gap: SPACE[8],
        boxShadow: cardShadow(),
      }}
    >
      <View style={{ flexDirection: "row", alignItems: "center", justifyContent: "space-between", gap: SPACE[8] }}>
        <View style={{ flexDirection: "row", alignItems: "baseline", gap: SPACE[8] }}>
          <Text style={{ fontSize: FONT_SIZE.body, fontWeight: FONT_WEIGHT.semibold }} class="text-foreground">
            {zhEn("做出的题有多难", "Solved Problem Difficulty")}
          </Text>
          <Text style={{ fontSize: FONT_SIZE.caption }} class="text-muted-foreground">
            {zhEn("90% / 75% 分位（月度平滑）", "90th / 75th pct (monthly)")}
          </Text>
        </View>
        <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[8] }}>
          <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[4] }}>
            <View style={{ width: 8, height: 8, borderRadius: RADIUS.xs }} class="bg-primary" />
            <Text style={{ fontSize: FONT_SIZE.caption }} class="text-secondary-foreground">
              P90
            </Text>
          </View>
          <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[4] }}>
            <View style={{ width: 8, height: 8, borderRadius: RADIUS.xs }} class="bg-chart-2" />
            <Text style={{ fontSize: FONT_SIZE.caption }} class="text-secondary-foreground">
              P75
            </Text>
          </View>
        </View>
      </View>

      <If
        when={ratedPoints().length > 0}
        else={
          <SectionEmpty message={t("training.trend.empty")} style={{ height: CHART_H }} />
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
              style={{ flexDirection: "row", alignItems: "center", justifyContent: "space-between", gap: SPACE[8] }}
            >
              <Text style={{ fontSize: FONT_SIZE.caption, fontWeight: FONT_WEIGHT.semibold, fontVariant: ["tabular-nums"] }} class="text-secondary-foreground">
                {pt().period}
              </Text>
              <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[8] }}>
                <If when={pt().p90 != null}>
                  <Text
                    testID="training-trend-p90-val"
                    style={{
                      fontSize: FONT_SIZE.caption,
                      fontWeight: FONT_WEIGHT.semibold,
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
                      fontSize: FONT_SIZE.caption,
                      fontWeight: FONT_WEIGHT.semibold,
                      fontVariant: ["tabular-nums"],
                    }}
                    class="text-chart-2"
                  >
                    {`P75 ${pt().p75}`}
                  </Text>
                </If>
                <Text style={{ fontSize: FONT_SIZE.caption, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
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

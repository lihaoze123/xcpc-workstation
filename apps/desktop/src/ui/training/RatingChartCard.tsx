// Platform Rating trajectory chart built on nsh-o11y <TimeSeriesChart>:
// Rating tier boundary thresholds, reactive light/dark theme tokens,
// and interactive crosshair driving the contest detail footer.

import { colorScheme, computed, effect, signal, Text, View } from "nsh";
import {
  TimeSeriesChart,
  withAlpha,
  type ArrayData,
  type ChartColors,
  type SeriesOptions,
  type Threshold,
} from "nsh-o11y";
import { t } from "../i18n";
import { Segmented } from "../kit";
import { ratingTier } from "../logic";
import { cardShadow, tierColor, token } from "../theme";
import {
  platformDisplayName,
  RATING_BANDS,
  type PlatformRatingSeries,
  type RatingPoint,
} from "./types";

const CHART_H = 210;

export function RatingChartCard(props: { series: readonly PlatformRatingSeries[] }) {
  const selectedPlatform = signal<string>("");
  const hoveredIdx = signal<number | undefined>(undefined);

  effect(() => {
    const list = props.series;
    if (list.length === 0) return;
    if (!list.some((s) => s.platform === selectedPlatform())) {
      selectedPlatform.set(list[0].platform);
      hoveredIdx.set(undefined);
    }
  });

  const activeSeries = computed(() => {
    const list = props.series;
    return list.find((s) => s.platform === selectedPlatform()) ?? list[0];
  });

  const points = computed<readonly RatingPoint[]>(() => activeSeries()?.points ?? []);

  const bounds = computed(() => {
    const pts = points();
    if (pts.length === 0) return { minY: 800, maxY: 2000 };
    let lo = Infinity;
    let hi = -Infinity;
    for (const p of pts) {
      if (p.rating_after < lo) lo = p.rating_after;
      if (p.rating_after > hi) hi = p.rating_after;
    }
    const minY = Math.max(0, Math.floor((lo - 150) / 200) * 200);
    const maxY = Math.max(minY + 400, Math.ceil((hi + 150) / 200) * 200);
    return { minY, maxY };
  });

  const chartData = computed<ArrayData>(() => {
    const pts = points();
    return {
      x: pts.map((p) => p.at * 1000),
      ys: [pts.map((p) => p.rating_after)],
    };
  });

  const chartSeries = computed<SeriesOptions[]>(() => [
    {
      label: platformDisplayName(activeSeries()?.platform ?? "Rating"),
      color: token("primary"),
      width: 2,
      fill: 0.08,
    },
  ]);

  const thresholds = computed<Threshold[]>(() => {
    const { minY, maxY } = bounds();
    const dark = colorScheme() === "dark";
    const alpha = dark ? 0.35 : 0.28;
    const out: Threshold[] = [];
    for (const band of RATING_BANDS) {
      if (band.min <= minY || band.min >= maxY) continue;
      out.push({
        value: band.min,
        color: withAlpha(tierColor(ratingTier(band.min)), alpha),
        dash: [4, 4],
      });
    }
    return out;
  });

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
    const pts = points();
    if (pts.length === 0) return undefined;
    const idx = hoveredIdx();
    if (idx !== undefined && idx >= 0 && idx < pts.length) return pts[idx];
    return pts[pts.length - 1];
  };

  const platformItems = computed(() =>
    props.series.map((s) => ({
      value: s.platform,
      label: platformDisplayName(s.platform),
      testID: `rating-platform-${s.platform}`,
    })),
  );

  return (
    <View
      testID="training-rating-chart"
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
        <Text style={{ fontSize: 13.5, fontWeight: 600 }} class="text-foreground">
          {t("training.rating.title")}
        </Text>
        <If when={props.series.length > 1}>
          <Segmented
            items={platformItems()}
            value={activeSeries()?.platform ?? ""}
            onChange={(plat) => {
              selectedPlatform.set(plat);
              hoveredIdx.set(undefined);
            }}
            itemWidth={82}
          />
        </If>
      </View>

      <If
        when={points().length > 0}
        else={
          <View style={{ height: CHART_H, alignItems: "center", justifyContent: "center" }}>
            <Text style={{ fontSize: 12.5 }} class="text-muted-foreground">
              {t("training.rating.empty")}
            </Text>
          </View>
        }
      >
        <TimeSeriesChart
          testID="training-rating-canvas"
          data={chartData()}
          series={chartSeries()}
          unit="none"
          yMin={bounds().minY}
          yMax={bounds().maxY}
          thresholds={thresholds()}
          colors={chartColors()}
          fontSize={10.5}
          legend={false}
          onCursor={(info) => hoveredIdx.set(info ? info.index : undefined)}
          style={{ width: "100%", height: CHART_H }}
        />

        {/* Selected / latest point detail */}
        <If when={activePoint()}>
          {(pt) => {
            const delta = () => pt().delta;
            return (
              <View
                testID="training-rating-detail"
                style={{ flexDirection: "row", alignItems: "center", justifyContent: "space-between", gap: 10 }}
              >
                <Text
                  style={{ fontSize: 12, flex: 1, minWidth: 0, numberOfLines: 1, textOverflow: "ellipsis" }}
                  class="text-secondary-foreground"
                >
                  {pt().contest_name}
                </Text>
                <View style={{ flexDirection: "row", alignItems: "center", gap: 8, flexShrink: 0 }}>
                  <If when={pt().rank != null}>
                    <Text style={{ fontSize: 12, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
                      {`#${pt().rank}`}
                    </Text>
                  </If>
                  <Text
                    style={{
                      fontSize: 12.5,
                      fontWeight: 600,
                      fontVariant: ["tabular-nums"],
                      color: tierColor(ratingTier(pt().rating_after)),
                    }}
                  >
                    {String(pt().rating_after)}
                  </Text>
                  <If when={delta() != null}>
                    <Text
                      style={{ fontSize: 12, fontWeight: 500, fontVariant: ["tabular-nums"] }}
                      class={delta()! >= 0 ? "text-chart-2" : "text-destructive"}
                    >
                      {delta()! >= 0 ? `+${delta()}` : String(delta())}
                    </Text>
                  </If>
                </View>
              </View>
            );
          }}
        </If>
      </If>
    </View>
  );
}

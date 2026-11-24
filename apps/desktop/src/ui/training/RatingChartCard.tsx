// Platform Rating trajectory chart on NSH <Canvas> with classic rating tier background bands
// (1200 / 1400 / 1600 / 1900 / 2100 / 2400 / 3000) and interactive point inspection.

import { Canvas, colorScheme, computed, effect, signal, Text, View } from "nsh";
import { t } from "../i18n";
import { ratingTier } from "../logic";
import { cardShadow, tierColor, token } from "../theme";
import {
  formatUnixDateShort,
  platformDisplayName,
  RATING_BANDS,
  type PlatformRatingSeries,
  type RatingPoint,
} from "./types";

const CHART_H = 210;
const PAD_L = 42;
const PAD_R = 14;
const PAD_T = 12;
const PAD_B = 22;

export function RatingChartCard(props: { series: readonly PlatformRatingSeries[] }) {
  const selectedPlatform = signal<string>("");
  const hoveredIdx = signal<number | undefined>(undefined);
  const pinnedIdx = signal<number | undefined>(undefined);
  const chartW = signal(500);

  effect(() => {
    const list = props.series;
    if (list.length === 0) return;
    if (!list.some((s) => s.platform === selectedPlatform())) {
      selectedPlatform.set(list[0].platform);
      hoveredIdx.set(undefined);
      pinnedIdx.set(undefined);
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

  const activeIndex = () => {
    const pts = points();
    if (pts.length === 0) return undefined;
    const idx = hoveredIdx() ?? pinnedIdx();
    if (idx !== undefined && idx >= 0 && idx < pts.length) return idx;
    return pts.length - 1;
  };

  const activePoint = () => {
    const idx = activeIndex();
    return idx !== undefined ? points()[idx] : undefined;
  };

  const xOfIndex = (idx: number, count: number, width: number): number => {
    const plotW = Math.max(1, width - PAD_L - PAD_R);
    if (count <= 1) return PAD_L + plotW / 2;
    return PAD_L + (idx / (count - 1)) * plotW;
  };

  const yOfRating = (rating: number, minY: number, maxY: number): number => {
    const plotH = CHART_H - PAD_T - PAD_B;
    const ratio = (rating - minY) / Math.max(1, maxY - minY);
    return PAD_T + plotH * (1 - Math.max(0, Math.min(1, ratio)));
  };

  const nearestIndex = (x: number): number | undefined => {
    const pts = points();
    if (pts.length === 0) return undefined;
    const w = chartW();
    let best = 0;
    let bestDist = Infinity;
    for (let i = 0; i < pts.length; i++) {
      const d = Math.abs(xOfIndex(i, pts.length, w) - x);
      if (d < bestDist) {
        bestDist = d;
        best = i;
      }
    }
    return best;
  };

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
          <View style={{ flexDirection: "row", alignItems: "center", gap: 4 }}>
            <Each of={props.series} key={(s) => s.platform}>
              {(s) => {
                const on = () => s().platform === activeSeries()?.platform;
                return (
                  <View
                    testID={`rating-platform-${s().platform}`}
                    onPress={() => {
                      selectedPlatform.set(s().platform);
                      hoveredIdx.set(undefined);
                      pinnedIdx.set(undefined);
                    }}
                    class={on() ? "bg-accent border border-border" : "hover:bg-muted"}
                    style={{ height: 24, paddingLeft: 8, paddingRight: 8, borderRadius: 6, justifyContent: "center" }}
                  >
                    <Text
                      style={{ fontSize: 11.5, fontWeight: on() ? 600 : 400 }}
                      class={on() ? "text-foreground" : "text-secondary-foreground"}
                    >
                      {platformDisplayName(s().platform)}
                    </Text>
                  </View>
                );
              }}
            </Each>
          </View>
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
        <Canvas
          testID="training-rating-canvas"
          style={{ width: "100%", height: CHART_H }}
          onLayout={(e) => {
            if (e.bounds.size.width > 0) chartW.set(e.bounds.size.width);
          }}
          onMouseMove={(e) => hoveredIdx.set(nearestIndex(e.position.x))}
          onMouseLeave={() => hoveredIdx.set(undefined)}
          onPress={(e) => {
            const idx = nearestIndex(e.position.x);
            if (idx !== undefined) pinnedIdx.set(idx);
          }}
          draw={(cx, size) => {
            const dark = colorScheme() === "dark";
            const pts = points();
            const { minY, maxY } = bounds();
            const plotW = Math.max(1, size.width - PAD_L - PAD_R);
            const plotH = CHART_H - PAD_T - PAD_B;
            const borderCol = token("border");
            const mutedText = token("muted-foreground");
            const lineCol = token("primary");
            const cardBg = token("card");

            cx.clearRect(0, 0, size.width, size.height);

            // Rating tier background bands
            for (const band of RATING_BANDS) {
              const bLo = Math.max(minY, band.min);
              const bHi = Math.min(maxY, band.max);
              if (bHi <= bLo) continue;
              const yTop = yOfRating(bHi, minY, maxY);
              const yBot = yOfRating(bLo, minY, maxY);
              cx.fillStyle = dark ? band.darkFill : band.lightFill;
              cx.fillRect(PAD_L, yTop, plotW, Math.max(0, yBot - yTop));
            }

            // Horizontal gridlines at rating thresholds
            cx.strokeStyle = borderCol;
            cx.lineWidth = 1;
            cx.font = "10.5px system-ui, sans-serif";
            cx.fillStyle = mutedText;
            cx.textAlign = "right";
            cx.textBaseline = "middle";
            const ticks = [800, 1200, 1400, 1600, 1900, 2100, 2400, 3000].filter(
              (v) => v >= minY && v <= maxY,
            );
            for (const v of ticks) {
              const y = yOfRating(v, minY, maxY);
              cx.beginPath();
              cx.moveTo(PAD_L, y);
              cx.lineTo(PAD_L + plotW, y);
              cx.stroke();
              cx.fillText(String(v), PAD_L - 6, y);
            }

            // Polyline
            if (pts.length > 0) {
              cx.strokeStyle = lineCol;
              cx.lineWidth = 2;
              cx.beginPath();
              pts.forEach((p, i) => {
                const x = xOfIndex(i, pts.length, size.width);
                const y = yOfRating(p.rating_after, minY, maxY);
                if (i === 0) cx.moveTo(x, y);
                else cx.lineTo(x, y);
              });
              cx.stroke();

              // Points
              const sel = activeIndex();
              pts.forEach((p, i) => {
                const x = xOfIndex(i, pts.length, size.width);
                const y = yOfRating(p.rating_after, minY, maxY);
                const isSel = i === sel;
                const dotColor = tierColor(ratingTier(p.rating_after));
                if (isSel) {
                  cx.strokeStyle = borderCol;
                  cx.lineWidth = 1;
                  cx.beginPath();
                  cx.moveTo(x, PAD_T);
                  cx.lineTo(x, PAD_T + plotH);
                  cx.stroke();
                }
                cx.fillStyle = dotColor;
                cx.beginPath();
                cx.arc(x, y, isSel ? 4.5 : pts.length > 40 ? 2 : 3, 0, Math.PI * 2);
                cx.fill();
                if (isSel) {
                  cx.strokeStyle = cardBg;
                  cx.lineWidth = 1.5;
                  cx.stroke();
                }
              });

              // First / Last date labels on X axis
              cx.fillStyle = mutedText;
              cx.textBaseline = "top";
              cx.textAlign = "left";
              cx.fillText(formatUnixDateShort(pts[0].at), PAD_L, CHART_H - 16);
              if (pts.length > 1) {
                cx.textAlign = "right";
                cx.fillText(formatUnixDateShort(pts[pts.length - 1].at), PAD_L + plotW, CHART_H - 16);
              }
            }
          }}
        />

        {/* Selected / hovered point detail */}
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

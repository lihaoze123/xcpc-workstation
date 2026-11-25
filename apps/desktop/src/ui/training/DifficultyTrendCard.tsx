// Solved problem difficulty trend: weekly / monthly AC difficulty P75 & P90 dual-line chart.
// Shares visual language with RatingChartCard (round Y ticks, intermediate X ticks, hover-only dots)
// and matches bottom numeric label colors to legend colors (P90 primary blue, P75 chart-2 green).

import { Canvas, computed, signal, Text, View } from "nsh";
import { t } from "../i18n";
import { cardShadow, token } from "../theme";
import { zhEn } from "./locale";
import { niceRatingTicks, niceXTickIndices, type DifficultyTrendPoint } from "./types";

const CHART_H = 210;
const PAD_L = 44;
const PAD_R = 14;
const PAD_T = 12;
const PAD_B = 22;

export function DifficultyTrendCard(props: { points: readonly DifficultyTrendPoint[] }) {
  const hoveredIdx = signal<number | undefined>(undefined);
  const pinnedIdx = signal<number | undefined>(undefined);
  const chartW = signal(500);

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

  const inspectedIndex = () => {
    const pts = ratedPoints();
    if (pts.length === 0) return undefined;
    const idx = hoveredIdx() ?? pinnedIdx();
    if (idx !== undefined && idx >= 0 && idx < pts.length) return idx;
    return undefined;
  };

  const activePoint = () => {
    const pts = ratedPoints();
    if (pts.length === 0) return undefined;
    const idx = inspectedIndex() ?? pts.length - 1;
    return pts[idx];
  };

  const xOfIndex = (idx: number, count: number, width: number): number => {
    const plotW = Math.max(1, width - PAD_L - PAD_R);
    if (count <= 1) return PAD_L + plotW / 2;
    return PAD_L + (idx / (count - 1)) * plotW;
  };

  const yOfVal = (v: number, minY: number, maxY: number): number => {
    const plotH = CHART_H - PAD_T - PAD_B;
    const ratio = (v - minY) / Math.max(1, maxY - minY);
    return PAD_T + plotH * (1 - Math.max(0, Math.min(1, ratio)));
  };

  const nearestIndex = (x: number): number | undefined => {
    const pts = ratedPoints();
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
        <Canvas
          testID="training-trend-canvas"
          style={{ width: "100%", height: CHART_H }}
          onLayout={(e) => {
            if (e.bounds.size.width > 0) chartW.set(e.bounds.size.width);
          }}
          onMouseMove={(e) => hoveredIdx.set(nearestIndex(e.position.x))}
          onMouseLeave={() => hoveredIdx.set(undefined)}
          onPress={(e) => {
            const idx = nearestIndex(e.position.x);
            if (idx !== undefined) pinnedIdx.set(pinnedIdx() === idx ? undefined : idx);
          }}
          draw={(cx, size) => {
            const pts = ratedPoints();
            const { minY, maxY } = bounds();
            const plotW = Math.max(1, size.width - PAD_L - PAD_R);
            const plotH = CHART_H - PAD_T - PAD_B;
            const borderCol = token("border");
            const mutedText = token("muted-foreground");
            const p90Col = token("primary");
            const p75Col = token("chart-2");
            const cardBg = token("card");

            cx.clearRect(0, 0, size.width, size.height);

            // Clean round horizontal gridlines matching RatingChartCard
            cx.strokeStyle = borderCol;
            cx.lineWidth = 1;
            cx.font = "10.5px system-ui, sans-serif";
            cx.fillStyle = mutedText;
            cx.textAlign = "right";
            cx.textBaseline = "middle";
            const ticks = niceRatingTicks(minY, maxY);
            for (const v of ticks) {
              const y = yOfVal(v, minY, maxY);
              cx.beginPath();
              cx.moveTo(PAD_L, y);
              cx.lineTo(PAD_L + plotW, y);
              cx.stroke();
              cx.fillText(String(v), PAD_L - 6, y);
            }

            const sel = inspectedIndex() ?? (pts.length === 1 ? 0 : undefined);

            const drawSeries = (pick: (p: DifficultyTrendPoint) => number | null | undefined, color: string) => {
              cx.strokeStyle = color;
              cx.lineWidth = 2;
              cx.lineJoin = "round";
              cx.beginPath();
              let started = false;
              pts.forEach((p, i) => {
                const v = pick(p);
                if (v == null) return;
                const x = xOfIndex(i, pts.length, size.width);
                const y = yOfVal(v, minY, maxY);
                if (!started) {
                  cx.moveTo(x, y);
                  started = true;
                } else {
                  cx.lineTo(x, y);
                }
              });
              cx.stroke();

              // Dot only on hover/pin (or single-point chart)
              if (sel !== undefined && sel < pts.length) {
                const v = pick(pts[sel]);
                if (v != null) {
                  const x = xOfIndex(sel, pts.length, size.width);
                  const y = yOfVal(v, minY, maxY);
                  cx.fillStyle = color;
                  cx.beginPath();
                  cx.arc(x, y, 4, 0, Math.PI * 2);
                  cx.fill();
                  cx.strokeStyle = cardBg;
                  cx.lineWidth = 1.5;
                  cx.stroke();
                }
              }
            };

            if (sel !== undefined && sel < pts.length) {
              const x = xOfIndex(sel, pts.length, size.width);
              cx.strokeStyle = borderCol;
              cx.lineWidth = 1;
              cx.beginPath();
              cx.moveTo(x, PAD_T);
              cx.lineTo(x, PAD_T + plotH);
              cx.stroke();
            }

            drawSeries((p) => p.p75, p75Col);
            drawSeries((p) => p.p90, p90Col);

            // Intermediate X-axis ticks matching RatingChartCard
            const maxXTicks = Math.max(2, Math.min(5, Math.floor(plotW / 85)));
            const xIndices = niceXTickIndices(pts.length, maxXTicks);
            cx.fillStyle = mutedText;
            cx.textBaseline = "top";
            xIndices.forEach((idx, pos) => {
              const x = xOfIndex(idx, pts.length, size.width);
              cx.textAlign =
                pos === 0 ? "left" : pos === xIndices.length - 1 ? "right" : "center";
              cx.fillText(pts[idx].period, x, CHART_H - 16);
            });
          }}
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

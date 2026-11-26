// Platform Rating trajectory chart on NSH <Canvas>:
// 3pt vertical tier color strip along the Y-axis (§7), clean round Y-axis ticks covering 3000+,
// intermediate X-axis date ticks, and tier-segmented polyline with dot shown only on hover/pin.

import { Canvas, computed, effect, signal, Text, View } from "nsh";
import { t } from "../i18n";
import { Segmented, TruncText } from "../kit";
import { SectionEmpty } from "../layout";
import { ratingTier } from "../logic";
import { cardShadow, tierColor, token, type Tier } from "../theme";
import {
  niceRatingTicks,
  niceXTickIndices,
  platformDisplayName,
  type PlatformRatingSeries,
  type RatingPoint,
} from "./types";

import { FONT_SIZE, FONT_WEIGHT, RADIUS, SPACE } from "../tokens";

const TIER_STRIP_RANGES: readonly { min: number; max: number; tier: Tier }[] = [
  { min: 0, max: 1200, tier: "newbie" },
  { min: 1200, max: 1400, tier: "pupil" },
  { min: 1400, max: 1600, tier: "specialist" },
  { min: 1600, max: 1900, tier: "expert" },
  { min: 1900, max: 2100, tier: "candidate_master" },
  { min: 2100, max: 2400, tier: "master" },
  { min: 2400, max: 5000, tier: "grandmaster" },
];

const TIER_THRESHOLDS = [1200, 1400, 1600, 1900, 2100, 2400] as const;
function formatUnixDateShort(sec: number): string {
  const d = new Date(sec * 1000);
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
}

function formatUnixYearMonth(sec: number): string {
  const d = new Date(sec * 1000);
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}`;
}

const CHART_H = 210;
const PAD_L = 44;
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

  const inspectedIndex = () => {
    const pts = points();
    if (pts.length === 0) return undefined;
    const idx = hoveredIdx() ?? pinnedIdx();
    if (idx !== undefined && idx >= 0 && idx < pts.length) return idx;
    return undefined;
  };

  const activePoint = () => {
    const pts = points();
    if (pts.length === 0) return undefined;
    const idx = inspectedIndex() ?? pts.length - 1;
    return pts[idx];
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
        borderRadius: RADIUS.lg,
        padding: SPACE[16],
        gap: SPACE[8],
        boxShadow: cardShadow(),
      }}
    >
      <View style={{ flexDirection: "row", alignItems: "center", justifyContent: "space-between", gap: SPACE[8] }}>
        <Text style={{ fontSize: FONT_SIZE.body, fontWeight: FONT_WEIGHT.semibold }} class="text-foreground">
          {t("training.rating.title")}
        </Text>
        <If when={props.series.length > 1}>
          <Segmented
            items={platformItems()}
            value={activeSeries()?.platform ?? ""}
            onChange={(plat) => {
              selectedPlatform.set(plat);
              hoveredIdx.set(undefined);
              pinnedIdx.set(undefined);
            }}
            itemWidth={82}
          />
        </If>
      </View>

      <If
        when={points().length > 0}
        else={
          <SectionEmpty message={t("training.rating.empty")} style={{ height: CHART_H }} />
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
            if (idx !== undefined) pinnedIdx.set(pinnedIdx() === idx ? undefined : idx);
          }}
          draw={(cx, size) => {
            const pts = points();
            const { minY, maxY } = bounds();
            const plotW = Math.max(1, size.width - PAD_L - PAD_R);
            const plotH = CHART_H - PAD_T - PAD_B;
            const borderCol = token("border");
            const mutedText = token("muted-foreground");
            const cardBg = token("card");

            cx.clearRect(0, 0, size.width, size.height);

            // Clean round horizontal gridlines (including 3000+)
            cx.strokeStyle = borderCol;
            cx.lineWidth = 1;
            cx.font = "10.5px system-ui, sans-serif";
            cx.fillStyle = mutedText;
            cx.textAlign = "right";
            cx.textBaseline = "middle";
            const ticks = niceRatingTicks(minY, maxY);
            for (const v of ticks) {
              const y = yOfRating(v, minY, maxY);
              cx.beginPath();
              cx.moveTo(PAD_L, y);
              cx.lineTo(PAD_L + plotW, y);
              cx.stroke();
              cx.fillText(String(v), PAD_L - 8, y);
            }

            // 3pt vertical tier color strip along the left edge of the Y-axis (§7)
            for (const band of TIER_STRIP_RANGES) {
              const bLo = Math.max(minY, band.min);
              const bHi = Math.min(maxY, band.max);
              if (bHi <= bLo) continue;
              const yTop = yOfRating(bHi, minY, maxY);
              const yBot = yOfRating(bLo, minY, maxY);
              cx.fillStyle = tierColor(band.tier);
              cx.fillRect(PAD_L - 3, yTop, 3, Math.max(0, yBot - yTop));
            }

            // Tier-colored segmented polyline (§7: rating color lives on the curve, not background bands)
            if (pts.length > 0) {
              cx.lineWidth = 2;
              cx.lineJoin = "round";
              cx.lineCap = "round";
              for (let i = 0; i < pts.length - 1; i++) {
                const r0 = pts[i].rating_after;
                const r1 = pts[i + 1].rating_after;
                const x0 = xOfIndex(i, pts.length, size.width);
                const x1 = xOfIndex(i + 1, pts.length, size.width);
                const loR = Math.min(r0, r1);
                const hiR = Math.max(r0, r1);
                const splits: number[] = [0];
                if (r1 !== r0) {
                  for (const th of TIER_THRESHOLDS) {
                    if (th > loR && th < hiR) {
                      splits.push((th - r0) / (r1 - r0));
                    }
                  }
                  splits.sort((a, b) => a - b);
                }
                splits.push(1);
                for (let k = 0; k < splits.length - 1; k++) {
                  const tA = splits[k];
                  const tB = splits[k + 1];
                  const rA = r0 + (r1 - r0) * tA;
                  const rB = r0 + (r1 - r0) * tB;
                  const rMid = (rA + rB) / 2;
                  cx.strokeStyle = tierColor(ratingTier(rMid));
                  cx.beginPath();
                  cx.moveTo(x0 + (x1 - x0) * tA, yOfRating(rA, minY, maxY));
                  cx.lineTo(x0 + (x1 - x0) * tB, yOfRating(rB, minY, maxY));
                  cx.stroke();
                }
              }

              // Dot ONLY on hover/pin (or single-point chart)
              const sel = inspectedIndex() ?? (pts.length === 1 ? 0 : undefined);
              if (sel !== undefined && sel < pts.length) {
                const p = pts[sel];
                const x = xOfIndex(sel, pts.length, size.width);
                const y = yOfRating(p.rating_after, minY, maxY);

                cx.strokeStyle = borderCol;
                cx.lineWidth = 1;
                cx.beginPath();
                cx.moveTo(x, PAD_T);
                cx.lineTo(x, PAD_T + plotH);
                cx.stroke();

                cx.fillStyle = tierColor(ratingTier(p.rating_after));
                cx.beginPath();
                cx.arc(x, y, 4.5, 0, Math.PI * 2);
                cx.fill();
                cx.strokeStyle = cardBg;
                cx.lineWidth = 1.5;
                cx.stroke();
              }

              // Intermediate X-axis date ticks
              const spanSec = pts.length > 1 ? pts[pts.length - 1].at - pts[0].at : 0;
              const useYearMonth = spanSec > 180 * 86_400;
              const maxXTicks = Math.max(2, Math.min(5, Math.floor(plotW / 85)));
              const xIndices = niceXTickIndices(pts.length, maxXTicks);
              cx.fillStyle = mutedText;
              cx.textBaseline = "top";
              xIndices.forEach((idx, pos) => {
                const x = xOfIndex(idx, pts.length, size.width);
                const label = useYearMonth
                  ? formatUnixYearMonth(pts[idx].at)
                  : formatUnixDateShort(pts[idx].at);
                cx.textAlign =
                  pos === 0 ? "left" : pos === xIndices.length - 1 ? "right" : "center";
                cx.fillText(label, x, CHART_H - 16);
              });
            }
          }}
        />

        {/* Selected / latest point detail */}
        <If when={activePoint()}>
          {(pt) => {
            const delta = () => pt().delta;
            return (
              <View
                testID="training-rating-detail"
                style={{ flexDirection: "row", alignItems: "center", justifyContent: "space-between", gap: SPACE[8] }}
              >
                <TruncText
                  style={{ fontSize: FONT_SIZE.caption, flex: 1, minWidth: 0, numberOfLines: 1, textOverflow: "ellipsis" }}
                  class="text-secondary-foreground"
                >
                  {pt().contest_name}
                </TruncText>
                <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[8], flexShrink: 0 }}>
                  <If when={pt().rank != null}>
                    <Text style={{ fontSize: FONT_SIZE.caption, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
                      {`#${pt().rank}`}
                    </Text>
                  </If>
                  <Text
                    style={{
                      fontSize: FONT_SIZE.caption,
                      fontWeight: FONT_WEIGHT.semibold,
                      fontVariant: ["tabular-nums"],
                      color: tierColor(ratingTier(pt().rating_after)),
                    }}
                  >
                    {String(pt().rating_after)}
                  </Text>
                  <If when={delta() != null}>
                    <Text
                      style={{ fontSize: FONT_SIZE.caption, fontWeight: FONT_WEIGHT.semibold, fontVariant: ["tabular-nums"] }}
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

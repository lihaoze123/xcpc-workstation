// Dual-mode annual training heatmap:
// Switches between daily distinct AC count and daily max solved difficulty,
// uses host Segmented controls for mode & year switching, localized Chinese/English labels,
// and fills the full card width cleanly with aligned first column.

import { Canvas, colorScheme, computed, signal, Text, View } from "nsh";
import { t } from "../i18n";
import { Segmented } from "../kit";
import { ratingTier } from "../logic";
import { cardShadow, tierColor, token } from "../theme";
import { isEn } from "./locale";
import {
  buildYearHeatmapGrid,
  heatmapCellColor,
  type HeatmapCell,
  type HeatmapGridDay,
  type HeatmapMode,
} from "./types";

const MONTH_LABELS_ZH = ["1月", "2月", "3月", "4月", "5月", "6月", "7月", "8月", "9月", "10月", "11月", "12月"];
const MONTH_LABELS_EN = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
const WEEKDAY_LABELS_ZH = [
  [1, "一"],
  [3, "三"],
  [5, "五"],
] as const;
const WEEKDAY_LABELS_EN = [
  [1, "M"],
  [3, "W"],
  [5, "F"],
] as const;

const LEFT_LABEL_W = 22;
const RIGHT_PAD = 2;
const TOP_LABEL_H = 18;
const CELL_GAP = 3;

function computeGridGeometry(width: number, weeks: number) {
  const safeWeeks = Math.max(1, weeks);
  const availW = Math.max(200, width - LEFT_LABEL_W - RIGHT_PAD);
  const step = (availW + CELL_GAP) / safeWeeks;
  const cellSize = Math.max(6, Math.min(18, step - CELL_GAP));
  const actualStep = cellSize + CELL_GAP;
  const height = Math.ceil(TOP_LABEL_H + 7 * actualStep - CELL_GAP + 4);
  return { cellSize, step: actualStep, height };
}

export function HeatmapCard(props: {
  year: number;
  availableYears: readonly number[];
  cells: readonly HeatmapCell[];
  onYearChange: (year: number) => void;
}) {
  const mode = signal<HeatmapMode>("count");
  const hovered = signal<HeatmapGridDay | undefined>(undefined);
  const pinned = signal<HeatmapGridDay | undefined>(undefined);
  const canvasWidth = signal(720);

  const grid = computed(() => buildYearHeatmapGrid(props.year, props.cells));
  const activeDay = () => hovered() ?? pinned();

  const cellMetrics = computed(() => computeGridGeometry(canvasWidth(), grid().weeks));

  const dayLookup = computed(() => {
    const map = new Map<string, HeatmapGridDay>();
    for (const d of grid().days) map.set(`${d.weekIndex}:${d.dayOfWeek}`, d);
    return map;
  });

  const hitDay = (x: number, y: number): HeatmapGridDay | undefined => {
    const { step } = cellMetrics();
    const gx = x - LEFT_LABEL_W;
    const gy = y - TOP_LABEL_H;
    if (gx < 0 || gy < 0) return undefined;
    const w = Math.floor(gx / step);
    const d = Math.floor(gy / step);
    if (d < 0 || d > 6) return undefined;
    return dayLookup().get(`${w}:${d}`);
  };

  const totalAcYear = computed(() => props.cells.reduce((acc, c) => acc + c.ac_count, 0));
  const activeDaysYear = computed(() => props.cells.filter((c) => c.ac_count > 0).length);

  const modeItems = computed(() => [
    { value: "count" as const, label: t("training.heatmap.modeCount"), testID: "heatmap-mode-count" },
    { value: "rating" as const, label: t("training.heatmap.modeRating"), testID: "heatmap-mode-rating" },
  ]);

  const yearItems = computed(() =>
    props.availableYears.slice(0, 4).map((y) => ({
      value: String(y),
      label: String(y),
      testID: `heatmap-year-${y}`,
    })),
  );

  return (
    <View
      testID="training-heatmap-card"
      class="bg-card border border-border"
      style={{
        borderRadius: 10,
        padding: 16,
        gap: 12,
        boxShadow: cardShadow(),
      }}
    >
      {/* Header: title + year summary + host Segmented mode & year switchers */}
      <View style={{ flexDirection: "row", alignItems: "center", justifyContent: "space-between", flexWrap: "wrap", gap: 10 }}>
        <View style={{ flexDirection: "row", alignItems: "baseline", gap: 10 }}>
          <Text style={{ fontSize: 13.5, fontWeight: 600 }} class="text-foreground">
            {t("training.heatmap.title")}
          </Text>
          <Text style={{ fontSize: 12, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
            {t("training.heatmap.yearSummary", { ac: totalAcYear(), days: activeDaysYear() })}
          </Text>
        </View>
        <View style={{ flexDirection: "row", alignItems: "center", gap: 8 }}>
          <Segmented
            items={modeItems()}
            value={mode()}
            onChange={(m) => mode.set(m)}
            itemWidth={88}
          />
          <If when={yearItems().length > 1}>
            <Segmented
              items={yearItems()}
              value={String(props.year)}
              onChange={(yr) => {
                pinned.set(undefined);
                props.onYearChange(Number(yr));
              }}
              itemWidth={50}
            />
          </If>
        </View>
      </View>

      {/* Canvas grid */}
      <Canvas
        testID="training-heatmap-canvas"
        style={{ width: "100%", height: cellMetrics().height }}
        onLayout={(e) => {
          if (e.bounds.size.width > 0 && Math.abs(e.bounds.size.width - canvasWidth()) > 1) {
            canvasWidth.set(e.bounds.size.width);
          }
        }}
        onMouseMove={(e) => hovered.set(hitDay(e.position.x, e.position.y))}
        onMouseLeave={() => hovered.set(undefined)}
        onPress={(e) => {
          const hit = hitDay(e.position.x, e.position.y);
          if (hit) pinned.set(pinned()?.date === hit.date ? undefined : hit);
        }}
        draw={(cx, size) => {
          const dark = colorScheme() === "dark";
          const en = isEn();
          const g = grid();
          const curMode = mode();
          const sel = activeDay();
          const { cellSize, step } = computeGridGeometry(size.width, g.weeks);
          const mutedText = token("muted-foreground");
          const fg = token("foreground");
          const monthLabels = en ? MONTH_LABELS_EN : MONTH_LABELS_ZH;
          const weekdayLabels = en ? WEEKDAY_LABELS_EN : WEEKDAY_LABELS_ZH;

          cx.clearRect(0, 0, size.width, size.height);
          cx.font = "10px system-ui, sans-serif";
          cx.fillStyle = mutedText;
          cx.textBaseline = "top";
          cx.textAlign = "left";

          // Month labels aligned to column start
          let prevX = -999;
          for (const m of g.monthStarts) {
            const mx = LEFT_LABEL_W + m.weekIndex * step;
            if (mx - prevX >= 28 && mx < size.width - 18) {
              cx.fillText(monthLabels[m.month - 1] ?? "", mx, 2);
              prevX = mx;
            }
          }

          // Weekday labels (Mon / Wed / Fri)
          cx.textBaseline = "middle";
          cx.textAlign = "left";
          for (const [dow, label] of weekdayLabels) {
            const my = TOP_LABEL_H + dow * step + cellSize / 2;
            cx.fillText(label, 2, my);
          }

          // Cells filling full width cleanly
          const radius = Math.min(3, Math.max(1.5, cellSize * 0.22));
          for (const d of g.days) {
            const x = LEFT_LABEL_W + d.weekIndex * step;
            const y = TOP_LABEL_H + d.dayOfWeek * step;
            cx.fillStyle = heatmapCellColor(d, curMode, dark);
            cx.beginPath();
            cx.roundRect(x, y, cellSize, cellSize, radius);
            cx.fill();

            if (sel && sel.date === d.date) {
              cx.strokeStyle = fg;
              cx.lineWidth = 1.5;
              cx.beginPath();
              cx.roundRect(x - 0.5, y - 0.5, cellSize + 1, cellSize + 1, radius);
              cx.stroke();
            }
          }
        }}
      />

      {/* Bottom detail strip (hovered/pinned day readout + compact legend) */}
      <View style={{ flexDirection: "row", alignItems: "center", justifyContent: "space-between", gap: 12, minHeight: 20 }}>
        <If
          when={activeDay()}
          else={
            <Text style={{ fontSize: 12 }} class="text-muted-foreground">
              {t("training.heatmap.hoverHint")}
            </Text>
          }
        >
          {(day) => (
            <View testID="training-heatmap-detail" style={{ flexDirection: "row", alignItems: "center", gap: 10 }}>
              <Text style={{ fontSize: 12, fontWeight: 500, fontVariant: ["tabular-nums"] }} class="text-foreground">
                {day().date}
              </Text>
              <Text style={{ fontSize: 12, fontVariant: ["tabular-nums"] }} class="text-secondary-foreground">
                {t("training.heatmap.dayAc", { count: day().acCount })}
              </Text>
              <Text style={{ fontSize: 12, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
                {t("training.heatmap.daySubs", { count: day().submissionCount })}
              </Text>
              <If when={day().maxRating != null}>
                <Text
                  style={{
                    fontSize: 12,
                    fontWeight: 500,
                    fontVariant: ["tabular-nums"],
                    color: tierColor(ratingTier(day().maxRating)),
                  }}
                >
                  {t("training.heatmap.dayMaxRating", { rating: day().maxRating! })}
                </Text>
              </If>
            </View>
          )}
        </If>

        {/* Legend */}
        <View style={{ flexDirection: "row", alignItems: "center", gap: 4 }}>
          <Text style={{ fontSize: 11, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
            {mode() === "count" ? t("training.heatmap.less") : "800"}
          </Text>
          <Each
            of={
              mode() === "count"
                ? [0, 1, 2, 4, 6]
                : [1000, 1300, 1500, 1700, 2000, 2200, 2500]
            }
            key={(v) => String(v)}
          >
            {(sample) => {
              const fakeDay: HeatmapGridDay = {
                date: "",
                weekIndex: 0,
                dayOfWeek: 0,
                month: 1,
                acCount: mode() === "count" ? sample() : 1,
                submissionCount: 1,
                maxRating: mode() === "rating" ? sample() : null,
              };
              return (
                <View
                  style={{
                    width: 10,
                    height: 10,
                    borderRadius: 2.5,
                    backgroundColor: heatmapCellColor(fakeDay, mode(), colorScheme() === "dark"),
                  }}
                />
              );
            }}
          </Each>
          <Text style={{ fontSize: 11, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
            {mode() === "count" ? t("training.heatmap.more") : "2400+"}
          </Text>
        </View>
      </View>
    </View>
  );
}

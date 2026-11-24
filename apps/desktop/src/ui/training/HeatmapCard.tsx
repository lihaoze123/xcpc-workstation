// Dual-mode annual training heatmap (cptracker style):
// Switches between daily distinct AC count and daily max solved difficulty,
// supports year selection, and shows exact per-day details on hover or click.

import { Canvas, colorScheme, computed, signal, Text, View } from "nsh";
import { t } from "../i18n";
import { ratingTier } from "../logic";
import { cardShadow, tierColor, token } from "../theme";
import {
  buildYearHeatmapGrid,
  heatmapCellColor,
  type HeatmapCell,
  type HeatmapGridDay,
  type HeatmapMode,
} from "./types";

const MONTH_LABELS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
const LEFT_LABEL_W = 26;
const TOP_LABEL_H = 18;
const CELL_GAP = 3;

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

  const cellMetrics = computed(() => {
    const g = grid();
    const availW = Math.max(200, canvasWidth() - LEFT_LABEL_W - 6);
    const cellSize = Math.max(8, Math.min(14, Math.floor((availW - (g.weeks - 1) * CELL_GAP) / g.weeks)));
    const step = cellSize + CELL_GAP;
    const height = TOP_LABEL_H + 7 * step + 4;
    return { cellSize, step, height };
  });

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

  const modeBtn = (m: HeatmapMode, label: string, testID: string) => {
    const on = () => mode() === m;
    return (
      <View
        testID={testID}
        onPress={() => mode.set(m)}
        class={on() ? "bg-card border border-border" : "hover:bg-background/60"}
        style={{
          height: 24,
          paddingLeft: 9,
          paddingRight: 9,
          borderRadius: 6,
          justifyContent: "center",
        }}
      >
        <Text
          style={{ fontSize: 12, fontWeight: on() ? 500 : 400 }}
          class={on() ? "text-foreground" : "text-secondary-foreground"}
        >
          {label}
        </Text>
      </View>
    );
  };

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
      {/* Header: title + year summary + mode switcher + year selector */}
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
          <View
            class="bg-muted border border-border"
            style={{ flexDirection: "row", padding: 2, borderRadius: 8, gap: 2 }}
          >
            {modeBtn("count", t("training.heatmap.modeCount"), "heatmap-mode-count")}
            {modeBtn("rating", t("training.heatmap.modeRating"), "heatmap-mode-rating")}
          </View>
          <View style={{ flexDirection: "row", alignItems: "center", gap: 4 }}>
            <Each of={props.availableYears.slice(0, 5)} key={(y) => String(y)}>
              {(yr) => {
                const active = () => yr() === props.year;
                return (
                  <View
                    testID={`heatmap-year-${yr()}`}
                    onPress={() => {
                      pinned.set(undefined);
                      props.onYearChange(yr());
                    }}
                    class={active() ? "bg-accent border border-border" : "hover:bg-muted"}
                    style={{
                      height: 26,
                      paddingLeft: 8,
                      paddingRight: 8,
                      borderRadius: 6,
                      justifyContent: "center",
                    }}
                  >
                    <Text
                      style={{ fontSize: 12, fontVariant: ["tabular-nums"], fontWeight: active() ? 600 : 400 }}
                      class={active() ? "text-foreground" : "text-secondary-foreground"}
                    >
                      {String(yr())}
                    </Text>
                  </View>
                );
              }}
            </Each>
          </View>
        </View>
      </View>

      {/* Canvas grid */}
      <Canvas
        testID="training-heatmap-canvas"
        style={{ width: "100%", height: cellMetrics().height }}
        onLayout={(e) => {
          if (e.bounds.size.width > 0) canvasWidth.set(e.bounds.size.width);
        }}
        onMouseMove={(e) => hovered.set(hitDay(e.position.x, e.position.y))}
        onMouseLeave={() => hovered.set(undefined)}
        onPress={(e) => {
          const hit = hitDay(e.position.x, e.position.y);
          if (hit) pinned.set(pinned()?.date === hit.date ? undefined : hit);
        }}
        draw={(cx, size) => {
          const dark = colorScheme() === "dark";
          const g = grid();
          const curMode = mode();
          const sel = activeDay();
          const { cellSize, step } = cellMetrics();
          const mutedText = token("muted-foreground");
          const fg = token("foreground");

          cx.clearRect(0, 0, size.width, size.height);
          cx.font = "10px system-ui, sans-serif";
          cx.fillStyle = mutedText;
          cx.textBaseline = "top";
          cx.textAlign = "left";

          // Month labels
          let prevX = -999;
          for (const m of g.monthStarts) {
            const mx = LEFT_LABEL_W + m.weekIndex * step;
            if (mx - prevX >= 28) {
              cx.fillText(MONTH_LABELS[m.month - 1] ?? "", mx, 2);
              prevX = mx;
            }
          }

          // Weekday labels (Mon / Wed / Fri)
          cx.textBaseline = "middle";
          for (const [dow, label] of [
            [1, "M"],
            [3, "W"],
            [5, "F"],
          ] as const) {
            const my = TOP_LABEL_H + dow * step + cellSize / 2;
            cx.fillText(label, 4, my);
          }

          // Cells
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
          <Text style={{ fontSize: 11 }} class="text-muted-foreground">
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
          <Text style={{ fontSize: 11 }} class="text-muted-foreground">
            {mode() === "count" ? t("training.heatmap.more") : "2400+"}
          </Text>
        </View>
      </View>
    </View>
  );
}

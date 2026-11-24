// Isolated matrix cell component (Phase 3): subscribes ONLY to its own per-cell
// signal from `MatrixCellStore` so streaming test-case updates never re-render
// untouched cells. Adds a subtle GPU-composited settle pulse when a cell arrives live.

import { reduceMotion, Text, View } from "nsh";
import { colorTransition } from "../motion";
import {
  cellTimeMs,
  cellVisualStyle,
  formatScoreRatioPercent,
  isMatrixCellAnomaly,
  type MatrixCellData,
  type SolutionHeaderSpec,
} from "./buildLogic";
import type { MatrixCellStore } from "./matrixStore";

export function MatrixCellView(props: {
  store: MatrixCellStore;
  testIndex: () => number;
  subtask: () => string | null | undefined;
  spec: () => SolutionHeaderSpec;
  matched: () => boolean | undefined;
  isPicked: () => boolean;
  onPick: (cell: MatrixCellData) => void;
}) {
  const cell = () => props.store.cell(props.testIndex(), props.spec().name)();
  const isFresh = () => props.store.fresh(props.testIndex(), props.spec().name)();

  const isAnom = () => {
    const c = cell();
    if (!c) return false;
    return isMatrixCellAnomaly(c.verdict, props.spec(), props.subtask(), props.matched());
  };

  return (
    <View
      onPress={() => {
        const c = cell();
        if (c) props.onPick(c);
      }}
      style={{
        flex: 1,
        minWidth: 120,
        maxWidth: 180,
        paddingRight: 10,
        justifyContent: "center",
      }}
    >
      <If
        when={cell()}
        else={
          <Text style={{ fontSize: 12, paddingLeft: 6 }} class="text-muted-foreground">
            {"-"}
          </Text>
        }
      >
        {(c) => {
          const st = () => cellVisualStyle(c().verdict, isAnom());
          const partial = () => formatScoreRatioPercent(c().score_ratio, false);
          return (
            <View
              class={`${st().cellClass} ${
                props.isPicked()
                  ? "ring-2 ring-inset ring-primary"
                  : isFresh() && !reduceMotion()
                    ? "ring-1 ring-inset ring-primary/45"
                    : ""
              }`}
              style={{
                height: 26,
                paddingLeft: 7,
                paddingRight: 7,
                borderRadius: 5,
                opacity: c().stale ? 0.5 : 1,
                flexDirection: "row",
                alignItems: "center",
                justifyContent: "space-between",
                gap: 6,
                ...colorTransition(160),
              }}
            >
              <Text
                style={{
                  fontSize: 12,
                  fontWeight: isAnom() ? 600 : 400,
                  fontVariant: ["tabular-nums"],
                }}
                class={st().textClass}
              >
                {partial() ? `${c().verdict} ${partial()!}` : c().verdict}
              </Text>
              <Text
                style={{
                  fontSize: 12,
                  fontVariant: ["tabular-nums"],
                  textAlign: "right",
                }}
                class={isAnom() ? "text-destructive" : "text-muted-foreground"}
              >
                {`${cellTimeMs(c())} ms`}
              </Text>
            </View>
          );
        }}
      </If>
    </View>
  );
}

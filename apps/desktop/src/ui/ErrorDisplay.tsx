// Every workspace error the views show (design-system.md §7.3; Phase 2.5 seam #3):
// what happened in plain words → a place to click → the next step only when there is
// one. A rule's refusal (`contest_frozen`, `problem_referenced`, `read_only`) is a fact
// in secondary text with its way out, not a red error. The Core's code and English text
// never show; 复制详情 puts them on the clipboard. The slot keeps its height whether or
// not there is an error, so nothing below it moves.

import { cleanup, Clipboard, signal, Text, View, type JSX } from "nsh";
import { Icon } from "./Icon";
import { errorCopy, errorDetails } from "./logic";
import { EASE_STANDARD, ms } from "./theme";
import type { UiError } from "./store";

export function ErrorDisplay(props: {
  error: UiError | undefined;
  /** Where to look: a label to press (a file and line, a page, a contest). */
  locate?: { label: string; onPress: () => void }[];
  /** The way out of a rule's refusal (重新编辑, 打开比赛). */
  action?: JSX.Element;
  /** Reserve the slot's height while there is no error. */
  slot?: boolean;
  align?: "start" | "center";
  /** Optional dismiss handler: shows a quiet close button when set. */
  onDismiss?: () => void;
}) {
  const copied = signal(false);
  let copyTimer: number | undefined;
  cleanup(() => clearTimeout(copyTimer));
  const copy = () => {
    const e = props.error;
    if (!e) return;
    const refs = e.referencing?.length ? `referencing: ${e.referencing.join(", ")}` : undefined;
    void Clipboard.writeText([errorDetails(e.code, e.error, e.context), refs].filter(Boolean).join("\n")).then(() => {
      copied.set(true);
      clearTimeout(copyTimer);
      copyTimer = setTimeout(() => copied.set(false), 1200);
    });
  };
  const words = () => (props.error ? errorCopy(props.error.code, props.error.error) : undefined);
  return (
    <View
      style={{
        minHeight: props.slot ? 20 : 0,
        flexDirection: "row",
        flexWrap: "wrap",
        alignItems: "center",
        justifyContent: props.align === "center" ? "center" : "flex-start",
        columnGap: 8,
        rowGap: 2,
      }}
    >
      <If when={words()}>
        {(w) => (
          <>
            <Text style={{ fontSize: 12 }} class={w().rule ? "text-secondary-foreground" : "text-destructive"}>
              {props.error?.context && !w().rule ? `${props.error.context}：${w().message}` : w().message}
            </Text>
            <Each of={props.locate ?? []} key={(l, i) => `${i}:${l.label}`}>
              {(l) => (
                <Text style={{ fontSize: 12, textDecorationLine: "underline" }} class="text-primary" onPress={() => l().onPress()}>
                  {l().label}
                </Text>
              )}
            </Each>
            {w().next && (
              <Text style={{ fontSize: 12 }} class="text-secondary-foreground">
                {w().next}
              </Text>
            )}
            {props.action}
            {(props.error?.code || props.error?.error) && (
              <Text style={{ fontSize: 11.5 }} class="text-muted-foreground hover:text-secondary-foreground" onPress={copy}>
                {copied() ? "已复制" : "复制详情"}
              </Text>
            )}
            {props.onDismiss && (
              <View
                onPress={() => props.onDismiss!()}
                class="hover:bg-muted active:bg-accent focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-ring"
                style={{
                  width: 18,
                  height: 18,
                  borderRadius: 4,
                  alignItems: "center",
                  justifyContent: "center",
                  transitionProperty: "colors",
                  transitionDuration: ms(120),
                  transitionTimingFunction: EASE_STANDARD,
                }}
              >
                <Icon name="x" size={12} tone="muted-foreground" />
              </View>
            )}
          </>
        )}
      </If>
    </View>
  );
}

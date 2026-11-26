// One status vocabulary for authoring (authoring-ui-refactor §3):
// - `StatusMark`: a fixed 14pt slot — gray hollow circle (not checked), red dot (has issues),
//   a very faint check (passed), a spinner (checking), nothing (statement-only problem).
// - `StatusSlot`: mark + one sentence at a fixed width; pressing it opens the status popover,
//   which lists each problem in plain words with 「前往」, a quiet 「提醒」 section for
//   warnings, an optional way out (「去设置」) and 「复制详情」. Errors land here instead of
//   in a banner, so nothing around it moves.

import { cleanup, Clipboard, Scroll, signal, Text, View } from "nsh";
import { Popover } from "nsh/base";
import { t } from "../i18n";
import { Icon } from "../Icon";
import { DelayedSpinner, FOCUS_RING } from "../kit";
import {
  anchorOrigin,
  colorTransition,
  overlayTransform,
  overlayTransition,
  popupEndingStyle,
  popupStartingStyle,
  type AnchorPlacement,
} from "../tokens";
import { popoverShadow } from "../theme";

import { CONTROL, FONT_SIZE, FONT_WEIGHT, LINE_HEIGHT, RADIUS, SPACE } from "../tokens";
export type StatusMarkKind = "unchecked" | "issues" | "passed" | "running" | "none";

export interface StatusItem {
  id: string;
  /** What is wrong, in plain words. */
  text: string;
  /** Where (a contest problem: 「B 区间求和」). */
  where?: string;
  /** Goes to the object to fix. */
  onGo?: () => void;
}

export interface StatusModel {
  mark: StatusMarkKind;
  /** One sentence next to the mark: 「2 题有问题」「已通过」「没有找到 C++ 编译器」. */
  label: string;
  items: StatusItem[];
  /** Warning-level notes: they never change the mark. */
  reminders: StatusItem[];
  /** The way out when there is exactly one (「去设置」). */
  action?: { label: string; run: () => void };
  /** What 复制详情 copies (codes and raw messages); hidden when absent. */
  details?: string;
}

export function StatusMark(props: { kind: StatusMarkKind; emphasis?: "slot" | "list"; testID?: string }) {
  return (
    <View
      testID={props.testID}
      style={{ width: 14, height: 14, alignItems: "center", justifyContent: "center", flexShrink: 0 }}
    >
      <If when={props.kind === "unchecked"}>
        <View style={{ width: 8, height: 8, borderRadius: RADIUS.xs, borderWidth: 1.5 }} class="border-muted-foreground" />
      </If>
      <If when={props.kind === "issues"}>
        <View style={{ width: 7, height: 7, borderRadius: RADIUS.xs }} class="bg-destructive" />
      </If>
      <If when={props.kind === "passed"}>
        <Icon
          name="check"
          size="sm"
          tone={props.emphasis === "slot" ? "chart-2" : "muted-foreground"}
          style={props.emphasis === "slot" ? undefined : { opacity: 0.6 }}
        />
      </If>
      <If when={props.kind === "running"}>
        <DelayedSpinner active size={11} class="border-muted-foreground" />
      </If>
    </View>
  );
}

export function StatusSlot(props: {
  model: StatusModel;
  /** Fixed width so a longer sentence never pushes the buttons beside it. */
  width?: number;
  open?: boolean;
  onOpenChange?: (open: boolean) => void;
  testID?: string;
}) {
  const ownOpen = signal(false);
  const isOpen = () => props.open ?? ownOpen();
  const setOpen = (v: boolean) => {
    if (props.onOpenChange) props.onOpenChange(v);
    else ownOpen.set(v);
  };
  const copied = signal(false);
  let copyTimer: number | undefined;
  cleanup(() => clearTimeout(copyTimer));
  const copy = () => {
    const d = props.model.details;
    if (!d) return;
    void Clipboard.writeText(d).then(() => {
      copied.set(true);
      clearTimeout(copyTimer);
      copyTimer = setTimeout(() => copied.set(false), 1200);
    });
  };
  const hasBody = () =>
    props.model.items.length > 0 || props.model.reminders.length > 0 || !!props.model.action || !!props.model.details;

  const actualPlacement = signal<AnchorPlacement>("bottom");

  return (
    <Popover
      open={isOpen() && hasBody()}
      onOpenChange={(e) => setOpen(e.open)}
      placement="bottom"
      align="start"
      offset={6}
      onPosition={(e) => actualPlacement.set(e.placement)}
      onClose={() => actualPlacement.set("bottom")}
      popupClass="bg-popover border border-border"
      popupStyle={{
        width: 340,
        maxHeight: 380,
        borderRadius: RADIUS.lg,
        boxShadow: popoverShadow(),
        transformOrigin: anchorOrigin(actualPlacement(), "start"),
        opacity: 1,
        transform: overlayTransform(true),
        ...overlayTransition(true),
      }}
      popupStartingStyle={popupStartingStyle()}
      popupEndingStyle={popupEndingStyle()}
      trigger={
        <View
          testID={props.testID ?? "status-slot"}
          style={{
            width: props.width ?? 168,
            height: CONTROL.sm,
            paddingLeft: SPACE[6],
            paddingRight: SPACE[8],
            borderRadius: RADIUS.sm,
            flexDirection: "row",
            alignItems: "center",
            gap: SPACE[6],
            flexShrink: 0,
            ...colorTransition(),
          }}
          class={
            hasBody()
              ? `hover:bg-muted active:bg-accent ${FOCUS_RING}`
              : ""
          }
        >
          <StatusMark kind={props.model.mark} emphasis="slot" />
          <Text
            style={{ flex: 1, minWidth: 0, fontSize: FONT_SIZE.body, numberOfLines: 1, textOverflow: "ellipsis", fontVariant: ["tabular-nums"] }}
            class={props.model.mark === "issues" ? "text-foreground" : "text-secondary-foreground"}
          >
            {props.model.label}
          </Text>
        </View>
      }
    >
      <Scroll style={{ maxHeight: 378 }} contentStyle={{ padding: SPACE[6], gap: SPACE[2] }}>
        <Each of={props.model.items} key={(it) => it.id}>
          {(it) => <StatusRow item={it()} tone="issue" onGo={() => setOpen(false)} />}
        </Each>
        <If when={props.model.reminders.length > 0}>
          <Text
            style={{ fontSize: FONT_SIZE.caption, paddingLeft: SPACE[8], paddingTop: props.model.items.length > 0 ? SPACE[8] : SPACE[4], paddingBottom: SPACE[2] }}
            class="text-muted-foreground"
          >
            {t("status.reminders")}
          </Text>
          <Each of={props.model.reminders} key={(it) => it.id}>
            {(it) => <StatusRow item={it()} tone="reminder" onGo={() => setOpen(false)} />}
          </Each>
        </If>
        <If when={props.model.action || props.model.details}>
          <View
            style={{
              flexDirection: "row",
              alignItems: "center",
              justifyContent: "space-between",
              gap: SPACE[8],
              paddingLeft: SPACE[8],
              paddingRight: SPACE[8],
              paddingTop: SPACE[8],
              paddingBottom: SPACE[4],
            }}
          >
            <If when={props.model.action} else={<View />}>
              {(a) => (
                <Text
                  testID="status-action"
                  onPress={() => {
                    setOpen(false);
                    a().run();
                  }}
                  style={{ fontSize: FONT_SIZE.body, fontWeight: FONT_WEIGHT.semibold }}
                  class={`text-primary ${FOCUS_RING}`}
                >
                  {a().label}
                </Text>
              )}
            </If>
            <If when={props.model.details}>
              <Text testID="status-copy-details" onPress={copy} style={{ fontSize: FONT_SIZE.caption }} class={`text-muted-foreground hover:text-secondary-foreground ${FOCUS_RING}`}>
                {copied() ? t("common.copied") : t("common.copyDetails")}
              </Text>
            </If>
          </View>
        </If>
      </Scroll>
    </Popover>
  );
}

function StatusRow(props: { item: StatusItem; tone: "issue" | "reminder"; onGo: () => void }) {
  return (
    <View
      style={{
        flexDirection: "row",
        alignItems: "flex-start",
        gap: SPACE[8],
        paddingTop: SPACE[6],
        paddingBottom: SPACE[6],
        paddingLeft: SPACE[8],
        paddingRight: SPACE[8],
        borderRadius: RADIUS.sm,
      }}
    >
      <View style={{ paddingTop: SPACE[4] }}>
        <View
          style={{ width: 6, height: 6, borderRadius: RADIUS.xs }}
          class={props.tone === "issue" ? "bg-destructive" : "bg-muted-foreground"}
        />
      </View>
      <View style={{ flex: 1, minWidth: 0, gap: SPACE[2] }}>
        <Text style={{ fontSize: FONT_SIZE.body, lineHeight: LINE_HEIGHT.body }} class={props.tone === "issue" ? "text-foreground" : "text-secondary-foreground"}>
          {props.item.text}
        </Text>
        <If when={props.item.where}>
          <Text style={{ fontSize: FONT_SIZE.caption }} class="text-muted-foreground">
            {props.item.where}
          </Text>
        </If>
      </View>
      <If when={props.item.onGo}>
        {(go) => (
          <Text
            onPress={() => {
              props.onGo();
              go()();
            }}
            style={{ fontSize: FONT_SIZE.caption, fontWeight: FONT_WEIGHT.semibold, paddingTop: SPACE[2] }}
            class={`text-primary ${FOCUS_RING}`}
          >
            {t("status.go")}
          </Text>
        )}
      </If>
    </View>
  );
}

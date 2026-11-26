// The contest's outline in the studio: the cover and each problem `A..Z` with the page
// it starts on (the compiled book's, `tabular-nums`), scrolled inside a native Scroll view,
// dragged to reorder (labels follow the order at once; the order stays as dropped until
// `composeContest` lands), and at the foot the difficulty bar and tag counts.

import { computed, effect, Scroll, signal, Text, View } from "nsh";
import type { ContestDetailJs, PreviewStart } from "xcpc-workstation/native";
import { ContestStats } from "../Contest";
import { t } from "../i18n";
import { Icon } from "../Icon";
import { FOCUS_RING, tip } from "../kit";
import { labels, moveItem } from "../logic";
import { EASE_STANDARD, ms, popoverShadow } from "../theme";
import { createReorder } from "./reorder";
import { StatusMark, type StatusMarkKind } from "./StatusSlot";

import { CONTROL, FONT_SIZE, FONT_WEIGHT, RADIUS, SPACE } from "../tokens";
export const OUTLINE_WIDTH = 232;
const ROW_GAP = 1;

export function Outline(props: {
  detail: ContestDetailJs;
  starts: readonly PreviewStart[];
  current: string | undefined;
  coverActive?: boolean;
  readOnly?: boolean;
  /** The problem's status mark (§3.1); none when absent. */
  markFor?: (problemId: string) => StatusMarkKind;
  onSelect: (label: string) => void;
  onCover: () => void;
  onReorder: (ids: string[]) => boolean;
  onPickProblems?: () => void;
}) {
  // The order as dropped, until the contest reads back the same.
  const pending = signal<string[] | undefined>(undefined);
  effect(() => {
    const p = pending();
    if (p && props.detail.items.map((i) => i.id).join() === p.join()) queueMicrotask(() => pending.set(undefined));
  });

  const items = computed(() => {
    const base = props.detail.items;
    const p = pending();
    const ordered = p ? p.map((id) => base.find((i) => i.id === id)).filter((x) => x !== undefined) : base;
    const ls = labels(ordered.length);
    return ordered.map((item, i) => ({ item, label: ls[i] }));
  });

  const r = createReorder((from, to) => {
    const ids = moveItem(
      items().map((x) => x.item.id),
      from,
      to,
    );
    pending.set(ids);
    if (!props.onReorder(ids)) pending.set(undefined);
  }, ROW_GAP);

  effect(() => {
    r.prune(items().length);
  });

  const startOf = (id: string) => props.starts.find((s) => s.id === id);
  const frozen = () => props.detail.frozen;
  const locked = () => frozen() || props.detail.archived || !!props.readOnly;
  const canDrag = () => !locked() && items().length > 1;

  return (
    <View
      style={{ width: OUTLINE_WIDTH, borderRightWidth: 1, overflow: "hidden" }}
      class="border-border bg-background"
      onMouseMove={(e) => r.move(e)}
      onMouseUp={(e) => r.end(e)}
      onMouseLeave={(e) => r.leave(e)}
    >
      <Scroll style={{ flex: 1 }}>
        <View ref={(v) => r.container(v)} style={{ paddingTop: SPACE[8], paddingBottom: SPACE[8], paddingLeft: SPACE[6], paddingRight: SPACE[6], gap: ROW_GAP }}>
          <If when={props.detail.hasCover}>
            <Row
              isCover
              label=""
              title={t("outline.cover")}
              page={1}
              active={!!props.coverActive}
              onPress={props.onCover}
            />
          </If>
          <Each of={items()} key={(x) => x.item.id}>
            {(x, index) => {
              const dragging = () => r.drag()?.from === index();
              return (
                <View
                  ref={(v) => r.row(index(), v)}
                  testID={`outline-${x().label}`}
                  onPressDown={(e) => canDrag() && r.pressDown(index(), e, items().length)}
                  onPress={() => r.clicked() && props.onSelect(x().label)}
                  tooltip={tip(x().item.title)}
                  style={{
                    zIndex: dragging() ? 2 : 0,
                    transform: dragging() ? `translateY(${r.offset()}px)` : "none",
                    boxShadow: dragging() ? popoverShadow() : "none",
                    borderRadius: RADIUS.sm,
                    cursor: canDrag() ? "grab" : "default",
                  }}
                  class={dragging() ? `bg-card border border-border ${FOCUS_RING}` : `border border-transparent ${FOCUS_RING}`}
                >
                  <Row
                    label={x().label}
                    title={x().item.title}
                    page={startOf(x().item.id)?.first}
                    active={!props.coverActive && x().label === props.current}
                    missing={x().item.missing}
                    mark={props.markFor?.(x().item.id)}
                  />
                </View>
              );
            }}
          </Each>
          <If when={r.line() !== undefined}>
            {/* token-exempt: 2px drag-drop insertion line */}
            <View style={{ position: "absolute", left: 8, right: 8, top: r.line()!, height: 2, borderRadius: 1 }} class="bg-primary" />
          </If>
        </View>
      </Scroll>
      <If when={!locked() && !!props.onPickProblems}>
        <View style={{ paddingLeft: SPACE[8], paddingRight: SPACE[8], paddingTop: SPACE[6], paddingBottom: SPACE[8] }}>
          <View
            testID="outline-pick-problems"
            onPress={() => props.onPickProblems?.()}
            class={`border border-border bg-card hover:bg-muted active:bg-accent ${FOCUS_RING}`}
            style={{
              height: CONTROL.md,
              borderRadius: RADIUS.sm,
              flexDirection: "row",
              alignItems: "center",
              justifyContent: "center",
              gap: SPACE[6],
              transitionProperty: "colors",
              transitionDuration: ms(120),
              transitionTimingFunction: EASE_STANDARD,
            }}
          >
            <Icon name="listPlus" size="sm" tone="secondary-foreground" />
            <Text style={{ fontSize: FONT_SIZE.caption }} class="text-foreground">
              {t("contests.pickProblems")}
            </Text>
          </View>
        </View>
      </If>
      <If when={props.detail.items.length > 0}>
        <View style={{ padding: SPACE[12], borderTopWidth: 1, overflow: "hidden" }} class="border-border">
          <ContestStats id={props.detail.id} compact />
        </View>
      </If>
    </View>
  );
}

function Row(props: {
  label: string;
  title: string;
  page?: number;
  active: boolean;
  missing?: boolean;
  isCover?: boolean;
  mark?: StatusMarkKind;
  onPress?: () => void;
}) {
  return (
    <View
      onPress={props.onPress ? () => props.onPress!() : undefined}
      class={`${props.active ? "bg-accent" : "hover:bg-muted active:bg-accent/70"}${props.onPress ? ` ${FOCUS_RING}` : ""}`}
      style={{
        height: CONTROL.lg,
        paddingLeft: SPACE[8],
        paddingRight: SPACE[8],
        borderRadius: RADIUS.sm,
        flexDirection: "row",
        alignItems: "center",
        gap: SPACE[8],
        transitionProperty: "colors",
        transitionDuration: ms(120),
        transitionTimingFunction: EASE_STANDARD,
      }}
    >
      <View style={{ width: 16, alignItems: "center", justifyContent: "center" }}>
        <If
          when={props.isCover}
          else={
            <Text
              style={{ fontSize: FONT_SIZE.caption, fontWeight: FONT_WEIGHT.semibold, fontVariant: ["tabular-nums"] }}
              class={props.active ? "text-foreground" : "text-secondary-foreground"}
            >
              {props.label}
            </Text>
          }
        >
          <Icon name="fileText" size="sm" tone={props.active ? "foreground" : "muted-foreground"} />
        </If>
      </View>
      <Text
        style={{ numberOfLines: 1, textOverflow: "ellipsis", flex: 1, fontSize: FONT_SIZE.body }}
        class={props.missing ? "text-destructive" : props.isCover ? "text-secondary-foreground" : "text-foreground"}
      >
        {props.title}
      </Text>
      <If when={props.missing}>
        <View tooltip={tip(t("contests.missingSuffix", { title: props.title }))}>
          <Icon name="triangleAlert" size="sm" tone="destructive" />
        </View>
      </If>
      <If when={!props.isCover && !props.missing}>
        <StatusMark kind={props.mark ?? "none"} />
      </If>
      <If when={props.page !== undefined}>
        <Text style={{ fontSize: FONT_SIZE.caption, fontVariant: ["tabular-nums"], minWidth: 24, textAlign: "right" }} class="text-muted-foreground">
          {`p.${props.page}`}
        </Text>
      </If>
    </View>
  );
}

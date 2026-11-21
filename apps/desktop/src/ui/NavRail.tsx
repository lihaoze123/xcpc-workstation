// The 3 spaces (§2.1): 题库 Cmd+1 and 比赛 Cmd+2 at the top, 设置 Cmd+3 at the bottom,
// each an icon over its name. The top indicator glides smoothly between adjacent top-group
// items (题库 ↔ 比赛); entering or leaving 设置 cross-fades in place (120ms opacity) rather
// than sweeping across the full rail height. The rail folds away in the studio.

import { effect, reduceMotion, signal, Text, View, type ViewStyle } from "nsh";
import { Icon, type IconName } from "./Icon";
import { tip } from "./kit";
import { SPACE_NAMES, type Space } from "./logic";
import { colorTransition, indicatorTransition } from "./motion";
import { EASE_STANDARD, ms } from "./theme";

export const RAIL_WIDTH = 68;
const ITEM_SIZE = 52;
const ITEM_GAP = 4;

function Item(props: { space: Space; icon: IconName; keys: string; current: Space; onChange: (s: Space) => void }) {
  const on = () => props.current === props.space;
  return (
    <View
      testID={`nav-${props.space}`}
      onPress={() => props.onChange(props.space)}
      tooltip={tip(SPACE_NAMES[props.space], props.keys, "right")}
      style={{
        width: ITEM_SIZE,
        height: ITEM_SIZE,
        borderRadius: 8,
        alignItems: "center",
        justifyContent: "center",
        gap: 4,
        ...colorTransition(120),
      }}
      class={
        on()
          ? "focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-ring"
          : "hover:bg-muted/70 active:bg-accent/60 focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-ring"
      }
    >
      <Icon name={props.icon} size={18} tone={on() ? "foreground" : "secondary-foreground"} />
      <Text style={{ fontSize: 11, lineHeight: 14 }} class={on() ? "text-foreground" : "text-secondary-foreground"}>
        {SPACE_NAMES[props.space]}
      </Text>
    </View>
  );
}

export function NavRail(props: { space: Space; onChange: (space: Space) => void }) {
  const lastTopSpace = signal<"problems" | "contests">(props.space === "contests" ? "contests" : "problems");
  const slideTop = signal(false);
  let prevSpace: Space = props.space;

  effect(() => {
    const cur = props.space;
    if (cur === "problems" || cur === "contests") {
      // Glide only between adjacent top-group items; returning from settings fades in at target.
      slideTop.set(prevSpace === "problems" || prevSpace === "contests");
      lastTopSpace.set(cur);
    } else {
      slideTop.set(false);
    }
    prevSpace = cur;
  });

  const inTopGroup = () => props.space === "problems" || props.space === "contests";
  const topIndicatorY = () => (lastTopSpace() === "contests" ? ITEM_SIZE + ITEM_GAP : 0);
  const fadeTransition = (): ViewStyle => ({
    transitionProperty: "opacity",
    transitionDuration: ms(120),
    transitionTimingFunction: EASE_STANDARD,
  });

  const item = (space: Space, icon: IconName, keys: string) => (
    <Item space={space} icon={icon} keys={keys} current={props.space} onChange={props.onChange} />
  );
  return (
    <View
      style={{ width: RAIL_WIDTH, paddingTop: 8, paddingBottom: 8, alignItems: "center", borderRightWidth: 1 }}
      class="border-border bg-background"
    >
      <View style={{ flex: 1, width: ITEM_SIZE, gap: ITEM_GAP }}>
        <View
          testID="nav-indicator"
          style={{
            position: "absolute",
            left: 0,
            top: 0,
            width: ITEM_SIZE,
            height: ITEM_SIZE,
            borderRadius: 8,
            opacity: inTopGroup() ? 1 : 0,
            transform: `translateY(${topIndicatorY()}px)`,
            ...(slideTop() && !reduceMotion() ? indicatorTransition() : fadeTransition()),
          }}
          class="bg-accent"
        />
        <View
          testID="nav-indicator-settings"
          style={{
            position: "absolute",
            left: 0,
            bottom: 0,
            width: ITEM_SIZE,
            height: ITEM_SIZE,
            borderRadius: 8,
            opacity: props.space === "settings" ? 1 : 0,
            ...fadeTransition(),
          }}
          class="bg-accent"
        />
        {item("problems", "libraryBig", "mod+1")}
        {item("contests", "trophy", "mod+2")}
        <View style={{ flex: 1 }} />
        {item("settings", "settings", "mod+3")}
      </View>
    </View>
  );
}

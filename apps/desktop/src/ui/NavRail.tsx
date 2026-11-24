// The 3 spaces (§2.1): 题库 Cmd+1 and 比赛 Cmd+2 at the top, 设置 Cmd+3 at the bottom,
// each an icon over its name. The top indicator glides smoothly between adjacent top-group
// items (题库 ↔ 比赛); entering or leaving 设置 cross-fades in place (120ms opacity) rather
// than sweeping across the full rail height. The rail folds away in the studio.

import { effect, reduceMotion, signal, Text, View, type ViewStyle } from "nsh";
import { Icon, type IconName } from "./Icon";
import { tip } from "./kit";
import { spaceName, type Space } from "./logic";
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
      tooltip={tip(spaceName(props.space), props.keys, "right")}
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
        {spaceName(props.space)}
      </Text>
    </View>
  );
}

type TopSpace = "problems" | "contests" | "training" | "tools";

function isTopSpace(s: Space): s is TopSpace {
  return s === "problems" || s === "contests" || s === "training" || s === "tools";
}

export function NavRail(props: {
  space: Space;
  trainingEnabled?: boolean;
  hasWorkspace?: boolean;
  onChange: (space: Space) => void;
}) {
  const wsOpen = () => props.hasWorkspace !== false;
  const showTraining = () => wsOpen() && props.trainingEnabled !== false;
  const lastTopSpace = signal<TopSpace>(isTopSpace(props.space) ? props.space : wsOpen() ? "problems" : "tools");
  const slideTop = signal(false);
  let prevSpace: Space = props.space;

  effect(() => {
    const cur = props.space;
    if (isTopSpace(cur)) {
      // Glide smoothly between top-group items; returning from settings fades in at target.
      slideTop.set(isTopSpace(prevSpace));
      lastTopSpace.set(cur);
    } else {
      slideTop.set(false);
    }
    prevSpace = cur;
  });

  const inTopGroup = () => isTopSpace(props.space);
  const topIndicatorY = () => {
    const cur = lastTopSpace();
    if (!wsOpen()) return 0;
    const step = ITEM_SIZE + ITEM_GAP;
    if (cur === "problems") return 0;
    if (cur === "contests") return step;
    if (cur === "training" && showTraining()) return 2 * step;
    if (cur === "tools") return (showTraining() ? 3 : 2) * step;
    return 0;
  };
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
        <If when={wsOpen()}>
          {item("problems", "libraryBig", "mod+1")}
          {item("contests", "trophy", "mod+2")}
          <If when={showTraining()}>
            {item("training", "crown", "mod+3")}
          </If>
        </If>
        {item("tools", "squareCode", "mod+4")}
        <View style={{ flex: 1 }} />
        {item("settings", "settings", "mod+5")}
      </View>
    </View>
  );
}

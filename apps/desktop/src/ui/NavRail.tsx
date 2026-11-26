// The 3 spaces (§2.1): 题库 Cmd+1 and 比赛 Cmd+2 at the top, 设置 Cmd+3 at the bottom,
// each an icon over its name. The top indicator glides smoothly between adjacent top-group
// items (题库 ↔ 比赛); entering or leaving 设置 cross-fades in place (120ms opacity) rather
// than sweeping across the full rail height. The rail folds away in the studio.

import { effect, reduceMotion, signal, Text, View, type ViewStyle } from "nsh";
import { Icon, type IconName } from "./Icon";
import { tip } from "./kit";
import { shortcutForSpace, spaceName, visibleSpaces, type Space } from "./logic";
import { colorTransition, indicatorTransition } from "./motion";
import { EASE_STANDARD, ms } from "./theme";

import { FONT_SIZE, LINE_HEIGHT, RADIUS, SPACE } from "./tokens";
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
        borderRadius: RADIUS.md,
        alignItems: "center",
        justifyContent: "center",
        gap: SPACE[4],
        ...colorTransition(120),
      }}
      class={
        on()
          ? "focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-ring"
          : "hover:bg-muted/70 active:bg-accent/60 focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-ring"
      }
    >
      <Icon name={props.icon} size="lg" tone={on() ? "foreground" : "secondary-foreground"} />
      <Text style={{ fontSize: FONT_SIZE.micro, lineHeight: LINE_HEIGHT.micro }} class={on() ? "text-foreground" : "text-secondary-foreground"}>
        {spaceName(props.space)}
      </Text>
    </View>
  );
}

type TopSpace = "problems" | "contests" | "snippets" | "training" | "tools";

function isTopSpace(s: Space): s is TopSpace {
  return s === "problems" || s === "contests" || s === "snippets" || s === "training" || s === "tools";
}

export function NavRail(props: {
  space: Space;
  snippetsEnabled?: boolean;
  trainingEnabled?: boolean;
  hasWorkspace?: boolean;
  onChange: (space: Space) => void;
}) {
  const wsOpen = () => props.hasWorkspace !== false;
  const showSnippets = () => wsOpen() && props.snippetsEnabled !== false;
  const showTraining = () => wsOpen() && props.trainingEnabled !== false;
  const navOpts = () => ({
    hasWorkspace: wsOpen(),
    snippetsEnabled: showSnippets(),
    trainingEnabled: showTraining(),
  });
  const topList = () => visibleSpaces(navOpts()).filter(isTopSpace);
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
    const idx = topList().indexOf(cur);
    return (idx >= 0 ? idx : 0) * (ITEM_SIZE + ITEM_GAP);
  };
  const fadeTransition = (): ViewStyle => ({
    transitionProperty: "opacity",
    transitionDuration: ms(120),
    transitionTimingFunction: EASE_STANDARD,
  });

  const item = (space: Space, icon: IconName) => (
    <Item
      space={space}
      icon={icon}
      keys={shortcutForSpace(space, navOpts()) ?? ""}
      current={props.space}
      onChange={props.onChange}
    />
  );
  return (
    <View
      style={{ width: RAIL_WIDTH, paddingTop: SPACE[8], paddingBottom: SPACE[8], alignItems: "center", borderRightWidth: 1 }}
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
            borderRadius: RADIUS.md,
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
            borderRadius: RADIUS.md,
            opacity: props.space === "settings" ? 1 : 0,
            ...fadeTransition(),
          }}
          class="bg-accent"
        />
        <If when={wsOpen()}>
          {item("problems", "libraryBig")}
          {item("contests", "trophy")}
          <If when={showSnippets()}>
            {item("snippets", "bookOpen")}
          </If>
          <If when={showTraining()}>
            {item("training", "crown")}
          </If>
        </If>
        {item("tools", "squareCode")}
        <View style={{ flex: 1 }} />
        {item("settings", "settings")}
      </View>
    </View>
  );
}

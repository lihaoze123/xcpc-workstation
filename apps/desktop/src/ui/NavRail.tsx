// The 3 spaces (§2.1): 题库 Cmd+1 and 比赛 Cmd+2 at the top, 设置 Cmd+3 at the bottom,
// each an icon over its name. The selected one sits on the light gray; the rail folds
// away in the studio.

import { Text, View } from "nsh";
import { Icon, type IconName } from "./Icon";
import { tip } from "./kit";
import { SPACE_NAMES, type Space } from "./logic";
import { EASE_STANDARD, ms } from "./theme";

export const RAIL_WIDTH = 68;

function Item(props: { space: Space; icon: IconName; keys: string; current: Space; onChange: (s: Space) => void }) {
  const on = () => props.current === props.space;
  return (
    <View
      testID={`nav-${props.space}`}
      onPress={() => props.onChange(props.space)}
      tooltip={tip(SPACE_NAMES[props.space], props.keys, "right")}
      style={{
        width: 52,
        height: 52,
        borderRadius: 8,
        alignItems: "center",
        justifyContent: "center",
        gap: 4,
        transitionProperty: "colors",
        transitionDuration: ms(120),
        transitionTimingFunction: EASE_STANDARD,
      }}
      class={
        on()
          ? "bg-accent focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-ring"
          : "hover:bg-muted active:bg-accent focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-ring"
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
  const item = (space: Space, icon: IconName, keys: string) => (
    <Item space={space} icon={icon} keys={keys} current={props.space} onChange={props.onChange} />
  );
  return (
    <View
      style={{ width: RAIL_WIDTH, paddingTop: 8, paddingBottom: 8, alignItems: "center", borderRightWidth: 1, gap: 4 }}
      class="border-border bg-background"
    >
      {item("problems", "libraryBig", "mod+1")}
      {item("contests", "trophy", "mod+2")}
      <View style={{ flex: 1 }} />
      {item("settings", "settings", "mod+3")}
    </View>
  );
}

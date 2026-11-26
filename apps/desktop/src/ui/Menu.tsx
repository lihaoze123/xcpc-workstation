// The app's menus on `nsh/base`'s `Menu`: one look for the filter menus, the ⋯ menus and
// the pickers. Rows are 32 points with a 16-point icon column (a check, or the item's
// icon), the label, and a shortcut on the right; a destructive row is red.

import { signal, Text, View, type JSX, type ViewStyle, type StyleProp } from "nsh";
import { Menu, MenuItem, MenuSeparator } from "nsh/base";
import { Icon, type IconName } from "./Icon";
import { Kbd, TruncText } from "./kit";
import {
  anchorOrigin,
  colorTransition,
  overlayTransform,
  overlayTransition,
  popupEndingStyle,
  popupStartingStyle,
  type AnchorPlacement,
} from "./tokens";
import { popoverShadow } from "./theme";

import { CONTROL, FONT_SIZE, LINE_HEIGHT, RADIUS, SPACE } from "./tokens";
export type MenuEntry =
  | {
      id: string;
      label: string;
      icon?: IconName;
      checked?: boolean;
      keys?: string;
      disabled?: boolean;
      destructive?: boolean;
      /** A short note after the label, muted (a count). */
      note?: string;
    }
  | { separator: true; id: string };

export function AppMenu(props: {
  entries: MenuEntry[];
  onSelect: (id: string) => void;
  /** Keep the menu open on a choice (multi-select filters). */
  stayOpen?: boolean;
  trigger: JSX.Element;
  align?: "start" | "end";
  minWidth?: number;
}) {
  // Whether any row has an icon or a check: then every row keeps the icon column.
  const column = () => props.entries.some((e) => !("separator" in e) && (e.icon || e.checked !== undefined));
  const actualPlacement = signal<AnchorPlacement>("bottom");
  return (
    <Menu
      trigger={props.trigger}
      placement="bottom"
      align={props.align ?? "start"}
      offset={4}
      onPosition={(e) => actualPlacement.set(e.placement)}
      onClose={() => actualPlacement.set("bottom")}
      popupClass="bg-popover border border-border"
      popupStyle={{
        minWidth: props.minWidth ?? 180,
        padding: SPACE[4],
        borderRadius: RADIUS.lg,
        boxShadow: popoverShadow(),
        transformOrigin: anchorOrigin(actualPlacement(), props.align ?? "start"),
        opacity: 1,
        transform: overlayTransform(true),
        ...overlayTransition(true),
      }}
      popupStartingStyle={popupStartingStyle()}
      popupEndingStyle={popupEndingStyle()}
    >
      <Each of={props.entries} key={(e) => e.id}>
        {(e) => {
          if ("separator" in e()) {
            return <MenuSeparator style={{ height: 1, marginTop: SPACE[4], marginBottom: SPACE[4], marginLeft: SPACE[4], marginRight: SPACE[4] }} class="bg-border" />;
          }
          const item = () => e() as Exclude<MenuEntry, { separator: true }>;
          return (
            <MenuItem
              disabled={item().disabled}
              closeOnSelect={!props.stayOpen}
              onSelect={() => setTimeout(() => props.onSelect(item().id), 0)}
              class={item().disabled ? "" : "hover:bg-muted focus:bg-muted active:bg-accent"}
              style={
                {
                  height: CONTROL.lg,
                  paddingLeft: SPACE[8],
                  paddingRight: SPACE[8],
                  borderRadius: RADIUS.sm,
                  flexDirection: "row",
                  alignItems: "center",
                  gap: SPACE[8],
                  opacity: item().disabled ? 0.5 : 1,
                  ...colorTransition(),
                } as StyleProp<ViewStyle>
              }
              indicator={<View />}
            >
              <If when={column()}>
                <View style={{ width: 16, alignItems: "center" }}>
                  {item().checked !== undefined ? (
                    <Icon name="check" size="sm" tone={item().disabled ? "muted-foreground" : "foreground"} style={{ opacity: item().checked ? 1 : 0 }} />
                  ) : item().icon ? (
                    <Icon
                      name={item().icon!}
                      size="md"
                      tone={item().disabled ? "muted-foreground" : item().destructive ? "destructive" : "secondary-foreground"}
                    />
                  ) : null}
                </View>
              </If>
              <TruncText
                style={{ flex: 1, fontSize: FONT_SIZE.body, lineHeight: LINE_HEIGHT.body, numberOfLines: 1, textOverflow: "ellipsis" }}
                class={item().disabled ? "text-muted-foreground" : item().destructive ? "text-destructive" : "text-foreground"}
              >
                {item().label}
              </TruncText>
              {item().note && (
                <Text style={{ fontSize: FONT_SIZE.caption, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
                  {item().note}
                </Text>
              )}
              {item().keys && <Kbd keys={item().keys!} faint />}
            </MenuItem>
          );
        }}
      </Each>
    </Menu>
  );
}

/** A trigger: an outlined square with the ⋯ icon. */
export function MoreTrigger(props: { testID?: string }) {
  return (
    <View
      testID={props.testID}
      class="border border-border bg-card hover:bg-muted active:bg-accent focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-ring"
      style={{
        width: 28,
        height: CONTROL.md,
        borderRadius: RADIUS.sm,
        alignItems: "center",
        justifyContent: "center",
        ...colorTransition(),
      }}
    >
      <Icon name="ellipsis" size="md" tone="foreground" />
    </View>
  );
}

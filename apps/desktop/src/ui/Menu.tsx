// The app's menus on `nsh/base`'s `Menu`: one look for the filter menus, the ⋯ menus and
// the pickers. Rows are 32 points with a 16-point icon column (a check, or the item's
// icon), the label, and a shortcut on the right; a destructive row is red.

import { Text, View, type JSX, type ViewStyle, type StyleProp } from "nsh";
import { Menu, MenuItem, MenuSeparator } from "nsh/base";
import { Icon, type IconName } from "./Icon";
import { Kbd } from "./kit";
import { colorTransition, overlayTransform, overlayTransition } from "./motion";
import { popoverShadow } from "./theme";

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
  return (
    <Menu
      trigger={props.trigger}
      placement="bottom"
      align={props.align ?? "start"}
      offset={4}
      popupClass="bg-popover border border-border"
      popupStyle={{
        minWidth: props.minWidth ?? 180,
        padding: 4,
        borderRadius: 10,
        boxShadow: popoverShadow(),
        opacity: 1,
        transform: overlayTransform(true),
        ...overlayTransition(),
      }}
      popupStartingStyle={{
        opacity: 0,
        transform: overlayTransform(false),
      }}
      popupEndingStyle={{
        opacity: 0,
        transform: overlayTransform(false),
      }}
    >
      <Each of={props.entries} key={(e) => e.id}>
        {(e) => {
          if ("separator" in e()) {
            return <MenuSeparator style={{ height: 1, marginTop: 4, marginBottom: 4, marginLeft: 4, marginRight: 4 }} class="bg-border" />;
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
                  height: 32,
                  paddingLeft: 8,
                  paddingRight: 8,
                  borderRadius: 6,
                  flexDirection: "row",
                  alignItems: "center",
                  gap: 8,
                  opacity: item().disabled ? 0.5 : 1,
                  ...colorTransition(140),
                } as StyleProp<ViewStyle>
              }
              indicator={<View />}
            >
              <If when={column()}>
                <View style={{ width: 16, alignItems: "center" }}>
                  {item().checked !== undefined ? (
                    <Icon name="check" size={14} tone={item().disabled ? "muted-foreground" : "foreground"} style={{ opacity: item().checked ? 1 : 0 }} />
                  ) : item().icon ? (
                    <Icon
                      name={item().icon!}
                      size={15}
                      tone={item().disabled ? "muted-foreground" : item().destructive ? "destructive" : "secondary-foreground"}
                    />
                  ) : null}
                </View>
              </If>
              <Text
                style={{ flex: 1, fontSize: 13, lineHeight: 18, numberOfLines: 1, textOverflow: "ellipsis" }}
                class={item().disabled ? "text-muted-foreground" : item().destructive ? "text-destructive" : "text-foreground"}
              >
                {item().label}
              </Text>
              {item().note && (
                <Text style={{ fontSize: 12, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
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
        height: 28,
        borderRadius: 6,
        alignItems: "center",
        justifyContent: "center",
        ...colorTransition(140),
      }}
    >
      <Icon name="ellipsis" size={16} tone="foreground" />
    </View>
  );
}

// The page structure every space shares, so the same thing sits in the same place:
// a list column whose 52-point toolbar ends with its 新建 action, and a detail column
// whose head puts the title and its facts on the left and the actions on the right,
// top-aligned with the title. Empty states are an icon, one sentence and one way on.

import { Text, View, type JSX, type StyleProp, type ViewStyle } from "nsh";
import { Icon, type IconName } from "./Icon";
import { cardShadow } from "./theme";

export const TOOLBAR = 52;
export const GUTTER = 20;

/** The list column's toolbar: search and filters on the left, 新建 at the right end. */
export function Toolbar(props: { children?: JSX.Element; style?: StyleProp<ViewStyle> }) {
  return (
    <View
      style={[
        { height: TOOLBAR, flexDirection: "row", alignItems: "center", gap: 6, paddingLeft: 16, paddingRight: 12, borderBottomWidth: 1 },
        props.style,
      ]}
      class="border-border"
    >
      {props.children}
    </View>
  );
}

/** A flexible gap in a row. */
export function Spacer() {
  return <View style={{ flex: 1 }} />;
}

/** A detail column's head: title (one line) with its badge and actions on the right,
 * centered on the list toolbar's center line. When `second` is omitted, the header
 * stays `TOOLBAR` (52pt) tall and vertically centered so its bottom edge aligns with
 * the left toolbar without a blank metadata row. When `second` is present, tags/facts
 * wrap cleanly rather than being sliced mid-chip. */
export function DetailHeader(props: { title: string; badge?: JSX.Element; second?: JSX.Element; actions?: JSX.Element; size?: "md" | "lg" }) {
  const lg = props.size === "lg";
  const hasSecond = () => props.second !== undefined && props.second !== null && props.second !== false;
  return (
    <View
      style={
        hasSecond()
          ? { paddingTop: (TOOLBAR - 28) / 2, paddingBottom: 12, paddingLeft: GUTTER, paddingRight: GUTTER, gap: 8 }
          : { height: TOOLBAR, justifyContent: "center", paddingLeft: GUTTER, paddingRight: GUTTER }
      }
    >
      <View style={{ height: 28, flexDirection: "row", alignItems: "center", gap: 8, overflow: "hidden" }}>
        <Text style={{ numberOfLines: 1, textOverflow: "ellipsis", flexShrink: 1, minWidth: 0, fontSize: lg ? 19 : 17, lineHeight: 26, fontWeight: 600 }} class="text-foreground">
          {props.title}
        </Text>
        {props.badge && <View style={{ flexShrink: 0 }}>{props.badge}</View>}
        <View style={{ flex: 1 }} />
        <View style={{ flexDirection: "row", alignItems: "center", gap: 6, flexShrink: 0 }}>{props.actions}</View>
      </View>
      <If when={hasSecond()}>
        <View style={{ minHeight: 22, flexDirection: "row", flexWrap: "wrap", alignItems: "center", columnGap: 4, rowGap: 4 }}>{props.second}</View>
      </If>
    </View>
  );
}

/** The detail head's default two-line height. */
export const HEADER = (TOOLBAR - 28) / 2 + 28 + 8 + 22 + 12;

/** An empty state: a soft icon, one sentence (and a quieter second), one way on. */
export function EmptyState(props: { icon: IconName; title: string; hint?: string; action?: JSX.Element; style?: StyleProp<ViewStyle> }) {
  return (
    <View style={[{ flex: 1, alignItems: "center", justifyContent: "center", gap: 12, paddingBottom: 48 }, props.style]}>
      <View style={{ width: 44, height: 44, borderRadius: 12, alignItems: "center", justifyContent: "center" }} class="bg-muted">
        <Icon name={props.icon} size={20} tone="muted-foreground" />
      </View>
      <View style={{ alignItems: "center", gap: 4 }}>
        <Text style={{ fontSize: 14, lineHeight: 20, fontWeight: 500 }} class="text-foreground">{props.title}</Text>
        {props.hint && (
          <Text style={{ fontSize: 12.5, lineHeight: 18 }} class="text-secondary-foreground">
            {props.hint}
          </Text>
        )}
      </View>
      {props.action && <View style={{ marginTop: 4 }}>{props.action}</View>}
    </View>
  );
}

/** A titled group: a small heading over a card (the contest's overview, settings). */
export function Section(props: { title: string; description?: string; trailing?: JSX.Element; children?: JSX.Element; style?: StyleProp<ViewStyle> }) {
  return (
    <View style={[{ gap: 8 }, props.style]}>
      <View style={{ flexDirection: "row", alignItems: "flex-end", gap: 8 }}>
        <View style={{ flex: 1, gap: 2 }}>
          <Text style={{ fontSize: 13, lineHeight: 18, fontWeight: 600 }} class="text-foreground">{props.title}</Text>
          {props.description && (
            <Text style={{ fontSize: 12.5, lineHeight: 18 }} class="text-secondary-foreground">
              {props.description}
            </Text>
          )}
        </View>
        {props.trailing}
      </View>
      <View style={{ borderRadius: 10, borderWidth: 1, overflow: "hidden", boxShadow: cardShadow() }} class="bg-card border-border">
        {props.children}
      </View>
    </View>
  );
}

/** A row inside a `Section`'s card: content on the left, an action on the right. */
export function SectionRow(props: { children?: JSX.Element; trailing?: JSX.Element; first?: boolean; style?: StyleProp<ViewStyle> }) {
  return (
    <View
      style={[
        { minHeight: 52, paddingLeft: 14, paddingRight: 12, paddingTop: 10, paddingBottom: 10, flexDirection: "row", alignItems: "center", gap: 12, borderTopWidth: props.first ? 0 : 1 },
        props.style,
      ]}
      class="border-border"
    >
      <View style={{ flex: 1, minWidth: 0 }}>{props.children}</View>
      {props.trailing}
    </View>
  );
}

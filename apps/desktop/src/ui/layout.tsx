// The page structure every space shares, so the same thing sits in the same place:
// a list column whose 52-point toolbar ends with its 新建 action, and a detail column
// whose head puts the title and its facts on the left and the actions on the right,
// top-aligned with the title. Empty states are an icon, one sentence and one way on.

import { Text, View, type JSX, type StyleProp, type ViewStyle } from "nsh";
import { t } from "./i18n";
import { Icon, type IconName } from "./Icon";
import { DelayedSpinner, FOCUS_RING } from "./kit";
import { cardShadow, panelShadow } from "./theme";
import { CONTROL, GUTTER, RADIUS, SPACE, TOOLBAR, TYPE } from "./tokens";

export { GUTTER, TOOLBAR };

/** Floating `--bg-surface` inset panel on `--bg-canvas` (Phase 6D §3). */
export function InsetPanel(props: {
  flushLeft?: boolean;
  direction?: "row" | "column";
  testID?: string;
  children?: JSX.Element;
}) {
  return (
    <View
      testID={props.testID}
      class="bg-card border border-border"
      style={{
        flex: 1,
        minWidth: 0,
        minHeight: 0,
        flexDirection: props.direction ?? "column",
        marginTop: SPACE[8],
        marginRight: SPACE[8],
        marginBottom: SPACE[8],
        marginLeft: props.flushLeft ? SPACE[0] : SPACE[8],
        borderRadius: RADIUS.lg,
        overflow: "hidden",
        boxShadow: panelShadow(),
      }}
    >
      {props.children}
    </View>
  );
}

/** The list column's toolbar: search and filters on the left, 新建 at the right end. */
export function Toolbar(props: { children?: JSX.Element; style?: StyleProp<ViewStyle>; testID?: string }) {
  return (
    <View
      testID={props.testID}
      style={[
        {
          height: TOOLBAR,
          flexDirection: "row",
          alignItems: "center",
          gap: SPACE[6],
          paddingLeft: SPACE[16],
          paddingRight: SPACE[12],
          borderBottomWidth: 1,
          flexShrink: 0,
        },
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
export function DetailHeader(props: {
  title: string;
  badge?: JSX.Element;
  second?: JSX.Element;
  actions?: JSX.Element;
  size?: "md" | "lg";
  bordered?: boolean;
}) {
  const hasSecond = () => props.second !== undefined && props.second !== null && props.second !== false;
  return (
    <View
      class={props.bordered ? "border-border" : undefined}
      style={[
        hasSecond()
          ? {
              paddingTop: (TOOLBAR - CONTROL.md) / 2,
              paddingBottom: SPACE[12],
              paddingLeft: GUTTER,
              paddingRight: GUTTER,
              gap: SPACE[8],
            }
          : { height: TOOLBAR, justifyContent: "center", paddingLeft: GUTTER, paddingRight: GUTTER },
        props.bordered ? { borderBottomWidth: 1, flexShrink: 0 } : { flexShrink: 0 },
      ]}
    >
      <View style={{ height: CONTROL.md, flexDirection: "row", alignItems: "center", gap: SPACE[8], overflow: "hidden" }}>
        <Text style={{ numberOfLines: 1, textOverflow: "ellipsis", flexShrink: 1, minWidth: 0, ...TYPE.title }} class="text-foreground">
          {props.title}
        </Text>
        {props.badge && <View style={{ flexShrink: 0 }}>{props.badge}</View>}
        <View style={{ flex: 1 }} />
        <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[6], flexShrink: 0 }}>{props.actions}</View>
      </View>
      <If when={hasSecond()}>
        <View style={{ minHeight: 22, flexDirection: "row", flexWrap: "wrap", alignItems: "center", columnGap: SPACE[4], rowGap: SPACE[4] }}>
          {props.second}
        </View>
      </If>
    </View>
  );
}

/** The detail head's default two-line height. */
export const HEADER = (TOOLBAR - CONTROL.md) / 2 + CONTROL.md + SPACE[8] + 22 + SPACE[12];

/** An empty state: a soft icon, one sentence (and a quieter second), one way on. */
export function EmptyState(props: { icon: IconName; title: string; hint?: string; action?: JSX.Element; style?: StyleProp<ViewStyle> }) {
  return (
    <View style={[{ flex: 1, alignItems: "center", justifyContent: "center", paddingBottom: SPACE[32] }, props.style]}>
      <View
        style={{
          width: 44,
          height: 44, // token-exempt: empty-state icon box square
          borderRadius: RADIUS.lg,
          alignItems: "center",
          justifyContent: "center",
          marginBottom: SPACE[24],
        }}
        class="bg-muted"
      >
        <Icon name={props.icon} size="lg" tone="muted-foreground" />
      </View>
      <View style={{ alignItems: "center", gap: SPACE[8], maxWidth: 320 }}>
        <Text style={{ ...TYPE.strong, textAlign: "center" }} class="text-foreground">
          {props.title}
        </Text>
        {props.hint && (
          <Text style={{ ...TYPE.caption, textAlign: "center" }} class="text-secondary-foreground">
            {props.hint}
          </Text>
        )}
      </View>
      {props.action && <View style={{ marginTop: SPACE[24] }}>{props.action}</View>}
    </View>
  );
}

/** Filter-yielded empty state (Phase 6F §3): no icon, one quiet sentence + text button to clear filters. */
export function FilterEmpty(props: {
  message: string;
  onClear?: () => void;
  clearLabel?: string;
  style?: StyleProp<ViewStyle>;
}) {
  return (
    <View
      style={[
        {
          flex: 1,
          flexDirection: "row",
          flexWrap: "wrap",
          alignItems: "center",
          justifyContent: "center",
          gap: SPACE[8],
          paddingHorizontal: SPACE[16],
          paddingVertical: SPACE[24],
        },
        props.style,
      ]}
    >
      <Text style={TYPE.caption} class="text-secondary-foreground">
        {props.message}
      </Text>
      {props.onClear && (
        <View
          onPress={() => props.onClear!()}
          class={`hover:bg-muted active:bg-accent ${FOCUS_RING}`}
          style={{
            paddingHorizontal: SPACE[6],
            paddingVertical: SPACE[2],
            borderRadius: RADIUS.xs,
          }}
        >
          <Text style={TYPE.caption} class="text-primary">
            {props.clearLabel ?? t("library.filter.clear")}
          </Text>
        </View>
      )}
    </View>
  );
}

/** Block / card / panel-level empty state (Phase 6F §3): a single muted line, no icon. */
export function SectionEmpty(props: { message: string; style?: StyleProp<ViewStyle> }) {
  return (
    <View style={[{ paddingHorizontal: SPACE[16], paddingVertical: SPACE[20], alignItems: "center", justifyContent: "center" }, props.style]}>
      <Text style={TYPE.caption} class="text-muted-foreground">
        {props.message}
      </Text>
    </View>
  );
}

/** Page-level loading state (Phase 6F §3): centered `DelayedSpinner` (appears only after 150ms). */
export function PageLoading(props: { active: boolean; size?: number; style?: StyleProp<ViewStyle> }) {
  return (
    <View style={[{ flex: 1, alignItems: "center", justifyContent: "center" }, props.style]}>
      <DelayedSpinner active={props.active} size={props.size ?? 18} />
    </View>
  );
}

/** A titled group: a small heading over a card (the contest's overview, settings). */
export function Section(props: { title: string; description?: string; trailing?: JSX.Element; children?: JSX.Element; style?: StyleProp<ViewStyle> }) {
  return (
    <View style={[{ gap: SPACE[8] }, props.style]}>
      <View style={{ flexDirection: "row", alignItems: "flex-end", gap: SPACE[8] }}>
        <View style={{ flex: 1, gap: SPACE[2] }}>
          <Text style={TYPE.strong} class="text-foreground">
            {props.title}
          </Text>
          {props.description && (
            <Text style={TYPE.caption} class="text-secondary-foreground">
              {props.description}
            </Text>
          )}
        </View>
        {props.trailing}
      </View>
      <View style={{ borderRadius: RADIUS.lg, borderWidth: 1, overflow: "hidden", boxShadow: cardShadow() }} class="bg-card border-border">
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
        {
          minHeight: TOOLBAR,
          paddingLeft: SPACE[16],
          paddingRight: SPACE[12],
          paddingTop: SPACE[12],
          paddingBottom: SPACE[12],
          flexDirection: "row",
          alignItems: "center",
          gap: SPACE[12],
          borderTopWidth: props.first ? 0 : 1,
        },
        props.style,
      ]}
      class="border-border"
    >
      <View style={{ flex: 1, minWidth: 0 }}>{props.children}</View>
      {props.trailing}
    </View>
  );
}

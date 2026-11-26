// Small building blocks shared by the views: buttons with color press feedback and
// focus-visible ring (§8.3), keycaps, tooltips that name their shortcut, rating badges,
// the freeze lock and a segmented control with a sliding indicator. All of them on `nsh` views.

import { cleanup, effect, keymap, signal, Text, View, type JSX, type ViewStyle, type TextStyle, type StyleProp } from "nsh";
import { Tooltip } from "nsh/base";
import { colorTransition, indicatorTransition, opacityTransition, pressActiveStyle, pressTransition } from "./tokens";
import { cardShadow, popoverShadow, tierColor, tierSoft, windowActive } from "./theme";
import { ratingTier } from "./logic";
import { Icon, type IconName, type IconSize, type IconTone } from "./Icon";
import { CONTROL, FONT_SIZE, FONT_WEIGHT, inner, RADIUS, SPACE, type ControlSize } from "./tokens";

export type TooltipSide = "top" | "bottom" | "left" | "right";

/** A keystroke as the platform writes it (`Ctrl+K`). */
export function keyLabel(keystroke: string): string {
  try {
    return keymap.label(keystroke);
  } catch {
    return keystroke;
  }
}

/** A keycap. */
export function Kbd(props: { keys: string; faint?: boolean }) {
  return (
    <View
      style={{
        height: 18, // token-exempt: compact inline keycap height
        paddingLeft: SPACE[4],
        paddingRight: SPACE[4],
        borderRadius: RADIUS.xs,
        borderWidth: 1,
        justifyContent: "center",
      }}
      class={props.faint ? "border-border" : "border-border bg-muted"}
    >
      <Text style={{ fontSize: FONT_SIZE.micro, fontWeight: FONT_WEIGHT.semibold, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
        {keyLabel(props.keys)}
      </Text>
    </View>
  );
}

/** A tooltip (600 ms) with the shortcut's keycap on its right (§9.6). */
export function tip(label: string, keys?: string, side?: TooltipSide): JSX.Element {
  return (
    <Tooltip
      delay={600}
      placement={side ?? "top"}
      style={{
        flexDirection: "row",
        alignItems: "center",
        gap: SPACE[8],
        paddingLeft: SPACE[8],
        paddingRight: SPACE[6],
        height: CONTROL.sm,
        borderRadius: RADIUS.sm,
        borderWidth: 1,
        boxShadow: popoverShadow(),
      }}
      class="bg-popover border-border"
    >
      <Text style={{ fontSize: FONT_SIZE.caption }}>{label}</Text>
      {keys && <Kbd keys={keys} />}
    </Tooltip>
  );
}

/** A tooltip with the full text of a truncated label (Phase 6F §1): `tip()`'s delay and side, up to 360pt wide and 4 lines. */
export function truncTip(label: string): JSX.Element {
  return (
    <Tooltip
      delay={600}
      placement="top"
      style={{
        maxWidth: 360,
        paddingLeft: SPACE[8],
        paddingRight: SPACE[8],
        paddingTop: SPACE[4],
        paddingBottom: SPACE[4],
        borderRadius: RADIUS.sm,
        borderWidth: 1,
        boxShadow: popoverShadow(),
      }}
      class="bg-popover border-border"
    >
      <Text style={{ fontSize: FONT_SIZE.caption, numberOfLines: 4, textOverflow: "ellipsis" }}>{label}</Text>
    </Tooltip>
  );
}

function flattenTextStyle(style: StyleProp<TextStyle>): TextStyle {
  if (!style) return {};
  if (Array.isArray(style)) return Object.assign({}, ...style.map((s) => flattenTextStyle(s as StyleProp<TextStyle>)));
  return style as TextStyle;
}

/**
 * A `<Text>` with `numberOfLines` + `textOverflow: "ellipsis"` that shows its full text in a
 * tooltip only when it is actually cut off (Phase 6F §1).
 *
 * `onTextLayout` cannot tell on its own: an ellipsized line reports the box's width, which a
 * text sized to its content reports too, and headless engines report the box for every line.
 * So an invisible, zero-height, clipped probe lays out the same text unconstrained and its own
 * frame is compared with the visible one: one line at `max-content` width against the box's
 * width; several lines at the box's width, without a line limit, against the box's height.
 * An explicit `tooltip` always wins.
 */
export function TruncText(props: {
  style?: StyleProp<TextStyle>;
  class?: string;
  tooltip?: JSX.Element;
  testID?: string;
  onPress?: () => void;
  /** Tooltip text when the children are not a plain string. */
  label?: string;
  children?: JSX.Element;
}) {
  const box = signal({ width: 0, height: 0 });
  const probe = signal({ width: 0, height: 0 });
  const multiLine = () => {
    const n = flattenTextStyle(props.style).numberOfLines;
    return typeof n === "number" && n > 1;
  };
  const label = () => {
    if (props.label !== undefined) return props.label;
    const c = props.children as unknown;
    return typeof c === "string" || typeof c === "number" ? String(c) : "";
  };
  const truncated = () => {
    const b = box();
    const p = probe();
    if (b.width <= 0 || p.width <= 0 || !label()) return false;
    return multiLine() ? p.height > b.height + 0.5 : p.width > b.width + 0.5;
  };
  return (
    <>
      <Text
        style={props.style}
        class={props.class}
        testID={props.testID}
        onPress={props.onPress}
        onLayout={(e) => box.set({ width: e.bounds.size.width, height: e.bounds.size.height })}
        tooltip={props.tooltip ?? (truncated() ? truncTip(label()) : undefined)}
      >
        {props.children}
      </Text>
      <View
        accessibilityElementsHidden
        importantForAccessibility="no-hide-descendants"
        style={{
          position: "absolute",
          left: 0,
          top: 0,
          width: multiLine() ? Math.max(1, box().width) : "max-content",
          height: 0,
          overflow: "hidden",
          opacity: 0,
          alignItems: "flex-start",
          pointerEvents: "none",
        }}
      >
        <Text
          style={[
            props.style,
            {
              position: "relative",
              flexGrow: 0,
              flexShrink: 0,
              flexBasis: "auto",
              minWidth: 0,
              width: multiLine() ? "100%" : "max-content",
              numberOfLines: multiLine() ? 0 : 1,
            },
          ]}
          onLayout={(e) => probe.set({ width: e.bounds.size.width, height: e.bounds.size.height })}
        >
          {label()}
        </Text>
      </View>
    </>
  );
}

type Variant = "primary" | "outline" | "ghost" | "danger";

export const FOCUS_RING = "focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-ring";

const VARIANT: Record<Variant, { box: string; disabledBox: string; text: string }> = {
  primary: {
    box: `bg-primary hover:bg-sidebar-primary active:opacity-90 ${FOCUS_RING}`,
    disabledBox: "bg-primary",
    text: "text-primary-foreground",
  },
  outline: {
    box: `border border-border bg-card hover:bg-muted active:bg-accent ${FOCUS_RING}`,
    disabledBox: "border border-border bg-card",
    text: "text-foreground",
  },
  ghost: {
    box: `hover:bg-muted active:bg-accent ${FOCUS_RING}`,
    disabledBox: "",
    text: "text-foreground",
  },
  danger: {
    box: `bg-chart-5 hover:opacity-90 active:opacity-80 ${FOCUS_RING}`,
    disabledBox: "bg-chart-5",
    text: "text-primary-foreground",
  },
};

const VARIANT_TONE: Record<Variant, IconTone> = {
  primary: "primary-foreground",
  outline: "foreground",
  ghost: "secondary-foreground",
  danger: "primary-foreground",
};

/** A button: `sm` (24), `md` (28, default), or `lg` (32) points tall, radius 6, color feedback on hover/press (§8.3). */
export function Btn(props: {
  label?: string;
  icon?: IconName;
  children?: JSX.Element;
  variant?: Variant;
  size?: ControlSize;
  disabled?: boolean;
  onPress?: () => void;
  tooltip?: JSX.Element;
  style?: StyleProp<ViewStyle>;
  textStyle?: StyleProp<TextStyle>;
  testID?: string;
}) {
  const variant = props.variant ?? "outline";
  const v = VARIANT[variant];
  const isPrimary = variant === "primary";
  const size = props.size ?? "md";
  const height = CONTROL[size];
  const iconOnly = props.label === undefined && !props.children;
  const padLeft = props.icon ? (size === "lg" ? SPACE[12] : SPACE[8]) : size === "sm" ? SPACE[8] : SPACE[12];
  const padRight = size === "sm" ? SPACE[8] : SPACE[12];
  return (
    <View
      testID={props.testID}
      disabled={props.disabled}
      onPress={() => {
        if (!props.disabled) props.onPress?.();
      }}
      tooltip={props.tooltip}
      class={props.disabled ? v.disabledBox : v.box}
      activeStyle={isPrimary ? pressActiveStyle(props.disabled) : undefined}
      style={[
        {
          height,
          // An icon alone makes a square.
          ...(iconOnly ? { width: height } : { paddingLeft: padLeft, paddingRight: padRight }),
          borderRadius: RADIUS.sm,
          flexDirection: "row",
          alignItems: "center",
          justifyContent: "center",
          gap: SPACE[6],
          opacity: props.disabled ? 0.6 : 1,
          ...(isPrimary ? { transform: "scale(1)", ...pressTransition() } : colorTransition()),
        },
        props.style,
      ]}
    >
      {props.icon && <Icon name={props.icon} size={size === "sm" ? "sm" : "md"} tone={VARIANT_TONE[variant]} />}
      {props.label !== undefined && (
        <Text class={v.text} style={[{ fontSize: size === "sm" ? FONT_SIZE.caption : FONT_SIZE.body }, props.textStyle]}>
          {props.label}
        </Text>
      )}
      {props.children}
    </View>
  );
}

/** A square icon button with color feedback on hover/press (§8.3). */
export function IconBtn(props: {
  icon: IconName;
  tone?: IconTone;
  iconSize?: IconSize;
  onPress?: () => void;
  tooltip?: JSX.Element;
  disabled?: boolean;
  size?: ControlSize;
  style?: StyleProp<ViewStyle>;
  testID?: string;
}) {
  const size = props.size ?? "md";
  const px = CONTROL[size];
  return (
    <View
      testID={props.testID}
      disabled={props.disabled}
      onPress={() => {
        if (!props.disabled) props.onPress?.();
      }}
      tooltip={props.tooltip}
      class={props.disabled ? "" : `hover:bg-muted active:bg-accent ${FOCUS_RING}`}
      style={[
        {
          width: px,
          height: px,
          borderRadius: RADIUS.sm,
          alignItems: "center",
          justifyContent: "center",
          opacity: props.disabled ? 0.4 : 1,
          ...colorTransition(),
        },
        props.style,
      ]}
    >
      <Icon name={props.icon} size={props.iconSize ?? (size === "sm" ? "sm" : "md")} tone={props.tone} />
    </View>
  );
}

/** A rating in its tier's color (§3.3), `tabular-nums`; an unrated problem keeps the slot, empty. */
export function RatingBadge(props: { rating: number | null | undefined }) {
  const tier = () => ratingTier(props.rating);
  return (
    <View
      style={{
        height: 18, // token-exempt: compact inline rating badge height
        minWidth: 38,
        paddingLeft: SPACE[6],
        paddingRight: SPACE[6],
        borderRadius: RADIUS.xs,
        alignItems: "center",
        justifyContent: "center",
        backgroundColor: props.rating == null ? "transparent" : tierSoft(tier()),
      }}
    >
      <Text style={{ fontSize: FONT_SIZE.micro, fontWeight: FONT_WEIGHT.semibold, fontVariant: ["tabular-nums"], color: tierColor(tier()) }}>
        {props.rating == null ? "" : String(props.rating)}
      </Text>
    </View>
  );
}

/** A tag chip. */
export function Chip(props: { label: string; active?: boolean; onPress?: () => void; maxWidth?: ViewStyle["maxWidth"]; tooltip?: JSX.Element }) {
  const interactive = !!props.onPress;
  const cls = props.active
    ? interactive
      ? `bg-accent active:opacity-90 ${FOCUS_RING}`
      : "bg-accent"
    : interactive
      ? `bg-muted hover:bg-accent active:bg-accent ${FOCUS_RING}`
      : "bg-muted";
  return (
    <View
      onPress={props.onPress ? () => props.onPress!() : undefined}
      tooltip={props.tooltip}
      class={cls}
      style={{
        height: 20, // token-exempt: compact inline tag chip height
        maxWidth: props.maxWidth ?? "100%",
        minWidth: 0,
        overflow: "hidden",
        paddingLeft: SPACE[6],
        paddingRight: SPACE[6],
        borderRadius: RADIUS.xs,
        flexDirection: "row",
        alignItems: "center",
        ...colorTransition(),
      }}
    >
      <TruncText
        style={{ fontSize: FONT_SIZE.caption, flexShrink: 1, minWidth: 0, numberOfLines: 1, textOverflow: "ellipsis" }}
        class={props.active ? "text-foreground" : "text-secondary-foreground"}
      >
        {props.label}
      </TruncText>
    </View>
  );
}

/** The finalized lock (§6.5): the same line lock as everywhere, fading in when `closed` turns true. */
export function Lock(props: { closed: boolean; size?: IconSize }) {
  return (
    <Icon
      name="lock"
      size={props.size ?? "sm"}
      tone="muted-foreground"
      style={{ opacity: props.closed ? 1 : 0, ...opacityTransition() }}
    />
  );
}

/**
 * A segmented control of equal-width items with a sliding indicator (150 ms, §8):
 * the active item changes in the frame of the press.
 */
export function Segmented<T extends string>(props: {
  items: readonly { value: T; label: string; keys?: string; testID?: string }[];
  value: T;
  onChange: (value: T) => void;
  itemWidth?: number;
  underline?: boolean;
  centered?: boolean;
  disabled?: boolean;
}) {
  const w = props.itemWidth ?? 56;
  const index = () => Math.max(0, props.items.findIndex((i) => i.value === props.value));
  const trackStyle = (): ViewStyle =>
    props.underline
      ? { flexDirection: "row", alignSelf: "stretch", alignItems: "stretch", opacity: props.disabled ? 0.5 : 1 }
      : {
          flexDirection: "row",
          width: props.items.length * w + SPACE[2] * 2 + 2,
          flexShrink: 0,
          ...(props.centered ? { alignSelf: "center" } : {}),
          height: CONTROL.md,
          padding: SPACE[2],
          borderRadius: RADIUS.md,
          borderWidth: 1,
          alignItems: "stretch",
          opacity: props.disabled ? 0.5 : 1,
        };
  return (
    <View style={trackStyle()} class={props.underline ? undefined : "bg-muted border-border"}>
      <View
        style={{
          position: "absolute",
          width: w,
          ...(props.underline
            ? { left: 0, bottom: 0, height: 2, borderRadius: 1 } // token-exempt: 2px underline tab indicator
            : { left: SPACE[2], top: SPACE[2], bottom: SPACE[2], borderRadius: inner(RADIUS.md, SPACE[2]), boxShadow: cardShadow() }),
          transform: `translateX(${index() * w}px)`,
          ...indicatorTransition(),
        }}
        class={props.underline ? (windowActive() ? "bg-primary" : "bg-muted-foreground") : "bg-card"}
      />
      <Each of={props.items} key={(i) => i.value}>
        {(item) => {
          const active = () => item().value === props.value;
          return (
            <View
              testID={item().testID}
              disabled={props.disabled}
              onPress={() => {
                if (!props.disabled) props.onChange(item().value);
              }}
              tooltip={item().keys ? tip(item().label, item().keys) : undefined}
              class={
                props.disabled
                  ? ""
                  : active()
                    ? FOCUS_RING
                    : props.underline
                      ? `hover:bg-muted/60 active:bg-accent/60 rounded-[6px] ${FOCUS_RING}`
                      : `hover:bg-background/60 active:bg-accent/60 rounded-[6px] ${FOCUS_RING}`
              }
              style={{
                width: w,
                alignItems: "center",
                justifyContent: "center",
                ...colorTransition(),
              }}
            >
              <Text
                style={{
                  fontSize: FONT_SIZE.body,
                  numberOfLines: 1,
                  ...colorTransition(),
                }}
                class={active() ? "text-foreground" : props.disabled ? "text-secondary-foreground" : "text-secondary-foreground hover:text-foreground"}
              >
                {item().label}
              </Text>
            </View>
          );
        }}
      </Each>
    </View>
  );
}

/** A spinner that shows only after 150 ms (§9.4): faster work never blinks it. */
export function DelayedSpinner(props: { active: boolean; size?: number; class?: string }) {
  const shown = signal(false);
  let timer: number | undefined;
  effect(() => {
    const active = props.active;
    clearTimeout(timer);
    if (!active) shown.set(false);
    else timer = setTimeout(() => shown.set(true), 150);
  });
  cleanup(() => clearTimeout(timer));
  const size = props.size ?? 12;
  return (
    <View
      style={{
        width: size,
        height: size,
        borderRadius: size / 2,
        borderWidth: 1.5,
        borderTopColor: "transparent",
        opacity: shown() ? 1 : 0,
        animationName: "spin",
        animationDuration: 700,
        animationIterationCount: "infinite",
        animationTimingFunction: "linear",
        animationFrameRate: 30,
      }}
      class={props.class ?? "border-secondary-foreground"}
    />
  );
}

/** A field label above a control in the inspector and dialogs. */
export function FieldLabel(props: { label: string }) {
  return (
    <Text style={{ fontSize: FONT_SIZE.caption }} class="text-secondary-foreground">
      {props.label}
    </Text>
  );
}

/** A small downward chevron. */
export function Chevron(props: { size?: IconSize; tone?: IconTone }) {
  return <Icon name="chevronDown" size={props.size ?? "sm"} tone={props.tone ?? "muted-foreground"} />;
}

/**
 * Reusable in-place transient flag (`design-system.md` §6.5): flips to `true` for
 * `durationMs` and automatically clears its timer on unmount via `cleanup()`.
 */
export function createTransientFlag(durationMs = 1600): {
  active: () => boolean;
  trigger: (msOverride?: number) => void;
  clear: () => void;
} {
  const state = signal(false);
  let timer: ReturnType<typeof setTimeout> | undefined;
  const clear = () => {
    if (timer !== undefined) {
      clearTimeout(timer);
      timer = undefined;
    }
    state.set(false);
  };
  const trigger = (msOverride?: number) => {
    if (timer !== undefined) clearTimeout(timer);
    state.set(true);
    timer = setTimeout(() => {
      timer = undefined;
      state.set(false);
    }, msOverride ?? durationMs);
  };
  cleanup(() => {
    if (timer !== undefined) clearTimeout(timer);
  });
  return { active: () => state(), trigger, clear };
}

/**
 * Reusable in-place transient value (`design-system.md` §6.5): holds a value `T` for
 * `durationMs`, then resets to `undefined`, cleaning up its timer automatically on unmount.
 */
export function createTransientValue<T>(durationMs = 1600): {
  value: () => T | undefined;
  trigger: (val: T, msOverride?: number) => void;
  clear: () => void;
} {
  const state = signal<T | undefined>(undefined);
  let timer: ReturnType<typeof setTimeout> | undefined;
  const clear = () => {
    if (timer !== undefined) {
      clearTimeout(timer);
      timer = undefined;
    }
    state.set(undefined);
  };
  const trigger = (val: T, msOverride?: number) => {
    if (timer !== undefined) clearTimeout(timer);
    state.set(val);
    timer = setTimeout(() => {
      timer = undefined;
      state.set(undefined);
    }, msOverride ?? durationMs);
  };
  cleanup(() => {
    if (timer !== undefined) clearTimeout(timer);
  });
  return { value: () => state(), trigger, clear };
}

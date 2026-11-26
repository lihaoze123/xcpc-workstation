// A modal layer for the command palette, shortcuts overlay, and dialogs: centered,
// native spring `scale(0.94→1) + translateY(-6→0) + opacity` (§8). The backdrop's strip
// under the title bar still drags the window; a press elsewhere on it or Escape closes.

import { cleanup, effect, Overlay, signal, Text, View, type JSX } from "nsh";
import { Icon, type IconName, type IconTone } from "./Icon";
import { TITLE_BAR } from "./TitleBar";
import { backdropTransition, overlayCloseMs, overlayTransform, overlayTransition } from "./tokens";
import { modalBackdrop, modalShadow } from "./theme";

import { CONTROL, FONT_SIZE, FONT_WEIGHT, LINE_HEIGHT, RADIUS, SPACE } from "./tokens";
export function Modal(props: {
  open: boolean;
  onClose: () => void;
  width?: number;
  top?: number;
  topOffset?: number;
  children?: JSX.Element;
  testID?: string;
}) {
  const mounted = signal(props.open);
  const shown = signal(false);
  let raf: number | undefined;
  let closeTimer: number | undefined;

  effect(() => {
    const isOpen = props.open;
    if (raf !== undefined) {
      cancelAnimationFrame(raf);
      raf = undefined;
    }
    clearTimeout(closeTimer);
    if (isOpen) {
      mounted.set(true);
      raf = requestAnimationFrame(() => {
        raf = undefined;
        shown.set(true);
      });
    } else if (mounted()) {
      shown.set(false);
      const dur = overlayCloseMs();
      if (dur === 0) mounted.set(false);
      else closeTimer = setTimeout(() => mounted.set(false), dur);
    }
  });
  cleanup(() => {
    if (raf !== undefined) cancelAnimationFrame(raf);
    clearTimeout(closeTimer);
  });

  const topPos = () => props.topOffset ?? props.top;

  return (
    <If when={mounted()}>
      <Overlay
        style={{ top: 0, left: 0, right: 0, bottom: 0 }}
        onCancel={() => {
          if (props.open) props.onClose();
        }}
      >
        <View
          focusable={false}
          style={{
            position: "absolute",
            top: 0,
            left: 0,
            right: 0,
            bottom: 0,
            backgroundColor: modalBackdrop(),
            opacity: shown() ? 1 : 0,
            pointerEvents: props.open ? "auto" : "none",
            ...backdropTransition(),
          }}
          onPress={() => {
            if (props.open) props.onClose();
          }}
        />
        <View desktop={{ windowDrag: true }} style={{ position: "absolute", top: 0, left: 0, right: 0, height: TITLE_BAR }} />
        {/* Centered in the window (a little above the middle, as the eye expects), or at
            `topOffset` / `top` for the command palette. */}
        <View
          style={{
            position: "absolute",
            top: topPos() ?? 0,
            left: 0,
            right: 0,
            bottom: 0,
            alignItems: "center",
            justifyContent: topPos() === undefined ? "center" : "flex-start",
            paddingBottom: topPos() === undefined ? TITLE_BAR : 0,
            pointerEvents: "box-none",
          }}
        >
          <View
            testID={props.testID}
            style={{
              width: props.width ?? 420,
              maxWidth: "90%",
              maxHeight: "85%",
              borderRadius: RADIUS.lg,
              borderWidth: 1,
              overflow: "hidden",
              boxShadow: modalShadow(),
              opacity: shown() ? 1 : 0,
              transform: overlayTransform(shown()),
              ...overlayTransition(),
              pointerEvents: props.open ? "auto" : "none",
            }}
            class="bg-card border-border"
          >
            {props.children}
          </View>
        </View>
      </Overlay>
    </If>
  );
}

/** A dialog's head: an icon in a soft square, the title, and one line on what it does. */
export function DialogHeader(props: { icon: IconName; tone?: IconTone; title: string; description?: string }) {
  return (
    <View style={{ flexDirection: "row", alignItems: props.description ? "flex-start" : "center", gap: SPACE[12] }}>
      <View style={{ width: 32, height: CONTROL.lg, borderRadius: RADIUS.md, alignItems: "center", justifyContent: "center" }} class="bg-muted">
        <Icon name={props.icon} size="md" tone={props.tone ?? "foreground"} />
      </View>
      <View style={{ flex: 1, gap: SPACE[2], paddingTop: props.description ? SPACE[2] : SPACE[0] }}>
        <Text style={{ fontSize: FONT_SIZE.strong, lineHeight: LINE_HEIGHT.strong, fontWeight: FONT_WEIGHT.semibold }}>{props.title}</Text>
        {props.description && (
          <Text style={{ fontSize: FONT_SIZE.caption, lineHeight: LINE_HEIGHT.body }} class="text-secondary-foreground">
            {props.description}
          </Text>
        )}
      </View>
    </View>
  );
}

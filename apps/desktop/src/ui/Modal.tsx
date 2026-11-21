// A modal layer for the command palette, shortcuts overlay, and dialogs: centered,
// native spring `scale(0.94→1) + translateY(-6→0) + opacity` (§8). The backdrop's strip
// under the title bar still drags the window; a press elsewhere on it or Escape closes.

import { cleanup, effect, Overlay, signal, Text, View, type JSX } from "nsh";
import { Icon, type IconName, type IconTone } from "./Icon";
import { TITLE_BAR } from "./TitleBar";
import { backdropTransition, overlayTransform, overlayTransition } from "./motion";
import { modalBackdrop, modalShadow, ms } from "./theme";

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
      const dur = ms(220);
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
              borderRadius: 12,
              borderWidth: 1,
              overflow: "hidden",
              boxShadow: modalShadow(),
              opacity: shown() ? 1 : 0,
              transform: overlayTransform(shown()),
              ...overlayTransition(),
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
    <View style={{ flexDirection: "row", alignItems: props.description ? "flex-start" : "center", gap: 12 }}>
      <View style={{ width: 32, height: 32, borderRadius: 8, alignItems: "center", justifyContent: "center" }} class="bg-muted">
        <Icon name={props.icon} size={17} tone={props.tone ?? "foreground"} />
      </View>
      <View style={{ flex: 1, gap: 3, paddingTop: props.description ? 1 : 0 }}>
        <Text style={{ fontSize: 15, lineHeight: 20, fontWeight: 600 }}>{props.title}</Text>
        {props.description && (
          <Text style={{ fontSize: 12.5, lineHeight: 18 }} class="text-secondary-foreground">
            {props.description}
          </Text>
        )}
      </View>
    </View>
  );
}

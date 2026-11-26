// The 44-point title bar the app draws (`nsh.json` `titleBar: hidden, 44`): backed by
// `nsh/base`'s `TitleBar` for window-control safe areas and drag/double-click, while
// keeping the window's title in sync with what is open (§6.4).

import { effect, View, type JSX } from "nsh";
import { TitleBar as BaseTitleBar } from "nsh/base";
import { currentWindow, titleBar } from "nsh/desktop";

import { SPACE } from "./tokens";
export const TITLE_BAR = 44;

function readSecondaryTitleBarArea() {
  try {
    return titleBar();
  } catch {
    return { height: 0, left: 0, right: 0 };
  }
}

function isSecondaryWindow(): boolean {
  try {
    return currentWindow().id !== "main";
  } catch {
    return false;
  }
}

/** Keeps the window's title at `title()`. Call while rendering (it takes `currentWindow()`). */
export function useWindowTitle(title: () => string) {
  let win: ReturnType<typeof currentWindow> | undefined;
  try {
    win = currentWindow();
  } catch {
    win = undefined; // headless tests, off desktops
  }
  let last = "";
  effect(() => {
    const t = title();
    if (t === last) return;
    last = t;
    win?.set({ title: t });
  });
}

export function TitleBar(props: { leading?: JSX.Element; center?: JSX.Element; trailing?: JSX.Element; inset?: number; borderless?: boolean }) {
  const pad = props.inset ?? 12;
  const content = (
    <>
      {/* The center is centered on the window, not between the sides, so the command
          palette (centered on the window too) opens right over it. */}
      <View style={{ position: "absolute", left: 0, right: 0, top: 0, bottom: 0, alignItems: "center", justifyContent: "center", pointerEvents: "box-none" }}>
        {props.center}
      </View>
      <View style={{ flex: 1, minWidth: 0, flexDirection: "row", alignItems: "center", gap: SPACE[8], paddingLeft: pad, paddingRight: pad }}>
        <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[6], flexShrink: 1, minWidth: 0, overflow: "hidden" }}>{props.leading}</View>
        <View style={{ flex: 1, minWidth: 4 }} />
        <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[6], flexShrink: 0 }}>{props.trailing}</View>
      </View>
    </>
  );

  // `nsh/base` TitleBar calculates right-edge insets using the main window's width,
  // which drops the Windows caption-button safe area to 0 on narrower secondary windows.
  // Full-width secondary windows reserve `titleBar().left/right` directly.
  if (isSecondaryWindow()) {
    const area = () => readSecondaryTitleBarArea();
    return (
      <View
        desktop={{ windowDrag: true }}
        style={{
          flexDirection: "row",
          alignItems: "center",
          flexShrink: 0,
          height: TITLE_BAR,
          minHeight: area().height,
          paddingLeft: area().left,
          paddingRight: area().right,
          borderBottomWidth: props.borderless ? 0 : 1,
        }}
        class={props.borderless ? "bg-background" : "border-border bg-background"}
      >
        {content}
      </View>
    );
  }

  return (
    <BaseTitleBar
      style={{
        height: TITLE_BAR,
        borderBottomWidth: props.borderless ? 0 : 1,
      }}
      class={props.borderless ? "bg-background" : "border-border bg-background"}
    >
      {content}
    </BaseTitleBar>
  );
}

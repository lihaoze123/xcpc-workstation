// Single-slot bottom-right toast for long-running user-triggered tasks and menu/shortcut
// completions (design-system §6.5 tier 3). New notifications replace the current one rather
// than stacking, hover pauses auto-dismiss, and when the PDF preview zoom pill is visible
// the toast lifts above it so it never overlaps preview controls.

import { cleanup, effect, signal, Text, View } from "nsh";
import { Icon } from "./Icon";
import { FOCUS_RING } from "./kit";
import { isReducedMotion, overlayCloseMs, overlayTransition, toastTransform } from "./tokens";
import { popoverShadow } from "./theme";

import { FONT_SIZE, FONT_WEIGHT, LINE_HEIGHT, RADIUS, SPACE } from "./tokens";
export type ToastTone = "success" | "issues" | "info";

export interface ToastAction {
  label: string;
  run: () => void;
}

export interface ToastPayload {
  id: number;
  tone: ToastTone;
  text: string;
  action?: ToastAction;
  durationMs: number;
}

const activeToast = signal<ToastPayload | undefined>(undefined);
const toastBottomLift = signal(false);

let nextToastId = 1;
let dismissTimer: number | undefined;
let hovered = false;

function scheduleDismiss(payload: ToastPayload) {
  clearTimeout(dismissTimer);
  dismissTimer = setTimeout(() => {
    if (activeToast()?.id === payload.id && !hovered) {
      activeToast.set(undefined);
    }
  }, payload.durationMs);
}

/** Shows a single-slot toast in the bottom-right corner, replacing any active toast. */
export function showToast(opts: {
  tone: ToastTone;
  text: string;
  action?: ToastAction;
  durationMs?: number;
}): void {
  const durationMs = opts.durationMs ?? (opts.tone === "issues" ? 4500 : 3000);
  const payload: ToastPayload = {
    id: nextToastId++,
    tone: opts.tone,
    text: opts.text,
    action: opts.action,
    durationMs,
  };
  hovered = false;
  activeToast.set(payload);
  scheduleDismiss(payload);
}

/** Dismisses the currently visible toast immediately. */
export function dismissToast(): void {
  clearTimeout(dismissTimer);
  dismissTimer = undefined;
  hovered = false;
  activeToast.set(undefined);
}

/** Lifts the toast above the preview canvas's bottom-right zoom pill when preview is visible. */
export function setToastBottomLift(lifted: boolean): void {
  if (toastBottomLift() !== lifted) toastBottomLift.set(lifted);
}

/** Reads the current toast payload (for tests). */
export function currentToast(): ToastPayload | undefined {
  return activeToast();
}

export function ToastHost() {
  const rendered = signal<ToastPayload | undefined>(activeToast());
  const shown = signal(false);
  const closing = signal(false);
  let raf: number | undefined;
  let unmountTimer: number | undefined;

  effect(() => {
    const next = activeToast();
    if (raf !== undefined) {
      cancelAnimationFrame(raf);
      raf = undefined;
    }
    clearTimeout(unmountTimer);
    if (next) {
      closing.set(false);
      rendered.set(next);
      if (isReducedMotion()) {
        shown.set(true);
      } else {
        raf = requestAnimationFrame(() => {
          raf = undefined;
          shown.set(true);
        });
      }
    } else if (rendered()) {
      closing.set(true);
      shown.set(false);
      const dur = overlayCloseMs();
      if (dur === 0) {
        rendered.set(undefined);
        closing.set(false);
      } else {
        unmountTimer = setTimeout(() => {
          rendered.set(undefined);
          closing.set(false);
        }, dur);
      }
    }
  });

  cleanup(() => {
    if (raf !== undefined) cancelAnimationFrame(raf);
    clearTimeout(unmountTimer);
  });

  return (
    <If when={rendered()}>
      {(item) => (
        <View
          testID="app-toast"
          onMouseEnter={() => {
            hovered = true;
            clearTimeout(dismissTimer);
          }}
          onMouseLeave={() => {
            hovered = false;
            const cur = activeToast();
            if (cur) scheduleDismiss({ ...cur, durationMs: 1200 });
          }}
          class="bg-popover border border-border"
          style={{
            position: "absolute",
            right: 20,
            bottom: 20,
            maxWidth: 420,
            minHeight: 36,
            paddingLeft: SPACE[12],
            paddingRight: SPACE[12],
            paddingTop: SPACE[8],
            paddingBottom: SPACE[8],
            borderRadius: RADIUS.md,
            flexDirection: "row",
            alignItems: "center",
            gap: SPACE[8],
            boxShadow: popoverShadow(),
            opacity: shown() ? 1 : 0,
            transform: toastTransform(shown() || closing(), toastBottomLift() ? -48 : 0),
            ...overlayTransition(!closing()),
          }}
        >
          <If when={item().tone === "success"}>
            <Icon name="check" size="sm" tone="chart-2" />
          </If>
          <If when={item().tone === "issues"}>
            <View style={{ width: 7, height: 7, borderRadius: RADIUS.xs, flexShrink: 0 }} class="bg-destructive" />
          </If>
          <If when={item().tone === "info"}>
            <Icon name="info" size="sm" tone="secondary-foreground" />
          </If>
          <Text
            testID="app-toast-text"
            style={{ flexShrink: 1, minWidth: 0, fontSize: FONT_SIZE.body, lineHeight: LINE_HEIGHT.body, numberOfLines: 1, textOverflow: "ellipsis", fontVariant: ["tabular-nums"] }}
            class="text-foreground"
          >
            {item().text}
          </Text>
          <If when={item().action}>
            {(act) => (
              <View
                testID="app-toast-action"
                onPress={() => {
                  const run = act().run;
                  dismissToast();
                  run();
                }}
                class={FOCUS_RING}
                style={{ borderRadius: RADIUS.xs, flexShrink: 0, paddingLeft: SPACE[4], paddingRight: SPACE[4] }}
              >
                <Text
                  style={{ fontSize: FONT_SIZE.caption, fontWeight: FONT_WEIGHT.semibold }}
                  class="text-primary"
                >
                  {act().label}
                </Text>
              </View>
            )}
          </If>
        </View>
      )}
    </If>
  );
}

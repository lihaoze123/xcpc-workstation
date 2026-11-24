// Single-slot bottom-right toast for long-running user-triggered tasks and menu/shortcut
// completions (design-system §6.5 tier 3). New notifications replace the current one rather
// than stacking, hover pauses auto-dismiss, and when the PDF preview zoom pill is visible
// the toast lifts above it so it never overlaps preview controls.

import { cleanup, effect, reduceMotion, signal, Text, View } from "nsh";
import { Icon } from "./Icon";
import { EASE_STANDARD, ms, popoverShadow } from "./theme";

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
      rendered.set(next);
      raf = requestAnimationFrame(() => {
        raf = undefined;
        shown.set(true);
      });
    } else if (rendered()) {
      shown.set(false);
      const dur = ms(150);
      if (dur === 0) rendered.set(undefined);
      else unmountTimer = setTimeout(() => rendered.set(undefined), dur);
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
            bottom: toastBottomLift() ? 68 : 20,
            maxWidth: 420,
            minHeight: 36,
            paddingLeft: 12,
            paddingRight: 12,
            paddingTop: 7,
            paddingBottom: 7,
            borderRadius: 9,
            flexDirection: "row",
            alignItems: "center",
            gap: 8,
            boxShadow: popoverShadow(),
            opacity: shown() ? 1 : 0,
            transform: reduceMotion() ? undefined : shown() ? "translateY(0px)" : "translateY(6px)",
            transitionProperty: "opacity, transform",
            transitionDuration: ms(150),
            transitionTimingFunction: EASE_STANDARD,
          }}
        >
          <If when={item().tone === "success"}>
            <Icon name="check" size={14} tone="chart-2" />
          </If>
          <If when={item().tone === "issues"}>
            <View style={{ width: 7, height: 7, borderRadius: 3.5, flexShrink: 0 }} class="bg-destructive" />
          </If>
          <If when={item().tone === "info"}>
            <Icon name="info" size={14} tone="secondary-foreground" />
          </If>
          <Text
            testID="app-toast-text"
            style={{ flexShrink: 1, minWidth: 0, fontSize: 13, lineHeight: 18, numberOfLines: 1, textOverflow: "ellipsis", fontVariant: ["tabular-nums"] }}
            class="text-foreground"
          >
            {item().text}
          </Text>
          <If when={item().action}>
            {(act) => (
              <Text
                testID="app-toast-action"
                onPress={() => {
                  const run = act().run;
                  dismissToast();
                  run();
                }}
                style={{ fontSize: 12.5, fontWeight: 500, flexShrink: 0, paddingLeft: 4 }}
                class="text-primary"
              >
                {act().label}
              </Text>
            )}
          </If>
        </View>
      )}
    </If>
  );
}

// Motion tokens and centralized reduced-motion / slow-motion handling (Phase 6G §4, §6, §7).
// All UI components must consume motion through this module instead of calling `reduceMotion()`,
// writing `transitionDuration: ms(<number>)`, or authoring inline `spring(...)` strings.

import { reduceMotion, type MotionSpring, type ViewStyle } from "nsh";
import { Preview } from "xcpc-workstation/native";

/** Three-tier warm physical spring scale (ζ = 0.872–0.895, overshoot ≤ 1%). */
export const SPRING = {
  /** Solid primary button press `scale(0.97)`: ζ = 0.872. */
  quick: { stiffness: 500, damping: 39, mass: 1 },
  /** Overlays, sliding indicators, structural layout, drag reorder settle: ζ = 0.875. */
  standard: { stiffness: 400, damping: 35, mass: 1 },
  /** Side panels, drawers, studio enter/exit: ζ = 0.895. */
  smooth: { stiffness: 300, damping: 31, mass: 1 },
} as const satisfies Record<string, MotionSpring>;

export type SpringToken = keyof typeof SPRING;

/** Two-tier duration scale (all paired with `EASE_STANDARD`). */
export const DURATION = {
  /** Overlay close pure fade-out (ms). */
  fast: 100,
  /** Hover/press color, icon fade, deleted row fade, NavRail cross-group fade, reduced-motion fade (ms). */
  base: 120,
} as const;

export type DurationToken = keyof typeof DURATION;

export const EASE_STANDARD = "cubic-bezier(0.2, 0, 0, 1)" as const;

let slowFactorOverride: number | undefined;
let cachedSlowFactor: number | undefined;
let reducedMotionOverride: boolean | undefined;

/** Returns the active motion slow-down factor (`XCPC_MOTION_SLOW` in dev builds, always `1` in release). */
export function motionSlowFactor(): number {
  if (slowFactorOverride !== undefined) return slowFactorOverride;
  if (cachedSlowFactor === undefined) {
    try {
      const f = Preview.motionSlowFactor();
      cachedSlowFactor = Number.isFinite(f) && f > 1 ? f : 1;
    } catch {
      cachedSlowFactor = 1;
    }
  }
  return cachedSlowFactor;
}

/** Test-only hook for overriding `XCPC_MOTION_SLOW`. */
export function setMotionSlowFactorForTest(factor: number | undefined): void {
  slowFactorOverride = factor;
}

/** Test-only hook for overriding `prefers-reduced-motion`. */
export function setReducedMotionForTest(reduced: boolean | undefined): void {
  reducedMotionOverride = reduced;
}

/** Centralized `prefers-reduced-motion` query. */
export function isReducedMotion(): boolean {
  return reducedMotionOverride !== undefined ? reducedMotionOverride : reduceMotion();
}

/** Scales a duration (ms) by `motionSlowFactor()`. */
export function scaledDuration(msVal: number): number {
  if (msVal <= 0) return 0;
  return Math.round(msVal * motionSlowFactor());
}

/** Scales a spring preset by `motionSlowFactor()` while preserving damping ratio ζ:
 *  stiffness ÷ f², damping ÷ f. */
export function scaledSpring(preset: MotionSpring): MotionSpring {
  const f = motionSlowFactor();
  const k = preset.stiffness ?? 400;
  const c = preset.damping ?? 35;
  const m = preset.mass ?? 1;
  if (f <= 1) return { stiffness: k, damping: c, mass: m };
  return {
    stiffness: Number((k / (f * f)).toFixed(4)),
    damping: Number((c / f).toFixed(4)),
    mass: m,
  };
}

/** Formats a `MotionSpring` as NSH's native `transitionTimingFunction: spring(...)`. */
export function springCss(preset: MotionSpring): `spring(${string})` {
  const s = scaledSpring(preset);
  const k = s.stiffness ?? 400;
  const c = s.damping ?? 35;
  const m = s.mass ?? 1;
  return m === 1 ? `spring(${k}, ${c})` : `spring(${k}, ${c}, ${m})`;
}

/** Hover / press / selection color feedback (`DURATION.base`, preserved under reduced motion). */
export function colorTransition(): ViewStyle {
  return {
    transitionProperty: "colors",
    transitionDuration: scaledDuration(DURATION.base),
    transitionTimingFunction: EASE_STANDARD,
  };
}

/** Pure opacity fade (`DURATION.fast` or `DURATION.base`, preserved under reduced motion). */
export function opacityTransition(speed: DurationToken = "base"): ViewStyle {
  return {
    transitionProperty: "opacity",
    transitionDuration: scaledDuration(DURATION[speed]),
    transitionTimingFunction: EASE_STANDARD,
  };
}

/** Chevron / disclosure rotation (`DURATION.base`, instant under reduced motion). */
export function chevronTransition(): ViewStyle {
  if (isReducedMotion()) {
    return { transitionProperty: "transform", transitionDuration: 0, transitionTimingFunction: "linear" };
  }
  return {
    transitionProperty: "transform",
    transitionDuration: scaledDuration(DURATION.base),
    transitionTimingFunction: EASE_STANDARD,
  };
}

/** Solid primary button press: color + `scale(0.97)` via `SPRING.quick` (color-only under reduced motion). */
export function pressTransition(): ViewStyle {
  if (isReducedMotion()) return colorTransition();
  return {
    transitionProperty: "colors, opacity, transform",
    transitionTimingFunction: springCss(SPRING.quick),
  };
}

/** Active press transform (`scale(0.97)`) reserved strictly for solid primary buttons. */
export function pressActiveStyle(disabled?: boolean): ViewStyle | undefined {
  if (disabled || isReducedMotion()) return undefined;
  return { transform: "scale(0.97)" };
}

/** Sliding nav / segmented indicator (`SPRING.standard`, snaps directly under reduced motion). */
export function indicatorTransition(): ViewStyle {
  if (isReducedMotion()) {
    return { transitionProperty: "transform, opacity", transitionDuration: 0, transitionTimingFunction: "linear" };
  }
  return {
    transitionProperty: "transform, opacity",
    transitionTimingFunction: springCss(SPRING.standard),
  };
}

/** Studio enter/exit transition (`SPRING.smooth`; `DURATION.base` pure fade under reduced motion). */
export function studioTransition(): ViewStyle {
  if (isReducedMotion()) return opacityTransition("base");
  return {
    transitionProperty: "opacity, transform",
    transitionTimingFunction: springCss(SPRING.smooth),
  };
}

/** Studio enter/exit transform (`translateX(14px <-> 0px)`; omitted under reduced motion). */
export function studioTransform(visible: boolean): string | undefined {
  if (isReducedMotion()) return undefined;
  return visible ? "translateX(0px)" : "translateX(14px)";
}

/** Time (ms) to keep an exiting studio mounted before unmounting. */
export function studioExitMs(): number {
  return scaledDuration(isReducedMotion() ? DURATION.base : 340);
}

/** Side panel / drawer transition (`SPRING.smooth`; `DURATION.base` pure fade under reduced motion). */
export function panelTransition(): ViewStyle {
  if (isReducedMotion()) return opacityTransition("base");
  return {
    transitionProperty: "opacity, transform",
    transitionTimingFunction: springCss(SPRING.smooth),
  };
}

/** Side panel / drawer transform (`translateX(offsetPx -> 0px)`; omitted under reduced motion). */
export function panelTransform(shown: boolean, offsetPx: number): string | undefined {
  if (isReducedMotion()) return undefined;
  return shown ? "translateX(0px)" : `translateX(${offsetPx}px)`;
}

/** Time (ms) to keep a closing side panel mounted before unmounting. */
export function panelExitMs(): number {
  return scaledDuration(isReducedMotion() ? DURATION.base : 220);
}

/** Overlay transition: `SPRING.standard` on open, `DURATION.fast` pure fade on close
 *  (`DURATION.base` pure fade under reduced motion). */
export function overlayTransition(open = true): ViewStyle {
  if (isReducedMotion()) return opacityTransition("base");
  if (!open) return opacityTransition("fast");
  return {
    transitionProperty: "opacity, transform",
    transitionTimingFunction: springCss(SPRING.standard),
  };
}

/** Overlay backdrop scrim fade (`DURATION.fast`, or `DURATION.base` under reduced motion). */
export function backdropTransition(): ViewStyle {
  return opacityTransition(isReducedMotion() ? "base" : "fast");
}

/** Overlay scale transform (`scale(0.96 -> 1)`, no `translateY`; omitted under reduced motion). */
export function overlayTransform(open: boolean): string | undefined {
  if (isReducedMotion()) return undefined;
  return open ? "scale(1)" : "scale(0.96)";
}

/** Time (ms) to keep a closing overlay mounted while it fades out. */
export function overlayCloseMs(): number {
  return scaledDuration(isReducedMotion() ? DURATION.base : DURATION.fast);
}

/** Toast transform (6px rise on entry + optional drag offset; drag-only under reduced motion). */
export function toastTransform(shown: boolean, dragY = 0): string | undefined {
  if (isReducedMotion()) {
    return dragY !== 0 ? `translateY(${dragY}px)` : undefined;
  }
  const entry = shown ? 0 : 6;
  return `translateY(${entry + dragY}px)`;
}

/** Structural deletion fade duration (`DURATION.base`, or `0` under reduced motion). */
export function structuralFadeMs(): number {
  return isReducedMotion() ? 0 : scaledDuration(DURATION.base);
}

/** `ViewRef.animateLayout` spring config (`SPRING.standard`, or `undefined` under reduced motion). */
export function layoutSpringConfig(): { type: "spring"; stiffness: number; damping: number; mass: number } | undefined {
  if (isReducedMotion()) return undefined;
  const s = scaledSpring(SPRING.standard);
  return {
    type: "spring",
    stiffness: s.stiffness ?? 400,
    damping: s.damping ?? 35,
    mass: s.mass ?? 1,
  };
}

/** Whether programmatic scrolling should animate (`false` under reduced motion). */
export function scrollAnimated(animated: boolean): boolean {
  return animated && !isReducedMotion();
}

/** Whether spatial motion is enabled (inverse of reduced motion). */
export function spatialEnabled(): boolean {
  return !isReducedMotion();
}

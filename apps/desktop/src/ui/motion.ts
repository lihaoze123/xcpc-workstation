// Unified warm physical motion primitives (§8): well-damped springs (ζ = 0.86–0.90,
// overshoot ≤ 1%) running on NSH's native platform frame clock (interruptible, zero
// layout reflow), automatically degrading under `prefers-reduced-motion`.

import { Easing, reduceMotion, type AnimatedValue, type MotionSpring, type ViewStyle } from "nsh";
import { EASE_STANDARD, ms } from "./theme";

/** Warm physical springs (§8): ζ = 0.86–0.90 (overshoot ≤ 1%), fluid and interruptible on native clock. */
/** Floating surfaces (modal / palette / menu / slash menu): ζ = 0.875. */
export const SPRING_OVERLAY: MotionSpring = { stiffness: 400, damping: 35, mass: 1 };
/** Sliding navigation / segmented indicators: ζ = 0.870. */
export const SPRING_INDICATOR: MotionSpring = { stiffness: 360, damping: 33, mass: 1 };
/** Side drawers / outline / preview panels: ζ = 0.895. */
export const SPRING_PANEL: MotionSpring = { stiffness: 300, damping: 31, mass: 1 };
/** Space & studio page transitions (soft deceleration): ζ = 0.899. */
export const SPRING_PAGE: MotionSpring = { stiffness: 260, damping: 29, mass: 1 };
/** Solid primary button press micro-rebound (`scale(0.97)`): snappy, ζ = 0.872. */
export const SPRING_PRESS: MotionSpring = { stiffness: 500, damping: 39, mass: 1 };
/** Gentle ambient spring for subtle emphasis: ζ = 0.871. */
export const SPRING_GENTLE: MotionSpring = { stiffness: 240, damping: 27, mass: 1 };

/** Exponential ease-out curve (`cubic-bezier(0.16, 1, 0.3, 1)`) for `Animated.timing`. */
export const EASE_EXPO_CURVE = Easing.bezier(0.16, 1, 0.3, 1);

/** Formats a `MotionSpring` as NSH's native `transitionTimingFunction: spring(...)`.
 *  NSH springs time themselves until settled (`spring.settle_ms()`), so `transitionDuration`
 *  is intentionally omitted when using `springCss(...)`. */
export function springCss(preset: MotionSpring): `spring(${string})` {
  const k = preset.stiffness ?? 400;
  const c = preset.damping ?? 35;
  const m = preset.mass ?? 1;
  return m === 1 ? `spring(${k}, ${c})` : `spring(${k}, ${c}, ${m})`;
}

/** Floating surface (modal / command palette / menu / slash menu) spring transition (§8). */
export function overlayTransition(): ViewStyle {
  if (reduceMotion()) {
    return { transitionProperty: "opacity", transitionDuration: 0, transitionTimingFunction: "linear" };
  }
  return {
    transitionProperty: "opacity, transform",
    transitionTimingFunction: springCss(SPRING_OVERLAY),
  };
}

/** Subtle backdrop scrim fade transition. */
export function backdropTransition(): ViewStyle {
  return {
    transitionProperty: "opacity",
    transitionDuration: ms(200),
    transitionTimingFunction: EASE_STANDARD,
  };
}

/** Sliding nav / segmented indicator spring transition (interruptible, GPU transform). */
export function indicatorTransition(): ViewStyle {
  if (reduceMotion()) {
    return { transitionProperty: "transform, opacity", transitionDuration: 0, transitionTimingFunction: "linear" };
  }
  return {
    transitionProperty: "transform, opacity",
    transitionTimingFunction: springCss(SPRING_INDICATOR),
  };
}

/** Side drawer / floating panel / outline spring transition (transform + opacity only). */
export function panelTransition(): ViewStyle {
  if (reduceMotion()) {
    return { transitionProperty: "opacity", transitionDuration: 0, transitionTimingFunction: "linear" };
  }
  return {
    transitionProperty: "opacity, transform",
    transitionTimingFunction: springCss(SPRING_PANEL),
  };
}

/** Space & studio view reveal transition (GPU opacity + transform only; spring self-times). */
export function pageTransition(_duration?: number): ViewStyle {
  if (reduceMotion()) {
    return { transitionProperty: "opacity", transitionDuration: 0, transitionTimingFunction: "linear" };
  }
  return {
    transitionProperty: "opacity, transform",
    transitionTimingFunction: springCss(SPRING_PAGE),
  };
}

/** Page / studio container transform states, disabled under `prefers-reduced-motion`.
 *  Uses pure GPU pixel translation (no `scale(...)`) so GTK4 avoids full-screen FBO
 *  resampling while still conveying spatial depth: spaces lift softly on Y (`6px -> 0`),
 *  and entering the studio glides forward on X (`14px -> 0`). */
export function pageTransform(visible: boolean, variant: "space" | "studio" = "space"): string | undefined {
  if (reduceMotion()) return undefined;
  if (variant === "studio") {
    return visible ? "translateX(0px)" : "translateX(14px)";
  }
  return visible ? "translateY(0px)" : "translateY(6px)";
}

/** Crisp tactile color transition for rows and chips (120–160ms, zero scale jitter). */
export function colorTransition(duration = 140): ViewStyle {
  return {
    transitionProperty: "colors",
    transitionDuration: ms(duration),
    transitionTimingFunction: EASE_STANDARD,
  };
}

/** Primary button press transition: color + GPU `scale(0.97)` micro-rebound (`SPRING_PRESS`), disabled under reduced motion. */
export function pressTransition(duration = 140): ViewStyle {
  if (reduceMotion()) {
    return colorTransition(duration);
  }
  return {
    transitionProperty: "colors, opacity, transform",
    transitionTimingFunction: springCss(SPRING_PRESS),
  };
}

/** Active press transform (`scale(0.97)`) reserved strictly for solid primary buttons; omitted when disabled or reduced motion. */
export function pressActiveStyle(disabled?: boolean): ViewStyle | undefined {
  if (disabled || reduceMotion()) return undefined;
  return { transform: "scale(0.97)" };
}

/** Floating surface transform states (`scale(0.94→1) + translateY(-6→0)`), disabled under reduced motion. */
export function overlayTransform(open: boolean): string | undefined {
  if (reduceMotion()) return undefined;
  return open ? "translateY(0px) scale(1)" : "translateY(-6px) scale(0.94)";
}

/** Springs an `AnimatedValue` to `toValue`, snapping immediately under `prefers-reduced-motion`. */
export function springTo(value: AnimatedValue, toValue: number, preset: MotionSpring = SPRING_OVERLAY): Promise<{ finished: boolean }> {
  if (reduceMotion()) {
    value.set(toValue);
    return Promise.resolve({ finished: true });
  }
  return value.spring(toValue, preset);
}

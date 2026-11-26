// Design token invariants and static guardrail scanner tests (Phase 6D §2.4).
// The full-repo filesystem scan also runs under `nsh test` via `apps/desktop/src/tests.rs`
// (`static_ui_design_token_guardrail`) so `nsh.json` does not need `capabilities.fs`.

import { describe, expect, test } from "nsh/test";
import { FOCUS_RING } from "./kit";
import { computeDropDeltaY, computeReleaseVelocity } from "./studio/reorder";
import { MONO, SANS, setWindowActiveForTest, windowActive } from "./theme";
import {
  anchorOrigin,
  colorTransition,
  CONTROL,
  DURATION,
  FONT_SIZE,
  FONT_WEIGHT,
  GUTTER,
  ICON_SIZE,
  ICON_STROKE_PX,
  iconStrokeWidth,
  indicatorTransition,
  inner,
  layoutSpringConfig,
  LINE_HEIGHT,
  overlayCloseMs,
  overlayTransform,
  overlayTransition,
  popupEndingStyle,
  popupStartingStyle,
  pressActiveStyle,
  pressTransition,
  RADIUS,
  ROW,
  scaledDuration,
  scaledSpring,
  setMotionSlowFactorForTest,
  setReducedMotionForTest,
  SPACE,
  SPRING,
  springCss,
  structuralFadeMs,
  studioTransform,
  studioTransition,
  TOOLBAR,
  TYPE,
} from "./tokens";

export interface TokenViolation {
  file: string;
  line: number;
  rule: string;
  snippet: string;
}

const RAW_STYLE_PROP_RE =
  /\b(fontSize|fontWeight|gap|columnGap|rowGap|padding|paddingTop|paddingBottom|paddingLeft|paddingRight|paddingHorizontal|paddingVertical|margin|marginTop|marginBottom|marginLeft|marginRight|marginHorizontal|marginVertical|borderRadius|borderTopLeftRadius|borderTopRightRadius|borderBottomLeftRadius|borderBottomRightRadius)\s*:\s*(-?\d+(?:\.\d+)?)\b/g;

const RAW_CONTROL_HEIGHT_RE =
  /\bheight\s*:\s*(18|19|20|21|22|23|24|25|26|27|28|29|30|31|32|33|34|35|36|37|38|39|40)\b/g;

const RAW_ICON_SIZE_RE = /<Icon\b[^>]*?\bsize=\{\s*\d+(?:\.\d+)?\s*\}/g;
const RAW_ICON_STROKE_RE = /<Icon\b[^>]*?\bstrokeWidth=/g;
const EXEMPT_RE = /token-exempt:(.*)$/;

function hasValidExemption(line: string, prevLine: string | undefined): "valid" | "empty" | "none" {
  for (const candidate of [line, prevLine ?? ""]) {
    const m = EXEMPT_RE.exec(candidate);
    if (m) {
      const reason = m[1].replace(/\*\/\}?\s*$/, "").trim();
      return reason.length > 0 ? "valid" : "empty";
    }
  }
  return "none";
}

/**
 * Scans a TSX source file for raw design-token literals and non-enum `<Icon>` usage.
 * Honors single-line `// token-exempt: <reason>` or `{/* token-exempt: <reason> *\/}` on the
 * same line or immediately preceding line; an empty reason is itself a violation.
 */
export function scanSourceForTokenViolations(file: string, source: string): TokenViolation[] {
  const violations: TokenViolation[] = [];
  const lines = source.split(/\r?\n/);

  for (let i = 0; i < lines.length; i += 1) {
    const line = lines[i];
    const prev = i > 0 ? lines[i - 1] : undefined;
    const exempt = hasValidExemption(line, prev);

    if (exempt === "empty") {
      violations.push({
        file,
        line: i + 1,
        rule: "token-exempt-empty-reason",
        snippet: line.trim(),
      });
      continue;
    }
    if (exempt === "valid") continue;

    for (const m of line.matchAll(RAW_STYLE_PROP_RE)) {
      violations.push({
        file,
        line: i + 1,
        rule: `raw-${m[1]}`,
        snippet: `${m[1]}: ${m[2]}`,
      });
    }
    for (const m of line.matchAll(RAW_CONTROL_HEIGHT_RE)) {
      violations.push({
        file,
        line: i + 1,
        rule: "raw-control-height",
        snippet: `height: ${m[1]}`,
      });
    }
    for (const m of line.matchAll(RAW_ICON_SIZE_RE)) {
      violations.push({
        file,
        line: i + 1,
        rule: "raw-icon-size",
        snippet: m[0],
      });
    }
    for (const m of line.matchAll(RAW_ICON_STROKE_RE)) {
      violations.push({
        file,
        line: i + 1,
        rule: "raw-icon-stroke",
        snippet: m[0],
      });
    }
  }
  return violations;
}

describe("Design tokens (Phase 6D §2)", () => {
  test("typography scale matches §2.1 exact values and restricts weights to 400 & 600", () => {
    expect(Object.values(FONT_WEIGHT).sort()).toEqual([400, 600]);
    expect(FONT_SIZE).toEqual({
      micro: 11,
      caption: 12,
      body: 13,
      strong: 14,
      title: 18,
      display: 26,
    });
    expect(LINE_HEIGHT).toEqual({
      micro: 14,
      caption: 16,
      body: 19,
      strong: 20,
      title: 24,
      display: 30,
    });
    expect(TYPE.micro).toEqual({ fontSize: 11, lineHeight: 14, fontWeight: 600 });
    expect(TYPE.caption).toEqual({ fontSize: 12, lineHeight: 16, fontWeight: 400 });
    expect(TYPE.body).toEqual({ fontSize: 13, lineHeight: 19, fontWeight: 400 });
    expect(TYPE.strong).toEqual({ fontSize: 14, lineHeight: 20, fontWeight: 600 });
    expect(TYPE.title).toEqual({ fontSize: 18, lineHeight: 24, fontWeight: 600, letterSpacing: -0.25 });
    expect(TYPE.display).toEqual({
      fontSize: 26,
      lineHeight: 30,
      fontWeight: 600,
      letterSpacing: -0.5,
      fontVariant: ["tabular-nums"],
    });
  });

  test("tracking and font stacks match Phase 6E §1 & §3", () => {
    expect(TYPE.title.letterSpacing).toBe(-0.25);
    expect(TYPE.display.letterSpacing).toBe(-0.5);
    expect(TYPE.micro.letterSpacing).toBeUndefined();
    expect(TYPE.caption.letterSpacing).toBeUndefined();
    expect(TYPE.body.letterSpacing).toBeUndefined();
    expect(TYPE.strong.letterSpacing).toBeUndefined();

    const expectedCjk = `"Noto Sans CJK SC", "Source Han Sans SC", "Microsoft YaHei UI", "Microsoft YaHei"`;
    expect(SANS).toBe(`Inter, ${expectedCjk}, sans-serif`);
    expect(MONO).toBe(`"JetBrains Mono", ${expectedCjk}, monospace`);
    expect(SANS).not.toContain("Maple Mono");
    expect(MONO).not.toContain("Maple Mono");
  });

  test("spacing, radii, concentric inner(), control heights, and row heights match §2.2", () => {
    expect(Object.values(SPACE).sort((a, b) => a - b)).toEqual([0, 2, 4, 6, 8, 12, 16, 20, 24, 32]);
    expect(GUTTER).toBe(20);
    expect(RADIUS).toEqual({ none: 0, xs: 4, sm: 6, md: 8, lg: 12, full: 9999 });
    expect(inner(RADIUS.md, SPACE[2])).toBe(6);
    expect(inner(RADIUS.lg, SPACE[4])).toBe(8);
    expect(inner(RADIUS.xs, SPACE[8])).toBe(0);
    expect(CONTROL).toEqual({ sm: 24, md: 28, lg: 32 });
    expect(ROW).toEqual({ list: 40, table: 36, menu: 32 });
    expect(TOOLBAR).toBe(52);
  });

  test("icon sizes and constant 1.5px visual stroke width match §2.3", () => {
    expect(ICON_SIZE).toEqual({ sm: 14, md: 16, lg: 20 });
    expect(ICON_STROKE_PX).toBe(1.5);
    expect(iconStrokeWidth("sm")).toBe(2.57);
    expect(iconStrokeWidth("md")).toBe(2.25);
    expect(iconStrokeWidth("lg")).toBe(1.8);
  });
});

describe("Token guardrail scanner (§2.4)", () => {
  test("accepts tokenized styles and valid token-exempt comments", () => {
    const ok = `
      <View style={{ height: CONTROL.md, gap: SPACE[6], paddingLeft: SPACE[12], borderRadius: RADIUS.sm }}>
        <Icon name="search" size="sm" />
        <Text style={{ fontSize: FONT_SIZE.body, fontWeight: FONT_WEIGHT.regular }}>ok</Text>
        <View style={{ height: 18 }} /> // token-exempt: compact inline badge height
        {/* token-exempt: heatmap cell corner radius */}
        <View style={{ borderRadius: 2.5 }} />
      </View>
    `;
    expect(scanSourceForTokenViolations("Sample.tsx", ok)).toEqual([]);
  });

  test("flags raw style literals, control heights, numeric Icon size, strokeWidth, and empty exemptions", () => {
    const bad = [
      `<Text style={{ fontSize: 12.5, fontWeight: 500 }}>bad</Text>`,
      `<View style={{ gap: 10, paddingLeft: 14, borderRadius: 5, height: 26 }} />`,
      `<Icon name="plus" size={14} strokeWidth={2} />`,
      `<View style={{ height: 20 }} /> // token-exempt:`,
    ].join("\n");
    const violations = scanSourceForTokenViolations("Bad.tsx", bad);
    const rules = violations.map((v) => v.rule);
    expect(rules).toContain("raw-fontSize");
    expect(rules).toContain("raw-fontWeight");
    expect(rules).toContain("raw-gap");
    expect(rules).toContain("raw-paddingLeft");
    expect(rules).toContain("raw-borderRadius");
    expect(rules).toContain("raw-control-height");
    expect(rules).toContain("raw-icon-size");
    expect(rules).toContain("raw-icon-stroke");
    expect(rules).toContain("token-exempt-empty-reason");
  });
});

describe("State completeness primitives (Phase 6F)", () => {
  test("windowActive defaults active and reacts to focus/blur override", () => {
    setWindowActiveForTest(undefined);
    expect(windowActive()).toBe(true);
    setWindowActiveForTest(false);
    expect(windowActive()).toBe(false);
    setWindowActiveForTest(true);
    expect(windowActive()).toBe(true);
    setWindowActiveForTest(undefined);
  });

  test("FOCUS_RING uses zero-layout-shift 1px inset ring", () => {
    expect(FOCUS_RING).toBe("focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-ring");
  });
});

describe("Motion tokens, slow-motion scaling, and reduced-motion tiers (Phase 6G §4, §6, §7)", () => {
  test("SPRING and DURATION match exact spec values", () => {
    expect(SPRING.quick).toEqual({ stiffness: 500, damping: 39, mass: 1 });
    expect(SPRING.standard).toEqual({ stiffness: 400, damping: 35, mass: 1 });
    expect(SPRING.smooth).toEqual({ stiffness: 300, damping: 31, mass: 1 });
    expect(DURATION).toEqual({ fast: 100, base: 120 });
  });

  test("XCPC_MOTION_SLOW scales durations by f and springs by k/f², c/f while preserving ζ", () => {
    try {
      setMotionSlowFactorForTest(10);
      expect(scaledDuration(DURATION.fast)).toBe(1000);
      expect(scaledDuration(DURATION.base)).toBe(1200);
      const s = scaledSpring(SPRING.standard);
      expect(s.stiffness).toBe(4);
      expect(s.damping).toBe(3.5);
      expect(s.mass).toBe(1);
      const zetaOrig = SPRING.standard.damping / (2 * Math.sqrt(SPRING.standard.stiffness));
      const zetaScaled = s.damping! / (2 * Math.sqrt(s.stiffness! * s.mass!));
      expect(Math.abs(zetaScaled - zetaOrig)).toBeLessThan(1e-6);
      expect(springCss(SPRING.standard)).toBe("spring(4, 3.5)");
    } finally {
      setMotionSlowFactorForTest(undefined);
    }
  });

  test("reduced-motion degrades spatial/structural motion while preserving color feedback", () => {
    try {
      setReducedMotionForTest(false);
      expect(studioTransform(false)).toBe("translateX(14px)");
      expect(overlayTransform(false)).toBe("scale(0.96)");
      expect(pressActiveStyle(false)).toEqual({ transform: "scale(0.97)" });
      expect(overlayTransition(true).transitionTimingFunction).toBe("spring(400, 35)");
      expect(overlayTransition(false).transitionProperty).toBe("opacity");
      expect(overlayTransition(false).transitionDuration).toBe(DURATION.fast);
      expect(structuralFadeMs()).toBe(DURATION.base);
      expect(layoutSpringConfig()?.type).toBe("spring");

      setReducedMotionForTest(true);
      // Spatial motion: no translation/scale; studio & overlay become DURATION.base opacity fade; indicator snaps
      expect(studioTransform(false)).toBeUndefined();
      expect(studioTransition().transitionProperty).toBe("opacity");
      expect(studioTransition().transitionDuration).toBe(DURATION.base);
      expect(overlayTransform(false)).toBeUndefined();
      expect(overlayTransition(true).transitionProperty).toBe("opacity");
      expect(overlayTransition(true).transitionDuration).toBe(DURATION.base);
      expect(overlayCloseMs()).toBe(DURATION.base);
      expect(indicatorTransition().transitionDuration).toBe(0);
      expect(pressActiveStyle(false)).toBeUndefined();
      expect(pressTransition().transitionProperty).toBe("colors");
      // Structural changes: instant, no delete fade
      expect(structuralFadeMs()).toBe(0);
      expect(layoutSpringConfig()).toBeUndefined();
      // Color feedback: preserved
      expect(colorTransition().transitionDuration).toBe(DURATION.base);
    } finally {
      setReducedMotionForTest(undefined);
    }
  });

  test("anchorOrigin and popup ending style grow from trigger corner and fade out without scaling (Phase 6G §3)", () => {
    expect(anchorOrigin("bottom", "start")).toBe("top left");
    expect(anchorOrigin("bottom", "end")).toBe("top right");
    expect(anchorOrigin("top", "start")).toBe("bottom left");
    expect(anchorOrigin("top", "center")).toBe("bottom center");
    expect(popupStartingStyle()).toEqual({ opacity: 0, transform: "scale(0.96)" });
    const end = popupEndingStyle();
    expect(end.opacity).toBe(0);
    expect(end.transform).toBeUndefined();
    expect(end.transitionProperty).toBe("opacity");
    expect(end.transitionDuration).toBe(DURATION.fast);
  });

  test("drag reorder drop delta preserves exact screen position and velocity over trailing 50ms (Phase 6G §5)", () => {
    // Variable-height cards (e.g. Samples): row 0 (100px), row 1 (200px), gap 12px.
    const slots = [
      { top: 0, height: 100 },
      { top: 112, height: 200 },
      { top: 324, height: 80 },
    ];
    // Moving row 0 down to index 1 while dragged 180px:
    // visualTop = 0 + 180 = 180; newSlotTop = 112 + 200 - 100 = 212 -> deltaY = -32
    expect(computeDropDeltaY(slots, 0, 1, 180)).toBe(-32);
    // Moving row 2 up to index 0 while dragged -300px:
    // visualTop = 324 - 300 = 24; newSlotTop = 0 -> deltaY = 24
    expect(computeDropDeltaY(slots, 2, 0, -300)).toBe(24);
    // Dropping on same index returns raw offset
    expect(computeDropDeltaY(slots, 1, 1, 15)).toBe(15);

    // Trailing 50ms velocity
    const samples = [
      { t: 100, y: 0 },
      { t: 180, y: 10 },
      { t: 200, y: 20 },
      { t: 220, y: 40 },
    ];
    // Window [170..220]: oldest inside/at cutoff is t=180 (y=10) -> (40-10)/40ms = 750 pt/s
    expect(computeReleaseVelocity(samples, 220)).toBe(750);
  });
});

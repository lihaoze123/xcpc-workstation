// Spacing, corner radii, control/row heights, and icon sizing tokens (Phase 6D §2.2–§2.3).

/** 9-step spacing scale (plus 0 reset): 2 · 4 · 6 · 8 · 12 · 16 · 20 · 24 · 32. */
export const SPACE = {
  0: 0,
  2: 2,
  4: 4,
  6: 6,
  8: 8,
  12: 12,
  16: 16,
  20: 20,
  24: 24,
  32: 32,
} as const;

export type SpaceToken = keyof typeof SPACE;

/** Standard horizontal page gutter (20pt). */
export const GUTTER = SPACE[20];

/** Standard corner radius scale. Nested radii must use `inner(outer, padding)`. */
export const RADIUS = {
  none: 0,
  /** Badges, keycaps. */
  xs: 4,
  /** Buttons, inputs, list rows, menu rows. */
  sm: 6,
  /** Segmented pill tracks, menus, popovers. */
  md: 8,
  /** Cards, dialogs, inset panels. */
  lg: 12,
  /** Pills, circles, avatars. */
  full: 9999,
} as const;

export type RadiusToken = keyof typeof RADIUS;

/** Concentric inner corner radius: `max(0, outer - padding)`. */
export function inner(outer: number, padding: number): number {
  return Math.max(0, outer - padding);
}

/** Interactive control heights (buttons, inputs, segmented pills). */
export const CONTROL = {
  /** Card header / inline table icon buttons & compact pills. */
  sm: 24,
  /** Toolbar controls: search inputs, buttons, segmented pills. */
  md: 28,
  /** Dialog footer buttons, menu rows, command palette rows. */
  lg: 32,
} as const;

export type ControlSize = keyof typeof CONTROL;

/** Standard row heights. */
export const ROW = {
  list: 40,
  table: 36,
  menu: 32,
} as const;

export type RowKind = keyof typeof ROW;

/** Standard space toolbar height (52pt). */
export const TOOLBAR = 52;

/** Vector icon sizes (14 / 16 / 20 pt). */
export const ICON_SIZE = {
  /** Pairs with `CONTROL.sm` (24pt) controls and inline table cells. */
  sm: 14,
  /** Default icon size. */
  md: 16,
  /** Navigation rail, dialog header icon boxes, empty-state icon boxes. */
  lg: 20,
} as const;

export type IconSize = keyof typeof ICON_SIZE;

/** Target constant visual stroke width in pixels across all icon sizes. */
export const ICON_STROKE_PX = 1.5;

/**
 * Computes Lucide `strokeWidth` (in 24×24 viewBox units) for constant 1.5px visual thickness:
 * `sm` (14) -> 2.57, `md` (16) -> 2.25, `lg` (20) -> 1.8.
 */
export function iconStrokeWidth(size: IconSize): number {
  return Math.round(((ICON_STROKE_PX * 24) / ICON_SIZE[size]) * 100) / 100;
}

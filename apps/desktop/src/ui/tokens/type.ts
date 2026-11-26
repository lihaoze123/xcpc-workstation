// Typography tokens (Phase 6D §2.1).
// Only two weights exist: 400 (regular) and 600 (semibold).
// Hierarchy comes from size, color, or weight — never stacking all three.

import type { TextStyle } from "nsh";

export const FONT_WEIGHT = {
  regular: 400,
  semibold: 600,
} as const;

export type FontWeightToken = keyof typeof FONT_WEIGHT;

export const FONT_SIZE = {
  micro: 11,
  caption: 12,
  body: 13,
  strong: 14,
  title: 18,
  display: 26,
} as const;

export type FontSizeToken = keyof typeof FONT_SIZE;

export const LINE_HEIGHT = {
  micro: 14,
  caption: 16,
  body: 19,
  strong: 20,
  title: 24,
  display: 30,
} as const;

export const TYPE: Record<FontSizeToken, TextStyle> = {
  /** Keycaps, badges, heatmap axis labels. */
  micro: { fontSize: FONT_SIZE.micro, lineHeight: LINE_HEIGHT.micro, fontWeight: FONT_WEIGHT.semibold },
  /** Metadata, secondary descriptions, table headers, subtitles. */
  caption: { fontSize: FONT_SIZE.caption, lineHeight: LINE_HEIGHT.caption, fontWeight: FONT_WEIGHT.regular },
  /** Primary body copy, list rows, buttons, inputs, menus. */
  body: { fontSize: FONT_SIZE.body, lineHeight: LINE_HEIGHT.body, fontWeight: FONT_WEIGHT.regular },
  /** Section headings, card titles. */
  strong: { fontSize: FONT_SIZE.strong, lineHeight: LINE_HEIGHT.strong, fontWeight: FONT_WEIGHT.semibold },
  /** DetailHeader titles. */
  title: { fontSize: FONT_SIZE.title, lineHeight: LINE_HEIGHT.title, fontWeight: FONT_WEIGHT.semibold, letterSpacing: -0.25 },
  /** Training overview hero figures (`tabular-nums`). */
  display: {
    fontSize: FONT_SIZE.display,
    lineHeight: LINE_HEIGHT.display,
    fontWeight: FONT_WEIGHT.semibold,
    letterSpacing: -0.5,
    fontVariant: ["tabular-nums"],
  },
};

export type TypeToken = keyof typeof TYPE;

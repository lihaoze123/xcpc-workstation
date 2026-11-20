import { colorScheme, computed, reduceMotion, setTheme, signal, token } from "nsh";
export { token };

// The design tokens of docs/design-system.md §3 on shadcn's names, which the classes
// know at build time (`nsh.json`'s theme names them; the app's own names would not
// compile): --bg-canvas `background`, --bg-surface `card`, --bg-elevated `muted`,
// --bg-paper-stage `sidebar`, --border-subtle `border`, --border-strong `input`,
// --text-secondary `secondary-foreground`, --text-muted `muted-foreground`,
// --accent-primary `primary`, --accent-hover `sidebar-primary`, the selected state's
// light gray `accent` (one step above the `muted` hover),
// a warning's amber `chart-4`, solid danger button fill `chart-5` (split from
// `destructive` text so dark-mode white-on-red buttons keep ≥4.5:1 contrast).
setTheme({
  cssVars: {
    theme: { radius: "8px" },
    light: {
      background: "#FAFAFA",
      foreground: "#09090B",
      card: "#FFFFFF",
      "card-foreground": "#09090B",
      popover: "#FFFFFF",
      "popover-foreground": "#09090B",
      muted: "#F4F4F5",
      "muted-foreground": "#A1A1AA",
      border: "#E4E4E7",
      input: "#D4D4D8",
      primary: "#0B6989",
      "primary-foreground": "#FFFFFF",
      accent: "#EBEBED",
      "accent-foreground": "#09090B",
      secondary: "#F4F4F5",
      "secondary-foreground": "#52525B",
      sidebar: "#E4E4E7",
      "sidebar-primary": "#09546D",
      "chart-4": "#B45309",
      "chart-5": "#B91C1C",
      ring: "#0B6989",
      destructive: "#B91C1C",
    },
    dark: {
      background: "#09090B",
      foreground: "#FAFAFA",
      card: "#111113",
      "card-foreground": "#FAFAFA",
      popover: "#18181B",
      "popover-foreground": "#FAFAFA",
      muted: "#18181B",
      "muted-foreground": "#71717A",
      border: "#27272A",
      input: "#3F3F46",
      primary: "#0E7FA6",
      "primary-foreground": "#FFFFFF",
      accent: "#232327",
      "accent-foreground": "#FAFAFA",
      secondary: "#18181B",
      "secondary-foreground": "#A1A1AA",
      sidebar: "#141417",
      "sidebar-primary": "#118EB8",
      "chart-4": "#FBBF24",
      "chart-5": "#DC2626",
      ring: "#249AC2",
      destructive: "#F87171",
    },
  },
});

/** §8's curves. */
export const EASE_STANDARD = "cubic-bezier(0.2, 0, 0, 1)";
export const EASE_DRAWER = "cubic-bezier(0.16, 1, 0.3, 1)";
export const EASE_EXPO = "cubic-bezier(0.16, 1, 0.3, 1)";

/** A duration, 0 when the system asks for reduced motion. */
export function ms(duration: number): number {
  return reduceMotion() ? 0 : duration;
}

/** §3.3: the 7 Codeforces / AtCoder tiers and `unrated`, as the Core names them. */
export const TIERS = ["newbie", "pupil", "specialist", "expert", "candidate_master", "master", "grandmaster", "unrated"] as const;
export type Tier = (typeof TIERS)[number];

const TIER_TEXT: Record<Tier, [light: string, dark: string]> = {
  newbie: ["#52525B", "#A1A1AA"],
  pupil: ["#15803D", "#4ADE80"],
  specialist: ["#0E7490", "#22D3EE"],
  expert: ["#1D4ED8", "#60A5FA"],
  candidate_master: ["#7E22CE", "#C084FC"],
  master: ["#C2410C", "#FB923C"],
  grandmaster: ["#B91C1C", "#F87171"],
  unrated: ["#71717A", "#71717A"],
};

/** The tier's text color in the appearance shown. */
export function tierColor(tier: Tier): string {
  return TIER_TEXT[tier][colorScheme() === "dark" ? 1 : 0];
}

/** The tier's badge background: its color at 10–12 %. */
export function tierSoft(tier: Tier): string {
  const hex = tierColor(tier);
  const alpha = tier === "newbie" || tier === "unrated" ? "1A" : "1F";
  return `${hex}${alpha}`;
}

/** The UI sans-serif font stack (§5). */
export const UI_SANS = "'Inter', 'Source Han Sans SC', 'Noto Sans CJK SC', system-ui, sans-serif";

/** The monospaced family of code and samples (§5). */
export const MONO = "'JetBrains Mono', 'Maple Mono', monospace";

/** The monospaced family for the Markdown/code editor with CJK fallback (§5). */
export const EDITOR_MONO = "'JetBrains Mono', 'Noto Sans CJK SC', 'Source Han Sans SC', monospace";

/** Flipped to `true` after `registerFont` finishes loading bundled JetBrains Mono faces. */
export const fontsReady = signal(false);

/** Reactive editor font family: switches to `EDITOR_MONO` after `registerFont` completes so `Editor` invalidates its cached glyph-width metrics. */
export const editorMono = computed(() => (fontsReady() ? EDITOR_MONO : "monospace"));

/** Reactive monospace font family for read-only code views. */
export const monoFont = computed(() => (fontsReady() ? MONO : "monospace"));

/**
 * Paper-and-ink semantic highlight classes for `MarkdownEditor`, reactive to `colorScheme()`.
 * Kept restrained on light/dark ink palettes so math commands and fenced code stay legible
 * without turning the statement source into a neon IDE theme.
 */
export interface EditorInk {
  readonly mathCommand: string;
  readonly mathOperator: string;
  readonly mathDelimiterMatch: string;
  readonly codeKeyword: string;
  readonly codeComment: string;
  readonly codeString: string;
  readonly codeNumber: string;
}

const EDITOR_INK_LIGHT: EditorInk = {
  mathCommand: "text-[#7C5338]",
  mathOperator: "text-[#9A7456]",
  mathDelimiterMatch: "font-semibold text-[#7C5338] bg-[#7C5338]/12",
  codeKeyword: "text-[#1D5A73]",
  codeComment: "italic text-[#8A8A93]",
  codeString: "text-[#2B6648]",
  codeNumber: "text-[#8C5328]",
};

const EDITOR_INK_DARK: EditorInk = {
  mathCommand: "text-[#D4A373]",
  mathOperator: "text-[#B8936A]",
  mathDelimiterMatch: "font-semibold text-[#D4A373] bg-[#D4A373]/18",
  codeKeyword: "text-[#76B5CE]",
  codeComment: "italic text-[#787882]",
  codeString: "text-[#74B894]",
  codeNumber: "text-[#D69F6E]",
};

/** Returns the active appearance's paper-and-ink editor syntax classes (reactive). */
export function editorInk(): EditorInk {
  return colorScheme() === "dark" ? EDITOR_INK_DARK : EDITOR_INK_LIGHT;
}

/** Solid danger button fill (split from `destructive` text for ≥4.5:1 white-on-red contrast in dark mode). */
export function dangerSolid(): string {
  return colorScheme() === "dark" ? "#DC2626" : "#B91C1C";
}

/** The dark appearance's 1px brightened inner rim of floating layers (§4); none in light. */
export function rim(): string {
  return colorScheme() === "dark" ? "rgba(255,255,255,0.12)" : "rgba(0,0,0,0)";
}

/** Modal backdrop scrim color: deeper in dark mode for clear layer separation. */
export function modalBackdrop(): string {
  return colorScheme() === "dark" ? "rgba(0,0,0,0.48)" : "rgba(9,9,11,0.12)";
}

/** Shadow + dark inner rim for modal dialogs and the command palette (§4). */
export function modalShadow(): string {
  return colorScheme() === "dark"
    ? `0 16px 40px rgba(0,0,0,0.52), 0 2px 8px rgba(0,0,0,0.28), inset 0 0 0 1px ${rim()}`
    : "0 12px 32px rgba(0,0,0,0.16), 0 2px 6px rgba(0,0,0,0.08)";
}

/** Shadow + dark inner rim for menus, popovers, tooltips, and floating panels (§4). */
export function popoverShadow(): string {
  return colorScheme() === "dark"
    ? `0 10px 28px rgba(0,0,0,0.44), 0 2px 6px rgba(0,0,0,0.24), inset 0 0 0 1px ${rim()}`
    : "0 8px 24px rgba(0,0,0,0.12), 0 1px 3px rgba(0,0,0,0.06)";
}

/** Subtle ambient shadow for ordinary cards (§4). */
export function cardShadow(): string {
  return colorScheme() === "dark" ? "0 1px 2px rgba(0,0,0,0.24)" : "0 1px 2px rgba(0,0,0,0.035)";
}

import { batch, colorScheme, computed, reduceMotion, setColorScheme, setTheme, signal, token } from "nsh";
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
const BASE_LIGHT_THEME: Record<string, string> = {
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
};

const BASE_DARK_THEME: Record<string, string> = {
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
};

setTheme({
  cssVars: {
    theme: { radius: "8px" },
    light: BASE_LIGHT_THEME,
    dark: BASE_DARK_THEME,
  },
});

const customEditorFontFamily = signal<string | undefined>(undefined);

/** Applies global `<dataDir>/theme.toml` overrides. */
export function applyThemeOverrides(overrides: {
  lightColors: readonly { key: string; value: string }[];
  darkColors: readonly { key: string; value: string }[];
  radius?: number | null;
  editorFontFamily?: string | null;
  editorFontSize?: number | null;
  editorLineHeight?: number | null;
}): void {
  const light = { ...BASE_LIGHT_THEME };
  const dark = { ...BASE_DARK_THEME };
  for (const { key, value } of overrides.lightColors) light[key] = value;
  for (const { key, value } of overrides.darkColors) dark[key] = value;
  const radiusPx = overrides.radius != null && Number.isFinite(overrides.radius) ? `${Math.round(overrides.radius)}px` : "8px";
  setTheme({
    cssVars: {
      theme: { radius: radiusPx },
      light,
      dark,
    },
  });
  customEditorFontFamily.set(overrides.editorFontFamily?.trim() || undefined);
  if (overrides.editorFontSize != null && Number.isFinite(overrides.editorFontSize)) {
    setEditorFontSize(overrides.editorFontSize);
  }
  if (overrides.editorLineHeight != null && Number.isFinite(overrides.editorLineHeight)) {
    const ratio = overrides.editorLineHeight > 3 ? overrides.editorLineHeight / editorFontSize() : overrides.editorLineHeight;
    setEditorLineHeight(ratio);
  }
}

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

/** The monospaced family for the Markdown/code editor with cross-platform monospaced CJK fallback (§5). */
export const EDITOR_MONO = "'JetBrains Mono', 'Noto Sans Mono CJK SC', 'Source Han Sans HW SC', 'Sarasa Mono SC', 'NSimSun', monospace";

/** Flipped to `true` after `registerFont` finishes loading bundled JetBrains Mono faces. */
export const fontsReady = signal(false);

/** Reactive editor font family: switches to `EDITOR_MONO` (or `theme.toml` override) after `registerFont` completes. */
export const editorMono = computed(() => {
  const custom = customEditorFontFamily();
  if (custom) return `'${custom}', 'Noto Sans Mono CJK SC', 'Source Han Sans HW SC', 'Sarasa Mono SC', 'NSimSun', monospace`;
  return fontsReady() ? EDITOR_MONO : "monospace";
});

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

/** Shadow + dark inner rim for modal dialogs and the command palette (§4).
 *  Kept single-layer with a tight blur radius so GTK4 HiDPI paints stay within 120Hz budget. */
export function modalShadow(): string {
  return colorScheme() === "dark"
    ? `0 8px 18px rgba(0,0,0,0.44), inset 0 0 0 1px ${rim()}`
    : "0 6px 14px rgba(0,0,0,0.14)";
}

/** Shadow + dark inner rim for menus, popovers, tooltips, and floating panels (§4). */
export function popoverShadow(): string {
  return colorScheme() === "dark"
    ? `0 6px 14px rgba(0,0,0,0.38), inset 0 0 0 1px ${rim()}`
    : "0 4px 12px rgba(0,0,0,0.11)";
}

/** Subtle ambient shadow for ordinary cards (§4). */
export function cardShadow(): string {
  return colorScheme() === "dark" ? "0 1px 2px rgba(0,0,0,0.24)" : "0 1px 2px rgba(0,0,0,0.035)";
}

// ── Appearance & editor typography preferences (Phase 2.5) ───────────────────

export type AppearanceMode = "system" | "light" | "dark";

export const DEFAULT_EDITOR_FONT_SIZE = 13.5;
export const DEFAULT_EDITOR_LINE_HEIGHT = 1.55;
export const DEFAULT_EDITOR_LIGATURES = true;
export const DEFAULT_EDITOR_VIM_MODE = false;

/** User-selected appearance mode: `"system"`, `"light"`, or `"dark"`. */
export const appearanceMode = signal<AppearanceMode>("system");

/** Editor font size in points (clamped to 11..20). */
export const editorFontSize = signal<number>(DEFAULT_EDITOR_FONT_SIZE);

/** Editor line-height multiplier (clamped to 1.25..2.0). */
export const editorLineHeight = signal<number>(DEFAULT_EDITOR_LINE_HEIGHT);

/** Whether programming ligatures are enabled in the code/statement editor. */
export const editorLigatures = signal<boolean>(DEFAULT_EDITOR_LIGATURES);

/** Whether practical-subset Vim mode is enabled in the Markdown editor (default off). */
export const editorVimMode = signal<boolean>(DEFAULT_EDITOR_VIM_MODE);

/** Switches the app's appearance immediately via NSH's native `setColorScheme`. */
export function setAppearanceMode(mode: AppearanceMode): void {
  const next: AppearanceMode = mode === "light" || mode === "dark" ? mode : "system";
  appearanceMode.set(next);
  try {
    setColorScheme(next);
  } catch {
    // Headless test environments may not expose a window appearance host.
  }
}

/** Updates the editor font size (clamped to 11..20 pt). */
export function setEditorFontSize(size: number): void {
  if (!Number.isFinite(size)) return;
  editorFontSize.set(Math.min(20, Math.max(11, Math.round(size * 2) / 2)));
}

/** Updates the editor line-height multiplier (clamped to 1.25..2.0). */
export function setEditorLineHeight(lineHeight: number): void {
  if (!Number.isFinite(lineHeight)) return;
  editorLineHeight.set(Math.min(2.0, Math.max(1.25, Math.round(lineHeight * 20) / 20)));
}

/** Toggles ligatures in the editor. */
export function setEditorLigatures(enabled: boolean): void {
  editorLigatures.set(Boolean(enabled));
}

/** Toggles Vim practical-subset mode in the editor. */
export function setEditorVimMode(enabled: boolean): void {
  editorVimMode.set(Boolean(enabled));
}

export interface AppearancePrefs {
  appearance?: AppearanceMode;
  editorFontSize?: number;
  editorLineHeight?: number;
  editorLigatures?: boolean;
  editorVimMode?: boolean;
}

/** Applies a batch of persisted appearance & editor typography preferences. */
export function applyAppearancePrefs(prefs: AppearancePrefs): void {
  batch(() => {
    if (prefs.appearance !== undefined) setAppearanceMode(prefs.appearance);
    if (prefs.editorFontSize !== undefined) setEditorFontSize(prefs.editorFontSize);
    if (prefs.editorLineHeight !== undefined) setEditorLineHeight(prefs.editorLineHeight);
    if (prefs.editorLigatures !== undefined) setEditorLigatures(prefs.editorLigatures);
    if (prefs.editorVimMode !== undefined) setEditorVimMode(prefs.editorVimMode);
  });
}

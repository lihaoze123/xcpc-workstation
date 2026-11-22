// Zero-dependency strongly-typed i18n runtime (`docs/specs/i18n.md` §4).
// Reactive `signal` / `computed` state drives hot-switching across the entire UI in one frame.

import { computed, signal } from "nsh";
import { en } from "./en";
import { zhCN, type MessageKey } from "./zh-CN";

export type { MessageKey };
export { zhCN, en };

export type Locale = "zh-CN" | "en";
export type LanguagePref = "system" | Locale;
export type LocalizedText = string | Record<string, string>;

const DICTS: Record<Locale, Record<MessageKey, string>> = {
  "zh-CN": zhCN,
  en,
};

/** Normalizes any OS locale tag (e.g. `"zh_CN.UTF-8"`, `"en-US"`, `"C"`) into a supported `Locale`. */
export function normalizeLocale(raw: string | null | undefined): Locale {
  if (!raw) return "en";
  return raw.trim().toLowerCase().startsWith("zh") ? "zh-CN" : "en";
}

/** Validates a stored language preference string, defaulting to `"system"`. */
export function normalizeLanguagePref(raw: string | null | undefined): LanguagePref {
  if (raw === "zh-CN" || raw === "en" || raw === "system") return raw;
  return "system";
}

/** Detected OS locale, initialized at startup from `preview.systemLocale()`. Defaults to `"zh-CN"` in headless tests. */
export const systemLocale = signal<Locale>("zh-CN");

/** User-selected language preference (`"system"` | `"zh-CN"` | `"en"`), persisted in `prefs.json`. */
export const languagePref = signal<LanguagePref>("system");

/** Effective active UI locale, reactive to both `languagePref` and `systemLocale`. */
export const locale = computed<Locale>(() => {
  const pref = languagePref();
  return pref === "system" ? systemLocale() : pref;
});

/** Translates `key` in the active `locale()`, interpolating `{name}` placeholders when `params` is provided. */
export function t(key: MessageKey, params?: Record<string, string | number>): string {
  const dict = DICTS[locale()];
  const template = dict[key] ?? zhCN[key] ?? key;
  if (!params) return template;
  return template.replace(/\{(\w+)\}/g, (match, name) =>
    Object.prototype.hasOwnProperty.call(params, name) ? String(params[name]) : match,
  );
}

function normalizeTag(tag: string): string {
  return tag.trim().toLowerCase().replace(/_/g, "-");
}

function primarySubtag(normTag: string): string {
  const dash = normTag.indexOf("-");
  return dash === -1 ? normTag : normTag.slice(0, dash);
}

/**
 * Resolves a polymorphic `LocalizedText` (plain string or BCP 47 dictionary) using the exact
 * 6-step fallback chain specified in `docs/specs/i18n.md` §3.2 (matching Rust `LocalizedText::resolve`):
 * 1. Plain string -> return as-is.
 * 2. Dictionary:
 *    a. Exact match (case-insensitive, `_` === `-`).
 *    b. Primary subtag match (exact primary subtag first, else first sorted key with same primary subtag e.g. `zh-Hans`).
 *    c. Fallback to `"en"` (or primary `"en"`).
 *    d. Fallback to `"zh"` / `"zh-CN"` (or primary `"zh"`).
 *    e. Fallback to first non-empty value sorted lexicographically by key.
 *    f. Otherwise `""`.
 */
export function resolveLocalized(
  text: LocalizedText | undefined | null,
  loc?: Locale | string,
): string {
  if (text == null) return "";
  if (typeof text === "string") return text;
  if (typeof text !== "object") return "";

  const entries: [string, string][] = Object.entries(text)
    .filter((entry): entry is [string, string] => typeof entry[1] === "string" && entry[1].trim().length > 0)
    .sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0));

  if (entries.length === 0) return "";

  const target = normalizeTag(loc ?? locale());
  const targetPrimary = primarySubtag(target);

  // 1. Exact match (case-insensitive, _ === -)
  for (const [k, v] of entries) {
    if (normalizeTag(k) === target) return v;
  }

  // 2. Primary subtag match (prefer exact primary tag, then first sorted key with same primary tag)
  if (targetPrimary) {
    for (const [k, v] of entries) {
      if (normalizeTag(k) === targetPrimary) return v;
    }
    for (const [k, v] of entries) {
      if (primarySubtag(normalizeTag(k)) === targetPrimary) return v;
    }
  }

  // 3. Fallback to "en"
  for (const [k, v] of entries) {
    if (normalizeTag(k) === "en") return v;
  }
  for (const [k, v] of entries) {
    if (primarySubtag(normalizeTag(k)) === "en") return v;
  }

  // 4. Fallback to "zh" / "zh-CN"
  for (const [k, v] of entries) {
    const nk = normalizeTag(k);
    if (nk === "zh" || nk === "zh-cn") return v;
  }
  for (const [k, v] of entries) {
    if (primarySubtag(normalizeTag(k)) === "zh") return v;
  }

  // 5. First non-empty value sorted by key
  return entries[0][1];
}

// Lightweight inline locale helper for Training space labels without modifying i18n dictionaries.

import { locale } from "../i18n";

export function isEn(): boolean {
  return locale() === "en";
}

export function zhEn(zh: string, en: string): string {
  return isEn() ? en : zh;
}

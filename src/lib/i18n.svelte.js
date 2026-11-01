// Minimal localization. The Vietnamese text is the key and is also the source language:
// t("Tiến trình") returns it unchanged for Vietnamese and looks it up in `en.js` for English.
// Placeholders use {name} and are filled from the params object.
//
// It reads `settings.lang` (reactive state), so any template or $derived that calls t() re-renders
// when the language changes.

import { EN } from "./en.js";
import { settings } from "./settings.svelte.js";

export function t(text, params) {
  let s = settings.lang === "en" ? (EN[text] ?? text) : text;
  if (params) s = s.replace(/\{(\w+)\}/g, (_, k) => String(params[k] ?? ""));
  return s;
}

/** Locale for dates and numbers */
export const locale = () => (settings.lang === "en" ? "en-US" : "vi-VN");

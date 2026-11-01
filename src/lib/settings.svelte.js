import { invoke } from "@tauri-apps/api/core";

const KEY = "lwtm.settings";
const defaultLang = () => {
  try {
    return navigator.language?.toLowerCase().startsWith("vi") ? "vi" : "en";
  } catch {
    return "vi";
  }
};

const DEFAULTS = { intervalMs: 1000, theme: "system", closeToTray: true, lang: defaultLang() };

function load() {
  try {
    return { ...DEFAULTS, ...JSON.parse(localStorage.getItem(KEY) ?? "{}") };
  } catch {
    return { ...DEFAULTS };
  }
}

export const settings = $state(load());

export function saveSettings() {
  try {
    localStorage.setItem(KEY, JSON.stringify({ ...settings }));
  } catch {}
}

/** Apply the options that take effect immediately (theme + close-button behaviour on the Rust side) */
export function applySettings() {
  const root = document.documentElement;
  if (settings.theme === "system") delete root.dataset.theme;
  else root.dataset.theme = settings.theme;
  root.lang = settings.lang;
  invoke("set_close_to_tray", { enabled: settings.closeToTray }).catch(() => {});
  // Rust-side messages (errors, alerts, tray menu) follow the UI language
  invoke("set_lang", { lang: settings.lang }).catch(() => {});
}

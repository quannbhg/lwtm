import { invoke } from "@tauri-apps/api/core";
import { assessAll } from "./critical.js";
import { confirmDialog } from "./dialog.svelte.js";
import { settings } from "./settings.svelte.js";
import { t } from "./i18n.svelte.js";

export const HISTORY = 60;

// `snap` and `extras` are replaced wholesale every poll and never mutated in place, so they use
// $state.raw: no deep reactive proxies get built for the ~300 process objects each second,
// which keeps both memory and garbage-collection churn low.
/** How long an error toast stays on screen */
const ERROR_MS = 5000;
let errorTimer;

/** Show an error toast that disappears by itself; pass "" to clear it. */
export function setError(message) {
  clearTimeout(errorTimer);
  app.error = message;
  if (message) errorTimer = setTimeout(() => (app.error = ""), ERROR_MS);
}

class AppState {
  snap = $state.raw(null);
  error = $state("");
  /** exe path -> { description, icon } */
  meta = $state({});
  /** pid -> { platform, priority, efficiency } (only filled when a view needs it and calls refreshExtras) */
  extras = $state.raw({});
  history = $state({ cpu: [], mem: [], disk: [], net: [] });
}

export const app = new AppState();

const requested = new Set();

function push(arr, v) {
  arr.push(v);
  if (arr.length > HISTORY) arr.shift();
}

async function loadMeta(snap) {
  const need = [...new Set(snap.processes.map((p) => p.exe).filter((e) => e && !requested.has(e)))];
  need.forEach((e) => requested.add(e));
  // load in batches so icons appear gradually instead of waiting for all of them
  for (let i = 0; i < need.length; i += 20) {
    try {
      Object.assign(app.meta, await invoke("get_app_meta", { exes: need.slice(i, i + 20) }));
    } catch (e) {
      console.error(e);
    }
  }
}

export async function refresh() {
  try {
    const s = await invoke("get_snapshot");
    app.snap = s;
    const h = app.history;
    push(h.cpu, s.cpu);
    push(h.mem, (s.memory_used / s.memory_total) * 100);
    push(h.disk, s.disk_read + s.disk_write);
    push(h.net, s.net_rx + s.net_tx);
    loadMeta(s);
  } catch (e) {
    setError(String(e));
  }
}

/** Fetch platform / priority / efficiency mode for every current process */
export async function refreshExtras() {
  if (!app.snap) return;
  try {
    app.extras = await invoke("get_process_extras", { pids: app.snap.processes.map((p) => p.pid) });
  } catch (e) {
    console.error(e);
  }
}

export async function killPids(pids) {
  try {
    await invoke("kill_processes", { pids });
    setError("");
  } catch (e) {
    setError(String(e));
  }
  refresh();
}

/** Tab navigation (shared so Ctrl+K and other views can switch tabs) */
export const nav = $state({ tab: "processes", focusPid: null, search: "" });

export function goto(tab, focusPid = null) {
  nav.tab = tab;
  nav.focusPid = focusPid;
}

/**
 * End processes, with warnings based on risk level.
 * @param {{pid:number,name:string,exe?:string|null}[]} procs
 * @returns {Promise<boolean>} true if the action was carried out
 */
export async function endTasks(procs, label) {
  const { level, flagged } = assessAll(procs);
  const n = procs.length;
  const what = `"${label}"${n > 1 ? ` (${t("{n} tiến trình", { n })})` : ""}`;
  const ok = await confirmDialog(
    level === "critical"
      ? {
          title: t("Đây là tiến trình quan trọng của hệ thống"),
          message: t("Kết thúc {what} có thể làm Windows sập (màn hình xanh), tự khởi động lại hoặc đăng xuất ngay lập tức. Dữ liệu chưa lưu sẽ mất.", { what }),
          details: flagged.map((f) => `${f.name} — ${f.why}`).slice(0, 5),
          level,
          okLabel: t("Vẫn kết thúc"),
          requireAck: true,
        }
      : level === "caution"
        ? {
            title: t("Thành phần hệ thống"),
            message: t("{what} là một phần của hệ điều hành. Kết thúc nó có thể làm mất một số tính năng cho đến khi khởi động lại.", { what }),
            details: flagged.map((f) => `${f.name} — ${f.why}`).slice(0, 5),
            level,
            okLabel: t("Kết thúc"),
          }
        : {
            title: t("Kết thúc tác vụ?"),
            message: t("Kết thúc {what}? Dữ liệu chưa lưu có thể bị mất.", { what }),
            okLabel: t("Kết thúc"),
          },
  );
  if (!ok) return false;
  await killPids(procs.map((p) => p.pid));
  return true;
}

async function run(cmd, args) {
  try {
    await invoke(cmd, args);
    setError("");
  } catch (e) {
    setError(String(e));
  }
}

/** Turn efficiency mode (lower priority + CPU throttling) on or off for processes. */
export async function setEfficiency(procs, label, enabled) {
  const { level, flagged } = assessAll(procs);
  if (enabled && level) {
    const ok = await confirmDialog({
      title: t("Bật chế độ tiết kiệm cho thành phần hệ thống?"),
      message: t('"{label}" thuộc hệ điều hành. Giảm ưu tiên của nó có thể làm giao diện hoặc âm thanh bị giật.', { label }),
      details: flagged.map((f) => `${f.name} — ${f.why}`).slice(0, 5),
      level: "caution",
      okLabel: t("Vẫn bật"),
    });
    if (!ok) return;
  }
  await run("set_efficiency", { pids: procs.map((p) => p.pid), enabled });
  await refreshExtras();
}

export async function setPriority(procs, label, priority) {
  if (priority === "realtime" || priority === "high") {
    const ok = await confirmDialog({
      title: priority === "realtime" ? t("Ưu tiên thời gian thực rất nguy hiểm") : t("Đặt ưu tiên cao?"),
      message:
        priority === "realtime"
          ? t('Đặt "{label}" ở mức Realtime có thể làm cả máy treo cứng vì nó giành quyền trước cả chuột, bàn phím và hệ thống.', { label })
          : t('Ưu tiên cao cho "{label}" sẽ làm các ứng dụng khác chậm đi khi nó bận.', { label }),
      level: priority === "realtime" ? "critical" : "caution",
      okLabel: t("Đặt ưu tiên"),
      requireAck: priority === "realtime",
    });
    if (!ok) return;
  }
  await run("set_priority", { pids: procs.map((p) => p.pid), priority });
  await refreshExtras();
}

let timer;
const stop = () => {
  clearInterval(timer);
  timer = undefined;
};
const start = () => {
  if (timer) return;
  refresh();
  timer = setInterval(refresh, settings.intervalMs);
};

/** Re-apply the refresh interval after the user changes settings */
export function restartPolling() {
  stop();
  if (!document.hidden) start();
}

/** Poll only while the window is visible, which keeps the app itself light. Returns a cleanup function. */
export function startPolling() {
  const onVis = () => (document.hidden ? stop() : start());
  document.addEventListener("visibilitychange", onVis);
  start();
  return () => {
    stop();
    document.removeEventListener("visibilitychange", onVis);
  };
}

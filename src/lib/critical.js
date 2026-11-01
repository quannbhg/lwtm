// Risk level classification for ending a process.
//   critical: ending it will crash the OS / log you out / hang the machine
//   caution : system component, ending it may break the UI or some features
//   null    : ordinary process

import { t } from "./i18n.svelte.js";

const CRITICAL = {
  system: "Nhân hệ điều hành",
  registry: "Lưu trữ cấu hình của Windows",
  smss: "Quản lý phiên đăng nhập của Windows",
  csrss: "Thành phần lõi của Windows (giao diện, console)",
  wininit: "Khởi tạo hệ thống Windows",
  winlogon: "Quản lý đăng nhập/đăng xuất",
  services: "Trình quản lý dịch vụ của Windows",
  lsass: "Xác thực người dùng và bảo mật",
  lsaiso: "Bảo vệ thông tin xác thực",
  "memory compression": "Nén bộ nhớ của Windows",
  "secure system": "Vùng bảo mật của Windows",
  // macOS / Linux
  kernel_task: "Nhân hệ điều hành macOS",
  launchd: "Tiến trình khởi tạo của macOS",
  init: "Tiến trình khởi tạo của hệ thống (PID 1)",
  systemd: "Trình quản lý hệ thống (PID 1)",
};

const CAUTION = {
  svchost: "Chứa nhiều dịch vụ của Windows — tắt có thể làm mất mạng, âm thanh hoặc cập nhật",
  explorer: "Giao diện Windows (thanh tác vụ, màn hình nền) — tắt sẽ mất tạm thời giao diện",
  dwm: "Quản lý hiển thị cửa sổ — tắt sẽ làm màn hình nháy hoặc mất hiệu ứng",
  spoolsv: "Dịch vụ in ấn",
  sihost: "Thành phần giao diện Windows (menu Start, thông báo)",
  taskhostw: "Tiến trình nền của Windows",
  ctfmon: "Nhập liệu (bộ gõ, bàn phím ảo)",
  runtimebroker: "Quản lý quyền cho ứng dụng Store",
  searchhost: "Tìm kiếm của Windows",
  msmpeng: "Windows Defender (diệt virus) — tắt sẽ mất bảo vệ",
  securityhealthservice: "Trung tâm bảo mật của Windows",
  audiodg: "Xử lý âm thanh — tắt sẽ mất tiếng tạm thời",
  fontdrvhost: "Trình điều khiển phông chữ",
  wudfhost: "Trình điều khiển thiết bị",
  dllhost: "Máy chủ COM của Windows",
  conhost: "Cửa sổ console",
  windowserver: "Hiển thị giao diện macOS — tắt sẽ đăng xuất bạn",
  loginwindow: "Màn hình đăng nhập macOS — tắt sẽ đăng xuất bạn",
  xorg: "Máy chủ hiển thị — tắt sẽ đóng toàn bộ ứng dụng đồ hoạ",
  gnome_shell: "Giao diện GNOME",
};

const WINDOWS_DIR = /^[a-z]:\\windows\\/i;

/** @returns {{ level: "critical" | "caution" | null, why: string }} */
export function assess(p) {
  const key = p.name.toLowerCase().replace(/\.exe$/, "");
  if (CRITICAL[key]) return { level: "critical", why: t(CRITICAL[key]) };
  if (CAUTION[key]) return { level: "caution", why: t(CAUTION[key]) };
  if (p.exe && WINDOWS_DIR.test(p.exe)) return { level: "caution", why: t("Thành phần của Windows") };
  return { level: null, why: "" };
}

const RANK = { critical: 2, caution: 1 };

/** Worst level within a group of processes + the list of flagged ones */
export function assessAll(procs) {
  let worst = null;
  const flagged = [];
  for (const p of procs) {
    const a = assess(p);
    if (!a.level) continue;
    flagged.push({ name: p.name, ...a });
    if (!worst || RANK[a.level] > RANK[worst]) worst = a.level;
  }
  return { level: worst, flagged };
}

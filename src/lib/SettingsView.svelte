<script>
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";
  import { settings, saveSettings, applySettings } from "./settings.svelte.js";
  import { restartPolling } from "./store.svelte.js";
  import { t } from "./i18n.svelte.js";
  import { fmtBytes } from "./format.js";

  /** Background watcher config; it lives in Rust so it works even when no window is open */
  let watch = $state({ history: true, notify: true, cpu_threshold: 90, ram_threshold: 90, sustain_secs: 120, lang: "vi" });

  async function saveWatch() {
    try {
      await invoke("set_watch_config", { cfg: { ...watch, lang: settings.lang } });
    } catch (e) {
      error = String(e);
    }
  }

  let autostart = $state(false);
  let elevated = $state(false);
  let error = $state("");

  function change() {
    saveSettings();
    applySettings();
    restartPolling();
  }

  async function toggleAutostart() {
    try {
      await invoke("set_autostart", { enabled: !autostart });
      autostart = !autostart;
      error = "";
    } catch (e) {
      error = String(e);
    }
  }

  async function adminRestart() {
    try {
      await invoke("restart_as_admin");
    } catch (e) {
      error = String(e);
    }
  }

  /** Memory used by this app: Rust process + WebView processes */
  let selfUsage = $state(null);
  /** False on desktops without a system tray (stock GNOME): the app then quits when the window is closed */
  let trayOk = $state(true);

  async function loadSelf() {
    selfUsage = await invoke("get_self_usage").catch(() => null);
  }

  onMount(async () => {
    autostart = await invoke("get_autostart").catch(() => false);
    elevated = await invoke("is_elevated").catch(() => false);
    trayOk = await invoke("tray_available").catch(() => true);
    Object.assign(watch, await invoke("get_watch_config").catch(() => ({})));
  });

  onMount(() => {
    loadSelf();
    const timer = setInterval(() => !document.hidden && loadSelf(), 3000);
    return () => clearInterval(timer);
  });
</script>

<section class="view">
  <div class="toolbar"><h2>{t("Cài đặt")}</h2></div>
  {#if error}<p class="error">{error}</p>{/if}

  <div class="settings">
    <div class="srow">
      <span>
        <b>{t("Bộ nhớ của ứng dụng này")}</b>
        <small>
          {#if selfUsage}
            {t("Ứng dụng {app} + giao diện {web} ({n} tiến trình). Tính theo bộ nhớ riêng, giống cột Memory của Task Manager.", {
              app: fmtBytes(selfUsage.app),
              web: fmtBytes(selfUsage.webview),
              n: selfUsage.webview_count,
            })}
          {:else}…{/if}
        </small>
      </span>
      <b>{selfUsage ? fmtBytes(selfUsage.app + selfUsage.webview) : "…"}</b>
    </div>

    <label class="srow">
      <span><b>{t("Ngôn ngữ")}</b><small>{t("Áp dụng cho giao diện, thông báo và thông báo lỗi")}</small></span>
      <select class="btn" bind:value={settings.lang} onchange={change}>
        <option value="vi">Tiếng Việt</option>
        <option value="en">English</option>
      </select>
    </label>

    <label class="srow">
      <span><b>{t("Chu kỳ làm mới")}</b><small>{t("Làm mới chậm hơn giúp app tốn ít CPU hơn")}</small></span>
      <select class="btn" bind:value={settings.intervalMs} onchange={change}>
        <option value={500}>{t("0,5 giây")}</option>
        <option value={1000}>{t("1 giây")}</option>
        <option value={2000}>{t("2 giây")}</option>
        <option value={5000}>{t("5 giây")}</option>
      </select>
    </label>

    <label class="srow">
      <span><b>{t("Giao diện")}</b><small>{t("Theo hệ thống, sáng hoặc tối")}</small></span>
      <select class="btn" bind:value={settings.theme} onchange={change}>
        <option value="system">{t("Theo hệ thống")}</option>
        <option value="light">{t("Sáng")}</option>
        <option value="dark">{t("Tối")}</option>
      </select>
    </label>

    <label class="srow">
      <span>
        <b>{t("Nút đóng cửa sổ")}</b>
        <small>{trayOk ? t("Thu xuống khay để widget và theo dõi vẫn chạy, hoặc thoát hẳn") : t("Môi trường desktop của bạn không có khay hệ thống, nên đóng cửa sổ sẽ thoát ứng dụng")}</small>
      </span>
      <select class="btn" bind:value={settings.closeToTray} onchange={change} disabled={!trayOk}>
        <option value={true}>{t("Thu xuống khay")}</option>
        <option value={false}>{t("Thoát ứng dụng")}</option>
      </select>
    </label>

    <div class="srow">
      <span><b>{t("Khởi động cùng hệ thống")}</b><small>{t("Chạy ẩn dưới khay khi đăng nhập")}</small></span>
      <button class="switch" class:on={autostart} role="switch" aria-checked={autostart} aria-label={t("Khởi động cùng hệ thống")} onclick={toggleAutostart}><i></i></button>
    </div>

    <div class="srow">
      <span><b>{t("Ghi lịch sử")}</b><small>{t("Lưu mức tải mỗi 10 giây trong 24 giờ để biết \"lúc đó máy giật vì cái gì\" (kể cả khi thu xuống khay)")}</small></span>
      <button class="switch" class:on={watch.history} role="switch" aria-checked={watch.history} aria-label={t("Ghi lịch sử")} onclick={() => { watch.history = !watch.history; saveWatch(); }}><i></i></button>
    </div>

    <div class="srow">
      <span><b>{t("Cảnh báo khi quá tải")}</b><small>{t("Gửi thông báo khi CPU hoặc RAM vượt ngưỡng liên tục")}</small></span>
      <button class="switch" class:on={watch.notify} role="switch" aria-checked={watch.notify} aria-label={t("Cảnh báo khi quá tải")} onclick={() => { watch.notify = !watch.notify; saveWatch(); }}><i></i></button>
    </div>

    {#if watch.notify}
      <label class="srow sub">
        <span><b>{t("Ngưỡng CPU")}</b></span>
        <select class="btn" bind:value={watch.cpu_threshold} onchange={saveWatch}>
          <option value={80}>80%</option>
          <option value={90}>90%</option>
          <option value={95}>95%</option>
        </select>
      </label>
      <label class="srow sub">
        <span><b>{t("Ngưỡng RAM")}</b></span>
        <select class="btn" bind:value={watch.ram_threshold} onchange={saveWatch}>
          <option value={85}>85%</option>
          <option value={90}>90%</option>
          <option value={95}>95%</option>
        </select>
      </label>
      <label class="srow sub">
        <span><b>{t("Kéo dài ít nhất")}</b><small>{t("Tránh báo nhầm vì những đợt tăng tải ngắn")}</small></span>
        <select class="btn" bind:value={watch.sustain_secs} onchange={saveWatch}>
          <option value={30}>{t("30 giây")}</option>
          <option value={60}>{t("1 phút")}</option>
          <option value={120}>{t("2 phút")}</option>
          <option value={300}>{t("5 phút")}</option>
        </select>
      </label>
    {/if}

    <div class="srow">
      <span>
        <b>{t("Quyền quản trị")}</b>
        <small>{elevated ? t("Đang chạy với quyền quản trị: có thể điều khiển mọi tiến trình và dịch vụ") : t("Đang chạy với quyền thường: một số thao tác trên hệ thống sẽ bị từ chối")}</small>
      </span>
      {#if elevated}<span class="tag good">✔ {t("Quản trị viên")}</span>
      {:else}<button class="btn" onclick={adminRestart}>🛡 {t("Chạy lại với quyền admin")}</button>{/if}
    </div>
  </div>
</section>

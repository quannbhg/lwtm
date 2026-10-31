<script>
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";
  import { settings, saveSettings, applySettings } from "./settings.svelte.js";
  import { restartPolling } from "./store.svelte.js";

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

  onMount(async () => {
    autostart = await invoke("get_autostart").catch(() => false);
    elevated = await invoke("is_elevated").catch(() => false);
  });
</script>

<section class="view">
  <div class="toolbar"><h2>Cài đặt</h2></div>
  {#if error}<p class="error">{error}</p>{/if}

  <div class="settings">
    <label class="srow">
      <span><b>Chu kỳ làm mới</b><small>Làm mới chậm hơn giúp app tốn ít CPU hơn</small></span>
      <select class="btn" bind:value={settings.intervalMs} onchange={change}>
        <option value={500}>0,5 giây</option>
        <option value={1000}>1 giây</option>
        <option value={2000}>2 giây</option>
        <option value={5000}>5 giây</option>
      </select>
    </label>

    <label class="srow">
      <span><b>Giao diện</b><small>Theo hệ thống, sáng hoặc tối</small></span>
      <select class="btn" bind:value={settings.theme} onchange={change}>
        <option value="system">Theo hệ thống</option>
        <option value="light">Sáng</option>
        <option value="dark">Tối</option>
      </select>
    </label>

    <label class="srow">
      <span><b>Nút đóng cửa sổ</b><small>Thu xuống khay để widget và theo dõi vẫn chạy, hoặc thoát hẳn</small></span>
      <select class="btn" bind:value={settings.closeToTray} onchange={change}>
        <option value={true}>Thu xuống khay</option>
        <option value={false}>Thoát ứng dụng</option>
      </select>
    </label>

    <div class="srow">
      <span><b>Khởi động cùng hệ thống</b><small>Chạy ẩn dưới khay khi đăng nhập</small></span>
      <button class="switch" class:on={autostart} role="switch" aria-checked={autostart} aria-label="Khởi động cùng hệ thống" onclick={toggleAutostart}><i></i></button>
    </div>

    <div class="srow">
      <span>
        <b>Quyền quản trị</b>
        <small>{elevated ? "Đang chạy với quyền quản trị: có thể điều khiển mọi tiến trình và dịch vụ" : "Đang chạy với quyền thường: một số thao tác trên hệ thống sẽ bị từ chối"}</small>
      </span>
      {#if elevated}<span class="tag good">✔ Quản trị viên</span>
      {:else}<button class="btn" onclick={adminRestart}>🛡 Chạy lại với quyền admin</button>{/if}
    </div>
  </div>
</section>

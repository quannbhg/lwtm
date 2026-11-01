<script>
  import { onMount } from "svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { app, nav, startPolling, setError } from "./lib/store.svelte.js";
  import ProcessesView from "./lib/ProcessesView.svelte";
  import PerformanceView from "./lib/PerformanceView.svelte";
  import DetailsView from "./lib/DetailsView.svelte";
  import HistoryView from "./lib/HistoryView.svelte";
  import ServicesView from "./lib/ServicesView.svelte";
  import SecurityView from "./lib/SecurityView.svelte";
  import StartupView from "./lib/StartupView.svelte";
  import SettingsView from "./lib/SettingsView.svelte";
  import { applySettings } from "./lib/settings.svelte.js";
  import { t } from "./lib/i18n.svelte.js";
  import MiniWidget from "./lib/MiniWidget.svelte";
  import Palette from "./lib/Palette.svelte";
  import Dialog from "./lib/Dialog.svelte";

  const tabs = [
    { id: "processes", label: "Tiến trình", icon: "▤" },
    { id: "performance", label: "Hiệu năng", icon: "◔" },
    { id: "history", label: "Lịch sử", icon: "◷" },
    { id: "details", label: "Chi tiết", icon: "☰" },
    { id: "startup", label: "Khởi động", icon: "⏻" },
    { id: "services", label: "Dịch vụ", icon: "⚙" },
    { id: "security", label: "Bảo mật", icon: "🛡" },
    { id: "settings", label: "Cài đặt", icon: "☼" },
  ];

  // One bundle serves both the main window and the mini-widget, told apart by the window label
  let isMini = false;
  try {
    isMini = getCurrentWindow().label === "mini";
  } catch {}

  onMount(() => {
    applySettings();
    return startPolling();
  });
</script>

{#if isMini}
  <MiniWidget />
{:else}
  <div class="shell">
    <nav>
      {#each tabs as tab (tab.id)}
        <button class:on={nav.tab === tab.id} onclick={() => (nav.tab = tab.id)}>
          <span class="ni">{tab.icon}</span>{t(tab.label)}
        </button>
      {/each}
      <span class="kbd-hint">{t("Ctrl+K tìm nhanh")}</span>
    </nav>
    <main>
      {#if nav.tab === "processes"}<ProcessesView />
      {:else if nav.tab === "performance"}<PerformanceView />
      {:else if nav.tab === "history"}<HistoryView />
      {:else if nav.tab === "details"}<DetailsView />
      {:else if nav.tab === "startup"}<StartupView />
      {:else if nav.tab === "services"}<ServicesView />
      {:else if nav.tab === "security"}<SecurityView />
      {:else}<SettingsView />{/if}
    </main>
  </div>
  <Palette />
  <Dialog />
  <!-- Floats above the page (no layout shift) and removes itself after a few seconds -->
  {#if app.error}
    <div class="toast" role="alert">
      <span>{app.error}</span>
      <button aria-label={t("Đóng thông báo")} onclick={() => setError("")}>×</button>
    </div>
  {/if}
{/if}

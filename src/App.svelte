<script>
  import { onMount } from "svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { nav, startPolling } from "./lib/store.svelte.js";
  import ProcessesView from "./lib/ProcessesView.svelte";
  import PerformanceView from "./lib/PerformanceView.svelte";
  import DetailsView from "./lib/DetailsView.svelte";
  import ServicesView from "./lib/ServicesView.svelte";
  import SecurityView from "./lib/SecurityView.svelte";
  import StartupView from "./lib/StartupView.svelte";
  import SettingsView from "./lib/SettingsView.svelte";
  import { applySettings } from "./lib/settings.svelte.js";
  import MiniWidget from "./lib/MiniWidget.svelte";
  import Palette from "./lib/Palette.svelte";
  import Dialog from "./lib/Dialog.svelte";

  const tabs = [
    { id: "processes", label: "Tiến trình", icon: "▤" },
    { id: "performance", label: "Hiệu năng", icon: "◔" },
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
      {#each tabs as t (t.id)}
        <button class:on={nav.tab === t.id} onclick={() => (nav.tab = t.id)}>
          <span class="ni">{t.icon}</span>{t.label}
        </button>
      {/each}
      <span class="kbd-hint">Ctrl+K tìm nhanh</span>
    </nav>
    <main>
      {#if nav.tab === "processes"}<ProcessesView />
      {:else if nav.tab === "performance"}<PerformanceView />
      {:else if nav.tab === "details"}<DetailsView />
      {:else if nav.tab === "startup"}<StartupView />
      {:else if nav.tab === "services"}<ServicesView />
      {:else if nav.tab === "security"}<SecurityView />
      {:else}<SettingsView />{/if}
    </main>
  </div>
  <Palette />
  <Dialog />
{/if}

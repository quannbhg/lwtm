<script>
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";
  import { app } from "./store.svelte.js";

  let entries = $state([]);
  let error = $state("");
  let loading = $state(true);
  let query = $state("");

  let rows = $derived.by(() => {
    const q = query.trim().toLowerCase();
    return entries
      .filter((e) => !q || e.name.toLowerCase().includes(q) || e.command.toLowerCase().includes(q))
      .sort((a, b) => Number(b.enabled) - Number(a.enabled) || a.name.localeCompare(b.name));
  });
  let enabledCount = $derived(entries.filter((e) => e.enabled).length);

  async function load() {
    try {
      entries = await invoke("list_startup");
      error = "";
      // fetch icons and friendly names for entries that do not have them yet
      const need = [...new Set(entries.map((e) => e.exe).filter((x) => x && !(x in app.meta)))];
      if (need.length) Object.assign(app.meta, await invoke("get_app_meta", { exes: need }));
    } catch (e) {
      error = String(e);
    }
    loading = false;
  }

  async function toggle(e) {
    try {
      await invoke("set_startup_enabled", { target: { source: e.source, key: e.key }, enabled: !e.enabled });
      error = "";
    } catch (err) {
      error = String(err);
    }
    load();
  }

  onMount(load);
</script>

<section class="view">
  <div class="toolbar">
    <h2>Ứng dụng khởi động</h2>
    <input type="search" placeholder="Tìm theo tên hoặc đường dẫn…" bind:value={query} />
    <button class="btn" onclick={load}>Làm mới</button>
  </div>
  <p class="hint">
    {enabledCount} / {entries.length} mục đang bật. Tắt bớt các mục không cần thiết giúp máy khởi động nhanh hơn.
    Việc tắt chỉ vô hiệu hoá mục khởi động, không gỡ cài đặt ứng dụng.
  </p>

  {#if error}<p class="error">{error}</p>{/if}

  <div class="findings">
    {#if loading}<p class="hint pad">Đang tải…</p>
    {:else if rows.length === 0 && !error}<p class="hint pad">Không có mục khởi động nào.</p>{/if}

    {#each rows as e (e.source + e.key)}
      {@const meta = app.meta[e.exe]}
      <article class="startup" class:off={!e.enabled}>
        {#if meta?.icon}<img class="ico lg" src={meta.icon} alt="" />
        {:else}<span class="ico lg fallback">{e.name.charAt(0).toUpperCase()}</span>{/if}
        <div class="info">
          <strong>{meta?.description ?? e.name}</strong>
          <div class="path" title={e.command}>{e.command}</div>
          <small>{e.source_label}</small>
        </div>
        <button class="switch" class:on={e.enabled} role="switch" aria-checked={e.enabled} aria-label="Bật/tắt {e.name}" onclick={() => toggle(e)}>
          <i></i>
        </button>
      </article>
    {/each}
  </div>
</section>

<script>
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";
  import { createVirtual } from "./virtual.svelte.js";
  import { nav } from "./store.svelte.js";
  import { confirmDialog } from "./dialog.svelte.js";
  import { t } from "./i18n.svelte.js";

  const REFRESH_MS = 5000;
  const STATUS = { running: "Đang chạy", stopped: "Đã dừng", paused: "Tạm dừng", pending: "Đang chuyển…" };
  const START = { auto: "Tự động", manual: "Thủ công", disabled: "Vô hiệu hoá", boot: "Boot", system: "Hệ thống" };

  let services = $state([]);
  let query = $state("");
  let filter = $state("all"); // all | running | stopped
  let selected = $state(null); // name
  let error = $state("");
  let busy = $state(false);

  let rows = $derived.by(() => {
    const q = query.trim().toLowerCase();
    return services
      .filter((s) => filter === "all" || s.status === filter)
      .filter(
        (s) =>
          !q ||
          s.display.toLowerCase().includes(q) ||
          s.name.toLowerCase().includes(q) ||
          (s.description ?? "").toLowerCase().includes(q),
      )
      .sort((a, b) => a.display.localeCompare(b.display));
  });
  const vl = createVirtual(34);
  let r = $derived(vl.range(rows.length));
  let visible = $derived(rows.slice(r.start, r.end));

  let current = $derived(services.find((s) => s.name === selected));

  async function load() {
    try {
      services = await invoke("list_services");
      error = "";
    } catch (e) {
      error = String(e);
    }
  }

  async function act(action) {
    if (!current || busy) return;
    if (action !== "start") {
      const ok = await confirmDialog({
        title: action === "stop" ? t("Dừng dịch vụ?") : t("Khởi động lại dịch vụ?"),
        message:
          action === "stop"
            ? t('Dừng "{name}"?', { name: current.display })
            : t('Khởi động lại "{name}"?', { name: current.display }),
        level: current.start_type === "boot" || current.start_type === "system" ? "caution" : null,
        okLabel: action === "stop" ? t("Dừng") : t("Khởi động lại"),
      });
      if (!ok) return;
    }
    busy = true;
    try {
      await invoke("control_service", { name: current.name, action });
      error = "";
    } catch (e) {
      error = String(e);
    }
    busy = false;
    load();
  }

  onMount(() => {
    if (nav.search) {
      query = nav.search;
      nav.search = "";
    }
    load();
    const timer = setInterval(() => !document.hidden && !busy && load(), REFRESH_MS);
    return () => clearInterval(timer);
  });
</script>

<section class="view">
  <div class="toolbar">
    <h2>{t("Dịch vụ")}</h2>
    <input type="search" placeholder={t("Tìm theo tên hoặc mô tả…")} bind:value={query} />
    <select bind:value={filter}>
      <option value="all">{t("Tất cả")}</option>
      <option value="running">{t("Đang chạy")}</option>
      <option value="stopped">{t("Đã dừng")}</option>
    </select>
  </div>
  <div class="toolbar">
    <button class="btn" disabled={!current || busy || current.status === "running"} onclick={() => act("start")}>{t("Bắt đầu")}</button>
    <button class="btn" disabled={!current || busy || !current.can_stop} onclick={() => act("stop")}>{t("Dừng")}</button>
    <button class="btn" disabled={!current || busy || !current.can_stop} onclick={() => act("restart")}>{t("Khởi động lại")}</button>
    <span class="hint">{t("{a} / {b} dịch vụ · cần quyền Administrator để điều khiển", { a: rows.length, b: services.length })}</span>
  </div>

  {#if error}<p class="error">{error}</p>{/if}

  <div class="table-wrap" use:vl.action>
    <table class="svc">
      <thead>
        <tr>
          <th class="l">{t("Tên")}</th>
          <th class="num">PID</th>
          <th class="l st">{t("Trạng thái")}</th>
          <th class="l st">{t("Khởi động")}</th>
          <th class="l">{t("Mô tả")}</th>
        </tr>
      </thead>
      <tbody>
        {#if r.padTop}<tr class="pad" style="height:{r.padTop}px"><td colspan="5"></td></tr>{/if}
        {#each visible as s (s.name)}
          <tr class:sel={selected === s.name} onclick={() => (selected = s.name)}>
            <td><span class="label" title={s.name}>{s.display} <small>{s.name}</small></span></td>
            <td class="num">{s.pid ?? ""}</td>
            <td><span class="dot {s.status}"></span>{t(STATUS[s.status])}</td>
            <td>{t(START[s.start_type])}</td>
            <td><span class="label" title={s.description ?? ""}>{s.description ?? ""}</span></td>
          </tr>
        {/each}
        {#if r.padBottom}<tr class="pad" style="height:{r.padBottom}px"><td colspan="5"></td></tr>{/if}
      </tbody>
    </table>
  </div>
</section>

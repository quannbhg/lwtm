<script>
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";
  import { createVirtual } from "./virtual.svelte.js";
  import { nav } from "./store.svelte.js";
  import { confirmDialog } from "./dialog.svelte.js";

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
        title: action === "stop" ? "Dừng dịch vụ?" : "Khởi động lại dịch vụ?",
        message: `${action === "stop" ? "Dừng" : "Khởi động lại"} "${current.display}"?`,
        level: current.start_type === "boot" || current.start_type === "system" ? "caution" : null,
        okLabel: action === "stop" ? "Dừng" : "Khởi động lại",
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
    const t = setInterval(() => !document.hidden && !busy && load(), REFRESH_MS);
    return () => clearInterval(t);
  });
</script>

<section class="view">
  <div class="toolbar">
    <h2>Dịch vụ</h2>
    <input type="search" placeholder="Tìm theo tên hoặc mô tả…" bind:value={query} />
    <select bind:value={filter}>
      <option value="all">Tất cả</option>
      <option value="running">Đang chạy</option>
      <option value="stopped">Đã dừng</option>
    </select>
  </div>
  <div class="toolbar">
    <button class="btn" disabled={!current || busy || current.status === "running"} onclick={() => act("start")}>Bắt đầu</button>
    <button class="btn" disabled={!current || busy || !current.can_stop} onclick={() => act("stop")}>Dừng</button>
    <button class="btn" disabled={!current || busy || !current.can_stop} onclick={() => act("restart")}>Khởi động lại</button>
    <span class="hint">{rows.length} / {services.length} dịch vụ · cần quyền Administrator để điều khiển</span>
  </div>

  {#if error}<p class="error">{error}</p>{/if}

  <div class="table-wrap" use:vl.action>
    <table class="svc">
      <thead>
        <tr>
          <th class="l">Tên</th>
          <th class="num">PID</th>
          <th class="l st">Trạng thái</th>
          <th class="l st">Khởi động</th>
          <th class="l">Mô tả</th>
        </tr>
      </thead>
      <tbody>
        {#if r.padTop}<tr class="pad" style="height:{r.padTop}px"><td colspan="5"></td></tr>{/if}
        {#each visible as s (s.name)}
          <tr class:sel={selected === s.name} onclick={() => (selected = s.name)}>
            <td><span class="label" title={s.name}>{s.display} <small>{s.name}</small></span></td>
            <td class="num">{s.pid ?? ""}</td>
            <td><span class="dot {s.status}"></span>{STATUS[s.status]}</td>
            <td>{START[s.start_type]}</td>
            <td><span class="label" title={s.description ?? ""}>{s.description ?? ""}</span></td>
          </tr>
        {/each}
        {#if r.padBottom}<tr class="pad" style="height:{r.padBottom}px"><td colspan="5"></td></tr>{/if}
      </tbody>
    </table>
  </div>
</section>

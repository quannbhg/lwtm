<script>
  import { onMount, tick } from "svelte";
  import { app, nav, endTasks, setEfficiency, setPriority, refreshExtras, useIo } from "./store.svelte.js";
  import { fmtBytes, fmtRate } from "./format.js";
  import { createVirtual } from "./virtual.svelte.js";
  import { t } from "./i18n.svelte.js";

  const PRIORITIES = [
    ["realtime", "Thời gian thực"],
    ["high", "Cao"],
    ["above_normal", "Trên bình thường"],
    ["normal", "Bình thường"],
    ["below_normal", "Dưới bình thường"],
    ["idle", "Thấp"],
  ];
  const PRIO_LABEL = Object.fromEntries(PRIORITIES);
  const STATUS = { Run: "Đang chạy", Sleep: "Ngủ", Idle: "Rảnh", Stop: "Đã dừng", Zombie: "Zombie" };

  let query = $state("");
  let sortKey = $state("name");
  let asc = $state(true);
  let selectedPid = $state(null);
  const extras = $derived(app.extras);

  let rows = $derived.by(() => {
    const q = query.trim().toLowerCase();
    const rates = app.io.net.rates;
    const gpu = app.io.gpu.usage;
    const list = (app.snap?.processes ?? [])
      .filter((p) => !q || p.name.toLowerCase().includes(q) || String(p.pid) === q || (p.user ?? "").toLowerCase().includes(q))
      .map((p) => ({ ...p, net: rates[p.pid] ? rates[p.pid][0] + rates[p.pid][1] : 0, gpu: gpu[p.pid] ?? 0 }));
    const val = (p) => (sortKey === "name" ? p.name.toLowerCase() : sortKey === "user" ? (p.user ?? "") : p[sortKey]);
    return list.sort((a, b) => {
      const x = val(a), y = val(b);
      const r = typeof x === "string" ? x.localeCompare(y) : x - y;
      return asc ? r : -r;
    });
  });

  const vl = createVirtual(34);
  let r = $derived(vl.range(rows.length));
  let visible = $derived(rows.slice(r.start, r.end));

  let current = $derived(app.snap?.processes.find((p) => p.pid === selectedPid));
  let currentExtra = $derived(extras[selectedPid]);

  function sortBy(k) {
    if (sortKey === k) asc = !asc;
    else {
      sortKey = k;
      asc = k === "name" || k === "user";
    }
  }
  const arrow = (k) => (sortKey === k ? (asc ? " ▲" : " ▼") : "");

  async function end() {
    if (current && (await endTasks([current], `${current.name} (PID ${current.pid})`))) selectedPid = null;
  }

  async function toggleEfficiency() {
    if (!current) return;
    await setEfficiency([current], current.name, !currentExtra?.efficiency);
  }

  async function changePriority(e) {
    const v = e.currentTarget.value;
    e.currentTarget.value = "";
    if (!current || !v) return;
    await setPriority([current], current.name, v);
  }

  onMount(() => {
    if (nav.focusPid != null) {
      selectedPid = nav.focusPid;
      nav.focusPid = null;
      tick().then(() => vl.scrollTo(rows.findIndex((p) => p.pid === selectedPid)));
    }
    refreshExtras();
    const stopIo = useIo();
    const timer = setInterval(() => !document.hidden && refreshExtras(), 2000);
    return () => {
      clearInterval(timer);
      stopIo();
    };
  });

  const netHint = $derived(
    app.io.net.supported ? "" : app.io.net.needs_admin ? t("Cần chạy với quyền quản trị để đo mạng từng tiến trình") : t("Hệ điều hành này không cung cấp dữ liệu này"),
  );
  const gpuHint = $derived(app.io.gpu.supported ? "" : t("Hệ điều hành này không cung cấp dữ liệu này"));
</script>

<section class="view">
  <div class="toolbar">
    <h2>{t("Chi tiết")}</h2>
    <input type="search" placeholder={t("Tìm theo tên, PID hoặc người dùng…")} bind:value={query} />
  </div>
  <div class="toolbar">
    <button class="primary" disabled={!current} onclick={end}>{t("Kết thúc tác vụ")}</button>
    <button class="btn" disabled={!current || !currentExtra} onclick={toggleEfficiency}>
      🌿 {currentExtra?.efficiency ? t("Tắt chế độ tiết kiệm") : t("Chế độ tiết kiệm")}
    </button>
    <select class="btn" disabled={!current || !currentExtra} onchange={changePriority} value="">
      <option value="">{t("Đặt độ ưu tiên…")}</option>
      {#each PRIORITIES as [k, label] (k)}<option value={k}>{t(label)}</option>{/each}
    </select>
    <span class="hint">{t("{n} tiến trình · cần quyền Administrator để đổi tiến trình của hệ thống", { n: rows.length })}</span>
  </div>

  <div class="table-wrap" use:vl.action>
    <table class="det">
      <thead>
        <tr>
          <th class="l"><button onclick={() => sortBy("name")}>{t("Tên")}{arrow("name")}</button></th>
          <th class="num"><button onclick={() => sortBy("pid")}>PID{arrow("pid")}</button></th>
          <th class="l">{t("Trạng thái")}</th>
          <th class="l"><button onclick={() => sortBy("user")}>{t("Người dùng")}{arrow("user")}</button></th>
          <th class="num"><button onclick={() => sortBy("cpu")}>CPU{arrow("cpu")}</button></th>
          <th class="num"><button onclick={() => sortBy("memory")}>{t("Bộ nhớ")}{arrow("memory")}</button></th>
          <th class="num" title={netHint}><button onclick={() => sortBy("net")}>{t("Mạng")}{arrow("net")}</button></th>
          <th class="num" title={gpuHint}><button onclick={() => sortBy("gpu")}>GPU{arrow("gpu")}</button></th>
          <th class="l">{t("Nền tảng")}</th>
          <th class="l">{t("Ưu tiên")}</th>
          <th class="l">{t("Tiết kiệm")}</th>
        </tr>
      </thead>
      <tbody>
        {#if r.padTop}<tr class="pad" style="height:{r.padTop}px"><td colspan="11"></td></tr>{/if}
        {#each visible as p (p.pid)}
          {@const x = extras[p.pid]}
          <tr class:sel={selectedPid === p.pid} onclick={() => (selectedPid = p.pid)}>
            <td>
              <span class="name">
                {#if app.meta[p.exe]?.icon}<img class="ico" src={app.meta[p.exe].icon} alt="" />
                {:else}<span class="ico fallback">{p.name.charAt(0).toUpperCase()}</span>{/if}
                <span class="label" title={p.exe ?? p.name}>{p.name}</span>
              </span>
            </td>
            <td class="num">{p.pid}</td>
            <td>{STATUS[p.status] ? t(STATUS[p.status]) : p.status}</td>
            <td><span class="label">{p.user ?? ""}</span></td>
            <td class="num">{p.cpu.toFixed(1)}%</td>
            <td class="num">{fmtBytes(p.memory)}</td>
            <td class="num" title={netHint}>{app.io.net.supported ? fmtRate(p.net) : "—"}</td>
            <td class="num" title={gpuHint}>{app.io.gpu.supported ? `${p.gpu.toFixed(p.gpu > 0 && p.gpu < 10 ? 1 : 0)}%` : "—"}</td>
            <td>{x?.platform ?? ""}</td>
            <td>{x ? t(PRIO_LABEL[x.priority]) : ""}</td>
            <td>{x?.efficiency ? `🌿 ${t("Bật")}` : ""}</td>
          </tr>
        {/each}
        {#if r.padBottom}<tr class="pad" style="height:{r.padBottom}px"><td colspan="11"></td></tr>{/if}
      </tbody>
    </table>
  </div>
</section>

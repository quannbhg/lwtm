<script>
  import { onMount, tick } from "svelte";
  import { SvelteSet } from "svelte/reactivity";
  import { createVirtual } from "./virtual.svelte.js";
  import { app, endTasks, setEfficiency, refreshExtras } from "./store.svelte.js";
  import { groupProcesses } from "./groups.js";
  import { diagnose } from "./diagnose.js";
  import { fmtBytes, fmtRate } from "./format.js";
  import { t } from "./i18n.svelte.js";

  let query = $state("");
  let sortKey = $state("cpu");
  let asc = $state(false);
  /** { id, label, procs } */
  let selected = $state(null);
  let showDiag = $state(true);
  const open = new SvelteSet();

  let allGroups = $derived(groupProcesses(app.snap, app.meta));
  let diag = $derived(app.snap ? diagnose(app.snap, allGroups) : null);

  let groups = $derived.by(() => {
    const q = query.trim().toLowerCase();
    const cmp = (a, b) => {
      const r = sortKey === "name" ? a.name.localeCompare(b.name) : a[sortKey] - b[sortKey];
      return asc ? r : -r;
    };
    return allGroups
      .filter(
        (g) =>
          !q ||
          g.name.toLowerCase().includes(q) ||
          g.procs.some((p) => p.name.toLowerCase().includes(q) || String(p.pid) === q),
      )
      .map((g) => ({ ...g, procs: [...g.procs].sort(cmp) }))
      .sort(cmp);
  });

  function sortBy(k) {
    if (sortKey === k) asc = !asc;
    else {
      sortKey = k;
      asc = k === "name";
    }
  }

  const selectGroup = (g) => (selected = { id: "g:" + g.key, label: g.name, procs: g.procs });

  // Flat rows (groups + expanded child processes) for virtualization
  let flat = $derived(
    groups.flatMap((g) => [
      { k: g.key, g },
      ...(g.procs.length > 1 && open.has(g.key) ? g.procs.map((p) => ({ k: "p" + p.pid, g, p })) : []),
    ]),
  );
  const vl = createVirtual(34);
  let r = $derived(vl.range(flat.length));
  let visible = $derived(flat.slice(r.start, r.end));

  async function focusCulprit(c) {
    const g = allGroups.find((x) => x.key === c.key);
    if (!g) return;
    query = "";
    selectGroup(g);
    if (g.procs.length > 1) open.add(g.key);
    await tick();
    vl.scrollTo(flat.findIndex((x) => x.k === g.key));
  }

  const isEco = (p) => app.extras[p.pid]?.efficiency === true;
  /** Every selected process is already in efficiency mode -> the button becomes "Turn off" */
  let selectedEco = $derived(!!selected && selected.procs.length > 0 && selected.procs.every(isEco));
  /** Number of processes in efficiency mode within a group (for the 🌿 icon and tooltip) */
  const ecoCount = (g) => g.procs.filter(isEco).length;

  onMount(() => {
    refreshExtras();
    const timer = setInterval(() => !document.hidden && refreshExtras(), 2000);
    return () => clearInterval(timer);
  });

  async function endSelected() {
    if (selected && (await endTasks(selected.procs, selected.label))) selected = null;
  }

  const heatCpu = (v) => Math.min(1, v / 40);
  const heatMem = (v) => Math.min(1, v / ((app.snap?.memory_total ?? 1) * 0.15));
  const heatDisk = (v) => Math.min(1, v / (20 << 20));
  const heat = (cpu, memory, disk) => `--c:${heatCpu(cpu)};--m:${heatMem(memory)};--d:${heatDisk(disk)}`;

  const arrow = (k) => (sortKey === k ? (asc ? " ▲" : " ▼") : "");
</script>

{#snippet icon(exe, name)}
  {#if app.meta[exe]?.icon}
    <img class="ico" src={app.meta[exe].icon} alt="" />
  {:else}
    <span class="ico fallback">{name.charAt(0).toUpperCase()}</span>
  {/if}
{/snippet}

<section class="view">
  <div class="toolbar">
    <h2>{t("Tiến trình")}</h2>
    <input type="search" placeholder={t("Tìm theo tên hoặc PID…  (Ctrl+K để tìm nhanh toàn app)")} bind:value={query} />
    <button
      class="btn"
      class:eco-on={selectedEco}
      disabled={!selected}
      onclick={() => selected && setEfficiency(selected.procs, selected.label, !selectedEco)}
      title={selectedEco ? t("Trả độ ưu tiên về bình thường") : t("Hạ độ ưu tiên và giảm xung CPU cho tiến trình đang chọn")}
    >
      🌿 {selectedEco ? t("Tắt chế độ tiết kiệm") : t("Chế độ tiết kiệm")}
    </button>
    <button class="primary" disabled={!selected} onclick={endSelected}>{t("Kết thúc tác vụ")}</button>
  </div>

  {#if diag}
    <!-- The card keeps a constant height: causes are short chips in the header and the culprits
         sit in three fixed slots, so nothing jumps when the numbers change. -->
    <div class="diag {diag.level}">
      <button class="diag-head" onclick={() => (showDiag = !showDiag)}>
        <span class="dot"></span>
        <strong>{t("Tại sao máy chậm?")}</strong>
        <span class="muted headline">{diag.headline}</span>
        <span class="chips">
          {#each diag.causes as c (c.label)}<span class="chip" title={c.text}>{c.label}</span>{/each}
        </span>
        <span class="chev">{showDiag ? "▾" : "▸"}</span>
      </button>
      {#if showDiag}
        <div class="culprits">
          {#each diag.culprits as c (c.key)}
            <div class="culprit" title={c.tip}>
              <div class="culprit-top">
                <b>{c.name}</b>
                {#if c.system}<span class="sys" aria-hidden="true">🔒</span>{/if}
                <button class="btn small" onclick={() => focusCulprit(c)}>{t("Xem")}</button>
              </div>
              <div class="why">{c.why}</div>
            </div>
          {/each}
          {#if diag.culprits.length === 0}
            <p class="muted none">{t("Không có ứng dụng nào đang dùng nhiều tài nguyên bất thường.")}</p>
          {/if}
        </div>
      {/if}
    </div>
  {/if}

  <div class="table-wrap" use:vl.action>
    <table class="procs">
      <thead>
        <tr>
          <th class="namecol"><button onclick={() => sortBy("name")}>{t("Tên")}{arrow("name")}</button></th>
          <th class="num">
            <button onclick={() => sortBy("cpu")}>
              <b>{app.snap ? app.snap.cpu.toFixed(0) : "–"}%</b><small>CPU{arrow("cpu")}</small>
            </button>
          </th>
          <th class="num">
            <button onclick={() => sortBy("memory")}>
              <b>{app.snap ? Math.round((app.snap.memory_used / app.snap.memory_total) * 100) : "–"}%</b>
              <small>{t("Bộ nhớ")}{arrow("memory")}</small>
            </button>
          </th>
          <th class="num">
            <button onclick={() => sortBy("disk")}>
              <b>{app.snap ? fmtRate(app.snap.disk_read + app.snap.disk_write) : "–"}</b>
              <small>{t("Ổ đĩa")}{arrow("disk")}</small>
            </button>
          </th>
        </tr>
      </thead>
      <tbody>
        {#if r.padTop}<tr class="pad" style="height:{r.padTop}px"><td colspan="4"></td></tr>{/if}
        {#each visible as row (row.k)}
          {#if !row.p}
            {@const g = row.g}
            {@const multi = g.procs.length > 1}
            <tr
              class:sel={selected?.id === "g:" + g.key}
              style={heat(g.cpu, g.memory, g.disk)}
              onclick={() => selectGroup(g)}
              ondblclick={() => multi && (open.has(g.key) ? open.delete(g.key) : open.add(g.key))}
            >
              <td class="namecol">
                <span class="name">
                  {#if multi}
                    <button
                      class="chev"
                      aria-label={t("Mở rộng")}
                      onclick={(e) => {
                        e.stopPropagation();
                        open.has(g.key) ? open.delete(g.key) : open.add(g.key);
                      }}>{open.has(g.key) ? "▾" : "▸"}</button
                    >
                  {:else}<span class="chev"></span>{/if}
                  {@render icon(g.exe, g.name)}
                  <span class="label" title={g.exe ?? g.name}>{g.name}{multi ? ` (${g.procs.length})` : ""}</span>
                  {#if ecoCount(g)}
                    <span class="eco" title={multi ? t("Chế độ tiết kiệm: {a}/{b} tiến trình", { a: ecoCount(g), b: g.procs.length }) : t("Đang ở chế độ tiết kiệm")}>🌿</span>
                  {/if}
                </span>
              </td>
              <td class="num c-cpu">{g.cpu.toFixed(1)}%</td>
              <td class="num c-mem">{fmtBytes(g.memory)}</td>
              <td class="num c-disk">{fmtRate(g.disk)}</td>
            </tr>
          {:else}
            {@const p = row.p}
            <tr
              class="child"
              class:sel={selected?.id === "p:" + p.pid}
              style={heat(p.cpu, p.memory, p.disk)}
              onclick={() => (selected = { id: "p:" + p.pid, label: `${p.name} (PID ${p.pid})`, procs: [p] })}
            >
              <td class="namecol"><span class="name"><span class="chev"></span><span class="ico"></span><span class="label">{p.name} <small>PID {p.pid}</small></span>{#if isEco(p)}<span class="eco" title={t("Đang ở chế độ tiết kiệm")}>🌿</span>{/if}</span></td>
              <td class="num c-cpu">{p.cpu.toFixed(1)}%</td>
              <td class="num c-mem">{fmtBytes(p.memory)}</td>
              <td class="num c-disk">{fmtRate(p.disk)}</td>
            </tr>
          {/if}
        {/each}
        {#if r.padBottom}<tr class="pad" style="height:{r.padBottom}px"><td colspan="4"></td></tr>{/if}
      </tbody>
    </table>
  </div>
</section>

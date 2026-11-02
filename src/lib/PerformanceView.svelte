<script>
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";
  import { app, HISTORY } from "./store.svelte.js";
  import Sparkline from "./Sparkline.svelte";
  import { fmtBytes, fmtRate, fmtDuration } from "./format.js";
  import { t } from "./i18n.svelte.js";

  let tab = $state("cpu");

  const s = $derived(app.snap);
  const h = $derived(app.history);

  /** GPU, temperatures and disk active time; each field may be null when the system does not expose it */
  let hw = $state.raw(null);
  const gpuHist = $state({});
  const diskActiveHist = $state([]);

  const keep = (arr, v) => {
    arr.push(v);
    if (arr.length > HISTORY) arr.shift();
  };

  async function loadHardware() {
    try {
      const x = await invoke("get_hardware");
      hw = x;
      x.gpus.forEach((g, i) => keep((gpuHist[i] ??= []), g.usage ?? 0));
      if (x.disk_active != null) keep(diskActiveHist, x.disk_active);
    } catch (e) {
      console.error(e);
    }
  }

  onMount(() => {
    loadHardware();
    const timer = setInterval(() => !document.hidden && loadHardware(), 1000);
    return () => clearInterval(timer);
  });

  const gpus = $derived(hw?.gpus ?? []);
  const gpuTitle = (i) => (gpus.length > 1 ? `GPU ${i}` : "GPU");
  const deg = (v) => `${v.toFixed(0)} °C`;

  const cards = $derived([
    { id: "cpu", title: "CPU", sub: s ? `${s.cpu.toFixed(0)}%${hw?.cpu_temp != null ? ` · ${deg(hw.cpu_temp)}` : ""}` : "–", data: h.cpu, max: 100, color: "#2563eb" },
    {
      id: "mem",
      title: t("Bộ nhớ"),
      sub: s ? `${fmtBytes(s.memory_used)} / ${fmtBytes(s.memory_total)}` : "–",
      data: h.mem,
      max: 100,
      color: "#9333ea",
    },
    {
      id: "disk",
      title: t("Ổ đĩa"),
      sub: s ? `${fmtRate(s.disk_read + s.disk_write)}${hw?.disk_active != null ? ` · ${hw.disk_active.toFixed(0)}%` : ""}` : "–",
      data: h.disk,
      max: null,
      color: "#16a34a",
    },
    { id: "net", title: t("Mạng"), sub: s ? `↓ ${fmtRate(s.net_rx)}  ↑ ${fmtRate(s.net_tx)}` : "–", data: h.net, max: null, color: "#d97706" },
    ...gpus.map((g, i) => ({
      id: "gpu" + i,
      title: gpuTitle(i),
      sub: g.usage != null ? `${g.usage.toFixed(0)}%${g.temp != null ? ` · ${deg(g.temp)}` : ""}` : g.name,
      data: gpuHist[i] ?? [],
      max: 100,
      color: "#db2777",
    })),
  ]);
  const cur = $derived(cards.find((c) => c.id === tab) ?? cards[0]);
  const gpuIndex = $derived(tab.startsWith("gpu") ? Number(tab.slice(3)) : -1);
  const gpu = $derived(gpuIndex >= 0 ? gpus[gpuIndex] : null);
</script>

<section class="view perf">
  <aside class="cards">
    {#each cards as c (c.id)}
      <button class="card" class:on={cur.id === c.id} onclick={() => (tab = c.id)}>
        <div class="mini"><Sparkline data={c.data} max={c.max} color={c.color} /></div>
        <div><strong>{c.title}</strong><br /><small>{c.sub}</small></div>
      </button>
    {/each}
  </aside>

  {#if s}
    <div class="detail">
      <h2>{cur.title}</h2>
      <div class="big"><Sparkline data={cur.data} max={cur.max} color={cur.color} /></div>
      <p class="hint">{t("60 giây gần nhất")}{cur.max ? t(" · thang 0–100%") : t(" · thang tự động")}</p>

      {#if cur.id === "cpu"}
        <p class="model">{s.cpu_name}</p>
        <dl>
          <dt>{t("Mức sử dụng")}</dt><dd>{s.cpu.toFixed(0)}%</dd>
          <dt>{t("Tốc độ")}</dt><dd>{(s.cpu_freq_mhz / 1000).toFixed(2)} GHz</dd>
          {#if hw?.cpu_temp != null}<dt>{t("Nhiệt độ")}</dt><dd>{deg(hw.cpu_temp)}</dd>{/if}
          <dt>{t("Số luồng")}</dt><dd>{s.cores.length}</dd>
          <dt>{t("Tiến trình")}</dt><dd>{s.processes.length}</dd>
          <dt>{t("Thời gian chạy")}</dt><dd>{fmtDuration(s.uptime)}</dd>
        </dl>
        <h3>{t("Từng nhân")}</h3>
        <div class="cores">
          {#each s.cores as c, i (i)}
            <div class="core" title={t("Nhân {i}: {pct}%", { i, pct: c.toFixed(0) })}>
              <i style="height:{c}%"></i>
            </div>
          {/each}
        </div>
        {#if hw && hw.temps.length > 1}
          <h3>{t("Cảm biến nhiệt độ")}</h3>
          <dl>
            {#each hw.temps.slice(0, 10) as sensor, i (i)}<dt>{sensor.label}</dt><dd>{deg(sensor.celsius)}</dd>{/each}
          </dl>
        {:else if hw && hw.cpu_temp == null}
          <p class="hint">{t("Không đọc được nhiệt độ: hệ thống hoặc phần cứng này không cung cấp cảm biến (trên Windows thường cần quyền quản trị).")}</p>
        {/if}
      {:else if cur.id === "mem"}
        <dl>
          <dt>{t("Đang dùng")}</dt><dd>{fmtBytes(s.memory_used)}</dd>
          <dt>{t("Tổng")}</dt><dd>{fmtBytes(s.memory_total)}</dd>
          <dt>Swap / Page file</dt><dd>{fmtBytes(s.swap_used)} / {fmtBytes(s.swap_total)}</dd>
        </dl>
      {:else if cur.id === "disk"}
        <dl>
          <dt>{t("Đọc")}</dt><dd>{fmtRate(s.disk_read)}</dd>
          <dt>{t("Ghi")}</dt><dd>{fmtRate(s.disk_write)}</dd>
          {#if hw?.disk_active != null}<dt>{t("Thời gian hoạt động")}</dt><dd>{hw.disk_active.toFixed(0)}%</dd>{/if}
        </dl>
        {#if hw?.disk_active != null}
          <h3>{t("Thời gian hoạt động")}</h3>
          <div class="mid"><Sparkline data={diskActiveHist} max={100} color="#16a34a" /></div>
        {/if}
        <p class="hint">{t("Tổng I/O của các tiến trình trong 1 giây vừa qua.")}</p>
      {:else if cur.id === "net"}
        <dl>
          <dt>{t("Nhận")}</dt><dd>{fmtRate(s.net_rx)}</dd>
          <dt>{t("Gửi")}</dt><dd>{fmtRate(s.net_tx)}</dd>
        </dl>
        <p class="hint">{t("Tổng của mọi card mạng.")}</p>
      {:else if gpu}
        <p class="model">{gpu.name}</p>
        <dl>
          <dt>{t("Mức sử dụng")}</dt><dd>{gpu.usage != null ? `${gpu.usage.toFixed(0)}%` : "—"}</dd>
          {#if gpu.mem_used != null}
            <dt>{t("Bộ nhớ GPU")}</dt><dd>{fmtBytes(gpu.mem_used)}{gpu.mem_total ? ` / ${fmtBytes(gpu.mem_total)}` : ""}</dd>
          {/if}
          {#if gpu.temp != null}<dt>{t("Nhiệt độ")}</dt><dd>{deg(gpu.temp)}</dd>{/if}
        </dl>
        {#if gpu.usage == null}
          <p class="hint">{t("Không đọc được mức dùng GPU: hệ thống hoặc driver này không cung cấp.")}</p>
        {/if}
      {/if}

      <p class="hint">{s.host} · {s.os}</p>
    </div>
  {/if}
</section>

<script>
  import { app } from "./store.svelte.js";
  import Sparkline from "./Sparkline.svelte";
  import { fmtBytes, fmtRate, fmtDuration } from "./format.js";
  import { t } from "./i18n.svelte.js";

  let tab = $state("cpu");

  const s = $derived(app.snap);
  const h = $derived(app.history);

  const cards = $derived([
    { id: "cpu", title: "CPU", sub: s ? `${s.cpu.toFixed(0)}%` : "–", data: h.cpu, max: 100, color: "#2563eb" },
    {
      id: "mem",
      title: t("Bộ nhớ"),
      sub: s ? `${fmtBytes(s.memory_used)} / ${fmtBytes(s.memory_total)}` : "–",
      data: h.mem,
      max: 100,
      color: "#9333ea",
    },
    { id: "disk", title: t("Ổ đĩa"), sub: s ? fmtRate(s.disk_read + s.disk_write) : "–", data: h.disk, max: null, color: "#16a34a" },
    { id: "net", title: t("Mạng"), sub: s ? `↓ ${fmtRate(s.net_rx)}  ↑ ${fmtRate(s.net_tx)}` : "–", data: h.net, max: null, color: "#d97706" },
  ]);
  const cur = $derived(cards.find((c) => c.id === tab));
</script>

<section class="view perf">
  <aside class="cards">
    {#each cards as c (c.id)}
      <button class="card" class:on={tab === c.id} onclick={() => (tab = c.id)}>
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

      {#if tab === "cpu"}
        <p class="model">{s.cpu_name}</p>
        <dl>
          <dt>{t("Mức sử dụng")}</dt><dd>{s.cpu.toFixed(0)}%</dd>
          <dt>{t("Tốc độ")}</dt><dd>{(s.cpu_freq_mhz / 1000).toFixed(2)} GHz</dd>
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
      {:else if tab === "mem"}
        <dl>
          <dt>{t("Đang dùng")}</dt><dd>{fmtBytes(s.memory_used)}</dd>
          <dt>{t("Tổng")}</dt><dd>{fmtBytes(s.memory_total)}</dd>
          <dt>Swap / Page file</dt><dd>{fmtBytes(s.swap_used)} / {fmtBytes(s.swap_total)}</dd>
        </dl>
      {:else if tab === "disk"}
        <dl>
          <dt>{t("Đọc")}</dt><dd>{fmtRate(s.disk_read)}</dd>
          <dt>{t("Ghi")}</dt><dd>{fmtRate(s.disk_write)}</dd>
        </dl>
        <p class="hint">{t("Tổng I/O của các tiến trình trong 1 giây vừa qua.")}</p>
      {:else}
        <dl>
          <dt>{t("Nhận")}</dt><dd>{fmtRate(s.net_rx)}</dd>
          <dt>{t("Gửi")}</dt><dd>{fmtRate(s.net_tx)}</dd>
        </dl>
        <p class="hint">{t("Tổng của mọi card mạng.")}</p>
      {/if}

      <p class="hint">{s.host} · {s.os}</p>
    </div>
  {/if}
</section>

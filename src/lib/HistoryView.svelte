<script>
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";
  import { downsample, findSlowPeriods, GAP_SECS } from "./history.js";
  import { fmtBytes, fmtRate } from "./format.js";
  import { confirmDialog } from "./dialog.svelte.js";
  import { t, locale } from "./i18n.svelte.js";

  const RANGES = [
    [3600, "1 giờ"],
    [6 * 3600, "6 giờ"],
    [24 * 3600, "24 giờ"],
  ];
  const W = 600;
  const H = 160;

  let samples = $state.raw([]);
  let range = $state(6 * 3600);
  let loaded = $state(false);
  let hover = $state(null);
  let nowSec = $state(Math.floor(Date.now() / 1000));

  let visible = $derived(samples.filter((s) => s.t >= nowSec - range));
  let points = $derived(downsample(visible));
  let periods = $derived(findSlowPeriods(samples));

  const x = (t) => ((t - (nowSec - range)) / range) * W;
  const y = (v) => H - (Math.min(v, 100) / 100) * H;

  /** Polyline segments, broken wherever the recording has a gap */
  function segments(key) {
    const segs = [];
    let cur = [];
    let prev = null;
    for (const s of points) {
      const limit = Math.max(GAP_SECS, (range / 500) * 2);
      if (prev && s.t - prev > limit) {
        segs.push(cur);
        cur = [];
      }
      cur.push(`${x(s.t).toFixed(1)},${y(s[key]).toFixed(1)}`);
      prev = s.t;
    }
    if (cur.length) segs.push(cur);
    return segs.map((c) => c.join(" "));
  }
  let cpuSegs = $derived(segments("cpu"));
  let ramSegs = $derived(segments("ram"));

  const hhmm = (sec) => new Date(sec * 1000).toLocaleTimeString(locale(), { hour: "2-digit", minute: "2-digit" });
  const day = (sec) => new Date(sec * 1000).toLocaleDateString(locale(), { day: "2-digit", month: "2-digit" });
  const minutes = (p) => Math.max(1, Math.round((p.end - p.start) / 60));

  let ticks = $derived.by(() => {
    const step = range <= 3600 ? 600 : range <= 6 * 3600 ? 3600 : 4 * 3600;
    const first = Math.ceil((nowSec - range) / step) * step;
    const out = [];
    for (let t = first; t <= nowSec; t += step) out.push(t);
    return out;
  });

  function onMove(e) {
    if (!points.length) return;
    const box = e.currentTarget.getBoundingClientRect();
    const t = nowSec - range + ((e.clientX - box.left) / box.width) * range;
    let best = points[0];
    for (const s of points) if (Math.abs(s.t - t) < Math.abs(best.t - t)) best = s;
    hover = Math.abs(best.t - t) < Math.max(GAP_SECS, range / 100) ? { s: best, left: ((e.clientX - box.left) / box.width) * 100 } : null;
  }

  async function load() {
    try {
      samples = await invoke("get_history");
      nowSec = Math.floor(Date.now() / 1000);
    } catch (e) {
      console.error(e);
    }
    loaded = true;
  }

  async function clear() {
    const ok = await confirmDialog({
      title: t("Xoá lịch sử?"),
      message: t("Toàn bộ dữ liệu 24 giờ gần nhất sẽ bị xoá."),
      level: "caution",
      okLabel: t("Xoá"),
    });
    if (ok) {
      await invoke("clear_history");
      load();
    }
  }

  onMount(() => {
    load();
    const timer = setInterval(() => !document.hidden && load(), 30_000);
    return () => clearInterval(timer);
  });
</script>

<section class="view">
  <div class="toolbar">
    <h2>{t("Lịch sử")}</h2>
    <span class="grow"></span>
    {#each RANGES as [sec, label] (sec)}
      <button class="btn" class:eco-on={range === sec} onclick={() => (range = sec)}>{t(label)}</button>
    {/each}
    <button class="btn" onclick={clear}>{t("Xoá lịch sử")}</button>
  </div>

  {#if loaded && samples.length === 0}
    <p class="disclaimer">
      {t("Chưa có dữ liệu. App ghi lại mức tải của máy mỗi 10 giây (kể cả khi thu xuống khay) và giữ 24 giờ gần nhất.")}
      {t("Hãy kiểm tra mục \"Ghi lịch sử\" trong Cài đặt đã bật chưa.")}
    </p>
  {:else}
    <div class="chart-card">
      <div class="legend"><span class="lg cpu"></span> CPU <span class="lg ram"></span> RAM</div>
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div class="chart" onmousemove={onMove} onmouseleave={() => (hover = null)}>
        <svg viewBox="0 0 {W} {H}" preserveAspectRatio="none">
          {#each [0, 25, 50, 75, 100] as g (g)}
            <line x1="0" x2={W} y1={y(g)} y2={y(g)} class="grid" />
          {/each}
          {#each ticks as tk (tk)}
            <line x1={x(tk)} x2={x(tk)} y1="0" y2={H} class="grid" />
          {/each}
          {#each ramSegs as seg, i (i)}<polyline points={seg} class="ln ram" vector-effect="non-scaling-stroke" />{/each}
          {#each cpuSegs as seg, i (i)}<polyline points={seg} class="ln cpu" vector-effect="non-scaling-stroke" />{/each}
          {#if hover}<line x1={x(hover.s.t)} x2={x(hover.s.t)} y1="0" y2={H} class="cursor" />{/if}
        </svg>
        {#if hover}
          <div class="tip" style="left:{Math.min(Math.max(hover.left, 18), 82)}%">
            <b>{day(hover.s.t)} {hhmm(hover.s.t)}</b>
            <div>CPU {hover.s.cpu.toFixed(0)}% · RAM {hover.s.ram.toFixed(0)}%</div>
            <div class="muted">💾 {fmtRate(hover.s.disk)} · 🌐 {fmtRate(hover.s.net)}</div>
            {#each hover.s.top.slice(0, 3) as tp (tp.n)}
              <div class="muted">{tp.n}: {tp.c.toFixed(0)}% · {fmtBytes(tp.m)}</div>
            {/each}
          </div>
        {/if}
      </div>
      <div class="xaxis">
        {#each ticks as tk (tk)}<span style="left:{(x(tk) / W) * 100}%">{hhmm(tk)}</span>{/each}
      </div>
    </div>

    <h3>{t("Những lúc máy bị quá tải")}</h3>
    <div class="findings">
      {#each periods as p (p.start)}
        <article class="finding {p.kind === 'ram' ? 'notice' : 'suspicious'}">
          <header>
            <strong>{day(p.start)} {hhmm(p.start)} – {hhmm(p.end)}</strong>
            <small>({t("{n} phút", { n: minutes(p) })})</small>
            <span class="grow"></span>
            <span class="badge {p.kind === 'ram' ? 'notice' : 'suspicious'}">
              {p.kind === "both" ? "CPU + RAM" : p.kind === "ram" ? t("RAM đầy") : t("CPU cao")}
            </span>
          </header>
          <div class="muted">{t("CPU trung bình {cpu}% · RAM trung bình {ram}%", { cpu: p.avgCpu.toFixed(0), ram: p.avgRam.toFixed(0) })}</div>
          <ul>
            {#each p.culprits as c (c.name)}
              <li><b>{c.name}</b>: {c.cpu.toFixed(0)}% CPU, {fmtBytes(c.mem)} RAM</li>
            {/each}
          </ul>
        </article>
      {:else}
        <p class="hint pad">✅ {t("Không có lúc nào máy bị quá tải (CPU ≥ 85% hoặc RAM ≥ 90%) trong 24 giờ qua.")}</p>
      {/each}
    </div>
  {/if}
</section>

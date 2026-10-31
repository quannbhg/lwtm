<script>
  import { invoke } from "@tauri-apps/api/core";
  import { app } from "./store.svelte.js";
  import { groupProcesses } from "./groups.js";
  import { fmtBytes, fmtRate } from "./format.js";

  let s = $derived(app.snap);
  let ramPct = $derived(s ? (s.memory_used / s.memory_total) * 100 : 0);
  let top = $derived(
    groupProcesses(s, app.meta)
      .filter((g) => g.name.toLowerCase() !== "system idle process")
      .sort((a, b) => b.cpu - a.cpu)[0],
  );
</script>

<div class="minibox" data-tauri-drag-region>
  <div class="row" data-tauri-drag-region>
    <span>CPU</span><b>{s ? s.cpu.toFixed(0) : "–"}%</b>
    <div class="bar"><i style="width:{s?.cpu ?? 0}%"></i></div>
  </div>
  <div class="row" data-tauri-drag-region>
    <span>RAM</span><b>{s ? ramPct.toFixed(0) : "–"}%</b>
    <div class="bar"><i class="m" style="width:{ramPct}%"></i></div>
  </div>
  <div class="io" data-tauri-drag-region>
    <span>💾 {s ? fmtRate(s.disk_read + s.disk_write) : "–"}</span>
    <span>↓ {s ? fmtRate(s.net_rx) : "–"}</span>
    <span>↑ {s ? fmtRate(s.net_tx) : "–"}</span>
  </div>
  {#if top}
    <div class="top" data-tauri-drag-region title="Ứng dụng dùng nhiều CPU nhất">
      🔥 {top.name} · {top.cpu.toFixed(0)}% · {fmtBytes(top.memory)}
    </div>
  {/if}
  <button class="open" onclick={() => invoke("show_main_window")}>Mở Task Manager</button>
</div>

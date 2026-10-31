<script>
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";
  import { confirmDialog } from "./dialog.svelte.js";

  const VERDICT = { suspicious: "Khả nghi", notice: "Cần chú ý", ok: "Bình thường" };

  let findings = $state([]);
  let scanning = $state(false);
  let scanned = $state(false);
  let showAll = $state(false);
  let error = $state("");
  let note = $state("");

  let counts = $derived({
    suspicious: findings.filter((f) => f.verdict === "suspicious").length,
    notice: findings.filter((f) => f.verdict === "notice").length,
    ok: findings.filter((f) => f.verdict === "ok").length,
  });
  let shown = $derived(showAll ? findings : findings.filter((f) => f.verdict !== "ok"));

  async function scan() {
    if (scanning) return;
    scanning = true;
    error = "";
    try {
      findings = await invoke("scan_services");
      scanned = true;
    } catch (e) {
      error = String(e);
    }
    scanning = false;
  }

  async function copy(hash) {
    try {
      await navigator.clipboard.writeText(hash);
      note = "Đã sao chép mã hash";
    } catch {
      note = hash;
    }
  }

  async function lookup(hash) {
    try {
      await invoke("open_virustotal", { hash });
    } catch (e) {
      error = String(e);
    }
  }

  async function stop(f) {
    const ok = await confirmDialog({
      title: "Dừng dịch vụ này?",
      message: `Dừng "${f.display}" (${f.name}). Việc này không xoá file, bạn vẫn cần gỡ cài đặt hoặc quét bằng phần mềm diệt virus nếu nghi ngờ.`,
      level: "caution",
      okLabel: "Dừng dịch vụ",
    });
    if (!ok) return;
    try {
      await invoke("control_service", { name: f.name, action: "stop" });
      error = "";
      scan();
    } catch (e) {
      error = String(e);
    }
  }

  onMount(scan);
</script>

<section class="view">
  <div class="toolbar">
    <h2>Bảo mật dịch vụ</h2>
    <span class="hint grow"></span>
    <label class="check"><input type="checkbox" bind:checked={showAll} /> Hiện cả mục bình thường</label>
    <button class="btn" disabled={scanning} onclick={scan}>{scanning ? "Đang quét…" : "Quét lại"}</button>
  </div>

  <p class="disclaimer">
    Đây là kiểm tra bằng dấu hiệu (heuristic), <b>không phải phần mềm diệt virus</b>. "Khả nghi" nghĩa là nên xem kỹ, không có nghĩa chắc chắn là virus. Phần mềm hợp lệ cũng có thể bị gắn cờ.
    Hãy dùng nút tra cứu VirusTotal hoặc Windows Security để kết luận.
  </p>

  {#if error}<p class="error">{error}</p>{/if}
  {#if note}<p class="hint">{note}</p>{/if}

  {#if scanned}
    <div class="summary">
      <div class="sum suspicious"><b>{counts.suspicious}</b><span>Khả nghi</span></div>
      <div class="sum notice"><b>{counts.notice}</b><span>Cần chú ý</span></div>
      <div class="sum ok"><b>{counts.ok}</b><span>Bình thường</span></div>
    </div>
  {/if}

  <div class="findings">
    {#if scanning && !scanned}
      <p class="hint pad">Đang kiểm tra đường dẫn, chữ ký số và dòng lệnh của từng dịch vụ… (có thể mất vài giây)</p>
    {:else if scanned && shown.length === 0}
      <p class="hint pad">✅ Không phát hiện dịch vụ nào có dấu hiệu bất thường.</p>
    {/if}

    {#each shown as f (f.name)}
      <article class="finding {f.verdict}">
        <header>
          <span class="badge {f.verdict}">{VERDICT[f.verdict]}</span>
          <strong>{f.display}</strong>
          <small>{f.name} · {f.status === "running" ? "đang chạy" : "đã dừng"}</small>
          <span class="grow"></span>
          <small>điểm rủi ro {f.score}</small>
        </header>
        {#if f.path}<div class="path" title={f.path}>{f.path}</div>{/if}
        {#if f.reasons.length}
          <ul>{#each f.reasons as r (r)}<li>{r}</li>{/each}</ul>
        {/if}
        <footer>
          {#if f.signed === true}<span class="tag good">✔ Có chữ ký số</span>{/if}
          {#if f.account}<span class="tag">{f.account}</span>{/if}
          <span class="grow"></span>
          {#if f.sha256}
            <button class="btn" onclick={() => lookup(f.sha256)}>Tra cứu VirusTotal</button>
            <button class="btn" onclick={() => copy(f.sha256)}>Sao chép hash</button>
          {/if}
          {#if f.status === "running" && f.verdict !== "ok"}
            <button class="btn danger" onclick={() => stop(f)}>Dừng dịch vụ</button>
          {/if}
        </footer>
      </article>
    {/each}
  </div>
</section>

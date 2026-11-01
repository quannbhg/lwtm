<script>
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";
  import { confirmDialog } from "./dialog.svelte.js";
  import { endTasks } from "./store.svelte.js";

  import { t } from "./i18n.svelte.js";

  const VERDICT = { suspicious: "Khả nghi", notice: "Cần chú ý", ok: "Bình thường" };
  const KINDS = [
    ["service", "Dịch vụ"],
    ["process", "Tiến trình đang chạy"],
    ["startup", "Ứng dụng khởi động"],
  ];

  let kind = $state("service");
  /** kind -> findings (null = not scanned yet) */
  let results = $state({ service: null, process: null, startup: null });
  let scanning = $state(false);
  let showAll = $state(false);
  let error = $state("");
  let note = $state("");

  let findings = $derived(results[kind] ?? []);
  let counts = $derived({
    suspicious: findings.filter((f) => f.verdict === "suspicious").length,
    notice: findings.filter((f) => f.verdict === "notice").length,
    ok: findings.filter((f) => f.verdict === "ok").length,
  });
  let shown = $derived(showAll ? findings : findings.filter((f) => f.verdict !== "ok"));

  async function scan(k = kind) {
    if (scanning) return;
    scanning = true;
    error = "";
    try {
      results[k] = await invoke("scan_security", { kind: k });
    } catch (e) {
      error = String(e);
    }
    scanning = false;
  }

  function pick(k) {
    kind = k;
    note = "";
    if (results[k] === null) scan(k);
  }

  async function copy(hash) {
    try {
      await navigator.clipboard.writeText(hash);
      note = t("Đã sao chép mã hash");
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

  async function stopService(f) {
    const ok = await confirmDialog({
      title: t("Dừng dịch vụ này?"),
      message: t('Dừng "{display}" ({name}). Việc này không xoá file, bạn vẫn cần gỡ cài đặt hoặc quét bằng phần mềm diệt virus nếu nghi ngờ.', {
        display: f.display,
        name: f.name,
      }),
      level: "caution",
      okLabel: t("Dừng dịch vụ"),
    });
    if (!ok) return;
    try {
      await invoke("control_service", { name: f.name, action: "stop" });
      scan();
    } catch (e) {
      error = String(e);
    }
  }

  async function endProcess(f) {
    const procs = f.pids.map((pid) => ({ pid, name: f.name, exe: f.path }));
    if (await endTasks(procs, f.display)) scan();
  }

  async function disableStartup(f) {
    const ok = await confirmDialog({
      title: t("Tắt mục khởi động này?"),
      message: t('"{display}" sẽ không tự chạy khi bật máy nữa. Bạn có thể bật lại trong tab Khởi động. Việc này không xoá file.', {
        display: f.display,
      }),
      level: "caution",
      okLabel: t("Tắt mục khởi động"),
    });
    if (!ok) return;
    try {
      await invoke("set_startup_enabled", { target: { source: f.source, key: f.key }, enabled: false });
      scan();
    } catch (e) {
      error = String(e);
    }
  }

  onMount(() => scan("service"));
</script>

<section class="view">
  <div class="toolbar">
    <h2>{t("Bảo mật")}</h2>
    <span class="grow"></span>
    <label class="check"><input type="checkbox" bind:checked={showAll} /> {t("Hiện cả mục bình thường")}</label>
    <button class="btn" disabled={scanning} onclick={() => scan()}>{scanning ? t("Đang quét…") : t("Quét lại")}</button>
  </div>

  <div class="subtabs">
    {#each KINDS as [k, label] (k)}
      <button class:on={kind === k} onclick={() => pick(k)}>{t(label)}</button>
    {/each}
  </div>

  <p class="disclaimer">
    {t("Đây là kiểm tra bằng dấu hiệu (heuristic), không phải phần mềm diệt virus. \"Khả nghi\" nghĩa là nên xem kỹ, không có nghĩa chắc chắn là virus. Phần mềm hợp lệ cũng có thể bị gắn cờ.")}
    {t("Hãy dùng nút tra cứu VirusTotal hoặc Windows Security để kết luận.")}
  </p>

  {#if error}<p class="error">{error}</p>{/if}
  {#if note}<p class="hint">{note}</p>{/if}

  {#if results[kind] !== null}
    <div class="summary">
      <div class="sum suspicious"><b>{counts.suspicious}</b><span>{t("Khả nghi")}</span></div>
      <div class="sum notice"><b>{counts.notice}</b><span>{t("Cần chú ý")}</span></div>
      <div class="sum ok"><b>{counts.ok}</b><span>{t("Bình thường")}</span></div>
    </div>
  {/if}

  <div class="findings">
    {#if scanning && results[kind] === null}
      <p class="hint pad">{t("Đang kiểm tra đường dẫn, chữ ký số và dòng lệnh… (có thể mất vài giây)")}</p>
    {:else if results[kind] !== null && shown.length === 0}
      <p class="hint pad">✅ {t("Không phát hiện mục nào có dấu hiệu bất thường.")}</p>
    {/if}

    {#each shown as f (f.kind + f.name + (f.path ?? ""))}
      <article class="finding {f.verdict}">
        <header>
          <span class="badge {f.verdict}">{t(VERDICT[f.verdict])}</span>
          <strong>{f.display}</strong>
          {#if f.kind === "service"}<small>{f.name} · {f.status === "running" ? t("đang chạy") : t("đã dừng")}</small>
          {:else if f.kind === "process"}<small>{t("{n} tiến trình", { n: f.pids.length })}</small>
          {:else}<small>{f.status === "enabled" ? t("đang bật") : t("đã tắt")}</small>{/if}
          <span class="grow"></span>
          <small>{t("điểm rủi ro {score}", { score: f.score })}</small>
        </header>
        {#if f.path}<div class="path" title={f.path}>{f.path}</div>{/if}
        {#if f.reasons.length}
          <ul>{#each f.reasons as r (r)}<li>{r}</li>{/each}</ul>
        {/if}
        <footer>
          {#if f.signed === true}<span class="tag good">✔ {t("Có chữ ký số")}</span>{/if}
          {#if f.account}<span class="tag">{f.account}</span>{/if}
          <span class="grow"></span>
          {#if f.sha256}
            <button class="btn" onclick={() => lookup(f.sha256)}>{t("Tra cứu VirusTotal")}</button>
            <button class="btn" onclick={() => copy(f.sha256)}>{t("Sao chép hash")}</button>
          {/if}
          {#if f.verdict !== "ok"}
            {#if f.kind === "service" && f.status === "running"}
              <button class="btn danger" onclick={() => stopService(f)}>{t("Dừng dịch vụ")}</button>
            {:else if f.kind === "process" && f.pids.length}
              <button class="btn danger" onclick={() => endProcess(f)}>{t("Kết thúc tiến trình")}</button>
            {:else if f.kind === "startup" && f.status === "enabled"}
              <button class="btn danger" onclick={() => disableStartup(f)}>{t("Tắt mục khởi động")}</button>
            {/if}
          {/if}
        </footer>
      </article>
    {/each}
  </div>
</section>

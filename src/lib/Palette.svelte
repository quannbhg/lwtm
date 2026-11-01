<script>
  import { invoke } from "@tauri-apps/api/core";
  import { app, nav, goto, endTasks } from "./store.svelte.js";
  import { t } from "./i18n.svelte.js";

  let isOpen = $state(false);
  let query = $state("");
  let index = $state(0);
  let inputEl = $state();
  let services = [];
  let servicesAt = 0;

  const TABS = [
    ["processes", "Tiến trình"],
    ["performance", "Hiệu năng"],
    ["history", "Lịch sử"],
    ["details", "Chi tiết"],
    ["startup", "Ứng dụng khởi động"],
    ["services", "Dịch vụ"],
    ["security", "Bảo mật dịch vụ"],
    ["settings", "Cài đặt"],
  ];

  async function show() {
    isOpen = true;
    query = "";
    index = 0;
    queueMicrotask(() => inputEl?.focus());
    if (Date.now() - servicesAt > 60_000) {
      servicesAt = Date.now();
      try {
        services = await invoke("list_services");
      } catch {
        services = [];
      }
    }
  }
  const hide = () => (isOpen = false);

  function onKey(e) {
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "k") {
      e.preventDefault();
      isOpen ? hide() : show();
    } else if (isOpen && e.key === "Escape") hide();
  }

  const rank = (text, q) => {
    const lower = text.toLowerCase();
    return lower.startsWith(q) ? 0 : lower.includes(q) ? 1 : -1;
  };

  let items = $derived.by(() => {
    const q = query.trim().toLowerCase();
    const out = [];

    for (const [id, name] of TABS) {
      const label = t(name);
      if (!q || rank(label, q) >= 0 || rank(t("đi tới") + " " + label, q) >= 0)
        out.push({ key: "tab:" + id, kind: t("Đi tới"), label, run: () => goto(id) });
    }
    if (!q) return out;

    // processes: grouped by name
    const byName = new Map();
    for (const p of app.snap?.processes ?? []) {
      const r = String(p.pid) === q ? 0 : rank(p.name, q);
      if (r < 0) continue;
      const e = byName.get(p.name) ?? { name: p.name, procs: [], r };
      e.procs.push(p);
      byName.set(p.name, e);
    }
    for (const e of [...byName.values()].sort((a, b) => a.r - b.r || b.procs.length - a.procs.length).slice(0, 5)) {
      const n = e.procs.length;
      out.push({
        key: "view:" + e.name,
        kind: t("Tiến trình"),
        label: `${e.name}${n > 1 ? ` (${n})` : ""}`,
        hint: t("Xem trong Chi tiết"),
        run: () => goto("details", e.procs[0].pid),
      });
      out.push({
        key: "end:" + e.name,
        kind: t("Tiến trình"),
        label: t("Kết thúc {name}", { name: `${e.name}${n > 1 ? ` (${n})` : ""}` }),
        hint: t("Có xác nhận"),
        run: () => endTasks(e.procs, e.name),
      });
    }

    const svc = services
      .map((s) => ({ s, r: Math.max(rank(s.display, q), rank(s.name, q)) }))
      .filter((x) => x.r >= 0)
      .sort((a, b) => a.r - b.r)
      .slice(0, 4);
    for (const { s } of svc) {
      out.push({
        key: "svc:" + s.name,
        kind: t("Dịch vụ"),
        label: s.display,
        hint: s.status === "running" ? t("đang chạy") : t("đã dừng"),
        run: () => {
          nav.search = s.display;
          goto("services");
        },
      });
    }
    return out;
  });

  $effect(() => {
    items;
    index = 0;
  });

  function onInputKey(e) {
    if (e.key === "ArrowDown") (e.preventDefault(), (index = Math.min(index + 1, items.length - 1)));
    else if (e.key === "ArrowUp") (e.preventDefault(), (index = Math.max(index - 1, 0)));
    else if (e.key === "Enter") run(items[index]);
  }

  function run(it) {
    if (!it) return;
    hide();
    it.run();
  }
</script>

<svelte:window onkeydown={onKey} />

{#if isOpen}
  <div class="backdrop top" role="presentation" onclick={hide}>
    <div class="palette" role="dialog" aria-modal="true" aria-label={t("Tìm nhanh")} tabindex="-1" onclick={(e) => e.stopPropagation()} onkeydown={() => {}}>
      <input bind:this={inputEl} bind:value={query} onkeydown={onInputKey} placeholder={t("Gõ tên tiến trình, dịch vụ hoặc tab…")} />
      <ul>
        {#each items as it, i (it.key)}
          <li class:on={i === index}>
            <button onclick={() => run(it)} onmousemove={() => (index = i)}>
              <small class="kind">{it.kind}</small>
              <span class="lbl">{it.label}</span>
              {#if it.hint}<small class="hnt">{it.hint}</small>{/if}
            </button>
          </li>
        {:else}
          <li class="empty">{t("Không có kết quả")}</li>
        {/each}
      </ul>
      <footer>{t("↑↓ chọn · Enter chạy · Esc đóng")}</footer>
    </div>
  </div>
{/if}

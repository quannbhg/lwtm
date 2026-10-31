<script>
  import { dialog, closeDialog } from "./dialog.svelte.js";

  let ack = $state(false);
  $effect(() => {
    if (dialog.open) ack = false;
  });

  const blocked = $derived(dialog.requireAck && !ack);
</script>

<svelte:window onkeydown={(e) => dialog.open && e.key === "Escape" && closeDialog(false)} />

{#if dialog.open}
  <div class="backdrop" role="presentation" onclick={() => closeDialog(false)}>
    <div class="modal {dialog.level ?? ''}" role="dialog" aria-modal="true" aria-label={dialog.title} tabindex="-1" onclick={(e) => e.stopPropagation()} onkeydown={() => {}}>
      <h3>{dialog.level === "critical" ? "⛔" : dialog.level === "caution" ? "⚠️" : ""} {dialog.title}</h3>
      <p>{dialog.message}</p>
      {#if dialog.details.length}
        <ul>
          {#each dialog.details as d (d)}<li>{d}</li>{/each}
        </ul>
      {/if}
      {#if dialog.requireAck}
        <label class="ack"><input type="checkbox" bind:checked={ack} /> Tôi hiểu rủi ro và vẫn muốn tiếp tục</label>
      {/if}
      <div class="actions">
        <button class="btn" onclick={() => closeDialog(false)}>Huỷ</button>
        <button class="primary" disabled={blocked} onclick={() => closeDialog(true)}>{dialog.okLabel}</button>
      </div>
    </div>
  </div>
{/if}

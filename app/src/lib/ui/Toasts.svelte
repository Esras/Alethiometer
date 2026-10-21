<script lang="ts">
  import { clockHM } from '../app.svelte';
  import { closeToast, hold, toasts } from '../toast.svelte';
</script>

{#each toasts.list as t (t.id)}
  <div class="toast popover" role="alert" onmouseenter={() => hold(t.id, true)} onmouseleave={() => hold(t.id, false)}>
    <div class="top">
      <i class="ph ph-warning icon"></i>Didn't work
      <span class="at tnum">{clockHM(t.at)}</span>
      <button class="x" onclick={() => closeToast(t.id)} title="Close" aria-label="Close"
        ><i class="ph ph-x"></i></button
      >
    </div>
    <div class="text">{t.text}</div>
  </div>
{/each}

<style>
  .toast {
    width: 420px;
    border-radius: var(--radius-md);
    color: var(--tx-body);
    font-size: 1rem;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .top {
    display: flex;
    gap: 8px;
    align-items: center;
    font-size: 0.8846rem;
    color: var(--tx-muted-2);
    white-space: nowrap;
  }
  .icon {
    font-size: 1.1538rem;
    color: var(--red);
  }
  .at {
    margin-left: auto;
  }
  .x {
    margin-left: 8px;
    font-size: 1.0769rem;
    color: var(--tx-muted);
    display: flex;
  }
  .x:hover {
    color: var(--tx-title);
  }
  .text {
    color: var(--tx-title);
    overflow-wrap: anywhere;
    user-select: text;
  }
</style>

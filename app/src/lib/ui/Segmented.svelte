<script lang="ts" generics="T">
  // A small segmented control (Settings): one button per option, the current one raised.
  // It sits on app.css's .seg, as before; the design system replaces it with a cycler.
  let {
    options,
    value,
    disabled = false,
    onpick,
  }: { options: { value: T; label: string }[]; value: T; disabled?: boolean; onpick: (v: T) => void } = $props();
</script>

<div class="seg">
  {#each options as o (o.value)}
    <button class:on={o.value === value} {disabled} onclick={() => onpick(o.value)}>{o.label}</button>
  {/each}
</div>

<style>
  .seg {
    display: inline-flex;
    padding: 2px;
    border: 1px solid var(--border-strong);
    border-radius: var(--r-input);
    gap: 2px;
  }
  .seg button {
    padding: 2px 9px;
    border-radius: 4px;
    color: var(--tx-muted-2);
  }
  .seg button.on {
    background: var(--bg-head);
    color: var(--tx-title);
    box-shadow: inset 0 0 0 1px var(--border-strong);
  }
</style>

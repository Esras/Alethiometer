<script lang="ts">
  // Raised panel with a header bar (icon, label, count). Collapses from its header.
  import type { Snippet } from 'svelte';

  let {
    icon,
    iconColor = 'var(--tx-muted-2)',
    label,
    count = null,
    open = $bindable(true),
    collapsible = true,
    accent = false,
    right,
    children,
  }: {
    icon: string;
    iconColor?: string;
    label: string;
    count?: number | string | null;
    open?: boolean;
    collapsible?: boolean;
    /** Aqua left edge (e.g. Upcoming when something is soon). */
    accent?: boolean;
    right?: Snippet;
    children: Snippet;
  } = $props();
</script>

<section class="panel" class:accent>
  <header>
    <button class="toggle" disabled={!collapsible} onclick={() => (open = !open)} aria-expanded={open}>
      {#if collapsible}<i class="ph ph-caret-{open ? 'down' : 'right'} caret"></i>{/if}
      <i class="ph-bold ph-{icon}" style:color={iconColor}></i>
      <span>{label}</span>
    </button>
    <span class="right">
      {#if right}{@render right()}{/if}
      {#if count !== null}<span class="count">{count}</span>{/if}
    </span>
  </header>
  {#if open}
    {@render children()}
  {/if}
</section>

<style>
  .panel {
    flex: none;
  }
  .panel.accent {
    border-color: var(--border-strong);
    box-shadow: inset 3px 0 0 var(--aqua);
  }
  header {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 12px 0 0;
    background: var(--bg-head);
  }
  .toggle {
    display: flex;
    gap: 8px;
    align-items: center;
    padding: 7px 0 7px 12px;
    font-size: 0.8462rem;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--tx-2);
    cursor: pointer;
    flex: 1;
    min-width: 0;
    text-align: left;
  }
  .toggle:disabled {
    cursor: default;
  }
  .toggle i {
    font-size: 1rem;
  }
  .toggle .caret {
    font-size: 0.9231rem;
    color: var(--tx-muted-2);
  }
  .right {
    display: flex;
    gap: 8px;
    align-items: center;
    font-size: 0.8462rem;
  }
  .count {
    color: var(--tx-muted-2);
  }
</style>

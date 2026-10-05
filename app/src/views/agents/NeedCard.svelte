<script lang="ts">
  // The frame an agent's ask or approval sits in (AskCard, ApprovalCard): a
  // tinted card with a header row (icon, title, then the caller's meta), or,
  // while a form has the composer, one compact line. `capped` stops the card at
  // 55vh, for a body that scrolls inside it (the approval's panels); the ask
  // card's form grows with its fields.
  import type { Snippet } from 'svelte';

  let {
    icon,
    compact = false,
    title,
    meta,
    summary,
    children,
    capped = false,
  }: {
    icon: string;
    compact?: boolean;
    title: Snippet;
    /** After the title in the header row. */
    meta?: Snippet;
    /** The compact line's text after the title. */
    summary?: Snippet;
    children?: Snippet;
    capped?: boolean;
  } = $props();
</script>

{#if compact}
  <div class="one">
    <i class="ph ph-{icon}"></i>
    <span class="t">{@render title()}</span>
    {@render summary?.()}
    <span class="hint">back after <kbd>Esc</kbd></span>
  </div>
{:else}
  <div class="card" class:capped>
    <div class="top">
      <i class="ph ph-{icon} big"></i>
      <span class="t">{@render title()}</span>
      {@render meta?.()}
    </div>
    {@render children?.()}
  </div>
{/if}

<style>
  .card {
    border-radius: var(--radius-md);
    background: var(--need-soft);
    border: 1px solid var(--need-soft-border);
    display: flex;
    flex-direction: column;
  }
  .card.capped {
    max-height: 55vh;
  }
  .top {
    display: flex;
    gap: 10px;
    align-items: center;
    padding: 8px 12px 6px;
  }
  .big {
    font-size: 1.231rem;
    color: var(--need);
  }
  .t {
    color: var(--tx-title);
    font-weight: 500;
    white-space: nowrap;
  }
  .one {
    display: flex;
    gap: 10px;
    align-items: center;
    padding: 6px 12px;
    border-radius: var(--radius-md);
    background: var(--need-soft);
    border: 1px solid var(--need-soft-border);
    min-width: 0;
  }
  .one i {
    font-size: 1.077rem;
    color: var(--need);
  }
  .hint {
    font-size: 0.846rem;
    color: var(--tx-label);
    white-space: nowrap;
  }
</style>

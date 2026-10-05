<script lang="ts">
  // Pick a topic's category: type to filter, ↑/↓ or ⌃j/⌃k move, ↵ picks, Esc closes.
  import { call } from '../../lib/api';
  import { pointerMoved, useKeys } from '../../lib/keys.svelte';
  import type { CategoryRow } from '../../lib/types';

  let { current, onpick, onclose }: { current: string; onpick: (key: string) => void; onclose: () => void } = $props();

  let cats = $state<CategoryRow[]>([]);
  let sel = $state(0);
  let q = $state('');
  let input = $state<HTMLInputElement>();
  call<CategoryRow[]>('categories.list').then((c) => {
    cats = c;
    sel = Math.max(0, c.findIndex((x) => x.key === current));
  });
  $effect(() => input?.focus());

  const matches = $derived(q.trim() ? cats.filter((c) => c.label.toLowerCase().includes(q.trim().toLowerCase())) : cats);
  $effect(() => {
    if (q) sel = 0;
  });

  useKeys((e) => {
    if (e.key === 'Escape') return onclose(), true;
    if (e.key === 'ArrowDown' || (e.ctrlKey && e.key === 'j')) return (sel = Math.min(sel + 1, matches.length - 1)), true;
    if (e.key === 'ArrowUp' || (e.ctrlKey && e.key === 'k')) return (sel = Math.max(sel - 1, 0)), true;
    if (e.key === 'Enter') return matches[sel] && onpick(matches[sel].key), true;
    return e.target !== input;
  });
</script>

<svelte:window onmousedown={onclose} />

<div class="picker" onmousedown={(e) => e.stopPropagation()} role="listbox" tabindex="-1">
  <input bind:this={input} bind:value={q} placeholder="Category…" />
  {#each matches as c, i (c.key)}
    <button class="opt" class:on={i === sel} onclick={() => onpick(c.key)} onmousemove={(e) => pointerMoved(e) && (sel = i)}>
      <span class="sq" style:background={c.color}></span>
      <span class="ellipsis">{c.label}</span>
      {#if c.key === current}<i class="ph ph-check"></i>{/if}
    </button>
  {:else}
    <div class="none">No category matches.</div>
  {/each}
</div>

<style>
  .picker {
    position: absolute;
    z-index: 30;
    min-width: 200px;
    padding: 4px;
    border-radius: var(--r-input);
    border: 1px solid var(--border-strong);
    background: var(--bg-app);
    box-shadow: var(--popover);
  }
  input {
    width: 100%;
    margin-bottom: 4px;
    padding: 5px 8px;
    border: none;
    border-bottom: 1px solid var(--border);
    background: none;
    outline: none;
    color: var(--tx-body);
    font: inherit;
  }
  .none {
    padding: 5px 8px;
    color: var(--tx-label);
  }
  .opt {
    width: 100%;
    display: flex;
    gap: 8px;
    align-items: center;
    padding: 5px 8px;
    border-radius: 4px;
    text-align: left;
    color: var(--tx-2);
  }
  .opt.on {
    background: var(--aqua-tint);
    color: var(--tx-title);
  }
  .opt i {
    margin-left: auto;
    color: var(--aqua);
  }
  .sq {
    width: 10px;
    height: 10px;
    flex: none;
    border-radius: 2px;
  }
</style>

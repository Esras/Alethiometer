<script lang="ts">
  // Pick a topic's category: type to filter, ↑/↓ move, ↵ picks, Esc
  // closes (lib/ui/picker.svelte.ts).
  import { call } from '../../lib/api';
  import { Picker } from '../../lib/ui/picker.svelte';
  import type { CategoryRow } from '../../lib/types';

  // `placement`: where the box sits, by the site's own anchor (it was reached
  // into through :global(.picker)). To settle in the visible pass, with the
  // design system's Popover, which owns placement.
  let {
    current,
    placement = {},
    onpick,
    onclose,
  }: {
    current: string;
    placement?: { position?: 'absolute' | 'static'; top?: string; left?: string };
    onpick: (key: string) => void;
    onclose: () => void;
  } = $props();

  let cats = $state<CategoryRow[]>([]);

  const p = new Picker<undefined>({ rows: (): number => matches.length, pick, onclose: () => onclose() });
  call<CategoryRow[]>('categories.list').then((c) => {
    cats = c;
    p.sel = Math.max(0, c.findIndex((x) => x.key === current));
  });

  const matches = $derived(p.q.trim() ? cats.filter((c) => c.label.toLowerCase().includes(p.q.trim().toLowerCase())) : cats);

  function pick(i: number) {
    if (matches[i]) onpick(matches[i].key);
  }
</script>

<div class="picker" role="listbox" use:p.frame style:position={placement.position} style:top={placement.top} style:left={placement.left}>
  <input class="input" use:p.field bind:value={p.q} placeholder="Category…" />
  {#each matches as c, i (c.key)}
    <button class="opt" class:on={i === p.sel} {...p.row(i)}>
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
    margin-bottom: 4px;
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

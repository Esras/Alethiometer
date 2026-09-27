<script lang="ts">
  import { call } from '../lib/api';
  import { catColor, openTopic, ui } from '../lib/app.svelte';
  import { useKeys } from '../lib/keys.svelte';
  import type { SearchResults } from '../lib/types';
  import AppIcon from '../lib/ui/AppIcon.svelte';

  let q = $state('');
  let res = $state<SearchResults>({ topics: [], items: [] });
  let sel = $state(0);
  let input: HTMLInputElement;

  type Row = { kind: 'topic' | 'item'; id: number; title: string; topicId: number | null; source?: string; category?: string };
  const rows = $derived<Row[]>([
    ...res.topics.map((t) => ({ kind: 'topic' as const, id: t.id, title: t.title, topicId: t.id, category: t.category })),
    ...res.items.map((i) => ({ kind: 'item' as const, id: i.id, title: i.title, topicId: i.topic_id, source: i.source })),
  ]);

  let seq = 0;
  $effect(() => {
    const query = q.trim();
    const mine = ++seq;
    if (!query) {
      res = { topics: [], items: [] };
      return;
    }
    call<SearchResults>('search.query', { q: query }).then((r) => {
      if (mine === seq) {
        res = r;
        sel = 0;
      }
    });
  });

  $effect(() => input?.focus());

  function choose(r: Row | undefined) {
    if (!r) return;
    ui.searchOpen = false;
    if (r.topicId !== null) openTopic(r.topicId);
  }

  useKeys((e) => {
    if (e.key === 'Escape') {
      ui.searchOpen = false;
      return true;
    }
    if (e.key === 'ArrowDown' || (e.ctrlKey && e.key === 'j')) {
      sel = Math.min(sel + 1, rows.length - 1);
      return true;
    }
    if (e.key === 'ArrowUp' || (e.ctrlKey && e.key === 'k')) {
      sel = Math.max(sel - 1, 0);
      return true;
    }
    if (e.key === 'Enter') {
      choose(rows[sel]);
      return true;
    }
  });
</script>

<div class="scrim" onclick={() => (ui.searchOpen = false)} role="presentation"></div>
<div class="palette" role="dialog" aria-label="Search">
  <div class="field"><i class="ph ph-magnifying-glass"></i><input bind:this={input} bind:value={q} placeholder="Search everything" /></div>
  {#if rows.length}
    <div class="results scroll">
      {#each rows as r, i}
        <button class="row" class:sel={i === sel} onclick={() => choose(r)} onmouseenter={() => (sel = i)}>
          {#if r.kind === 'topic'}
            <span class="sq" style:background={catColor(r.category ?? '')}></span>
          {:else}
            <AppIcon source={r.source ?? ''} size={14} />
          {/if}
          <span class="ellipsis">{r.title}</span>
          <span class="kind">{r.kind === 'topic' ? 'Topic' : r.topicId === null ? 'Unsorted' : ''}</span>
        </button>
      {/each}
    </div>
  {:else if q.trim()}
    <div class="empty">No matches</div>
  {/if}
</div>

<style>
  .scrim {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.35);
    z-index: 50;
  }
  .palette {
    position: fixed;
    z-index: 51;
    top: 90px;
    left: 50%;
    transform: translateX(-50%);
    width: 560px;
    border-radius: var(--r-panel);
    background: var(--bg-panel);
    border: 1px solid var(--border-strong);
    box-shadow: 0 12px 32px rgba(0, 0, 0, 0.5);
    overflow: hidden;
  }
  .field {
    display: flex;
    gap: 10px;
    align-items: center;
    padding: 10px 14px;
    border-bottom: 1px solid var(--border);
    color: var(--tx-label);
  }
  .field input {
    flex: 1;
    background: none;
    border: 0;
    outline: none;
    font-size: 14px;
    color: var(--tx-body);
  }
  .results {
    max-height: 360px;
    padding: 4px 0;
  }
  .row {
    width: 100%;
    display: grid;
    grid-template-columns: 16px minmax(0, 1fr) auto;
    gap: 10px;
    align-items: center;
    padding: 7px 14px;
    text-align: left;
  }
  .row.sel {
    background: var(--aqua-tint);
    box-shadow: inset -2px 0 0 var(--aqua);
  }
  .sq {
    width: 8px;
    height: 8px;
    border-radius: 2px;
    justify-self: center;
  }
  .kind {
    font-size: 11px;
    color: var(--tx-label);
  }
  .empty {
    padding: 12px 14px;
    color: var(--tx-label);
  }
</style>

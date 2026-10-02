<script lang="ts">
  // Search results (18d). ⌘K stays a quick jump; ⌘↵ there (or ↵ with nothing
  // to pick) opens this view with focus on the list, so single keys work:
  // j/k move, ↵ opens (the topic, else the app), space expands in place (as
  // in a topic's chronology), t moves the signal to another topic, s opens
  // the split review for it, u undoes a move, / goes back to the query, Esc
  // goes back to where you were. Each result shows its source, a snippet with
  // the match marked, its topic ("no topic" when it has none) and its date.
  // Source chips filter the list.
  import { call, openExternal } from '../lib/api';
  import { catColor, closeSearch, openTopic, refresh, ui } from '../lib/app.svelte';
  import { plain, useKeys } from '../lib/keys.svelte';
  import { moveSignal, undoMove, type Move } from '../lib/reclassify';
  import { sourceLabel } from '../lib/sources';
  import AppIcon from '../lib/ui/AppIcon.svelte';
  import Dock from '../lib/ui/Dock.svelte';
  import DockHost from '../lib/ui/DockHost.svelte';
  import SplitReview from './incoming/SplitReview.svelte';
  import TopicPicker from './incoming/TopicPicker.svelte';
  import SignalDetail from './shared/SignalDetail.svelte';

  interface Result {
    id: number;
    title: string;
    source: string;
    when: string;
    snippet: string;
    topic: { id: number; name: string; category: string; state: 'confirmed' | 'suggested' } | null;
    msgs: number;
  }

  let results = $state<Result[] | null>(null);
  let filter = $state<string | null>(null);
  let sel = $state<number | null>(null);
  let expanded = $state<number | null>(null);
  let picking = $state(false);
  let reviewing = $state<number | null>(null);
  let moves = $state<Move[]>([]);
  let err = $state<string | null>(null);

  $effect(() => {
    const q = ui.searchQuery;
    void ui.rev;
    call<{ q: string; results: Result[] }>('search.results', { q }).then((r) => {
      if (r.q !== ui.searchQuery) return;
      results = r.results;
      if (sel === null || !r.results.some((x) => x.id === sel)) sel = r.results[0]?.id ?? null;
    });
  });

  // A new query starts fresh.
  let lastQuery = '';
  $effect(() => {
    if (ui.searchQuery === lastQuery) return;
    lastQuery = ui.searchQuery;
    filter = null;
    sel = null;
    expanded = null;
    moves = [];
  });

  const chips = $derived.by(() => {
    const out: { source: string; count: number }[] = [];
    for (const r of results ?? []) {
      const c = out.find((x) => x.source === r.source);
      if (c) c.count++;
      else out.push({ source: r.source, count: 1 });
    }
    return out;
  });
  const shown = $derived((results ?? []).filter((r) => filter === null || r.source === filter));
  const current = $derived(shown.find((r) => r.id === sel) ?? null);

  $effect(() => {
    if (sel !== null) document.querySelector(`[data-result="${sel}"]`)?.scrollIntoView({ block: 'nearest' });
  });

  /** "a \u0001b\u0002 c" → text and marked parts. */
  function parts(snippet: string): { text: string; hit: boolean }[] {
    return snippet
      .split(/(\u0001[^\u0002]*\u0002)/)
      .filter(Boolean)
      .map((p) => (p.startsWith('\u0001') ? { text: p.slice(1, -1), hit: true } : { text: p, hit: false }));
  }

  function step(d: number) {
    if (!shown.length) return;
    const i = shown.findIndex((r) => r.id === sel);
    sel = shown[Math.max(0, Math.min(shown.length - 1, i < 0 ? 0 : i + d))].id;
    if (expanded !== sel) expanded = null;
  }

  function open(r: Result | null) {
    if (!r) return;
    if (r.topic) return openTopic(r.topic.id);
    call<{ url: string | null }>('incoming.detail', { item_id: r.id }).then((d) => {
      if (d.url) openExternal(d.url);
    });
  }

  async function moveTo(to: number, name: string) {
    picking = false;
    const r = current;
    if (!r) return;
    try {
      const m = await moveSignal(r.id, r.topic?.id ?? null, to, name);
      moves = [...moves.filter((x) => x.itemId !== r.id), m];
      err = null;
      refresh();
    } catch (e) {
      err = String((e as Error).message ?? e);
    }
  }

  async function undo() {
    const m = moves.find((x) => x.itemId === sel) ?? moves[moves.length - 1];
    if (!m) return;
    try {
      await undoMove(m);
      moves = moves.filter((x) => x !== m);
      sel = m.itemId;
      err = null;
      refresh();
    } catch (e) {
      err = String((e as Error).message ?? e);
    }
  }

  function endReview(changed: boolean) {
    reviewing = null;
    if (changed) refresh();
  }

  useKeys((e) => {
    if (picking || reviewing !== null || ui.searchOpen) return;
    if (plain(e, 'j') || plain(e, 'ArrowDown')) return step(1), true;
    if (plain(e, 'k') || plain(e, 'ArrowUp')) return step(-1), true;
    if (plain(e, 'Enter')) return open(current), true;
    if (plain(e, ' ')) return sel !== null && (expanded = expanded === sel ? null : sel), true;
    if (plain(e, 't')) return (picking = !!current), true;
    if (plain(e, 's')) return current && current.msgs > 1 && (reviewing = current.id), true;
    if (plain(e, 'u')) return undo(), true;
    if (plain(e, 'Escape')) return expanded !== null ? (expanded = null) : closeSearch(), true;
  });
</script>

<div class="view">
  <div class="panel">
    <div class="head">
      <span class="count">
        {#if results}{shown.length} {shown.length === 1 ? 'result' : 'results'} for <span class="q">"{ui.searchQuery}"</span>{/if}
      </span>
      <span class="chips">
        <button class="chip" class:on={filter === null} onclick={() => (filter = null)}>All <span class="n">{results?.length ?? 0}</span></button>
        {#each chips as c (c.source)}
          <button class="chip" class:on={filter === c.source} onclick={() => (filter = filter === c.source ? null : c.source)}>
            {sourceLabel(c.source)} <span class="n">{c.count}</span>
          </button>
        {/each}
      </span>
    </div>
    <div class="list scroll">
      {#each shown as r (r.id)}
        {@const move = moves.find((m) => m.itemId === r.id)}
        {#if reviewing === r.id}
          <div class="review"><SplitReview itemId={r.id} filed onclose={endReview} /></div>
        {:else}
          <div class="row" class:sel={sel === r.id} data-result={r.id} onclick={() => (sel = r.id)} ondblclick={() => open(r)} role="presentation">
            <span class="icon"><AppIcon source={r.source} size={16} /></span>
            <div class="main">
              <span class="title">{r.title}</span>
              {#if move}
                <span class="quiet">moved to {move.toName} · <span class="u">u</span> undo</span>
              {:else}
                <span class="snippet">
                  {#each parts(r.snippet) as p, i (i)}{#if p.hit}<mark>{p.text}</mark>{:else}{p.text}{/if}{/each}
                </span>
              {/if}
              {#if expanded === r.id}
                <div onclick={(ev) => ev.stopPropagation()} role="presentation">
                  <SignalDetail itemId={r.id} hints={r.msgs > 1 ? 't move · s split · esc collapse' : 't move · esc collapse'} />
                </div>
              {/if}
            </div>
            {#if r.topic}
              <span class="topic"><span class="bar" style:background={catColor(r.topic.category)}></span><span class="ellipsis">{r.topic.name}</span></span>
            {:else}
              <span class="topic none"><span class="bar"></span>no topic</span>
            {/if}
            <span class="when">{r.when}</span>
          </div>
        {/if}
      {:else}
        {#if results}<div class="empty">Nothing matches “{ui.searchQuery}”.</div>{/if}
      {/each}
    </div>
    {#if picking && current}
      <Dock>
        <TopicPicker exclude={current.topic?.id ?? null} placeholder="Move this signal to…" onpick={moveTo} onclose={() => (picking = false)} />
      </Dock>
    {/if}
    <DockHost left={18} />
    <div class="hints">
      <span>j/k move</span><span>↵ open</span><span>space expand</span><span class="key">t move to topic</span>
      {#if current && current.msgs > 1}<span class="key">s split</span>{/if}
      {#if moves.length}<span>u undo</span>{/if}<span>/ edit search</span><span>esc back</span>
      {#if err}<span class="err"><i class="ph ph-warning"></i>{err}</span>{/if}
    </div>
  </div>
</div>

<style>
  .view {
    flex: 1;
    min-height: 0;
    display: flex;
    justify-content: center;
    padding: 18px 28px 16px;
  }
  .panel {
    position: relative;
    width: 100%;
    max-width: 1060px;
    display: flex;
    flex-direction: column;
    border-radius: var(--r-panel);
    background: var(--bg-panel);
    border: 1px solid var(--border);
    overflow: hidden;
  }
  .head {
    flex: none;
    display: flex;
    gap: 12px;
    align-items: center;
    flex-wrap: wrap;
    padding: 12px 18px;
    border-bottom: 1px solid var(--bg-head);
  }
  .count {
    font-size: 1.0769rem;
    color: var(--tx-body);
  }
  .q {
    color: var(--tx-title);
  }
  .chips {
    display: flex;
    gap: 6px;
    margin-left: auto;
  }
  .chip {
    font-size: 0.8846rem;
    padding: 3px 9px;
    border-radius: 6px;
    border: 1px solid var(--border);
    color: var(--tx-muted);
    white-space: nowrap;
  }
  .chip .n {
    color: var(--tx-label);
  }
  .chip.on {
    background: var(--aqua);
    border-color: var(--aqua);
    color: var(--bg-app);
  }
  .chip.on .n {
    color: inherit;
  }
  .list {
    flex: 1;
    min-height: 0;
    padding: 8px 0;
  }
  .row {
    display: grid;
    grid-template-columns: 18px minmax(0, 1fr) 170px 70px;
    gap: 14px;
    align-items: start;
    padding: 10px 18px;
    border-radius: 0 6px 6px 0;
    cursor: pointer;
  }
  .row.sel {
    background: var(--aqua-tint);
    box-shadow: inset 3px 0 0 var(--aqua);
  }
  .icon {
    margin-top: 2px;
  }
  .main {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 3px;
  }
  .title {
    font-size: 1.0385rem;
  }
  .sel .title {
    color: var(--tx-title);
    font-weight: 500;
  }
  .snippet {
    font-size: 0.9231rem;
    color: var(--tx-muted-2);
    line-height: 1.5;
  }
  mark {
    background: rgba(250, 189, 47, 0.18);
    color: var(--tx-body);
    border-radius: 2px;
    padding: 0 1px;
  }
  .quiet {
    font-size: 0.9231rem;
    color: var(--tx-label);
  }
  .u {
    color: var(--tx-2);
  }
  .topic {
    display: flex;
    gap: 7px;
    align-items: center;
    font-size: 0.9231rem;
    color: var(--tx-2);
    padding-top: 2px;
    min-width: 0;
  }
  .topic .bar {
    flex: none;
    width: 3px;
    height: 13px;
    border-radius: 2px;
  }
  .topic.none {
    color: var(--tx-disabled);
  }
  .topic.none .bar {
    background: var(--border-strong);
  }
  .when {
    font-size: 0.9231rem;
    color: var(--tx-label);
    padding-top: 2px;
    text-align: right;
  }
  .review {
    margin: 4px 0;
    background: var(--aqua-tint-soft);
    box-shadow: inset 3px 0 0 var(--aqua);
  }
  .empty {
    padding: 16px 18px;
    color: var(--tx-label);
  }
  .hints {
    flex: none;
    display: flex;
    gap: 16px;
    padding: 8px 16px;
    border-top: 1px solid var(--border);
    font-size: 0.8846rem;
    color: var(--tx-label);
    white-space: nowrap;
    overflow: hidden;
  }
  .hints .key {
    color: var(--tx-2);
  }
  .err {
    margin-left: auto;
    color: var(--orange);
    display: flex;
    gap: 6px;
    align-items: center;
  }
</style>

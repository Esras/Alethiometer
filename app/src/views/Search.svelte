<script lang="ts">
  // Search results (18d). ⌘K stays a quick jump; ⌘↵ there (or ↵ with nothing
  // to pick) opens this view with focus on the list, so single keys work:
  // j/k move, ↵ opens (the topic, else the app), space expands in place (as
  // in a topic's chronology), t moves the signal to another topic, s opens
  // the split review for it, u undoes a move, / goes back to the query, Esc
  // goes back to where you were. Each result shows its source, a snippet with
  // the match marked, its topic ("no topic" when it has none) and its date.
  // Glean results join local ones (⌥↵ asks, or ⌘K had already asked); the
  // facet bar groups them by route (hover highlights, click filters, ⌥click
  // excludes). A Glean result opens in its own app; it isn't stored, so it
  // can't be moved or split.
  import { openExternal } from '../lib/api';
  import { catColor, closeSearch, openTopic, refresh, ui } from '../lib/app.svelte';
  import { hydrated, inLens, parts, ROUTE_COLOR, type HRow, type Hydrated, type Lens } from '../lib/hydrated';
  import { chord, plain, useKeys } from '../lib/keys.svelte';
  import { moveSignal, undoMove, type Move } from '../lib/reclassify';
  import AppIcon from '../lib/ui/AppIcon.svelte';
  import Dock from '../lib/ui/Dock.svelte';
  import DockHost from '../lib/ui/DockHost.svelte';
  import FacetBar from '../lib/ui/FacetBar.svelte';
  import SplitReview from './incoming/SplitReview.svelte';
  import TopicPicker from './incoming/TopicPicker.svelte';
  import SignalDetail from './shared/SignalDetail.svelte';

  let res = $state<Hydrated | null>(null);
  let hover = $state<Lens | null>(null);
  let filter = $state<Lens | null>(null);
  let exclude = $state<Lens | null>(null);
  let sel = $state<string | null>(null);
  let expanded = $state<string | null>(null);
  let picking = $state(false);
  let reviewing = $state<number | null>(null);
  let moves = $state<Move[]>([]);
  let err = $state<string | null>(null);
  let asking = $state(false);
  // Bumped after this view's own changes. Not a live query: results are for
  // the query as asked, and asking again can call Glean.
  let again = $state(0);

  function reload() {
    again++;
    refresh();
  }

  $effect(() => {
    const q = ui.searchQuery;
    const glean = ui.searchGlean;
    void again;
    asking = glean;
    hydrated(q, null, glean)
      .then((r) => {
        if (r.q !== ui.searchQuery) return;
        res = r;
        if (sel === null || !r.rows.some((x) => x.key === sel)) sel = r.rows[0]?.key ?? null;
      })
      .catch((e) => (err = String(e.message ?? e)))
      .finally(() => (asking = false));
  });

  // A new query starts fresh.
  let lastQuery = '';
  $effect(() => {
    if (ui.searchQuery === lastQuery) return;
    lastQuery = ui.searchQuery;
    filter = exclude = null;
    sel = null;
    expanded = null;
    moves = [];
  });

  const shown = $derived((res?.rows ?? []).filter((r) => inLens(r, filter) && !(exclude && inLens(r, exclude))));
  const current = $derived(shown.find((r) => r.key === sel) ?? null);

  $effect(() => {
    if (sel !== null) document.querySelector(`[data-result="${CSS.escape(sel)}"]`)?.scrollIntoView({ block: 'nearest' });
  });

  function step(d: number) {
    if (!shown.length) return;
    const i = shown.findIndex((r) => r.key === sel);
    sel = shown[Math.max(0, Math.min(shown.length - 1, i < 0 ? 0 : i + d))].key;
    if (expanded !== sel) expanded = null;
  }

  function open(r: HRow | null) {
    if (!r) return;
    if (r.topic) return openTopic(r.topic.id);
    if (r.url) openExternal(r.url);
  }

  async function moveTo(to: number, name: string) {
    picking = false;
    const r = current;
    if (r?.item_id == null) return;
    try {
      const m = await moveSignal(r.item_id, r.topic?.id ?? null, to, name);
      moves = [...moves.filter((x) => x.itemId !== r.item_id), m];
      err = null;
      reload();
    } catch (e) {
      err = String((e as Error).message ?? e);
    }
  }

  async function undo() {
    const m = moves.find((x) => `item:${x.itemId}` === sel) ?? moves[moves.length - 1];
    if (!m) return;
    try {
      await undoMove(m);
      moves = moves.filter((x) => x !== m);
      sel = `item:${m.itemId}`;
      err = null;
      reload();
    } catch (e) {
      err = String((e as Error).message ?? e);
    }
  }

  function endReview(changed: boolean) {
    reviewing = null;
    if (changed) reload();
  }

  const local = $derived(current?.item_id != null);

  useKeys((e) => {
    if (picking || reviewing !== null || ui.searchOpen) return;
    if (e.key === 'Enter' && e.altKey) return (ui.searchGlean = true), true;
    if (plain(e, 'j') || plain(e, 'ArrowDown')) return step(1), true;
    if (plain(e, 'k') || plain(e, 'ArrowUp')) return step(-1), true;
    if (plain(e, 'Enter')) return open(current), true;
    if (plain(e, ' ')) return sel !== null && local && (expanded = expanded === sel ? null : sel), true;
    if (plain(e, 't')) return (picking = local), true;
    if (plain(e, 's')) return current?.item_id != null && current.msgs > 1 && (reviewing = current.item_id), true;
    if (plain(e, 'u')) return undo(), true;
    if (plain(e, 'Escape')) return expanded !== null ? (expanded = null) : closeSearch(), true;
  });
</script>

<div class="view">
  <div class="panel col">
    <div class="head">
      <span class="total">
        {#if res}{shown.length} {shown.length === 1 ? 'result' : 'results'} for <span class="q">"{ui.searchQuery}"</span>{/if}
      </span>
      {#if asking}<span class="asking">asking Glean…</span>{/if}
    </div>
    {#if res}
      <div class="facets"><FacetBar facets={res.facets} total={shown.length} bind:hover bind:filter bind:exclude /></div>
    {/if}
    <div class="list scroll">
      {#each shown as r (r.key)}
        {@const move = moves.find((m) => r.item_id === m.itemId)}
        {@const lit = hover !== null && inLens(r, hover)}
        {#if r.item_id !== null && reviewing === r.item_id}
          <div class="review"><SplitReview itemId={r.item_id} filed onclose={endReview} /></div>
        {:else}
          <div
            class="row"
            class:sel={sel === r.key}
            class:dim={hover !== null && !lit}
            class:lit
            style:--rc={hover ? ROUTE_COLOR[hover.route] : 'transparent'}
            data-result={r.key}
            onclick={() => (sel = r.key)}
            ondblclick={() => open(r)}
            role="presentation">
            <span class="icon" title="{r.system}{r.routes.length > 1 ? ' · local and Glean' : r.routes[0] === 'glean' ? ' via Glean' : ''}"><AppIcon source={r.icon} size={16} /></span>
            <div class="main">
              <span class="title">{r.title}</span>
              {#if move}
                <span class="quiet">moved to {move.toName} · <kbd>u</kbd> undo</span>
              {:else}
                <span class="snippet">
                  {#each parts(r.snippet) as p, i (i)}{#if p.hit}<mark>{p.text}</mark>{:else}{p.text}{/if}{/each}
                </span>
              {/if}
              {#if expanded === r.key && r.item_id !== null}
                <div onclick={(ev) => ev.stopPropagation()} role="presentation">
                  <SignalDetail itemId={r.item_id}>
                    {#snippet hints()}<kbd>t</kbd> move · {#if r.msgs > 1}<kbd>s</kbd> split{' · '}{/if}<kbd>Esc</kbd> collapse{/snippet}
                  </SignalDetail>
                </div>
              {/if}
            </div>
            {#if r.topic}
              <span class="topic"><span class="bar" style:background={catColor(r.topic.category)}></span><span class="ellipsis">{r.topic.name}</span></span>
            {:else if r.item_id === null}
              <span class="topic none"><span class="ellipsis">{r.system}{r.container ? ` › ${r.container}` : ''}</span></span>
            {:else}
              <span class="topic none"><span class="bar"></span>no topic</span>
            {/if}
            <span class="when">{r.when}</span>
          </div>
        {/if}
      {:else}
        {#if res}<div class="empty">Nothing matches “{ui.searchQuery}”.</div>{/if}
      {/each}
    </div>
    {#if picking && current}
      <Dock>
        <TopicPicker exclude={current.topic?.id ?? null} placeholder="Move this signal to…" onpick={moveTo} onclose={() => (picking = false)} />
      </Dock>
    {/if}
    <DockHost left={18} />
    <div class="hint-bar">
      <span><kbd>j</kbd> <kbd>k</kbd> move</span><span><kbd>↵</kbd> open</span>
      {#if local}<span><kbd>Space</kbd> expand</span><span class="key"><kbd>t</kbd> move to topic</span>{/if}
      {#if current && local && current.msgs > 1}<span class="key"><kbd>s</kbd> split</span>{/if}
      {#if !ui.searchGlean}<span><kbd>{chord('⌥↵', 'Alt+↵')}</kbd> Glean</span>{/if}
      {#if moves.length}<span><kbd>u</kbd> undo</span>{/if}<span><kbd>/</kbd> edit search</span><span><kbd>Esc</kbd> back</span>
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
  .facets {
    border-bottom: 1px solid var(--bg-head);
    padding: 0 4px;
  }
  .asking {
    color: var(--tx-label);
    font-size: 0.9231rem;
  }
  .row.dim {
    opacity: 0.4;
  }
  .row.lit {
    box-shadow: inset 2px 0 0 var(--rc);
  }
  .total {
    font-size: 1.0769rem;
    color: var(--tx-body);
  }
  .q {
    color: var(--tx-title);
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
  .hint-bar .key {
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

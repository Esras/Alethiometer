<script lang="ts">
  // Priorities (8b): urgent × important in a fixed 2×2 grid. Quadrants never
  // collapse away; the eye switches one between full rows and topic chips with
  // counts (Can wait starts compact). Clicking or j/k/h/l *selects* (tinted row,
  // aqua right edge) and fills the Selected bar; ↵ opens the topic, o opens the
  // item in its app, p peeks, v toggles the selected quadrant's eye.
  import { openExternal } from '../lib/api';
  import { catColor, openTopic, ui } from '../lib/app.svelte';
  import { plain, useKeys } from '../lib/keys.svelte';
  import { live } from '../lib/live';
  import { sourceLabel } from '../lib/sources';
  import AppIcon from '../lib/ui/AppIcon.svelte';
  import { popover } from '../lib/ui/popover';
  import Pill from '../lib/ui/Pill.svelte';
  import AssistantPane from '../shell/AssistantPane.svelte';
  import type { PrioritiesData, PriorityQuadrant, PriorityRow, QuadrantKey, TopicChip } from './priorities/types';

  // Grid order: top-left, top-right, bottom-left, bottom-right.
  const LABELS: Record<QuadrantKey, string> = {
    now: 'Now-ish',
    make_room: 'Make room for',
    quick: 'Quick touch',
    can_wait: 'Can wait',
  };

  let quads = $state<PriorityQuadrant[]>([]);
  let compact = $state<Record<QuadrantKey, boolean>>({ now: false, make_room: false, quick: false, can_wait: true });
  /** Selected quadrant index and row (or chip, when that quadrant is compact) index. */
  let sel = $state({ q: 0, i: 0 });
  let peek = $state(false);
  let peekPos = $state<{ top: number; left: number } | null>(null);
  let mainEl = $state<HTMLElement>();
  const rowEls: Record<string, HTMLElement> = {};

  // A live query: re-run on writes, and once a minute (the "meeting in 8m"
  // pill is computed from the time).
  $effect(() =>
    live<PrioritiesData>('priorities.list', {}, (d) => {
      quads = d.quadrants;
      clampSel();
    }),
  );

  function chipsOf(items: PriorityRow[]): TopicChip[] {
    const out: TopicChip[] = [];
    for (const r of items) {
      const c = out.find((x) => (x.topic?.id ?? null) === (r.topic?.id ?? null));
      if (c) c.count++;
      else out.push({ topic: r.topic, count: 1 });
    }
    return out.sort((a, b) => b.count - a.count);
  }

  const chips = $derived(quads.map((q) => chipsOf(q.items)));
  const size = (q: number) => (quads[q] ? (compact[quads[q].key] ? chips[q].length : quads[q].items.length) : 0);

  function clampSel() {
    if (!quads.length) return;
    if (size(sel.q) === 0) {
      const q = [0, 1, 2, 3].find((x) => size(x) > 0);
      sel = { q: q ?? 0, i: 0 };
    } else if (sel.i >= size(sel.q)) sel = { q: sel.q, i: size(sel.q) - 1 };
  }

  const selQuad = $derived(quads[sel.q]);
  const selCompact = $derived(selQuad ? compact[selQuad.key] : false);
  const selRow = $derived(selQuad && !selCompact ? (selQuad.items[sel.i] ?? null) : null);
  const selChip = $derived(selQuad && selCompact ? (chips[sel.q][sel.i] ?? null) : null);
  const selTopic = $derived(selRow ? selRow.topic : (selChip?.topic ?? null));
  const selId = $derived(selRow?.item_id ?? null);

  // Light the selected item's topic meetings on the day strip.
  $effect(() => {
    ui.topicId = selTopic?.id ?? null;
  });

  // Keep the selected row in view; close the peek when the selection changes.
  $effect(() => {
    const key = selId !== null ? `r${selId}` : `c${sel.q}-${sel.i}`;
    rowEls[key]?.scrollIntoView({ block: 'nearest' });
  });
  $effect(() => {
    void selId;
    peek = false;
  });

  function select(q: number, i: number) {
    if (size(q) === 0) return;
    sel = { q, i: Math.max(0, Math.min(i, size(q) - 1)) };
  }

  function move(dq: 'down' | 'up' | 'left' | 'right') {
    const { q, i } = sel;
    const col = q % 2;
    const row = Math.floor(q / 2);
    if (dq === 'down') {
      if (i < size(q) - 1) select(q, i + 1);
      else if (row === 0 && size(q + 2) > 0) select(q + 2, 0);
    } else if (dq === 'up') {
      if (i > 0) select(q, i - 1);
      else if (row === 1 && size(q - 2) > 0) select(q - 2, size(q - 2) - 1);
    } else if (dq === 'left' && col === 1 && size(q - 1) > 0) select(q - 1, i);
    else if (dq === 'right' && col === 0 && size(q + 1) > 0) select(q + 1, i);
  }

  /** Eye toggle. Keeps the selection on the same topic when it is the selected quadrant. */
  function toggle(q: number) {
    const key = quads[q].key;
    const toCompact = !compact[key];
    if (q === sel.q) {
      const tid = selTopic?.id ?? null;
      compact[key] = toCompact;
      const i = toCompact
        ? chips[q].findIndex((c) => (c.topic?.id ?? null) === tid)
        : quads[q].items.findIndex((r) => (r.topic?.id ?? null) === tid);
      sel = { q, i: Math.max(0, i) };
    } else {
      compact[key] = toCompact;
    }
  }

  function open() {
    if (selTopic) openTopic(selTopic.id);
  }

  function openInApp() {
    if (selRow?.url) openExternal(selRow.url);
  }

  function togglePeek() {
    if (!selRow) return;
    if (peek) {
      peek = false;
      return;
    }
    const el = rowEls[`r${selRow.item_id}`];
    if (el && mainEl) {
      const r = el.getBoundingClientRect();
      const m = mainEl.getBoundingClientRect();
      const below = r.bottom - m.top + 4;
      const fits = below + 200 < m.height;
      peekPos = { top: fits ? below : Math.max(8, r.top - m.top - 204), left: Math.max(8, r.left - m.left + 36) };
    }
    peek = true;
  }

  useKeys((e) => {
    if (plain(e, 'j') || plain(e, 'ArrowDown')) return move('down'), true;
    if (plain(e, 'k') || plain(e, 'ArrowUp')) return move('up'), true;
    if (plain(e, 'h') || plain(e, 'ArrowLeft')) return move('left'), true;
    if (plain(e, 'l') || plain(e, 'ArrowRight')) return move('right'), true;
    if (plain(e, 'Enter')) return open(), true;
    if (plain(e, 'o')) return openInApp(), true;
    if (plain(e, 'p')) return togglePeek(), true;
    if (plain(e, 'v')) return quads.length > 0 && toggle(sel.q), true;
  });

  const meta = $derived(
    selRow ? [selRow.topic?.name, selRow.when, selRow.people].filter(Boolean).join(' · ') : selChip ? `${selChip.count} ${selChip.count === 1 ? 'item' : 'items'} in ${selQuad ? LABELS[selQuad.key] : ''}` : '',
  );
</script>

{#snippet quadrant(qi: number)}
  {@const q = quads[qi]}
  {#if q}
    <section class="quad">
      <header class="panel-head">
        {LABELS[q.key]}<span class="n tnum">{q.items.length}</span>
        <button class="eye" onclick={() => toggle(qi)} title="{compact[q.key] ? 'Show rows' : 'Group by topic'} (v)">
          <i class="ph ph-{compact[q.key] ? 'eye-slash' : 'eye'}"></i>
        </button>
      </header>
      {#if compact[q.key]}
        <div class="chips">
          {#each chips[qi] as c, i}
            <button
              class="chip"
              class:on={sel.q === qi && sel.i === i}
              bind:this={rowEls[`c${qi}-${i}`]}
              onclick={() => select(qi, i)}
              ondblclick={() => c.topic && openTopic(c.topic.id)}
            >
              <span class="sq" style:background={c.topic ? catColor(c.topic.category) : 'var(--tx-label)'}></span>
              {c.topic?.name ?? 'Unsorted'}<span class="n tnum">{c.count}</span>
            </button>
          {/each}
        </div>
      {:else}
        <div class="rows scroll">
          {#each q.items as r, i (r.item_id)}
            <!-- svelte-ignore a11y_click_events_have_key_events -->
            <div
              class="row"
              class:sel={sel.q === qi && sel.i === i}
              role="option"
              tabindex="-1"
              aria-selected={sel.q === qi && sel.i === i}
              bind:this={rowEls[`r${r.item_id}`]}
              onclick={() => select(qi, i)}
              ondblclick={() => r.topic && openTopic(r.topic.id)}
            >
              <span class="bar" style:background={r.topic ? catColor(r.topic.category) : 'var(--tx-label)'}></span>
              <AppIcon source={r.source} />
              <div class="min0">
                <div class="line">
                  <span class="title">{r.title}</span>
                  {#if r.topic}<span class="topic">{r.topic.name}</span>{/if}
                </div>
                {#if r.reasons.length}
                  <div class="pills">{#each r.reasons as p}<Pill pill={p} />{/each}</div>
                {/if}
              </div>
              <span class="score tnum">{r.score.toFixed(1)}</span>
            </div>
          {:else}
            <div class="empty">Nothing here.</div>
          {/each}
        </div>
      {/if}
    </section>
  {/if}
{/snippet}

<div class="view">
  <div class="main" bind:this={mainEl}>
    <div class="head">
      <span class="h1">Priorities</span>
      <span class="legend"><span class="dot" style:background="var(--red)"></span>adds a lot</span>
      <span class="legend"><span class="dot" style:background="var(--yellow)"></span>adds some</span>
      <span class="legend"><span class="dot" style:background="var(--green)"></span>adds a little</span>
    </div>

    <div class="grid">
      <span></span>
      <div class="label">Urgent</div>
      <div class="label">Not urgent</div>
      <div class="label vert">Important</div>
      {@render quadrant(0)}
      {@render quadrant(1)}
      <div class="label vert">Less important</div>
      {@render quadrant(2)}
      {@render quadrant(3)}
    </div>

    <div class="selected">
      <span class="label">Selected</span>
      {#if selRow}
        <AppIcon source={selRow.source} />
        <span class="sel-title ellipsis">{selRow.title}</span>
        <span class="sel-meta ellipsis tnum">{meta}</span>
        <span class="hints">
          <span><kbd>↵</kbd> open topic</span>
          {#if selRow.url}<span><kbd>o</kbd> open in {sourceLabel(selRow.source)}</span>{/if}
          <span><kbd>p</kbd> peek</span>
          <span><kbd>j</kbd> <kbd>k</kbd> <kbd>h</kbd> <kbd>l</kbd> move</span>
        </span>
      {:else if selChip}
        <span class="sq" style:background={selChip.topic ? catColor(selChip.topic.category) : 'var(--tx-label)'}></span>
        <span class="sel-title ellipsis">{selChip.topic?.name ?? 'Unsorted'}</span>
        <span class="sel-meta ellipsis">{meta}</span>
        <span class="hints">
          {#if selChip.topic}<span><kbd>↵</kbd> open topic</span>{/if}
          <span><kbd>v</kbd> show rows</span>
          <span><kbd>j</kbd> <kbd>k</kbd> <kbd>h</kbd> <kbd>l</kbd> move</span>
        </span>
      {:else}
        <span class="sel-meta">Nothing to prioritize.</span>
      {/if}
    </div>

    {#if peek && selRow && peekPos}
      <div class="peek popover" use:popover={{ onclose: () => (peek = false), trigger: () => rowEls[`r${selRow.item_id}`] }} style:top="{peekPos.top}px" style:left="{peekPos.left}px" role="dialog" aria-label="Peek">
        <div class="peek-head">
          <AppIcon source={selRow.source} size={14} />{sourceLabel(selRow.source)}
          {#if selRow.when}<span class="tnum">· {selRow.when}</span>{/if}
          <span class="esc"><kbd>Esc</kbd></span>
        </div>
        <div class="peek-title">{selRow.title}</div>
        {#if selRow.summary}<div class="peek-sum">{selRow.summary}</div>{/if}
        {#if selRow.body}<div class="peek-body scroll">{selRow.body}</div>{/if}
        {#if selRow.people}<div class="peek-people">{selRow.people}</div>{/if}
      </div>
    {/if}
  </div>
  <AssistantPane topic={selTopic} />
</div>

<style>
  .view {
    flex: 1;
    min-width: 0;
    display: flex;
  }
  .main {
    position: relative;
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 14px 22px;
  }
  .head {
    display: flex;
    align-items: center;
    gap: 16px;
    font-size: 0.8846rem;
    color: var(--tx-muted-2);
    white-space: nowrap;
  }
  .h1 {
    font-size: 1.5385rem;
    font-weight: 500;
    color: var(--tx-title);
    margin-right: 8px;
  }
  .legend {
    display: flex;
    gap: 5px;
    align-items: center;
  }
  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
  }
  .grid {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: 22px minmax(0, 1fr) minmax(0, 1fr);
    grid-template-rows: auto minmax(0, 1fr) minmax(0, 1fr);
    gap: 10px;
  }
  .label {
    white-space: nowrap;
  }
  .vert {
    writing-mode: vertical-rl;
    transform: rotate(180deg);
    text-align: center;
  }
  .quad {
    min-height: 0;
    display: flex;
    flex-direction: column;
    border-radius: var(--radius-md);
    background: var(--bg-panel);
    border: 1px solid var(--border);
    overflow: hidden;
  }
  /* Neutral header: darkest ground, never aqua, so it can't read as selected. */
  header {
    flex: none;
    display: flex;
    gap: 8px;
    align-items: center;
    padding: 7px 12px;
    background: var(--bg-app);
    border-bottom: 1px solid var(--border);
    white-space: nowrap;
  }
  .eye {
    margin-left: auto;
    display: flex;
    color: var(--tx-muted-2);
    font-size: 1.0769rem;
    border-radius: var(--radius-md);
    transition: color 120ms;
  }
  .eye:hover {
    color: var(--tx-body);
  }
  .rows {
    flex: 1;
    min-height: 0;
  }
  .row {
    display: grid;
    grid-template-columns: 4px 22px minmax(0, 1fr) auto;
    gap: 10px;
    align-items: center;
    padding: var(--row-pad-comfortable) 12px var(--row-pad-comfortable) 0;
    min-height: var(--row-min-comfortable);
    border-top: 1px solid var(--bg-head);
    cursor: pointer;
    outline: none;
    transition: background 120ms;
  }
  .row:first-child {
    border-top: 0;
  }
  .row:hover:not(.sel) {
    background: var(--row-hover);
  }
  .bar {
    align-self: stretch;
  }
  .min0 {
    min-width: 0;
  }
  .line {
    display: flex;
    gap: 8px;
    align-items: baseline;
    min-width: 0;
  }
  .title {
    font-weight: 500;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    flex: 0 1 auto;
  }
  .topic {
    font-size: 0.9231rem;
    color: var(--tx-label);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    flex: 0 1000 auto;
    min-width: 0;
  }
  .pills {
    display: flex;
    gap: 4px;
    flex-wrap: wrap;
    margin-top: 4px;
  }
  .score {
    font-size: 0.8846rem;
    color: var(--tx-2);
  }
  .empty {
    padding: 10px 12px;
    color: var(--tx-label);
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    align-content: flex-start;
    gap: 6px;
    padding: 10px 12px;
    font-size: 0.9231rem;
    color: var(--tx-muted);
    overflow-y: auto;
  }
  .chip {
    transition: background 120ms;
  }
  .chip:hover:not(.on) {
    background: var(--row-hover);
  }
  .sq {
    width: 6px;
    height: 6px;
    border-radius: 2px;
    flex: none;
  }
  .n {
    color: var(--tx-label);
  }
  .selected {
    flex: none;
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 9px 14px;
    border-radius: var(--radius-md);
    background: rgba(142, 192, 124, 0.08);
    border: 1px solid var(--aqua-line);
    min-width: 0;
    white-space: nowrap;
  }
  .selected .sq {
    width: 8px;
    height: 8px;
    border-radius: 3px;
  }
  .sel-title {
    font-weight: 500;
    flex: 0 1 auto;
  }
  .sel-meta {
    font-size: 0.9231rem;
    color: var(--tx-label);
    flex: 0 1000 auto;
    min-width: 0;
  }
  .hints {
    margin-left: auto;
    display: flex;
    gap: 14px;
    font-size: 0.8846rem;
    color: var(--tx-muted);
    flex: none;
  }
  .peek {
    position: absolute;
    z-index: 10;
    width: 380px;
    border-radius: var(--radius-md);
    animation: fade 120ms ease-out;
  }
  @keyframes fade {
    from {
      opacity: 0;
    }
  }
  .peek-head {
    display: flex;
    gap: 6px;
    align-items: center;
    font-size: 0.8846rem;
    color: var(--tx-muted-2);
    white-space: nowrap;
  }
  .esc {
    margin-left: auto;
    font-size: 0.8462rem;
    color: var(--tx-label);
  }
  .peek-title {
    margin-top: 6px;
    font-weight: 500;
    color: var(--tx-title);
  }
  .peek-sum {
    margin-top: 2px;
    font-size: 0.9231rem;
    color: var(--tx-muted);
  }
  .peek-body {
    margin-top: 8px;
    max-height: 120px;
    font-size: calc(var(--fs-read) * 12.5 / 14);
    line-height: 1.55;
    color: var(--tx-2);
  }
  .peek-people {
    margin-top: 8px;
    font-size: 0.8846rem;
    color: var(--tx-label);
  }
</style>

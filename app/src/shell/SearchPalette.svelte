<script lang="ts">
  // ⌘K (hi-fi 3b/3c): local results as you type; ⌥↵ asks Glean too. The
  // anchor chip (⌥A) ties the search to the topic you're in: results that
  // aren't about it are hidden behind a count (⇧↵ shows them). The helper
  // row sits right under the field so it never moves; the facet bar groups
  // what each route returned (hover highlights, click filters, ⌥click
  // excludes). ↵ opens, ⌘T links to the topic, ⌘⌫ rejects for the topic,
  // ⌘↵ opens every result in the Search view.
  import { call, openExternal } from '../lib/api';
  import { catColor, openSearch, openTopic, refresh, ui } from '../lib/app.svelte';
  import { feedback, hydrated, inLens, ROUTE_COLOR, type HRow, type Hydrated, type Lens } from '../lib/hydrated';
  import { mod, pointerMoved } from '../lib/keys.svelte';
  import { moveSignal } from '../lib/reclassify';
  import AppIcon from '../lib/ui/AppIcon.svelte';
  import FacetBar from '../lib/ui/FacetBar.svelte';
  import FilterField from '../lib/ui/FilterField.svelte';
  import { listNav, moveSel } from '../lib/ui/listNav';
  import Modal from '../lib/ui/Modal.svelte';
  import TopicPicker from '../views/incoming/TopicPicker.svelte';

  const isMac = navigator.platform.toLowerCase().includes('mac');
  const k = (mac: string, other: string) => (isMac ? mac : other);

  // From the Search results view, `/` comes back here with its query.
  const initial = ui.tab === 'search' ? ui.searchQuery : '';
  let q = $state(initial);
  /** The topic you're in, if any: what ⌥A anchors to. */
  const context = ui.tab === 'topics' && ui.openTopicId !== null ? ui.openTopicId : ui.topicId;
  let anchored = $state(context !== null);
  let anchorTopic = $state<{ name: string; category: string } | null>(null);
  if (context !== null) call<{ name: string; category: string }>('topics.get', { id: context }).then((t) => (anchorTopic = t)).catch(() => {});
  let glean = $state(ui.tab === 'search' && ui.searchGlean);
  let res = $state<Hydrated | null>(null);
  let loading = $state(false);
  let showHidden = $state(false);
  let sel = $state(0);
  let hover = $state<Lens | null>(null);
  let filter = $state<Lens | null>(null);
  let exclude = $state<Lens | null>(null);
  let picking = $state(false);
  let note = $state<string | null>(null);
  let input = $state<HTMLInputElement | null>(null);

  let seq = 0;
  async function run() {
    const query = q.trim();
    const mine = ++seq;
    if (!query) {
      res = null;
      return;
    }
    loading = glean;
    try {
      const r = await hydrated(query, anchored ? context : null, glean);
      if (mine === seq) {
        res = r;
        sel = Math.min(sel, Math.max(0, rows.length - 1));
      }
    } catch (e) {
      if (mine === seq) note = String((e as Error).message ?? e);
    } finally {
      if (mine === seq) loading = false;
    }
  }

  // Typing searches locally; a new query forgets Glean until ⌥↵ again.
  let lastQ = initial.trim();
  $effect(() => {
    const query = q.trim();
    if (query !== lastQ) {
      lastQ = query;
      glean = false;
      sel = 0;
    }
    void anchored;
    void glean;
    run();
  });

  $effect(() => input?.focus());

  type Item = { kind: 'topic'; id: number; name: string; category: string } | { kind: 'row'; row: HRow };
  const visible = $derived((res?.rows ?? []).filter((r) => (r.kept || showHidden) && inLens(r, filter) && !(exclude && inLens(r, exclude))));
  const rows = $derived<Item[]>([
    ...(res?.topics ?? []).map((t) => ({ kind: 'topic' as const, ...t })),
    ...visible.map((row) => ({ kind: 'row' as const, row })),
  ]);
  const current = $derived(rows[sel]);
  const anchorName = $derived(res?.anchor?.name ?? anchorTopic?.name ?? null);

  function open(it: Item | undefined) {
    if (!it) return;
    if (it.kind === 'topic') {
      ui.searchOpen = false;
      return openTopic(it.id);
    }
    const r = it.row;
    ui.searchOpen = false;
    if (r.item_id !== null && r.topic) return openTopic(r.topic.id);
    if (r.url) openExternal(r.url);
  }

  async function linkTo(topicId: number, name: string) {
    picking = false;
    const it = current;
    if (it?.kind !== 'row') return;
    const r = it.row;
    try {
      if (r.item_id !== null) {
        await moveSignal(r.item_id, r.topic?.id ?? null, topicId, name);
        note = `Filed under ${name}`;
      } else {
        // Glean results aren't stored: linking pins it to the topic.
        await feedback(topicId, r, 'pin');
        note = `Pinned to ${name}`;
      }
      refresh();
      run();
    } catch (e) {
      note = String((e as Error).message ?? e);
    }
  }

  async function reject() {
    const it = current;
    if (it?.kind !== 'row') return;
    if (!anchored || context === null) {
      note = `Reject is per topic: anchor first (${k('⌥A', 'Alt+A')})`;
      return;
    }
    await feedback(context, it.row, it.row.kept ? 'reject' : 'clear');
    note = it.row.kept ? `Rejected for ${anchorName}` : `Kept for ${anchorName}`;
    run();
  }

  // Modal closes on Escape.
  function onkeys(e: KeyboardEvent): boolean {
    if (picking) return false;
    const d = listNav(e);
    if (d) {
      sel = moveSel(sel, d, rows.length);
      return true;
    }
    // ⌥A: anchor on / off (e.code: ⌥A types "å" on a Mac).
    if (e.altKey && e.code === 'KeyA') {
      if (context !== null) anchored = !anchored;
      return true;
    }
    if (e.key === 'Enter' && e.altKey) {
      if (q.trim()) glean = true;
      return true;
    }
    if (e.key === 'Enter' && e.shiftKey) {
      showHidden = !showHidden;
      return true;
    }
    // ⌘↵ (or ↵ with nothing to pick): every result, in the Search view (18d).
    if (e.key === 'Enter' && (mod(e) || !rows.length)) {
      if (q.trim()) {
        ui.searchGlean = glean;
        openSearch(q.trim());
      }
      return true;
    }
    if (e.key === 'Enter') {
      open(current);
      return true;
    }
    if (mod(e) && e.code === 'KeyT') {
      if (current?.kind !== 'row') return true;
      if (anchored && context !== null && anchorName) linkTo(context, anchorName);
      else picking = true;
      return true;
    }
    if (mod(e) && e.key === 'Backspace') {
      reject();
      return true;
    }
    return false;
  }

  $effect(() => {
    document.querySelector(`.palette [data-i="${sel}"]`)?.scrollIntoView({ block: 'nearest' });
  });
</script>

<Modal label="Search" onclose={() => (ui.searchOpen = false)} {onkeys} z={50} scrim="rgba(0, 0, 0, 0.35)">
<div class="palette">
  <div class="field">
    <FilterField class="big" bind:value={q} bind:el={input} placeholder="Search everything">
      {#if context !== null}
        <button class="chip anchor" class:on={anchored} onclick={() => (anchored = !anchored)} title="Anchor to the topic you're in ({k('⌥A', 'Alt+A')})">
          {#if anchored}
            <span class="sq" style:background={catColor(res?.anchor?.category ?? anchorTopic?.category ?? '')}></span>{anchorName ?? 'This topic'}
          {:else}
            <i class="ph ph-anchor-simple"></i>No anchor
          {/if}
          <span class="glean-key">{k('⌥A', 'Alt+A')}</span>
        </button>
      {/if}
    </FilterField>
  </div>
  <div class="helper">
    {#if anchored && res && res.hidden}
      <button class="hidden" onclick={() => (showHidden = !showHidden)}>{res.hidden} off-topic {showHidden ? 'shown' : 'hidden'}</button>
      <span class="k">⇧↵ {showHidden ? 'hide' : 'show'}</span>
    {:else if !anchored && context !== null}
      <span>Ranked by match and recency</span>
      <span class="k">{k('⌥A', 'Alt+A')} anchor to {anchorName ?? 'this topic'}</span>
    {:else if anchored}
      <span>Anchored to {anchorName ?? 'this topic'}</span>
    {:else}
      <span>Ranked by match and recency</span>
    {/if}
    <span class="grow"></span>
    {#if loading}<span class="k">asking Glean…</span>{:else if q.trim() && !glean}<span class="k">{k('⌥↵', 'Alt+↵')} Glean</span>{/if}
    <span class="k">↵ open</span>
    <span class="k">{k('⌘T', 'Ctrl+T')} link to topic</span>
    {#if anchored}<span class="k">{k('⌘⌫', 'Ctrl+⌫')} reject</span>{/if}
  </div>
  {#if res && q.trim()}
    <FacetBar facets={res.facets} total={visible.length} bind:hover bind:filter bind:exclude />
  {/if}
  {#if rows.length}
    <div class="results scroll">
      {#each rows as it, i}
        {#if it.kind === 'topic'}
          <button class="row" data-i={i} class:sel={i === sel} onclick={() => open(it)} onmousemove={(e) => pointerMoved(e) && (sel = i)}>
            <span class="sq topic" style:background={catColor(it.category)}></span>
            <span class="ellipsis">{it.name}</span>
            <span class="when">Topic</span>
          </button>
        {:else}
          {@const r = it.row}
          {@const lit = hover !== null && inLens(r, hover)}
          <button
            class="row"
            data-i={i}
            class:sel={i === sel}
            class:dim={hover !== null && !lit}
            class:dropped={!r.kept}
            style:--rc={hover ? ROUTE_COLOR[hover.route] : 'transparent'}
            class:lit
            onclick={() => open(it)}
            onmousemove={(e) => pointerMoved(e) && (sel = i)}
            title={r.reason || r.system}>
            <AppIcon source={r.icon} size={16} />
            <span class="ellipsis">{r.title}</span>
            {#if !r.kept || (i === sel && r.reason)}<span class="why">{r.reason}</span>{/if}
            {#if r.pinned}<i class="ph ph-push-pin pin"></i>{/if}
            <span class="when tnum">{r.when}</span>
          </button>
        {/if}
      {/each}
    </div>
  {:else if q.trim() && res}
    <div class="empty">No quick matches{glean ? '' : ` · ${k('⌥↵', 'Alt+↵')} asks Glean`} · {k('⌘↵', 'Ctrl+↵')} searches everything</div>
  {/if}
  {#if note}<div class="note">{note}</div>{/if}
  {#if picking && current?.kind === 'row'}
    <div class="picker">
      <TopicPicker exclude={current.row.topic?.id ?? null} placeholder="Link to topic…" onpick={linkTo} onclose={() => (picking = false)} />
    </div>
  {/if}
</div>
</Modal>

<style>
  .palette {
    position: fixed;
    top: 90px;
    left: 50%;
    transform: translateX(-50%);
    width: 740px;
    max-width: calc(100vw - 80px);
    border-radius: var(--r-panel);
    background: var(--bg-panel);
    border: 1px solid var(--border-strong);
    box-shadow: 0 12px 32px rgba(0, 0, 0, 0.5);
    overflow: hidden;
  }
  .field {
    padding: 10px 14px 4px;
  }
  /* The palette's field is the big one: same box, larger text. */
  .field :global(.big) {
    font-size: 1.1538rem;
  }
  .field :global(.big input) {
    color: var(--tx-title);
  }
  .anchor {
    padding-right: 4px;
  }
  .anchor.on {
    border-color: color-mix(in srgb, var(--yellow) 60%, transparent);
    color: var(--yellow);
    background: color-mix(in srgb, var(--yellow) 8%, transparent);
  }
  .glean-key {
    padding: 0 5px;
    border-radius: 4px;
    background: var(--bg-head);
    color: var(--tx-label);
    font-size: 0.8077rem;
  }
  .sq {
    width: 8px;
    height: 8px;
    border-radius: 2px;
    flex: none;
  }
  .helper {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 2px 14px 8px 38px;
    border-bottom: 1px solid var(--border);
    font-size: 0.9231rem;
    color: var(--tx-label);
  }
  .helper .hidden {
    color: var(--orange);
  }
  .grow {
    flex: 1;
  }
  .results {
    max-height: 420px;
    padding: 2px 0 4px;
    border-top: 1px solid var(--border);
  }
  .row {
    width: 100%;
    display: flex;
    gap: 10px;
    align-items: center;
    padding: 8px 14px;
    text-align: left;
    color: var(--tx-2);
    box-shadow: inset 2px 0 0 transparent;
    transition: opacity 120ms;
  }
  .row .ellipsis {
    flex: 1;
    min-width: 0;
  }
  .row.sel {
    background: var(--aqua-tint);
    color: var(--tx-title);
  }
  .row.lit {
    box-shadow: inset 2px 0 0 var(--rc);
    color: var(--tx-title);
  }
  .row.dim {
    opacity: 0.4;
  }
  .row.dropped {
    color: var(--tx-label);
  }
  .sq.topic {
    margin: 0 4px;
  }
  .why {
    color: var(--tx-label);
    font-size: 0.8846rem;
    white-space: nowrap;
  }
  .pin {
    color: var(--yellow);
  }
  .when {
    color: var(--tx-label);
    font-size: 0.9231rem;
    min-width: 28px;
    text-align: right;
  }
  .empty,
  .note {
    padding: 10px 14px;
    color: var(--tx-label);
    border-top: 1px solid var(--border);
  }
  .picker {
    border-top: 1px solid var(--border);
  }
</style>

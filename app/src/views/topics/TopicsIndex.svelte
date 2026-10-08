<script lang="ts" module>
  const saved = { scroll: 0, id: null as number | null, q: '', catFilter: null as string | null, archived: false };
</script>

<script lang="ts">
  // Topics index (12c): j/k move, ↵ opens, a archives, c category, n new topic, / filter
  // (in the filter: ↑/↓ move, ↵ opens, Esc leaves). m merges the
  // selected topic into another; it's staged ("merged into B · u undo") and
  // written when you leave the index (lib/merges.svelte.ts).
  import { cyclePriority, PRIORITY_NAME } from '../../lib/topicPriority';
  import { onDestroy, tick } from 'svelte';
  import { call } from '../../lib/api';
  import { live } from '../../lib/live';
  import { catColor, category, meetingPill, refresh, ui, UNCATEGORIZED } from '../../lib/app.svelte';
  import { plain, pointerMoved, useKeys } from '../../lib/keys.svelte';
  import { cancelMerge, mergingAway, merges, pendingMerge, stageMerge } from '../../lib/merges.svelte';
  import Dock from '../../lib/ui/Dock.svelte';
  import DockHost from '../../lib/ui/DockHost.svelte';
  import FilterField from '../../lib/ui/FilterField.svelte';
  import { listNav, moveSel } from '../../lib/ui/listNav';
  import TopicPicker from '../incoming/TopicPicker.svelte';
  import type { CategoryRow, TopicRow } from '../../lib/types';
  import CategoryManager from './CategoryManager.svelte';
  import CategoryPicker from './CategoryPicker.svelte';

  let archived = $state(saved.archived);
  let rows = $state<TopicRow[]>([]);
  let q = $state(saved.q);
  let selId = $state<number | null>(null);
  let input = $state<HTMLInputElement>();
  let err = $state<string | null>(null);
  let cats = $state<CategoryRow[]>([]);
  let catFilter = $state<string | null>(saved.catFilter);
  let body = $state<HTMLDivElement>();
  let restored = false;
  let managing = $state(false);
  let picking = $state<number | null>(null);
  // Picking the topic to merge the selected one into.
  let merging = $state<TopicRow | null>(null);

  $effect(() => live<TopicRow[]>('topics.list', { archived }, (r) => (rows = r)));
  $effect(() => live<CategoryRow[]>('categories.list', {}, (c) => (cats = c)));

  const matches = $derived.by(() => {
    const needle = q.trim().toLowerCase();
    const inCat = catFilter === null ? rows : rows.filter((t) => t.category === catFilter);
    return needle ? inCat.filter((t) => t.name.toLowerCase().includes(needle)) : inCat;
  });
  const active = $derived(archived ? matches : matches.filter((t) => !t.quiet));
  const quiet = $derived(archived ? [] : matches.filter((t) => t.quiet));
  const flat = $derived([...active, ...quiet]);
  const sel = $derived(
    Math.max(
      0,
      flat.findIndex((t) => t.id === selId),
    ),
  );
  const current = $derived(flat[sel] ?? null);
  const canCreate = $derived(
    !archived && q.trim().length > 0 && !rows.some((t) => t.name.toLowerCase() === q.trim().toLowerCase()),
  );
  // Chip order for [ / ] cycling: All, then categories, then Uncategorized if any.
  const chipKeys = $derived<(string | null)[]>([
    null,
    ...cats.map((c) => c.key),
    ...(rows.some((t) => !t.category) ? [''] : []),
  ]);
  function cycleCat(d: number) {
    const i = chipKeys.indexOf(catFilter);
    catFilter = chipKeys[(i < 0 ? 0 : i + d + chipKeys.length) % chipKeys.length];
  }

  $effect(() => {
    void q;
    void catFilter;
    selId = null;
  });

  $effect(() => {
    ui.topicId = current?.id ?? null;
  });

  $effect(() => {
    if (restored || !rows.length) return;
    restored = true;
    selId = saved.id;
    tick().then(() => body && (body.scrollTop = saved.scroll));
  });

  onDestroy(() => {
    Object.assign(saved, { scroll: body?.scrollTop ?? 0, id: current?.id ?? null, q, catFilter, archived });
  });

  function open(t: TopicRow | null) {
    if (!t) return;
    ui.openedFrom = null;
    ui.openTopicId = t.id;
  }

  let naming = $state<string | null>(null);
  let nameInput = $state<HTMLInputElement>();
  $effect(() => {
    if (naming !== null) nameInput?.focus();
  });

  async function create(named?: string) {
    const name = (named ?? q).trim();
    if (named === undefined && !canCreate) {
      naming = q.trim();
      return;
    }
    if (!name) return;
    try {
      const r = await call<{ id: number }>('topics.create', { name, category: catFilter ?? undefined });
      ui.openedFrom = null;
      ui.openTopicId = r.id;
      naming = null;
      refresh();
    } catch (e) {
      err = String((e as Error).message ?? e);
    }
  }

  async function toggleArchive(t: TopicRow | null) {
    if (!t) return;
    const i = flat.findIndex((r) => r.id === t.id);
    const next = (flat[i + 1] ?? flat[i - 1])?.id ?? null;
    try {
      await call(archived ? 'topics.unarchive' : 'topics.archive', { id: t.id });
      if (selId === t.id) selId = next;
      err = null;
      refresh();
    } catch (e) {
      err = String((e as Error).message ?? e);
    }
  }

  async function setPriority(t: TopicRow) {
    try {
      await cyclePriority(t.id, t.priority);
      err = null;
    } catch (e) {
      err = String((e as Error).message ?? e);
    }
  }

  async function setCategory(id: number, key: string) {
    picking = null;
    try {
      await call('topics.set_category', { id, category: key });
      err = null;
      refresh();
    } catch (e) {
      err = String((e as Error).message ?? e);
    }
  }

  function move(d: number) {
    const next = flat[moveSel(sel, d, flat.length)];
    selId = next?.id ?? null;
    document.querySelector(`[data-topic="${next?.id}"]`)?.scrollIntoView({ block: 'nearest' });
  }

  function mergeInto(into: number, intoName: string) {
    const from = merging;
    merging = null;
    if (from) stageMerge({ from: from.id, fromName: from.name, into, intoName });
  }

  useKeys((e) => {
    if (merging) return;
    if (naming !== null && e.target === nameInput) {
      if (e.key === 'Enter') return (void create(naming), true);
      if (e.key === 'Escape') return ((naming = null), true);
      return;
    }
    if (e.target === input) {
      const d = listNav(e);
      if (d) return (move(d), true);
      if (e.key === 'Enter') return (current ? open(current) : void create(), true);
      if (e.key === 'Escape') return (input?.blur(), true);
      return;
    }
    const d = listNav(e, { bare: true });
    if (d) return (move(d), true);
    if (plain(e, 'Enter')) return (open(current), true);
    if (plain(e, 'a')) return (toggleArchive(current), true);
    if (plain(e, 'n')) return (create(), true);
    if (plain(e, 'c')) return (current && (picking = current.id), true);
    if (plain(e, 'p')) return (current && setPriority(current), true);
    if (plain(e, '[')) return (cycleCat(-1), true);
    if (plain(e, ']')) return (cycleCat(1), true);
    if (plain(e, 'm')) return (current && !archived && (merging = current), true);
    if (plain(e, 'u')) return (cancelMerge(current && pendingMerge(current.id) ? current.id : undefined), true);
    if (plain(e, '/')) return (input?.focus(), true);
  });
</script>

{#snippet row(t: TopicRow)}
  {@const i = flat.indexOf(t)}
  {@const merge = pendingMerge(t.id)}
  <div
    class="row"
    class:sel={i === sel}
    class:quiet={t.quiet && !archived}
    class:merged={!!merge}
    data-topic={t.id}
    onclick={() => open(t)}
    onmousemove={(e) => pointerMoved(e) && (selId = t.id)}
    role="presentation"
  >
    <button
      class="bar"
      style:background={catColor(t.category)}
      title="Category (c)"
      aria-label="Category"
      onclick={(e) => (e.stopPropagation(), (picking = t.id))}
    ></button>
    {#if picking === t.id}
      <div class="pick">
        <CategoryPicker
          current={t.category}
          placement={{ position: 'static' }}
          onpick={(k) => setCategory(t.id, k)}
          onclose={() => (picking = null)}
        />
      </div>
    {/if}
    <div class="min0">
      <div class="name">
        <span class="ellipsis" title={t.name}>{t.name}</span>
        {#if t.priority}<span class="mono-tag prio" title="Priority: {PRIORITY_NAME[t.priority]} (p)">{t.priority}</span
          >{/if}
        {#if t.new_count > 0}<span class="count new tnum">{t.new_count} new</span>{/if}
      </div>
      {#if merge}
        <div class="path merging">merged into {merge.intoName} · <kbd>u</kbd> undo</div>
      {:else if t.obsidian_path}<div class="path ellipsis" title={t.obsidian_path}>{t.obsidian_path}</div>{:else}<div
          class="path none"
        >
          no note
        </div>{/if}
    </div>
    <span class="cell tnum">{t.open_count ? `${t.open_count} open` : '—'}</span>
    <span class="cell tnum ellipsis" title={t.last_signal ?? ''}>{t.last_signal ?? '—'}</span>
    <div class="meet">
      <button class="btn archive" onclick={(e) => (e.stopPropagation(), toggleArchive(t))}>
        <i class="ph ph-{archived ? 'arrow-counter-clockwise' : 'archive'}"></i>{archived ? 'Unarchive' : 'Archive'}
        <kbd>a</kbd>
      </button>
      {#if t.next_meeting}
        {@const p = meetingPill(t.next_meeting.start)}
        <span class="meeting"
          ><span class="pill {p.weight} tnum">{p.label}</span><span class="ellipsis" title={t.next_meeting.title}
            >{t.next_meeting.title}</span
          ></span
        >
      {/if}
    </div>
  </div>
{/snippet}

<div class="view">
  <div class="panel col">
    <div class="head">
      <FilterField
        class="topics-filter"
        icon="funnel-simple"
        bind:value={q}
        bind:el={input}
        placeholder="Filter topics"
      >
        <span class="total tnum">{rows.length} topics</span>
      </FilterField>
      <div class="seg" role="tablist" aria-label="Topics">
        <button class:on={!archived} onclick={() => (archived = false)}>Active</button>
        <button class:on={archived} onclick={() => (archived = true)}>Archived</button>
      </div>
      <button class="btn md" class:on={managing} onclick={() => (managing = !managing)} title="Manage categories"
        ><i class="ph ph-palette"></i>Categories</button
      >
      <button class="btn md primary" onclick={() => create()}><i class="ph ph-plus"></i>New topic</button>
    </div>
    {#if managing}<CategoryManager />{/if}
    <div class="chips">
      <button class="chip" class:on={catFilter === null} onclick={() => (catFilter = null)}>All</button>
      {#each cats as c (c.key)}
        <button
          class="chip"
          class:on={catFilter === c.key}
          onclick={() => (catFilter = catFilter === c.key ? null : c.key)}
        >
          <span class="sq" style:background={c.color}></span>{c.label}
        </button>
      {/each}
      {#if rows.some((t) => !t.category)}
        <button class="chip" class:on={catFilter === ''} onclick={() => (catFilter = catFilter === '' ? null : '')}>
          <span class="sq" style:background={UNCATEGORIZED.color}></span>{UNCATEGORIZED.label}
        </button>
      {/if}
    </div>
    <div class="cols label">
      <span></span><span>Topic</span><span>Open</span><span>Last signal</span><span>Next meeting</span>
    </div>
    {#if naming !== null}
      <FilterField
        class="naming"
        icon="plus"
        bind:value={naming}
        bind:el={nameInput}
        placeholder="New topic name"
        onblur={() => !naming?.trim() && (naming = null)}
      >
        {#if catFilter !== null}<span class="in">in {category(catFilter).label}</span>{/if}
        <span class="hint"><kbd>↵</kbd> create · <kbd>Esc</kbd></span>
      </FilterField>
    {/if}
    <div class="body scroll" bind:this={body}>
      {#if active.length}
        <div class="section label">{archived ? 'Archived' : 'Active'} <span class="n">{active.length}</span></div>
        {#each active as t (t.id)}{@render row(t)}{/each}
      {/if}
      {#if quiet.length}
        <div class="section label">
          Quiet · no signal in {ui.info?.quiet_days ?? 7} days <span class="n">{quiet.length}</span>
        </div>
        {#each quiet as t (t.id)}{@render row(t)}{/each}
      {/if}
      {#if !flat.length}
        {#if canCreate}
          <button class="create" onclick={() => create()}><i class="ph ph-plus"></i>Create “{q.trim()}”</button>
        {:else}
          <div class="empty-pane">{archived ? 'No archived topics.' : q ? 'No topic matches.' : 'No topics yet.'}</div>
        {/if}
      {/if}
    </div>
    {#if merging}
      <Dock under=".row.sel">
        <TopicPicker
          exclude={merging.id}
          excludeIds={mergingAway()}
          placeholder={`Merge “${merging.name}” into…`}
          onpick={mergeInto}
          onclose={() => (merging = null)}
        />
      </Dock>
    {/if}
    <DockHost left={18} />
    <div class="hint-bar">
      <span><kbd>j</kbd> <kbd>k</kbd> move</span><span><kbd>↵</kbd> open</span><span><kbd>/</kbd> filter</span>
      <span><kbd>a</kbd> {archived ? 'unarchive' : 'archive'}</span><span><kbd>c</kbd> category</span><span
        ><kbd>p</kbd> priority</span
      ><span><kbd>[</kbd> <kbd>]</kbd> filter chip</span><span><kbd>n</kbd> new topic</span>{#if !archived}<span
          ><kbd>m</kbd> merge</span
        >{/if}
      {#if merges.list.length}<span><kbd>u</kbd> undo merge</span>{/if}
      {#if err || merges.err}<span class="err"><i class="ph ph-warning"></i>{err ?? merges.err}</span>{:else}<span
          class="note">Archived topics keep their note and stop matching new signals</span
        >{/if}
    </div>
  </div>
</div>

<style>
  .view {
    flex: 1;
    min-width: 0;
    display: flex;
    justify-content: center;
    padding: 18px 22px;
  }
  .panel {
    position: relative;
    flex: 1;
    max-width: 1200px;
    min-height: 0;
  }
  .head {
    display: flex;
    gap: 12px;
    align-items: center;
    padding: 12px 18px;
    border-bottom: 1px solid var(--border);
  }
  .head :global(.topics-filter) {
    flex: 1;
  }
  .total {
    font-size: 0.8846rem;
  }
  .cols,
  .row {
    display: grid;
    grid-template-columns: 4px minmax(0, 1fr) 70px 120px 240px;
    gap: 14px;
    align-items: center;
  }
  .cols {
    padding: 12px 18px 4px 0;
  }
  .section {
    padding: 10px 18px 6px;
  }
  .body {
    flex: 1;
    min-height: 0;
  }
  .row {
    padding: var(--row-pad-comfortable) 18px var(--row-pad-comfortable) 0;
    min-height: var(--row-min-comfortable);
    cursor: pointer;
  }
  .bar {
    align-self: stretch;
    border-radius: 0 var(--radius-sm) var(--radius-sm) 0;
    cursor: pointer;
  }
  .row {
    position: relative;
  }
  .pick {
    position: absolute;
    z-index: 30;
    left: 10px;
    top: calc(100% - 6px);
  }
  .panel :global(.naming) {
    width: auto;
    margin: 10px 18px 0;
  }
  .in,
  .hint {
    font-size: 0.8846rem;
    color: var(--tx-label);
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
    padding: 8px 18px 0;
    font-size: 0.9231rem;
  }
  .sq {
    width: 9px;
    height: 9px;
    border-radius: 2px;
  }
  .min0 {
    min-width: 0;
  }
  .name {
    display: flex;
    gap: 8px;
    align-items: center;
    font-size: 1.0769rem;
    color: var(--tx-title);
  }
  .sel .name {
    font-weight: 500;
  }
  .count.new {
    flex: none;
  }
  .prio {
    flex: none;
    font-weight: 400;
    color: var(--tx-2);
  }
  .path {
    font-size: 0.8846rem;
    color: var(--tx-muted-2);
  }
  .path.none {
    font-style: italic;
  }
  .path.merging {
    color: var(--tx-label);
  }
  .row.merged {
    background: var(--hatch);
  }
  .row.merged .name {
    color: var(--tx-label);
    text-decoration: line-through;
    text-decoration-color: var(--tx-disabled);
  }
  .cell {
    font-size: 0.9615rem;
    color: var(--tx-muted);
  }
  .quiet > :not(.bar):not(.pick) {
    opacity: 0.6;
  }
  .meet {
    min-width: 0;
    display: flex;
    justify-content: flex-start;
  }
  .meeting {
    min-width: 0;
    display: flex;
    gap: 8px;
    align-items: center;
    font-size: 0.9615rem;
    color: var(--tx-muted);
  }
  .archive {
    display: none;
    margin-left: auto;
  }
  .row:hover .archive {
    display: flex;
  }
  .row:hover .meeting {
    display: none;
  }
  .create {
    display: flex;
    gap: 8px;
    align-items: center;
    margin: 10px 18px;
    color: var(--aqua);
  }
  .empty-pane {
    display: block;
    padding: 16px 18px;
    font: inherit;
    text-align: left;
  }
  .hint-bar {
    padding: 8px 18px;
  }
  .note {
    margin-left: auto;
    color: var(--tx-muted);
  }
  .err {
    margin-left: auto;
    display: flex;
    gap: 6px;
    align-items: center;
  }
</style>

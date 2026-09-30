<script lang="ts" module>
  const saved = { scroll: 0, id: null as number | null, q: '', catFilter: null as string | null, archived: false };
</script>

<script lang="ts">
  // Topics index (12c): j/k move, ↵ opens, a archives, c category, n new topic, / filter
  // (in the filter: ↑/↓ or ⌃j/⌃k move, ↵ opens, Esc leaves).
  import { onDestroy, tick } from 'svelte';
  import { call } from '../../lib/api';
  import { catColor, meetingPill, refresh, ui } from '../../lib/app.svelte';
  import { plain, pointerMoved, useKeys } from '../../lib/keys.svelte';
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

  $effect(() => {
    void ui.rev;
    const a = archived;
    call<TopicRow[]>('topics.list', { archived: a }).then((r) => {
      if (archived === a) rows = r;
    });
  });

  $effect(() => {
    void ui.rev;
    call<CategoryRow[]>('categories.list').then((c) => (cats = c));
  });


  const matches = $derived.by(() => {
    const needle = q.trim().toLowerCase();
    const inCat = catFilter === null ? rows : rows.filter((t) => t.category === catFilter);
    return needle ? inCat.filter((t) => t.name.toLowerCase().includes(needle)) : inCat;
  });
  const active = $derived(archived ? matches : matches.filter((t) => !t.quiet));
  const quiet = $derived(archived ? [] : matches.filter((t) => t.quiet));
  const flat = $derived([...active, ...quiet]);
  const sel = $derived(Math.max(0, flat.findIndex((t) => t.id === selId)));
  const current = $derived(flat[sel] ?? null);
  const canCreate = $derived(!archived && q.trim().length > 0 && !rows.some((t) => t.name.toLowerCase() === q.trim().toLowerCase()));

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
    selId = (flat[i + 1] ?? flat[i - 1])?.id ?? null;
    try {
      await call(archived ? 'topics.unarchive' : 'topics.archive', { id: t.id });
      err = null;
      refresh();
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
    const next = flat[Math.max(0, Math.min(flat.length - 1, sel + d))];
    selId = next?.id ?? null;
    document.querySelector(`[data-topic="${next?.id}"]`)?.scrollIntoView({ block: 'nearest' });
  }

  useKeys((e) => {
    if (naming !== null && e.target === nameInput) {
      if (e.key === 'Enter') return void create(naming), true;
      if (e.key === 'Escape') return (naming = null), true;
      return;
    }
    if (e.target === input) {
      if (e.key === 'ArrowDown' || (e.ctrlKey && e.key === 'j')) return move(1), true;
      if (e.key === 'ArrowUp' || (e.ctrlKey && e.key === 'k')) return move(-1), true;
      if (e.key === 'Enter') return current ? open(current) : void create(), true;
      if (e.key === 'Escape') return input?.blur(), true;
      return;
    }
    if (plain(e, 'j') || plain(e, 'ArrowDown')) return move(1), true;
    if (plain(e, 'k') || plain(e, 'ArrowUp')) return move(-1), true;
    if (plain(e, 'Enter')) return open(current), true;
    if (plain(e, 'a')) return toggleArchive(current), true;
    if (plain(e, 'n')) return create(), true;
    if (plain(e, 'c')) return current && (picking = current.id), true;
    if (plain(e, '/')) return input?.focus(), true;
  });
</script>

{#snippet row(t: TopicRow)}
  {@const i = flat.indexOf(t)}
  <div class="row" class:sel={i === sel} class:quiet={t.quiet && !archived} data-topic={t.id} onclick={() => open(t)} onmousemove={(e) => pointerMoved(e) && (selId = t.id)} role="presentation">
    <button class="bar" style:background={catColor(t.category)} title="Category (c)" aria-label="Category" onclick={(e) => (e.stopPropagation(), (picking = t.id))}></button>
    {#if picking === t.id}
      <div class="pick"><CategoryPicker current={t.category} onpick={(k) => setCategory(t.id, k)} onclose={() => (picking = null)} /></div>
    {/if}
    <div class="min0">
      <div class="name">
        <span class="ellipsis" title={t.name}>{t.name}</span>
        {#if t.new_count > 0}<span class="badge tnum">{t.new_count} new</span>{/if}
      </div>
      {#if t.obsidian_path}<div class="path ellipsis" title={t.obsidian_path}>{t.obsidian_path}</div>{:else}<div class="path none">no note</div>{/if}
    </div>
    <span class="cell tnum">{t.open_count ? `${t.open_count} open` : '—'}</span>
    <span class="cell tnum ellipsis" title={t.last_signal ?? ''}>{t.last_signal ?? '—'}</span>
    <div class="meet">
      <button class="btn archive" onclick={(e) => (e.stopPropagation(), toggleArchive(t))}>
        <i class="ph ph-{archived ? 'arrow-counter-clockwise' : 'archive'}"></i>{archived ? 'Unarchive' : 'Archive'} <kbd>a</kbd>
      </button>
      {#if t.next_meeting}
        {@const p = meetingPill(t.next_meeting.start)}
        <span class="meeting"><span class="pill {p.weight} tnum">{p.label}</span><span class="ellipsis" title={t.next_meeting.title}>{t.next_meeting.title}</span></span>
      {/if}
    </div>
  </div>
{/snippet}

<div class="view">
  <div class="panel">
    <div class="head">
      <label class="filter">
        <i class="ph ph-funnel-simple"></i>
        <input bind:this={input} bind:value={q} placeholder="Filter topics" />
        <span class="count tnum">{rows.length} topics</span>
      </label>
      <div class="seg" role="tablist" aria-label="Topics">
        <button class:on={!archived} onclick={() => (archived = false)}>Active</button>
        <button class:on={archived} onclick={() => (archived = true)}>Archived</button>
      </div>
      <button class="btn cats-btn" class:on={managing} onclick={() => (managing = !managing)} title="Manage categories"><i class="ph ph-palette"></i>Categories</button>
      <button class="btn primary new" onclick={() => create()}><i class="ph ph-plus"></i>New topic</button>
    </div>
    {#if managing}<CategoryManager />{/if}
    <div class="chips">
      <button class="chip" class:on={catFilter === null} onclick={() => (catFilter = null)}>All</button>
      {#each cats as c (c.key)}
        <button class="chip" class:on={catFilter === c.key} onclick={() => (catFilter = catFilter === c.key ? null : c.key)}>
          <span class="sq" style:background={c.color}></span>{c.label}
        </button>
      {/each}
    </div>
    <div class="cols label"><span></span><span>Topic</span><span>Open</span><span>Last signal</span><span>Next meeting</span></div>
    {#if naming !== null}
      <div class="naming">
        <i class="ph ph-plus"></i>
        <input bind:this={nameInput} bind:value={naming} placeholder="New topic name" onblur={() => !naming?.trim() && (naming = null)} />
        {#if catFilter}<span class="in">in {cats.find((c) => c.key === catFilter)?.label}</span>{/if}
        <span class="hint">↵ create · esc</span>
      </div>
    {/if}
    <div class="body scroll" bind:this={body}>
      {#if active.length}
        <div class="section label">{archived ? 'Archived' : 'Active'} <span class="n">{active.length}</span></div>
        {#each active as t (t.id)}{@render row(t)}{/each}
      {/if}
      {#if quiet.length}
        <div class="section label">Quiet · no signal in {ui.info?.quiet_days ?? 7} days <span class="n">{quiet.length}</span></div>
        {#each quiet as t (t.id)}{@render row(t)}{/each}
      {/if}
      {#if !flat.length}
        {#if canCreate}
          <button class="create" onclick={() => create()}><i class="ph ph-plus"></i>Create “{q.trim()}”</button>
        {:else}
          <div class="empty">{archived ? 'No archived topics.' : q ? 'No topic matches.' : 'No topics yet.'}</div>
        {/if}
      {/if}
    </div>
    <div class="hints">
      <span>j/k move</span><span>↵ open</span><span>/ filter</span>
      <span>a {archived ? 'unarchive' : 'archive'}</span><span>c category</span><span>n new topic</span>
      {#if err}<span class="err"><i class="ph ph-warning"></i>{err}</span>{:else}<span class="note">Archived topics keep their note and stop matching new signals</span>{/if}
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
    flex: 1;
    max-width: 1200px;
    min-height: 0;
    display: flex;
    flex-direction: column;
    border-radius: var(--r-panel);
    background: var(--bg-panel);
    border: 1px solid var(--border);
    overflow: hidden;
  }
  .head {
    display: flex;
    gap: 12px;
    align-items: center;
    padding: 12px 18px;
    border-bottom: 1px solid var(--border);
  }
  .filter {
    flex: 1;
    display: flex;
    gap: 10px;
    align-items: center;
    padding: 7px 12px;
    border-radius: var(--r-input);
    border: 1px solid var(--border-strong);
    background: var(--bg-app);
    color: var(--tx-label);
  }
  .filter input {
    flex: 1;
    min-width: 0;
    background: none;
    border: none;
    outline: none;
    color: var(--tx-body);
    font: inherit;
  }
  .count {
    font-size: 11.5px;
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
  .n {
    color: var(--tx-muted-2);
    margin-left: 4px;
  }
  .body {
    flex: 1;
    min-height: 0;
  }
  .row {
    padding: 9px 18px 9px 0;
    cursor: pointer;
  }
  .bar {
    align-self: stretch;
    border-radius: 0 2px 2px 0;
    cursor: pointer;
  }
  .row {
    position: relative;
  }
  .pick {
    position: absolute;
    left: 10px;
    top: calc(100% - 6px);
  }
  .pick :global(.picker) {
    position: static;
  }
  .cats-btn,
  .new {
    display: flex;
    gap: 6px;
    align-items: center;
    padding: 5px 12px;
  }
  .cats-btn.on {
    border-color: var(--aqua);
    color: var(--aqua);
  }
  .naming {
    display: flex;
    gap: 10px;
    align-items: center;
    margin: 10px 18px 0;
    padding: 7px 12px;
    border-radius: var(--r-input);
    border: 1px solid var(--aqua);
    color: var(--aqua);
  }
  .naming input {
    flex: 1;
    min-width: 0;
    background: none;
    border: none;
    outline: none;
    color: var(--tx-body);
    font: inherit;
  }
  .in,
  .hint {
    font-size: 11.5px;
    color: var(--tx-label);
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
    padding: 8px 18px 0;
    font-size: 12px;
  }
  .chip {
    display: flex;
    gap: 6px;
    align-items: center;
    padding: 2px 9px;
    border-radius: 10px;
    color: var(--tx-muted);
  }
  .chip.on {
    background: rgba(142, 192, 124, 0.18);
    color: var(--aqua);
    box-shadow: inset 0 0 0 1px rgba(142, 192, 124, 0.5);
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
    font-size: 14px;
    color: var(--tx-title);
  }
  .sel .name {
    font-weight: 500;
  }
  .badge {
    flex: none;
    font-size: 10.5px;
    padding: 1px 7px;
    border-radius: 9px;
    background: rgba(142, 192, 124, 0.2);
    color: var(--aqua);
  }
  .path {
    font-size: 11.5px;
    color: var(--tx-muted-2);
  }
  .path.none {
    font-style: italic;
  }
  .cell {
    font-size: 12.5px;
    color: var(--tx-muted);
  }
  .quiet > :not(.bar) {
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
    font-size: 12.5px;
    color: var(--tx-muted);
  }
  .archive {
    display: none;
    gap: 6px;
    align-items: center;
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
  .empty {
    padding: 16px 18px;
    color: var(--tx-label);
  }
  .hints {
    flex: none;
    display: flex;
    gap: 16px;
    padding: 8px 18px;
    border-top: 1px solid var(--border);
    font-size: 11.5px;
    color: var(--tx-label);
    white-space: nowrap;
    overflow: hidden;
  }
  .note {
    margin-left: auto;
    color: var(--tx-muted);
  }
  .err {
    margin-left: auto;
    color: var(--orange);
    display: flex;
    gap: 6px;
    align-items: center;
  }
</style>

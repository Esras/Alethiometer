<script lang="ts">
  import { call, openExternal } from '../lib/api';
  import { live } from '../lib/live';
  import { catColor, openTopic, refresh, ui } from '../lib/app.svelte';
  import { plain, useKeys } from '../lib/keys.svelte';
  import AppIcon from '../lib/ui/AppIcon.svelte';
  import FilterField from '../lib/ui/FilterField.svelte';
  import AssistantPane from '../shell/AssistantPane.svelte';
  import SignalDetail from './shared/SignalDetail.svelte';
  import TodoForm from './todos/TodoForm.svelte';
  import type { Draft, JiraTarget, Todo, TodosData } from './todos/types';

  const ICON = { local: 'notes', jira: 'jira', obsidian: 'obsidian' } as const;
  const SOURCES = [
    { key: 'all', label: 'All' },
    { key: 'local', label: 'Alethiometer' },
    { key: 'jira', label: 'Jira' },
    { key: 'obsidian', label: 'Obsidian' },
  ] as const;
  type SortKey = 'due' | 'sched' | 'title' | 'status' | 'topic' | 'priority' | 'estimate';
  const SORTS: SortKey[] = ['due', 'priority', 'estimate', 'sched', 'topic', 'status', 'title'];
  function parseDur(v: string): number | null | undefined {
    const t = v.trim().toLowerCase().replace(/\s+/g, '');
    if (!t) return null;
    if (/^\d+(\.\d+)?$/.test(t)) return Math.round(Number(t));
    const m = t.match(/^(?:(\d+(?:\.\d+)?)d)?(?:(\d+(?:\.\d+)?)h)?(?:(\d+)m?)?$/);
    if (!m || (!m[1] && !m[2] && !m[3])) return undefined;
    const mins = Math.round(Number(m[1] ?? 0) * DAY + Number(m[2] ?? 0) * 60 + Number(m[3] ?? 0));
    return mins > 0 ? mins : undefined;
  }
  const DAY = 480;
  const dur = (m: number) =>
    m >= DAY && m % DAY === 0
      ? `${m / DAY}d`
      : m < 60
        ? `${m}m`
        : `${Math.floor(m / 60)}h${m % 60 ? `${m % 60}m` : ''}`;
  const signalOf = (t: Todo | null) => (t?.ext.type === 'local' ? t.ext.from_item : null);
  const isEpic = (t: Todo) => t.ext.type === 'jira' && t.ext.issue_type?.toLowerCase() === 'epic';

  let todos = $state<Todo[]>([]);
  let jira = $state<JiraTarget | null>(null);
  let struck = $state<Set<number>>(new Set());
  let source = $state<(typeof SOURCES)[number]['key']>('all');
  let q = $state('');
  let sort = $state<{ key: SortKey; asc: boolean }>({ key: 'due', asc: true });
  let sel = $state<number | null>(null);
  let creating = $state(false);
  let err = $state<string | null>(null);
  let filterEl = $state<HTMLInputElement>();
  let editing = $state<number | null>(null);
  /** The todo whose scheduled day is being edited (`d`, local todos only). */
  let scheduling = $state<number | null>(null);
  let open = $state<number | null>(null);
  let asked = false;
  const focusEl = (el: HTMLInputElement) => el.select();

  // Kept current by the core (lib/live.ts): writes here and elsewhere show up on their own.
  async function apply(d: TodosData) {
    const kept = todos.filter((t) => struck.has(t.item_id) && !d.todos.some((n) => n.item_id === t.item_id));
    todos = [...d.todos, ...kept];
    jira = d.jira;
    if (!asked && d.todos.some((t) => !t.estimate)) {
      asked = true;
      // What it estimates arrives as a new result.
      await call('ai.estimate_todos').catch(() => {});
    }
  }

  $effect(() => live<TodosData>('todos.list', {}, apply));

  async function setEstimate(t: Todo, value: string) {
    const minutes = parseDur(value);
    if (minutes === undefined) {
      err = `"${value}" isn't a time: try 45m, 1h30m, 2h or 1d (empty lets the AI guess)`;
      return;
    }
    editing = null;
    try {
      await call('todos.estimate', { item_id: t.item_id, minutes });
      err = null;
      asked = minutes !== null && asked;
    } catch (e) {
      err = String((e as Error).message ?? e);
    }
  }

  const counts = $derived(
    Object.fromEntries(
      SOURCES.map((s) => [s.key, todos.filter((t) => s.key === 'all' || t.ext.type === s.key).length]),
    ),
  );

  function cmp(a: Todo, b: Todo, key: SortKey): number {
    const last = (x: unknown) => (x === null || x === undefined ? 1 : 0);
    switch (key) {
      case 'due':
        return last(a.due) - last(b.due) || (a.due ?? '').localeCompare(b.due ?? '');
      case 'priority':
        return last(a.rank) - last(b.rank) || (a.rank ?? 0) - (b.rank ?? 0);
      case 'sched':
        return last(a.sched) - last(b.sched) || (a.sched ?? '').localeCompare(b.sched ?? '');
      case 'topic':
        return last(a.topic) - last(b.topic) || (a.topic?.name ?? '').localeCompare(b.topic?.name ?? '');
      case 'estimate':
        return last(a.estimate) - last(b.estimate) || (a.estimate?.minutes ?? 0) - (b.estimate?.minutes ?? 0);
      case 'status':
        return (a.ext.type === 'jira' ? a.ext.status : '~').localeCompare(b.ext.type === 'jira' ? b.ext.status : '~');
      default:
        return a.title.localeCompare(b.title);
    }
  }

  const rows = $derived.by(() => {
    const needle = q.trim().toLowerCase();
    const out = todos.filter(
      (t) =>
        (source === 'all' || t.ext.type === source) &&
        (!needle || [t.title, t.from, t.topic?.name].some((s) => s?.toLowerCase().includes(needle))),
    );
    return out.sort((a, b) => (sort.asc ? 1 : -1) * cmp(a, b, sort.key) || cmp(a, b, 'due'));
  });

  $effect(() => {
    const id = ui.openTodoId;
    if (id === null || !todos.some((t) => t.item_id === id)) return;
    if (!rows.some((t) => t.item_id === id)) {
      source = 'all';
      q = '';
    }
    sel = id;
    open = id;
    ui.openTodoId = null;
  });

  $effect(() => {
    if (!rows.some((t) => t.item_id === sel)) sel = rows[0]?.item_id ?? null;
  });

  $effect(() => {
    if (sel !== null) document.querySelector(`[data-todo="${sel}"]`)?.scrollIntoView({ block: 'nearest' });
  });

  const current = $derived(rows.find((t) => t.item_id === sel) ?? null);

  function expand(t: Todo | null) {
    if (!t) return;
    sel = t.item_id;
    open = open === t.item_id ? null : t.item_id;
  }

  function sortBy(key: SortKey) {
    sort = sort.key === key ? { key, asc: !sort.asc } : { key, asc: true };
  }

  function step(d: number) {
    const i = rows.findIndex((t) => t.item_id === sel);
    sel = rows[Math.max(0, Math.min(rows.length - 1, i + d))]?.item_id ?? null;
  }

  async function toggle(t: Todo | null) {
    if (!t) return;
    const done = !struck.has(t.item_id);
    const next = new Set(struck);
    if (done) next.add(t.item_id);
    else next.delete(t.item_id);
    struck = next;
    try {
      await call(done ? 'todos.complete' : 'todos.reopen', { item_id: t.item_id });
      err = null;
      refresh();
    } catch (e) {
      const back = new Set(struck);
      if (done) back.delete(t.item_id);
      else back.add(t.item_id);
      struck = back;
      err = String((e as Error).message ?? e);
    }
  }

  async function setSched(t: Todo, value: string) {
    scheduling = null;
    try {
      await call('todos.schedule', { item_id: t.item_id, sched: value || null });
      err = null;
    } catch (e) {
      err = String((e as Error).message ?? e);
    }
  }

  async function create(d: Draft) {
    try {
      await call('todos.create', d);
      creating = false;
      err = null;
      refresh();
    } catch (e) {
      err = String((e as Error).message ?? e);
    }
  }

  useKeys((e) => {
    if (creating || editing !== null || scheduling !== null) return;
    if (plain(e, 'e')) return (current && !isEpic(current) && (editing = current.item_id), true);
    if (plain(e, 'd')) return (current?.ext.type === 'local' && (scheduling = current.item_id), true);
    if (plain(e, 'j') || plain(e, 'ArrowDown')) return (step(1), true);
    if (plain(e, 'k') || plain(e, 'ArrowUp')) return (step(-1), true);
    if (plain(e, 'x') || plain(e, ' ')) return (toggle(current), true);
    if (plain(e, 'u')) return (current && struck.has(current.item_id) && toggle(current), true);
    if (plain(e, 'o')) return (current?.url && openExternal(current.url), true);
    if (plain(e, 'Enter')) return (expand(current), true);
    if (plain(e, 'Escape') && open !== null) return ((open = null), true);
    if (plain(e, 't')) return (current?.topic && openTopic(current.topic.id), true);
    if (plain(e, 'n')) return ((creating = true), true);
    if (plain(e, 's')) return ((sort = { key: SORTS[(SORTS.indexOf(sort.key) + 1) % SORTS.length], asc: true }), true);
    if (plain(e, '/')) return (filterEl?.focus(), true);
  });
</script>

<div class="view">
  <div class="main">
    <div class="bar">
      <FilterField
        class="todos-filter"
        bind:value={q}
        bind:el={filterEl}
        placeholder="Filter todos"
        onkeydown={(e) => e.key === 'Escape' && ((q = ''), filterEl?.blur())}
      >
        <kbd>/</kbd>
      </FilterField>
      <div class="chips">
        {#each SOURCES as s (s.key)}
          {#if s.key === 'all' || counts[s.key]}
            <button class="chip" class:on={source === s.key} onclick={() => (source = s.key)}
              >{s.label} <span class="tnum">{counts[s.key]}</span></button
            >
          {/if}
        {/each}
      </div>
      <span class="grow"></span>
      <button class="btn primary" onclick={() => (creating = true)}><i class="ph ph-plus"></i> New <kbd>n</kbd></button>
    </div>
    {#if err}<div class="err">{err}</div>{/if}
    {#if creating}
      <TodoForm {jira} choose onsave={create} oncancel={() => (creating = false)} />
    {/if}
    <div class="panel col">
      <div class="head row panel-head">
        <span></span>
        <button onclick={() => sortBy('title')}
          >Todo{#if sort.key === 'title'}<i class="ph ph-caret-{sort.asc ? 'up' : 'down'}"></i>{/if}</button
        >
        <button onclick={() => sortBy('status')}
          >Status{#if sort.key === 'status'}<i class="ph ph-caret-{sort.asc ? 'up' : 'down'}"></i>{/if}</button
        >
        <button onclick={() => sortBy('topic')}
          >Topic{#if sort.key === 'topic'}<i class="ph ph-caret-{sort.asc ? 'up' : 'down'}"></i>{/if}</button
        >
        <button onclick={() => sortBy('priority')}
          >Priority{#if sort.key === 'priority'}<i class="ph ph-caret-{sort.asc ? 'up' : 'down'}"></i>{/if}</button
        >
        <button onclick={() => sortBy('estimate')}
          >Est.{#if sort.key === 'estimate'}<i class="ph ph-caret-{sort.asc ? 'up' : 'down'}"></i>{/if}</button
        >
        <button onclick={() => sortBy('sched')}
          >Sched{#if sort.key === 'sched'}<i class="ph ph-caret-{sort.asc ? 'up' : 'down'}"></i>{/if}</button
        >
        <button onclick={() => sortBy('due')}
          >Due{#if sort.key === 'due'}<i class="ph ph-caret-{sort.asc ? 'up' : 'down'}"></i>{/if}</button
        >
      </div>
      <div class="body scroll">
        {#each rows as t (t.item_id)}
          {@const done = struck.has(t.item_id)}
          <div
            class="row todo"
            class:sel={t.item_id === sel}
            class:open={t.item_id === open}
            class:done
            style:--topic={t.topic ? catColor(t.topic.category) : null}
            data-todo={t.item_id}
            onclick={() => (sel = t.item_id)}
            ondblclick={() => t.url && openExternal(t.url)}
            role="presentation"
          >
            <button
              class="check"
              onclick={(e) => (e.stopPropagation(), toggle(t))}
              aria-label={done ? 'Reopen' : 'Done'}
            >
              <i class={done ? 'ph-fill ph-check-square' : 'ph ph-square'}></i>
            </button>
            <span class="title-cell">
              <AppIcon source={ICON[t.ext.type]} size={14} />
              <span class="title ellipsis" title={t.title}>{t.title}</span>
              {#if t.from && signalOf(t) !== null}
                <button
                  class="from link-nav ellipsis"
                  onclick={(e) => (e.stopPropagation(), expand(t))}
                  title="Show the signal (↵)">{t.from}</button
                >
              {:else if t.from}<span class="from ellipsis">{t.from}</span>{/if}
            </span>
            <span
              >{#if t.ext.type === 'jira'}<span class="chip status ellipsis" title={t.ext.status}>{t.ext.status}</span
                >{/if}</span
            >
            <span class="topic ellipsis">
              {#if t.topic}<span class="cbar" style:background={catColor(t.topic.category)}></span>{t.topic
                  .name}{:else}<span class="none">No topic</span>{/if}
            </span>
            <span class="muted tnum" title="Place in the Priorities tab's Priority column"
              >{t.rank ? `#${t.rank}` : ''}</span
            >
            {#if isEpic(t)}
              <span class="muted" title="Epics aren't estimated: you track them, the work is in their issues">-</span>
            {:else if editing === t.item_id}
              <input
                class="input sm est-pick"
                use:focusEl
                value={t.estimate ? dur(t.estimate.minutes) : ''}
                placeholder="45m"
                title="45m, 1h30m, 2h, 1d (8h) · empty lets the AI guess · ↵ save · esc"
                onblur={() => (editing = null)}
                onkeydown={(e) => {
                  if (e.key === 'Enter') setEstimate(t, e.currentTarget.value);
                  else if (e.key === 'Escape') editing = null;
                  else return;
                  e.preventDefault();
                  e.stopPropagation();
                }}
              />
            {:else}
              <button
                class="est"
                class:ai={t.estimate?.by === 'ai'}
                title={t.estimate?.by === 'ai' ? 'AI guess · e to change' : 'e to change'}
                onclick={(e) => (e.stopPropagation(), (editing = t.item_id))}
              >
                {t.estimate ? `${t.estimate.by === 'ai' ? '~' : ''}${dur(t.estimate.minutes)}` : '—'}
                {#if t.estimate && t.estimate.minutes > 120}<span class="split" title="Over 2h: worth breaking up?"
                    >split?</span
                  >{/if}
              </button>
            {/if}
            {#if scheduling === t.item_id}
              <input
                class="input sm sched-pick"
                type="date"
                use:focusEl
                value={t.sched ?? ''}
                title="↵ save · empty clears · esc"
                onblur={() => (scheduling = null)}
                onkeydown={(e) => {
                  if (e.key === 'Enter') setSched(t, e.currentTarget.value);
                  else if (e.key === 'Escape') scheduling = null;
                  else return;
                  e.preventDefault();
                  e.stopPropagation();
                }}
              />
            {:else if t.ext.type === 'local'}
              <button
                class="est tnum"
                title="Scheduled: the day you mean to do it · d to change"
                onclick={(e) => (e.stopPropagation(), (scheduling = t.item_id))}>{t.sched_label ?? '—'}</button
              >
            {:else}
              <span></span>
            {/if}
            <span class="tnum" class:overdue={t.overdue && !done}>{done ? 'Done' : (t.due_label ?? '')}</span>
          </div>
          {#if open === t.item_id && signalOf(t) !== null}
            <div class="signal" role="presentation" onclick={() => (sel = t.item_id)}>
              <SignalDetail itemId={signalOf(t)!}>
                {#snippet hints()}{#if t.topic}<kbd>t</kbd> open topic{' · '}{/if}<kbd>Esc</kbd> collapse{/snippet}
              </SignalDetail>
            </div>
          {/if}
        {:else}
          <div class="empty-pane">
            {#if todos.length}Nothing matches.{:else}No todos. Make one from a signal in Incoming (<kbd>n</kbd>), or
              here (<kbd>n</kbd>).{/if}
          </div>
        {/each}
      </div>
      <div class="hint-bar">
        <span><kbd>j</kbd> <kbd>k</kbd> todo</span>
        <span><kbd>x</kbd> done</span>
        <span><kbd>u</kbd> undo</span>
        <span><kbd>n</kbd> new todo</span>
        <span><kbd>/</kbd> filter</span>
        <span><kbd>e</kbd> estimate</span>
        {#if current?.ext.type === 'local'}<span><kbd>d</kbd> schedule</span>{/if}
        <span><kbd>s</kbd> sort</span>
        <span><kbd>o</kbd> open source</span>
        <span
          ><kbd>{open === sel ? 'Esc' : '↵'}</kbd>
          {open === sel ? 'collapse' : signalOf(current) !== null ? 'signal' : 'expand'}</span
        >
        {#if current?.topic}<span><kbd>t</kbd> topic</span>{/if}
      </div>
    </div>
  </div>
  <AssistantPane topic={current?.topic ?? null} />
</div>

<style>
  .view {
    flex: 1;
    min-width: 0;
    display: flex;
    line-height: 1.5;
  }
  .main {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    padding: 14px 22px;
  }
  .bar {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-bottom: 10px;
  }
  .bar :global(.todos-filter) {
    width: 260px;
  }
  .chips {
    display: flex;
    gap: 4px;
  }
  .grow {
    flex: 1;
  }
  .err {
    margin-bottom: 8px;
    font-size: 0.9615rem;
  }
  .panel {
    flex: 1;
    min-height: 0;
  }
  .body {
    flex: 1;
    min-height: 0;
  }
  .row {
    display: grid;
    grid-template-columns: 28px minmax(0, 1fr) 190px 220px 80px 96px 90px 90px;
    gap: 12px;
    align-items: center;
    padding: 7px 16px;
    font-size: 0.9615rem;
  }
  .head {
    border-bottom: 1px solid var(--border);
  }
  .head button {
    display: flex;
    align-items: center;
    gap: 4px;
    text-align: left;
    letter-spacing: inherit;
    text-transform: inherit;
  }
  .todo {
    padding-top: var(--row-pad-comfortable);
    min-height: var(--row-min-comfortable);
    padding-bottom: var(--row-pad-comfortable);
    box-shadow: inset 3px 0 0 var(--topic, transparent);
  }
  .todo:hover:not(.sel) {
    background: var(--row-hover);
  }
  .todo.sel {
    background: var(--aqua-tint);
  }
  .check {
    color: var(--tx-muted-2);
    font-size: 1.1538rem;
  }
  .done .check {
    color: var(--aqua);
  }
  .title-cell {
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .title {
    color: var(--tx-body);
  }
  .todo.open .title {
    white-space: normal;
  }
  /* Done: the cells fade (the row's fill and topic bar stay); no strikethrough. */
  .todo.done > * {
    opacity: 0.45;
  }
  .from {
    flex: none;
    max-width: 40%;
    font-size: 0.9231rem;
    color: var(--tx-label);
  }
  .from.link-nav {
    display: block;
  }
  .from.link-nav:hover {
    color: var(--tx-2);
    text-decoration: underline;
  }
  .signal {
    padding: 0 16px 10px 56px;
  }
  /* A chip that truncates: .ellipsis needs a block box, not .chip's inline-flex. */
  .status {
    display: inline-block;
    max-width: 100%;
    vertical-align: middle;
  }
  .topic {
    display: flex;
    align-items: center;
    gap: 6px;
    color: var(--tx-2);
  }
  .cbar {
    flex: none;
    width: 3px;
    height: 12px;
    border-radius: 2px;
  }
  .none,
  .muted {
    color: var(--tx-muted-2);
  }
  .est {
    text-align: left;
    color: var(--tx-2);
  }
  .est.ai {
    color: var(--tx-muted-2);
    font-style: italic;
  }
  .split {
    margin-left: 4px;
    font-size: 0.8077rem;
    font-style: normal;
    color: var(--tx-label);
  }
  .overdue {
    color: var(--time-now);
  }
  .empty-pane {
    display: block;
    padding: 24px 16px;
    color: var(--tx-muted-2);
    font: inherit;
    text-align: left;
  }
  /* Wraps (the shared bar clips): with a signal and a topic the hints can be wider
     than the column at the minimum window width. */
  .hint-bar {
    flex-wrap: wrap;
    gap: 4px 16px;
  }
</style>

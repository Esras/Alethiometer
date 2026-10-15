<script lang="ts">
  // Priorities (app/docs/priorities.md): open todos in three orders side by
  // side, Priority, Quick wins and Neglect. Every column holds every todo; a
  // filter removes a todo from all three. The cursor is one todo: aqua in the
  // focused column, the open-row grey in the others, with splines between the
  // columns (aqua for the cursor, dashed grey for the hovered todo) that pin to
  // a column's edge, hollow, when the todo is scrolled out of it.
  // j/k move in the focused column, [ ] move to the same todo in the next
  // column, / filters, ↵ opens the topic, o opens the todo in its app (local ones in Todos), x completes it.
  import { tick } from 'svelte';
  import { call, openExternal } from '../lib/api';
  import { catColor, openTodo, openTopic, ui } from '../lib/app.svelte';
  import { plain, useKeys } from '../lib/keys.svelte';
  import { live } from '../lib/live';
  import AppIcon from '../lib/ui/AppIcon.svelte';
  import FilterField from '../lib/ui/FilterField.svelte';
  import Pill from '../lib/ui/Pill.svelte';
  import { popover } from '../lib/ui/popover';
  import AssistantPane from '../shell/AssistantPane.svelte';
  import type { ColumnKey, PrioritiesData, PriorityRow } from './priorities/types';

  const ICON = { local: 'notes', jira: 'jira', obsidian: 'obsidian' } as const;
  const COLS: { key: ColumnKey; name: string }[] = [
    { key: 'priority', name: 'Priority' },
    { key: 'quick', name: 'Quick wins' },
    { key: 'neglect', name: 'Neglect' },
  ];
  /** Row pitch (24px row and its 1px line), header height with the panel's top border, gutter width. */
  const ROW = 25;
  const HEAD = 31;
  const GW = 56;

  type Dim = 'topic' | 'score' | 'date' | 'effort' | 'source' | 'factor';
  type Opt = { k: string; label: string; sq?: string } | { head: string };
  const FACTORS: [string, string][] = [
    ['due', 'Hard due date'],
    ['sched', 'Soft date'],
    ['meeting', 'Topic meeting soon'],
    ['waiting', 'Waiting on you'],
    ['blocks', 'Blocks others'],
    ['topic_priority', 'Topic priority'],
    ['who_asked', 'Someone asked'],
    ['breadth', 'Topic breadth'],
    ['estimate', 'Estimated'],
  ];
  const none = (): Record<Dim, string[]> => ({ topic: [], score: [], date: [], effort: [], source: [], factor: [] });

  let data = $state<PrioritiesData | null>(null);
  let picked = $state<Record<Dim, string[]>>(none());
  let text = $state('');
  let openDim = $state<Dim | null>(null);
  let topicQ = $state('');
  let focus = $state(0);
  let selId = $state<number | null>(null);
  let hover = $state<number | null>(null);
  let scroll = $state([0, 0, 0]);
  /** Each column's visible height; they're the same. */
  let heights = $state([0, 0, 0]);
  const viewH = $derived(heights[0]);
  let err = $state<string | null>(null);
  let filterEl = $state<HTMLInputElement | null>(null);
  const colEls: (HTMLElement | undefined)[] = $state([]);
  const chipEls: Partial<Record<Dim, HTMLElement>> = $state({});

  $effect(() => live<PrioritiesData>('priorities.list', {}, (d) => (data = d)));

  const all = $derived(data?.todos ?? []);
  const byId = $derived(new Map(all.map((t) => [t.item_id, t])));
  const quickMax = $derived(data?.quick_max ?? 30);

  const effortBand = (t: PriorityRow) =>
    !t.estimate ? null : t.estimate.minutes <= 15 ? 's' : t.estimate.minutes <= 60 ? 'm' : 'l';
  const keyOf: Record<Exclude<Dim, 'factor'>, (t: PriorityRow) => string | null> = {
    topic: (t) => (t.topic ? String(t.topic.id) : ''),
    score: (t) => t.band,
    date: (t) => t.when,
    effort: effortBand,
    source: (t) => t.source,
  };
  const matches = (t: PriorityRow, dim: Dim, keys: string[]) =>
    dim === 'factor' ? keys.some((k) => t.factors.includes(k)) : keys.includes(keyOf[dim](t) ?? '\u0000');
  const needle = $derived(text.trim().toLowerCase());
  const ok = (t: PriorityRow) =>
    (Object.keys(picked) as Dim[]).every((d) => !picked[d].length || matches(t, d, picked[d])) &&
    (!needle || `${t.title} ${t.topic?.name ?? ''}`.toLowerCase().includes(needle));
  const shown = $derived(all.filter(ok));
  const anyFilter = $derived((Object.keys(picked) as Dim[]).some((d) => picked[d].length > 0) || !!needle);

  const orders = $derived.by(() => {
    const ids = new Set(shown.map((t) => t.item_id));
    return COLS.map((c) => (data?.orders[c.key] ?? []).filter((id) => ids.has(id)).map((id) => byId.get(id)!));
  });
  const idx = $derived(orders.map((o) => new Map(o.map((t, i) => [t.item_id, i]))));
  const sel = $derived(selId !== null && idx[0].has(selId) ? selId : (orders[focus][0]?.item_id ?? null));
  const selT = $derived(sel === null ? null : (byId.get(sel) ?? null));

  // Light the selected todo's topic meetings on the day strip.
  $effect(() => {
    ui.topicId = selT?.topic?.id ?? null;
  });

  const faded = (c: number, t: PriorityRow) => c === 1 && !(t.estimate && t.estimate.minutes <= quickMax);

  // ---- filters

  const dims = $derived.by((): { key: Dim; name: string; opts: Opt[] }[] => {
    const topics = new Map<string, { label: string; sq: string; n: number }>();
    for (const t of all) {
      const k = t.topic ? String(t.topic.id) : '';
      const x = topics.get(k);
      if (x) x.n++;
      else
        topics.set(k, {
          label: t.topic?.name ?? 'No topic',
          sq: t.topic ? catColor(t.topic.category) : 'var(--tx-label)',
          n: 1,
        });
    }
    const topicOpts = [...topics.entries()]
      .sort((a, b) => b[1].n - a[1].n || a[1].label.localeCompare(b[1].label))
      .map(([k, v]) => ({ k, label: v.label, sq: v.sq }));
    return [
      { key: 'topic', name: 'Topic', opts: topicOpts },
      {
        key: 'score',
        name: 'Score',
        opts: [
          { k: 'hi', label: 'Top third' },
          { k: 'mid', label: 'Middle third' },
          { k: 'lo', label: 'Bottom third' },
        ],
      },
      {
        key: 'date',
        name: 'Date',
        opts: [
          { k: 'od', label: 'Overdue' },
          { k: 'today', label: 'Today' },
          { k: 'week', label: 'Next 7 days' },
          { k: 'later', label: 'Later' },
          { k: 'none', label: 'No date' },
        ],
      },
      {
        key: 'effort',
        name: 'Effort',
        opts: [
          { k: 's', label: '15 min or less' },
          { k: 'm', label: '16 min to 1 h' },
          { k: 'l', label: 'Over 1 h' },
        ],
      },
      {
        key: 'source',
        name: 'Source',
        opts: [
          { k: 'local', label: 'Local' },
          { k: 'jira', label: 'Jira' },
          { k: 'obsidian', label: 'Obsidian' },
        ],
      },
      {
        key: 'factor',
        name: 'Factor',
        opts: [
          { head: 'Factors' },
          ...FACTORS.map(([k, label]) => ({ k, label })),
          ...((data?.tags.length ?? 0) > 0
            ? [{ head: 'Tags' }, ...(data?.tags ?? []).map((t) => ({ k: `tag:${t}`, label: t }))]
            : []),
        ],
      },
    ];
  });

  function toggleOpt(dim: Dim, k: string) {
    const cur = picked[dim];
    picked = { ...picked, [dim]: cur.includes(k) ? cur.filter((x) => x !== k) : [...cur, k] };
  }

  function clearAll() {
    picked = none();
    text = '';
    openDim = null;
  }

  // ---- cursor and scrolling

  async function reveal(c: number, id: number | null, center: boolean) {
    await tick();
    const el = colEls[c];
    const i = id === null ? undefined : idx[c].get(id);
    if (!el || i === undefined) return;
    const top = i * ROW;
    const view = el.clientHeight;
    const out = top < el.scrollTop || top + ROW > el.scrollTop + view;
    if (center) {
      if (out) el.scrollTop = Math.max(0, top - (view - ROW) / 2);
    } else if (top < el.scrollTop) el.scrollTop = top;
    else if (top + ROW > el.scrollTop + view) el.scrollTop = top + ROW - view;
  }

  function step(d: number) {
    const o = orders[focus];
    if (!o.length) return;
    let i = o.findIndex((t) => t.item_id === sel);
    i = i < 0 ? 0 : Math.max(0, Math.min(o.length - 1, i + d));
    selId = o[i].item_id;
    reveal(focus, selId, false);
  }

  function toCol(c: number) {
    if (c < 0 || c >= COLS.length || sel === null) return;
    focus = c;
    reveal(c, sel, true);
  }

  function pick(c: number, t: PriorityRow) {
    selId = t.item_id;
    focus = c;
  }

  async function complete(t: PriorityRow | null) {
    if (!t) return;
    try {
      await call('todos.complete', { item_id: t.item_id });
      err = null;
    } catch (e) {
      err = String((e as Error).message ?? e);
    }
  }

  useKeys((e) => {
    if (plain(e, 'j') || plain(e, 'ArrowDown')) return (step(1), true);
    if (plain(e, 'k') || plain(e, 'ArrowUp')) return (step(-1), true);
    if (plain(e, ']')) return (toCol(focus + 1), true);
    if (plain(e, '[')) return (toCol(focus - 1), true);
    if (plain(e, '/')) return (filterEl?.focus(), true);
    if (plain(e, 'Enter')) return (selT?.topic && openTopic(selT.topic.id), true);
    if (plain(e, 'o')) return (selT && (selT.url ? openExternal(selT.url) : openTodo(selT.item_id)), true);
    if (plain(e, 'x')) return (complete(selT), true);
  });

  // ---- splines

  function yOf(c: number, id: number): number | null {
    const i = idx[c].get(id);
    if (i === undefined) return null;
    const y = HEAD + i * ROW + 12 - scroll[c];
    return Math.max(HEAD + 3, Math.min(HEAD + viewH - 3, y));
  }
  function where(c: number, id: number | null): 'above' | 'below' | 'in' | null {
    const i = id === null ? undefined : idx[c].get(id);
    if (i === undefined) return null;
    const top = i * ROW - scroll[c];
    return top < -12 ? 'above' : top + 13 > viewH ? 'below' : 'in';
  }
  const curve = (y1: number | null, y2: number | null) =>
    y1 === null || y2 === null ? '' : `M0 ${y1} C${GW / 2} ${y1} ${GW / 2} ${y2} ${GW} ${y2}`;

  // ---- display

  const est = (m: number) => (m < 60 ? `${m}m` : `${Math.floor(m / 60)}h${m % 60 ? m % 60 : ''}`);
  const metric = (c: number, t: PriorityRow) =>
    c === 0 ? t.score.toFixed(1) : c === 1 ? (t.estimate ? est(t.estimate.minutes) : '–') : `${t.age_days}d`;
  const dateText = (c: number, t: PriorityRow) =>
    c === 2 ? (t.quiet_days === 0 ? 'today' : `quiet ${t.quiet_days}d`) : (t.date?.label ?? '');
  function dateColor(c: number, t: PriorityRow) {
    if (c === 2 || !t.date) return 'var(--tx-label)';
    if (t.date.overdue) return 'var(--red)';
    if (t.date.kind === 'due') return t.when === 'today' ? 'var(--tx-2)' : 'var(--tx-muted-2)';
    return 'var(--tx-label)';
  }
  const SORTS = $derived([
    'score: every factor',
    `up to ${quickMax} min by value per hour, then by length`,
    'age and quiet time',
  ]);
  const groups = $derived(
    selT
      ? [
          { name: 'Value', val: selT.value.toFixed(1), pills: selT.reasons.value },
          { name: 'Pressure', val: selT.pressure.toFixed(1), pills: selT.reasons.pressure },
          { name: 'Effort', val: selT.estimate ? est(selT.estimate.minutes) : '–', pills: selT.reasons.effort },
        ]
      : [],
  );
</script>

<div class="view">
  <div class="main">
    <header class="head">
      <h1 class="page-title">Priorities</h1>
    </header>

    <section class="selected panel" aria-label="Selected todo">
      {#if selT}
        <div class="line">
          <span class="label lab">Selected</span>
          <AppIcon source={ICON[selT.source]} size={14} />
          <span class="sel-title ellipsis">{selT.title}</span>
          {#if selT.topic}<span class="sel-topic ellipsis">{selT.topic.name}</span>{/if}
          <span class="sel-date tnum" style:color={selT.date ? dateColor(0, selT) : 'var(--tx-label)'}
            >{selT.date?.label ?? 'no date'}</span
          >
          <span class="ranks">
            {#each COLS as col, c}
              <button class="btn" class:on={c === focus} onclick={() => toCol(c)}
                ><span class="tnum">#{(idx[c].get(selT.item_id) ?? 0) + 1}</span> {col.name}</button
              >
            {/each}
          </span>
        </div>
        <div class="line why">
          {#each groups as g, gi}
            <span class="group">
              <span class="gname">{g.name}</span>
              <span class="gval tnum">{g.val}</span>
              {#each g.pills as p}<Pill pill={p} />{/each}
            </span>
            {#if gi < groups.length - 1}<span class="sep"></span>{/if}
          {/each}
        </div>
      {:else}
        <div class="line">
          <span class="label lab">Selected</span>
          <span class="none">{all.length ? 'No todos match these filters.' : 'Nothing to prioritize.'}</span>
        </div>
      {/if}
    </section>

    <div class="filters">
      <FilterField
        sm
        class="pfilter"
        bind:value={text}
        bind:el={filterEl}
        placeholder="Filter todos"
        aria-label="Filter todos by text"
        onkeydown={(e) => e.key === 'Escape' && filterEl?.blur()}
      >
        <kbd>/</kbd>
      </FilterField>
      {#each dims as d (d.key)}
        <div class="dim">
          <button
            class="chip"
            class:on={picked[d.key].length > 0}
            bind:this={chipEls[d.key]}
            aria-expanded={openDim === d.key}
            onclick={() => ((openDim = openDim === d.key ? null : d.key), (topicQ = ''))}
          >
            <span>{d.name}</span>
            <span class="count on tnum n" style:visibility={picked[d.key].length ? 'visible' : 'hidden'}
              >{picked[d.key].length || 1}</span
            >
            <i class="ph ph-caret-down"></i>
          </button>
          {#if openDim === d.key}
            <div
              class="menu popover flush scroll"
              use:popover={{ onclose: () => (openDim = null), trigger: () => chipEls[d.key] }}
            >
              {#if d.key === 'topic'}
                <div class="find">
                  <FilterField sm bind:value={topicQ} placeholder="Find a topic" aria-label="Find a topic" />
                </div>
              {/if}
              {#each d.opts.filter((o) => !(d.key === 'topic' && 'k' in o && topicQ.trim() && !o.label
                      .toLowerCase()
                      .includes(topicQ.trim().toLowerCase()))) as o}
                {#if 'head' in o}
                  <div class="label mhead">{o.head}</div>
                {:else}
                  <button class="opt" aria-pressed={picked[d.key].includes(o.k)} onclick={() => toggleOpt(d.key, o.k)}>
                    <span class="check"
                      >{#if picked[d.key].includes(o.k)}<i class="ph ph-check"></i>{/if}</span
                    >
                    {#if o.sq}<span class="sq" style:background={o.sq}></span>{/if}
                    <span class="ellipsis olabel">{o.label}</span>
                    <span class="tnum on-n">{all.filter((t) => matches(t, d.key, [o.k])).length}</span>
                  </button>
                {/if}
              {/each}
            </div>
          {/if}
        </div>
      {/each}
      <span class="shown tnum">{shown.length === all.length ? '' : `${shown.length} of ${all.length}`}</span>
      <button class="btn" style:visibility={anyFilter ? 'visible' : 'hidden'} onclick={clearAll}>Clear</button>
    </div>

    <div class="cols">
      {#each COLS as col, c (col.key)}
        <section class="panel col column">
          <div class="colhead">
            <span class="panel-head" class:focused={c === focus}>{col.name}</span>
            <span class="count tnum" class:on={c === focus}>{orders[c].length}</span>
            <span class="sort ellipsis">{SORTS[c]}</span>
          </div>
          <div class="body">
            <div
              class="rows scroll"
              role="group"
              aria-label={col.name}
              bind:this={colEls[c]}
              bind:clientHeight={heights[c]}
              onscroll={(e) => (scroll[c] = e.currentTarget.scrollTop)}
              onmouseleave={() => (hover = null)}
            >
              {@render rows(c)}
            </div>
            {#if where(c, sel) === 'above'}
              <button class="count on tnum jump top" onclick={() => toCol(c)}
                >↑ #{(idx[c].get(sel!) ?? 0) + 1} selected</button
              >
            {:else if where(c, sel) === 'below'}
              <button class="count on tnum jump bottom" onclick={() => toCol(c)}
                >↓ #{(idx[c].get(sel!) ?? 0) + 1} selected</button
              >
            {/if}
          </div>
        </section>
        {#if c < COLS.length - 1}
          {@const y1 = sel === null ? null : yOf(c, sel)}
          {@const y2 = sel === null ? null : yOf(c + 1, sel)}
          <svg
            class="gutter"
            width={GW}
            height={HEAD + viewH + 1}
            viewBox="0 0 {GW} {HEAD + viewH + 1}"
            aria-hidden="true"
          >
            {#if hover !== null && hover !== sel}<path class="hover" d={curve(yOf(c, hover), yOf(c + 1, hover))}
              ></path>{/if}
            {#if y1 !== null && y2 !== null}
              <path class="cur" d={curve(y1, y2)}></path>
              <circle cx="2.5" cy={y1} r="2.5" class:hollow={where(c, sel) !== 'in'}></circle>
              <circle cx={GW - 2.5} cy={y2} r="2.5" class:hollow={where(c + 1, sel) !== 'in'}></circle>
            {/if}
          </svg>
        {/if}
      {/each}
    </div>

    <div class="keys">
      <span><kbd>j</kbd> <kbd>k</kbd> move</span>
      <span><kbd>[</kbd> <kbd>]</kbd> same todo, other column</span>
      <span><kbd>↵</kbd> open topic</span>
      <span><kbd>o</kbd> open in its app</span>
      <span><kbd>x</kbd> done</span>
      <span><kbd>/</kbd> filter</span>
      <span><kbd>Esc</kbd> close</span>
      {#if err}<span class="err">{err}</span>{/if}
    </div>
  </div>
  <AssistantPane topic={selT?.topic ?? null} />
</div>

{#snippet rows(c: number)}
  {#each orders[c] as t, i (t.item_id)}
    {@const isSel = t.item_id === sel}
    {@const isCur = isSel && c === focus}
    {@const next = orders[c][i + 1]}
    <button
      class="prow"
      class:cur={isCur}
      class:open={isSel && !isCur}
      class:hov={t.item_id === hover && !isSel}
      class:faded={faded(c, t) && !isCur}
      class:boundary={c === 1 && next && !faded(c, t) && faded(c, next)}
      onclick={() => pick(c, t)}
      onmouseenter={() => (hover = t.item_id)}
    >
      <span class="rank tnum" class:on={isSel}>{i + 1}</span>
      <span class="bar" style:background={t.topic ? catColor(t.topic.category) : 'var(--tx-label)'}></span>
      <AppIcon source={ICON[t.source]} size={14} />
      <span class="tt">
        <span class="title ellipsis">{t.title}</span>
        {#if t.topic}<span class="topic ellipsis">{t.topic.name}</span>{/if}
      </span>
      <span class="date ellipsis tnum" style:color={dateColor(c, t)}>{dateText(c, t)}</span>
      <span class="metric tnum">{metric(c, t)}</span>
    </button>
  {:else}
    <div class="empty-row">Nothing matches.</div>
  {/each}
{/snippet}

<style>
  .view {
    flex: 1;
    min-width: 0;
    display: flex;
  }
  .main {
    flex: 1;
    min-width: 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 14px 22px 12px;
  }
  .head {
    display: flex;
    align-items: center;
    min-height: 32px;
  }
  .head h1 {
    margin: 0;
  }

  /* Always 64px, so picking a todo never moves the page. */
  .selected {
    flex: none;
    height: 64px;
    box-sizing: border-box;
    display: flex;
    flex-direction: column;
    justify-content: center;
    gap: 7px;
    padding: 0 14px;
  }
  .line {
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 0;
  }
  .lab {
    flex: none;
    width: 64px;
  }
  .sel-title {
    min-width: 0;
    font-weight: 500;
    color: var(--tx-title);
  }
  .sel-topic {
    flex: none;
    max-width: 200px;
    font-size: 0.9231rem;
    color: var(--tx-label);
  }
  .sel-date {
    flex: none;
    font-size: 0.9231rem;
  }
  .none {
    font-size: 0.9231rem;
    color: var(--tx-label);
  }
  .ranks {
    margin-left: auto;
    display: flex;
    gap: 6px;
    flex: none;
  }
  .why {
    padding-left: 74px;
    overflow: hidden;
    white-space: nowrap;
  }
  .group {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    flex: none;
  }
  .gname {
    font-size: 0.8846rem;
    color: var(--tx-label);
  }
  .gval {
    font-size: 0.9231rem;
    color: var(--tx-2);
    margin-right: 2px;
  }
  .sep {
    width: 1px;
    height: 12px;
    flex: none;
    background: var(--border-strong);
  }

  .filters {
    flex: none;
    display: flex;
    align-items: center;
    gap: 6px;
    flex-wrap: wrap;
    min-height: 26px;
  }
  .filters :global(.pfilter) {
    width: 200px;
  }
  .dim {
    position: relative;
  }
  .dim .n {
    min-width: 18px;
    text-align: center;
    box-sizing: border-box;
    padding: 0 5px;
  }
  .dim .ph-caret-down {
    font-size: 10px;
  }
  .menu {
    position: absolute;
    top: calc(100% + 4px);
    left: 0;
    z-index: 20;
    width: 252px;
    max-height: 380px;
    border-radius: var(--radius-md);
  }
  .find {
    padding: 6px 8px 4px;
  }
  .mhead {
    padding: 8px 12px 2px;
  }
  .opt {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    min-height: 24px;
    padding: 2px 12px;
    box-sizing: border-box;
    text-align: left;
    font-size: 0.9231rem;
  }
  .opt:hover {
    background: var(--row-hover);
  }
  .check {
    width: 12px;
    flex: none;
    display: inline-flex;
    color: var(--aqua);
  }
  .sq {
    width: 8px;
    height: 8px;
    flex: none;
    border-radius: var(--radius-sm);
  }
  .olabel {
    flex: 1;
    min-width: 0;
    color: var(--tx-body);
  }
  .on-n {
    flex: none;
    font-size: 0.8462rem;
    color: var(--tx-label);
  }
  .shown {
    min-width: 56px;
    margin-left: 8px;
    font-size: 0.9231rem;
    color: var(--tx-label);
  }

  .cols {
    flex: 1;
    min-height: 0;
    display: flex;
    align-items: stretch;
  }
  .column {
    flex: 1 1 0;
    min-width: 280px;
  }
  .colhead {
    flex: none;
    display: flex;
    align-items: center;
    gap: 8px;
    height: 30px;
    padding: 0 12px;
    box-sizing: border-box;
    background: var(--bg-head);
  }
  .colhead .panel-head {
    color: var(--tx-label);
  }
  .colhead .panel-head.focused {
    color: var(--tx-title);
  }
  .sort {
    margin-left: auto;
    font-size: 0.8846rem;
    color: var(--tx-label);
  }
  .body {
    position: relative;
    flex: 1;
    min-height: 0;
    display: flex;
  }
  .rows {
    flex: 1;
    min-height: 0;
  }
  .jump {
    position: absolute;
    right: 14px;
    background: var(--bg-head);
    box-shadow:
      0 0 0 1px var(--aqua-line),
      var(--popover);
  }
  .jump.top {
    top: 5px;
  }
  .jump.bottom {
    bottom: 5px;
  }

  /* Compact rows (decision 14: Priorities is compact). */
  .prow {
    display: grid;
    grid-template-columns: 22px 3px 14px minmax(0, 1fr) 66px 34px;
    gap: 8px;
    align-items: center;
    width: 100%;
    height: 25px;
    box-sizing: border-box;
    padding: 0 10px 0 4px;
    text-align: left;
    line-height: 24px;
    border-bottom: 1px solid var(--border);
  }
  .prow.boundary {
    border-bottom: 1px dashed var(--tx-label);
  }
  .prow.hov {
    background: var(--hover-tint);
  }
  .prow.open {
    background: var(--bg-head);
  }
  .prow.cur {
    background: var(--aqua-tint);
  }
  .prow.faded {
    opacity: 0.42;
  }
  .rank {
    text-align: right;
    font-size: 0.8462rem;
    color: var(--tx-label);
  }
  .rank.on {
    color: var(--aqua);
  }
  .bar {
    width: 3px;
    height: 14px;
    border-radius: 2px;
  }
  .tt {
    display: flex;
    gap: 8px;
    align-items: baseline;
    min-width: 0;
  }
  .title {
    flex: 0 1 auto;
    min-width: 0;
    color: var(--tx-body);
  }
  .prow.cur .title {
    color: var(--tx-title);
  }
  .topic {
    flex: 0 1000 auto;
    min-width: 0;
    font-size: 0.9231rem;
    color: var(--tx-label);
  }
  .date {
    text-align: right;
    font-size: 0.8846rem;
  }
  .metric {
    text-align: right;
    font-size: 0.9231rem;
    color: var(--tx-muted-2);
  }

  .gutter {
    flex: none;
    display: block;
  }
  .gutter path {
    fill: none;
  }
  .gutter .hover {
    stroke: var(--tx-muted-2);
    stroke-width: 1;
    stroke-dasharray: 3 3;
  }
  .gutter .cur {
    stroke: var(--aqua);
    stroke-width: 1.5;
  }
  .gutter circle {
    fill: var(--aqua);
    stroke: var(--aqua);
    stroke-width: 1;
  }
  .gutter circle.hollow {
    fill: var(--bg-app);
  }

  .keys {
    flex: none;
    display: flex;
    gap: 16px;
    flex-wrap: wrap;
    font-size: 0.8846rem;
    color: var(--tx-label);
  }

  /* Narrow: the columns stack and the splines go. */
  @media (max-width: 900px) {
    .cols {
      flex-direction: column;
      gap: 12px;
      overflow-y: auto;
    }
    .column {
      flex: none;
      width: 100%;
      min-width: 0;
      height: 420px;
    }
    .gutter {
      display: none;
    }
  }
</style>

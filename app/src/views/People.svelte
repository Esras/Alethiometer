<script lang="ts" module>
  import type { SortKey, StagedMerge } from './people/types';
  type Picked = { id: number; name: string; category: string };
  const saved = {
    q: '',
    sort: 'last' as SortKey,
    reverse: false,
    picked: [] as Picked[],
    key: null as string | null,
    scroll: 0,
  };
</script>

<script lang="ts">
  // People index (design canvas "People and the graph": `PeopleSort`,
  // `PeopleTop3Filtered`; decisions "People tab design"). One row per People
  // note, then senders with no note. A row: the person, their own top three
  // topics (filled by length against the busiest box in the list), confirmed
  // signals in 30 days and a 12-week strip, last signal, next meeting.
  // s sorts (a picker; ⇧S reverses), t filters by topic (AND, up to 8): each
  // picked topic becomes a column in place of the top three. m merges into
  // another person (staged; written when you leave People, u undoes). l links
  // a sender with no note to a person. / filters by name, alias, role or
  // account. n makes a new person. ↵ does nothing until the person page exists.
  import { onDestroy, tick } from 'svelte';
  import { call } from '../lib/api';
  import { catColor, meetingPill, ui } from '../lib/app.svelte';
  import { plain, pointerMoved, useKeys } from '../lib/keys.svelte';
  import { live } from '../lib/live';
  import { accountLabel, initial, personColor, personTint, type PeopleRow, type Who } from '../lib/people';
  import { toast } from '../lib/toast.svelte';
  import { fillWidth, shade, topicBox } from '../lib/topicTone';
  import AppIcon from '../lib/ui/AppIcon.svelte';
  import Avatar from '../lib/ui/Avatar.svelte';
  import Dock from '../lib/ui/Dock.svelte';
  import DockHost from '../lib/ui/DockHost.svelte';
  import FilterField from '../lib/ui/FilterField.svelte';
  import { listNav, moveSel } from '../lib/ui/listNav';
  import { popover } from '../lib/ui/popover';
  import PersonName from '../lib/ui/PersonName.svelte';
  import PersonPicker from './incoming/PersonPicker.svelte';
  import TopicPicker from './incoming/TopicPicker.svelte';
  import MergeModal from './people/MergeModal.svelte';
  import MergePicker from './people/MergePicker.svelte';

  ui.topicId = null;

  const MAX_TOPICS = 8;
  const SORTS: { key: SortKey; label: string; hint: string }[] = [
    { key: 'last', label: 'Last signal', hint: 'most recent first' },
    { key: 'signals', label: 'Signals', hint: '30 days, most first' },
    { key: 'meeting', label: 'Next meeting', hint: 'soonest first' },
    { key: 'name', label: 'Name', hint: 'A to Z' },
  ];
  const GROUPS: { id: PeopleRow['group']; label: string }[] = [
    { id: 'recent', label: 'Recent · 30 days' },
    { id: 'quiet', label: 'Quiet' },
    { id: 'unknown', label: 'No People note' },
  ];

  let rows = $state<PeopleRow[]>([]);
  let loaded = $state(false);
  let q = $state(saved.q);
  let sort = $state<SortKey>(saved.sort);
  let reverse = $state(saved.reverse);
  let picked = $state<Picked[]>(saved.picked);
  let selKey = $state<string | null>(saved.key);
  let input = $state<HTMLInputElement>();
  let body = $state<HTMLDivElement>();
  let sortBtn = $state<HTMLButtonElement>();
  let sorting = $state(false);
  let sortSel = $state(0);
  let pickingTopic = $state(false);
  let naming = $state<string | null>(null);
  let nameInput = $state<HTMLInputElement>();
  let wantName = $state<string | null>(null);
  let linking = $state<PeopleRow | null>(null);
  let mergingFrom = $state<PeopleRow | null>(null);
  let mergeWith = $state<{ keep: PeopleRow; gone: PeopleRow } | null>(null);
  let staged = $state<StagedMerge[]>([]);
  let restored = false;

  $effect(() =>
    live<PeopleRow[]>('people.index', {}, (r) => {
      rows = r;
      loaded = true;
    }),
  );

  const matches = $derived.by(() => {
    const needle = q.trim().toLowerCase();
    return rows.filter(
      (r) =>
        (!needle ||
          r.name.toLowerCase().includes(needle) ||
          (r.role ?? '').toLowerCase().includes(needle) ||
          r.terms.some((t) => t.includes(needle))) &&
        picked.every((p) => r.topics.some((t) => t.id === p.id)),
    );
  });

  /** A sort's own order, before ⇧S; a row without the value goes last either way. */
  function cmp(a: PeopleRow, b: PeopleRow): number {
    const dir = reverse ? -1 : 1;
    const byName = a.name.localeCompare(b.name, undefined, { sensitivity: 'base' });
    const nulls = <T,>(x: T | null, y: T | null, f: (x: T, y: T) => number) =>
      x === null && y === null ? 0 : x === null ? 1 : y === null ? -1 : dir * f(x, y);
    switch (sort) {
      case 'last':
        return nulls(a.last_at, b.last_at, (x, y) => y.localeCompare(x)) || byName;
      case 'signals':
        return dir * (b.total - a.total) || nulls(a.last_at, b.last_at, (x, y) => y.localeCompare(x)) || byName;
      case 'meeting':
        return (
          nulls(a.next_meeting?.start ?? null, b.next_meeting?.start ?? null, (x, y) => x.localeCompare(y)) || byName
        );
      case 'name':
        return dir * byName;
    }
  }

  const groups = $derived(
    GROUPS.map((g) => ({ ...g, rows: matches.filter((r) => r.group === g.id).sort(cmp) })).filter((g) => g.rows.length),
  );
  const flat = $derived(groups.flatMap((g) => g.rows));
  const sel = $derived(
    Math.max(
      0,
      flat.findIndex((r) => r.key === selKey),
    ),
  );
  const current = $derived(flat[sel] ?? null);
  const filtered = $derived(!!q.trim() || picked.length > 0);

  // The top three: their own busiest topics in 30 days.
  const top = (r: PeopleRow) => r.topics.filter((t) => t.n > 0).slice(0, 3);
  // Boxes fill against the busiest box in the list; the strip against the busiest week.
  const boxMax = $derived(Math.max(0, ...flat.flatMap((r) => top(r).map((t) => t.n))));
  const weekMax = $derived(Math.max(0, ...flat.flatMap((r) => r.weeks)));
  const SHADE = [0, 0.25, 0.45, 0.7, 1];

  const sortLabel = $derived(SORTS.find((s) => s.key === sort)?.label ?? '');
  const arrow = (key: SortKey) => ((key === 'meeting' || key === 'name' ? !reverse : reverse) ? '↑' : '↓');

  function cellTint(color: string, n: number): { bg: string; fg: string } {
    if (n <= 0) return { bg: 'transparent', fg: 'var(--tx-disabled)' };
    const a = n >= 15 ? 42 : n >= 8 ? 28 : n >= 4 ? 18 : 10;
    return { bg: `color-mix(in srgb, ${color} ${a}%, transparent)`, fg: n >= 4 ? 'var(--tx-title)' : 'var(--tx-2)' };
  }

  const stagedFor = (r: PeopleRow) => (r.id === null ? undefined : staged.find((m) => m.gone === r.id));

  $effect(() => {
    void q;
    void picked.length;
    if (restored) selKey = null;
  });

  $effect(() => {
    if (restored || !loaded) return;
    restored = true;
    tick().then(() => body && (body.scrollTop = saved.scroll));
  });

  // A person just made with n: the cursor goes to them once they're listed.
  $effect(() => {
    if (wantName === null) return;
    const r = rows.find((x) => x.known && x.name === wantName);
    if (r) {
      selKey = r.key;
      wantName = null;
      tick().then(() => scrollTo(r.key));
    }
  });

  $effect(() => {
    if (naming !== null) nameInput?.focus();
  });

  onDestroy(() => {
    Object.assign(saved, { q, sort, reverse, picked, key: current?.key ?? null });
    void commit(staged);
  });

  function scrollTo(key: string | undefined) {
    if (key) document.querySelector(`[data-person="${CSS.escape(key)}"]`)?.scrollIntoView({ block: 'nearest' });
  }

  function move(d: number) {
    const next = flat[moveSel(sel, d, flat.length)];
    selKey = next?.key ?? null;
    scrollTo(next?.key);
  }

  function openSort() {
    sortSel = Math.max(
      0,
      SORTS.findIndex((s) => s.key === sort),
    );
    sorting = true;
  }

  function pickSort(key: SortKey) {
    if (key !== sort) reverse = false;
    sort = key;
    sorting = false;
  }

  function addTopic(id: number, name: string, category: string) {
    pickingTopic = false;
    if (picked.length < MAX_TOPICS && !picked.some((p) => p.id === id)) picked = [...picked, { id, name, category }];
  }

  async function create() {
    const name = (naming ?? '').trim();
    if (!name) return;
    try {
      const r = await call<{ path: string }>('people.create', { name });
      naming = null;
      wantName = r.path.replace(/^People\//, '').replace(/\.md$/, '');
    } catch (e) {
      toast(e);
    }
  }

  function stage(m: StagedMerge) {
    mergeWith = null;
    staged = [...staged.filter((x) => x.gone !== m.gone), m];
  }

  function undo(r: PeopleRow | null) {
    const m = (r && stagedFor(r)) ?? staged.at(-1);
    if (m) staged = staged.filter((x) => x !== m);
  }

  /** Leaving People: write every staged merge, in the order they were made. */
  async function commit(list: StagedMerge[]) {
    for (const m of list) {
      try {
        await call('people.merge', {
          keep: m.keep,
          gone: m.gone,
          take: m.take,
          keep_text: m.keep_text,
          gone_text: m.gone_text,
        });
      } catch (e) {
        toast(`Merging ${m.goneName} into ${m.keepName} failed: ${String((e as Error).message ?? e)}`);
      }
    }
  }

  function who(r: PeopleRow): Who {
    return { color: r.color, known: r.known, me: false, account: r.account ?? undefined };
  }

  useKeys((e) => {
    if (pickingTopic || linking || mergingFrom || mergeWith) return;
    if (sorting) {
      const d = listNav(e, { bare: true });
      if (d) return ((sortSel = moveSel(sortSel, d, SORTS.length)), true);
      if (plain(e, 'Enter')) return (pickSort(SORTS[sortSel].key), true);
      if (plain(e, 'S')) return ((reverse = !reverse), (sorting = false), true);
      if (plain(e, 's')) return ((sorting = false), true);
      return;
    }
    if (naming !== null && e.target === nameInput) {
      if (e.key === 'Enter') return (void create(), true);
      if (e.key === 'Escape') return ((naming = null), true);
      return;
    }
    if (e.target === input) {
      const d = listNav(e);
      if (d) return (move(d), true);
      if (e.key === 'Escape' || e.key === 'Enter') return (input?.blur(), true);
      return;
    }
    const d = listNav(e, { bare: true });
    if (d) return (move(d), true);
    if (plain(e, 's')) return (openSort(), true);
    if (plain(e, 'S')) return ((reverse = !reverse), true);
    if (plain(e, 't')) return (picked.length < MAX_TOPICS && (pickingTopic = true), true);
    if (plain(e, 'Backspace')) return (picked.length && (picked = picked.slice(0, -1)), true);
    if (plain(e, 'm')) return (current?.known && !stagedFor(current) && (mergingFrom = current), true);
    if (plain(e, 'l')) return (current && !current.known && current.account && (linking = current), true);
    if (plain(e, 'u')) return (undo(current), true);
    if (plain(e, 'n')) return ((naming = ''), true);
    if (plain(e, '/')) return (input?.focus(), true);
  });
</script>

{#snippet personRow(r: PeopleRow)}
  {@const i = flat.indexOf(r)}
  {@const merge = stagedFor(r)}
  <div
    class="prow"
    class:sel={i === sel}
    class:merged={!!merge}
    data-person={r.key}
    onclick={() => (selKey = r.key)}
    onmousemove={(e) => pointerMoved(e) && (selKey = r.key)}
    role="presentation"
  >
    <div class="who" class:unknown={!r.known}>
      {#if r.known}
        <Avatar initials={initial(r.name)} color={personColor(r.color)} size="sm" />
        <span class="name ellipsis" style:color={personColor(r.color)} title={r.name}>{r.name}</span>
      {:else}
        <PersonName who={who(r)} name={r.name} onlink={() => ((selKey = r.key), (linking = r))} />
      {/if}
      {#if merge}
        <span class="merging">merged into {merge.keepName} · <kbd>u</kbd> undo</span>
      {:else if r.role}<span class="role ellipsis" title={r.role}>{r.role}</span>{/if}
    </div>
    {#if picked.length}
      <div class="cols8">
        {#each picked as p (p.id)}
          {@const n = r.topics.find((t) => t.id === p.id)?.n ?? 0}
          {@const c = cellTint(catColor(p.category), n)}
          <span class="cell tnum" style:background={c.bg} style:color={c.fg}>{n}</span>
        {/each}
      </div>
    {:else}
      <div class="cols3">
        {#each top(r) as t (t.id)}
          {@const box = topicBox(catColor(t.category))}
          <span class="slot" style:background={box.bg} title="{t.name} · {t.n} in 30 days"
            ><span class="bf" style:width="{fillWidth(t.n, boxMax)}%" style:background={box.fill}></span><span
              class="tn ellipsis">{t.name}</span
            ><span class="tc tnum">{t.n}</span></span
          >
        {/each}
      </div>
    {/if}
    <span class="sig">
      <span class="tot tnum">{r.total || '—'}</span>
      <span class="wk" aria-label="Signals per week, 12 weeks">
        {#each r.weeks as n, w (w)}
          {@const s = shade(n, weekMax)}
          <span
            style:background={s ? personTint(r.color, SHADE[s]) : 'var(--bg-head)'}
            title="{n} signal{n === 1 ? '' : 's'}"
          ></span>
        {/each}
      </span>
    </span>
    <span class="last">
      {#if r.last}
        {#if r.last_source}<AppIcon source={r.last_source} size={14} />{/if}<span class="ellipsis">{r.last}</span>
      {:else}<span class="none">—</span>{/if}
    </span>
    <span class="meet">
      {#if r.next_meeting}
        {@const p = meetingPill(r.next_meeting.start)}
        <span class="pill {p.weight} tnum">{p.label}</span><span class="ellipsis" title={r.next_meeting.title}
          >{r.next_meeting.title}</span
        >
      {:else}<span class="none">—</span>{/if}
    </span>
  </div>
{/snippet}

<div class="view">
  <div class="panel col">
    <div class="head" class:joined={picked.length > 0}>
      <span class="title">People</span>
      <FilterField
        class="people-filter"
        icon="funnel-simple"
        bind:value={q}
        bind:el={input}
        placeholder="Filter people"
      >
        <span class="total tnum">{filtered ? `${flat.length} of ${rows.length}` : rows.length} people</span>
      </FilterField>
      <span class="sort-wrap">
        <button
          class="btn md"
          class:on={sorting}
          bind:this={sortBtn}
          onclick={() => (sorting ? (sorting = false) : openSort())}
          >Sort: {sortLabel}<i class="ph ph-caret-down"></i><kbd>s</kbd></button
        >
        {#if sorting}
          <div
            class="sort-pop popover flush"
            role="listbox"
            aria-label="Sort people by"
            use:popover={{ onclose: () => (sorting = false), trigger: sortBtn }}
          >
            <div class="pop-head label">Sort by</div>
            {#each SORTS as s, i (s.key)}
              <button
                class="opt"
                class:on={i === sortSel}
                role="option"
                aria-selected={s.key === sort}
                onmousemove={(e) => pointerMoved(e) && (sortSel = i)}
                onclick={() => pickSort(s.key)}
                ><span class="check">{s.key === sort ? '✓' : ''}</span><span>{s.label}</span><span class="d"
                  >{s.hint}</span
                ></button
              >
            {/each}
            <div class="pop-foot">
              <span><kbd>↵</kbd> sort</span><span><kbd>⇧S</kbd> reverse</span><span><kbd>Esc</kbd></span>
            </div>
          </div>
        {/if}
      </span>
      <button
        class="btn md topic-btn"
        class:on={pickingTopic}
        onclick={() => (pickingTopic = picked.length < MAX_TOPICS)}>Topic…<kbd>t</kbd></button
      >
      <button class="btn md primary" onclick={() => (naming = '')}
        ><i class="ph ph-plus"></i>New person<kbd>n</kbd></button
      >
    </div>
    {#if picked.length}
      <div class="chips">
        <span class="label in">In</span>
        {#each picked as p, i (p.id)}
          {#if i > 0}<span class="and">and</span>{/if}
          <span class="fc"
            ><span class="sq" style:background={catColor(p.category)}></span>{p.name}<button
              class="x"
              aria-label="Remove the {p.name} filter"
              onclick={() => (picked = picked.filter((x) => x.id !== p.id))}>×</button
            ></span
          >
        {/each}
        <span class="grow"></span>
        <span class="hint"><kbd>t</kbd> add a topic (AND), up to {MAX_TOPICS} columns</span>
        <span class="hint"><kbd>⌫</kbd> remove the last</span>
      </div>
    {/if}
    {#if naming !== null}
      <FilterField
        class="naming"
        icon="plus"
        bind:value={naming}
        bind:el={nameInput}
        placeholder="New person's name"
        onblur={() => !naming?.trim() && (naming = null)}
      >
        <span class="hint">People/{naming.trim() || '…'}.md · <kbd>↵</kbd> create · <kbd>Esc</kbd></span>
      </FilterField>
    {/if}
    <div class="prow cols-head" class:tall={picked.length > 0}>
      <span class="label" class:on={sort === 'name'}>Person{sort === 'name' ? ` ${arrow('name')}` : ''}</span>
      {#if picked.length}
        <div class="cols8">
          {#each picked as p (p.id)}
            <span class="th"
              ><span class="cb" style:background={catColor(p.category)}></span><span class="thn">{p.name}</span></span
            >
          {/each}
        </div>
      {:else}
        <span class="label">Top 3 topics</span>
      {/if}
      <span class="label" class:on={sort === 'signals'}
        >Signals{sort === 'signals' ? ` ${arrow('signals')}` : ''} · 12 weeks</span
      >
      <span class="label" class:on={sort === 'last'}>Last signal{sort === 'last' ? ` ${arrow('last')}` : ''}</span>
      <span class="label" class:on={sort === 'meeting'}
        >Next meeting{sort === 'meeting' ? ` ${arrow('meeting')}` : ''}</span
      >
    </div>
    <div class="body scroll" bind:this={body} onscroll={() => body && (saved.scroll = body.scrollTop)}>
      {#each groups as g (g.id)}
        <div class="section label">{g.label} <span class="n">{g.rows.length}</span></div>
        {#each g.rows as r (r.key)}{@render personRow(r)}{/each}
      {/each}
      {#if loaded && !flat.length}
        <div class="empty-pane">
          {rows.length ? 'No one matches.' : 'No people yet. A person is a note in People/ in your vault.'}
        </div>
      {/if}
    </div>
    {#if pickingTopic}
      <Dock under=".topic-btn">
        <TopicPicker
          create={false}
          excludeIds={picked.map((p) => p.id)}
          placeholder="Filter by topic…"
          onpick={addTopic}
          onclose={() => (pickingTopic = false)}
        />
      </Dock>
    {/if}
    {#if linking && linking.account}
      <Dock under=".prow.sel">
        <PersonPicker
          account={linking.account}
          name={linking.name}
          source={accountLabel(linking.account.kind)}
          ondone={() => (linking = null)}
          onclose={() => (linking = null)}
        />
      </Dock>
    {/if}
    {#if mergingFrom}
      <Dock under=".prow.sel">
        <MergePicker
          from={mergingFrom}
          people={rows}
          exclude={staged.map((m) => m.gone)}
          onpick={(into) => mergingFrom && ((mergeWith = { keep: into, gone: mergingFrom }), (mergingFrom = null))}
          onclose={() => (mergingFrom = null)}
        />
      </Dock>
    {/if}
    {#if mergeWith}
      <MergeModal keep={mergeWith.keep} gone={mergeWith.gone} onstage={stage} onclose={() => (mergeWith = null)} />
    {/if}
    <DockHost left={20} />
    <div class="hint-bar">
      <span><kbd>j</kbd> <kbd>k</kbd> move</span><span><kbd>s</kbd> sort by…</span><span
        ><kbd>t</kbd> filter by topic, it becomes a column</span
      ><span><kbd>m</kbd> merge into…</span><span><kbd>l</kbd> link a sender to a person</span><span
        ><kbd>/</kbd> filter</span
      ><span><kbd>n</kbd> new person</span>{#if staged.length}<span><kbd>u</kbd> undo merge</span>{/if}
      <span class="note"><kbd>↵</kbd> opens the person page once it's built</span>
    </div>
  </div>
</div>

<style>
  .view {
    flex: 1;
    min-width: 0;
    display: flex;
    padding: 18px 22px;
  }
  .panel {
    position: relative;
    flex: 1;
    min-width: 0;
    min-height: 0;
  }
  .head {
    display: flex;
    gap: 12px;
    align-items: center;
    padding: 12px 20px;
    border-bottom: 1px solid var(--border);
  }
  .head.joined {
    border-bottom: 0;
  }
  .title {
    font-size: 1.4615rem;
    font-weight: 500;
    color: var(--tx-title);
  }
  .head :global(.people-filter) {
    flex: 1;
  }
  .total {
    font-size: 0.9231rem;
    white-space: nowrap;
  }
  .sort-wrap {
    position: relative;
  }
  .sort-pop {
    position: absolute;
    z-index: 30;
    top: calc(100% + 6px);
    left: 0;
    width: 300px;
    border-radius: var(--radius-md);
    overflow: hidden;
  }
  .pop-head {
    padding: 8px 12px 4px;
  }
  .opt {
    width: 100%;
    display: grid;
    grid-template-columns: 16px minmax(0, 1fr) auto;
    gap: 8px;
    align-items: center;
    min-height: var(--row-min-cozy);
    padding: 0 12px 0 10px;
    text-align: left;
    font-size: 0.9615rem;
    color: var(--tx-2);
  }
  .opt.on {
    background: var(--aqua-tint);
    color: var(--tx-title);
  }
  .check {
    color: var(--aqua);
  }
  .d {
    font-size: 0.8846rem;
    color: var(--tx-label);
    white-space: nowrap;
  }
  .pop-foot {
    display: flex;
    gap: 14px;
    padding: 8px 12px;
    margin-top: 4px;
    border-top: 1px solid var(--border);
    font-size: 0.8846rem;
    color: var(--tx-label);
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
    padding: 0 20px 12px;
    border-bottom: 1px solid var(--border);
  }
  .in {
    margin-right: 4px;
  }
  .and,
  .hint {
    font-size: 0.9231rem;
    color: var(--tx-label);
  }
  .grow {
    flex: 1;
  }
  .fc {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 24px;
    padding: 0 4px 0 10px;
    border: 1px solid var(--aqua-line);
    border-radius: var(--radius-full);
    background: var(--aqua-tint);
    color: var(--tx-body);
    font-size: 0.9231rem;
    white-space: nowrap;
  }
  .sq {
    width: 9px;
    height: 9px;
    border-radius: var(--radius-sm);
  }
  .x {
    width: 18px;
    height: 18px;
    color: var(--tx-label);
  }
  .panel :global(.naming) {
    width: auto;
    margin: 10px 20px 0;
  }
  .prow {
    display: grid;
    grid-template-columns: minmax(200px, 1fr) 660px 196px 130px 220px;
    align-items: center;
    min-height: var(--row-min-comfortable);
    padding: 0 20px;
    border-bottom: 1px solid var(--border);
  }
  .cols-head {
    min-height: 34px;
  }
  .cols-head.tall {
    min-height: 64px;
    align-items: stretch;
  }
  .cols-head.tall > .label {
    align-self: end;
    padding-bottom: 10px;
  }
  .cols-head .label.on {
    color: var(--aqua);
  }
  .section {
    padding: 14px 20px 4px;
  }
  .body {
    flex: 1;
    min-height: 0;
  }
  .who {
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 0;
    padding-right: 12px;
  }
  .who.unknown {
    padding-left: 28px;
  }
  .name {
    font-weight: 500;
    font-size: 1.0385rem;
    white-space: nowrap;
  }
  .role,
  .merging {
    font-size: 0.9231rem;
    color: var(--tx-label);
  }
  .prow.merged {
    background: var(--hatch);
  }
  .prow.merged .name {
    text-decoration: line-through;
    text-decoration-color: var(--tx-disabled);
  }
  .cols3 {
    display: grid;
    grid-template-columns: repeat(3, 220px);
  }
  .cols8 {
    display: grid;
    grid-template-columns: repeat(8, 82px);
    align-self: stretch;
    align-items: center;
  }
  .slot {
    position: relative;
    overflow: hidden;
    display: flex;
    align-items: center;
    gap: 8px;
    height: 26px;
    margin-right: 6px;
    padding: 0 8px;
    border-radius: var(--radius-sm);
    font-size: 0.9615rem;
    color: var(--tx-body);
    min-width: 0;
  }
  .bf {
    position: absolute;
    left: 0;
    top: 0;
    bottom: 0;
  }
  .tn,
  .tc {
    position: relative;
  }
  .tn {
    flex: 1;
    min-width: 0;
  }
  .tc {
    font-size: 0.9231rem;
    font-weight: 500;
    color: var(--tx-title);
  }
  .th {
    position: relative;
    align-self: stretch;
    display: flex;
    flex-direction: column;
    justify-content: flex-end;
    padding: 8px 8px 8px 0;
  }
  .cb {
    position: absolute;
    left: 0;
    right: 8px;
    top: 0;
    height: 3px;
    border-radius: 0 0 var(--radius-sm) var(--radius-sm);
  }
  .thn {
    font-size: 0.9231rem;
    line-height: 15px;
    color: var(--tx-2);
  }
  .cell {
    width: 52px;
    height: 24px;
    border-radius: var(--radius-sm);
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 0.9231rem;
  }
  .sig {
    display: flex;
    align-items: center;
    gap: 12px;
  }
  .tot {
    width: 28px;
    text-align: right;
    font-size: 0.9615rem;
    color: var(--tx-2);
  }
  .wk {
    display: flex;
    gap: 2px;
  }
  .wk span {
    width: 10px;
    height: 16px;
    border-radius: var(--radius-sm);
  }
  .last,
  .meet {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
    font-size: 0.9615rem;
    color: var(--tx-muted);
    white-space: nowrap;
  }
  .meet {
    color: var(--tx-2);
  }
  .none {
    color: var(--tx-disabled);
  }
  .empty-pane {
    display: block;
    padding: 16px 20px;
    font: inherit;
    text-align: left;
  }
  .hint-bar {
    padding: 8px 20px;
  }
  .note {
    margin-left: auto;
    color: var(--tx-muted);
  }
</style>

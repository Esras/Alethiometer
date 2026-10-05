<script lang="ts">
  // Topics (7a): the root object. Left column: Upcoming (only when something is
  // soon), Open, Chronology, People. Right: the topic's Obsidian note. The two
  // columns scroll independently; z folds the left column. e edits the note in
  // place (topics/NotePane), o opens it in Obsidian.
  import { call } from '../lib/api';
  import { catColor, category, refresh, setTab, TABS, ui } from '../lib/app.svelte';
  import { plain, useKeys } from '../lib/keys.svelte';
  import { live } from '../lib/live';
  import { cancelMerge, commitMerges, mergingAway, merges, pendingMerge, stageMerge } from '../lib/merges.svelte';
  import { untrack } from 'svelte';
  import TopicPicker from './incoming/TopicPicker.svelte';
  import { SOURCES, sourceLabel } from '../lib/sources';
  import { personColor } from '../lib/people';
  import type { TopicDetail, TopicPerson, TopicRow } from '../lib/types';
  import AppIcon from '../lib/ui/AppIcon.svelte';
  import Avatar from '../lib/ui/Avatar.svelte';
  import Dock from '../lib/ui/Dock.svelte';
  import DockHost from '../lib/ui/DockHost.svelte';
  import Panel from '../lib/ui/Panel.svelte';
  import Pill from '../lib/ui/Pill.svelte';
  import AssistantPane from '../shell/AssistantPane.svelte';
  import NotePane from './topics/NotePane.svelte';
  import TopicsIndex from './topics/TopicsIndex.svelte';
  import CategoryPicker from './topics/CategoryPicker.svelte';
  import Chronology from './topics/Chronology.svelte';
  import PersonPicker from './incoming/PersonPicker.svelte';

  let topics = $state<TopicRow[]>([]);
  let d = $state<TopicDetail | null>(null);
  let filter = $state<string | null>(null);
  let open = $state({ up: true, open: true, chron: true, people: true });
  let picking = $state(false);
  // Link to person (17b) for someone in the People panel without a note.
  let linking = $state<TopicPerson | null>(null);
  // Picking the topic to merge this one into (m).
  let merging = $state(false);
  // Renaming the open topic (r); the draft name, or null when not renaming.
  let renaming = $state<string | null>(null);
  let renameInput = $state<HTMLInputElement>();
  let renameErr = $state('');
  $effect(() => {
    if (renaming !== null) renameInput?.focus();
  });

  // Staged merges are written when you leave the screen: the index for a
  // topic, one topic for another, or the Topics tab.
  $effect(() => {
    void ui.openTopicId;
    return () => untrack(commitMerges);
  });

  function mergeInto(into: number, intoName: string) {
    merging = false;
    if (d) stageMerge({ from: d.topic.id, fromName: d.topic.name, into, intoName });
  }

  async function setCategory(key: string) {
    picking = false;
    if (!d) return;
    await call('topics.set_category', { id: d.topic.id, category: key });
    refresh();
  }

  $effect(() => live<TopicRow[]>('topics.list', { archived: false }, (t) => (topics = t)));

  // A live query that also reads the note file: re-run when the topic's
  // signals change or the vault does.
  $effect(() => {
    const id = ui.openTopicId;
    if (id === null) return;
    ui.topicId = id;
    return live<TopicDetail>('topics.get', { id }, (r) => {
      if (ui.openTopicId === id) d = r;
    });
  });

  const allFolded = $derived(!open.up && !open.open && !open.chron && !open.people);
  function fold() {
    const v = allFolded;
    open = { up: v, open: v, chron: v, people: v };
  }

  function step(delta: number) {
    const i = topics.findIndex((t) => t.id === ui.openTopicId);
    const next = topics[(i + delta + topics.length) % topics.length];
    if (next) {
      filter = null;
      ui.openTopicId = next.id;
    }
  }

  function cycleFilter() {
    if (!d) return;
    const keys: (string | null)[] = [null, ...d.filters.map((f) => f.source)];
    filter = keys[(keys.indexOf(filter) + 1) % keys.length];
  }

  // e (edit here) and o (open in Obsidian) live in the note pane.
  function toIndex() {
    ui.openTopicId = null;
    ui.openedFrom = null;
    d = null;
  }

  function back() {
    const from = ui.openedFrom;
    toIndex();
    if (from && from !== 'topics') setTab(from);
  }

  async function commitRename(name: string | null) {
    const trimmed = name?.trim() ?? '';
    renaming = null;
    if (!d || !trimmed || trimmed === d.topic.name) return;
    try {
      await call('topics.rename', { id: d.topic.id, name: trimmed });
      renameErr = '';
      refresh();
    } catch (e) {
      renameErr = String((e as Error).message ?? e);
    }
  }

  useKeys((e) => {
    if (ui.openTopicId === null || linking || merging) return;
    if (renaming !== null && e.target === renameInput) {
      if (e.key === 'Enter') return void commitRename(renaming), true;
      if (e.key === 'Escape') return (renaming = null), true;
      return;
    }
    if (plain(e, 'Escape')) return toIndex(), true;
    if (plain(e, 'c')) return (picking = true), true;
    if (plain(e, 'z')) return fold(), true;
    if (plain(e, '[')) return step(-1), true;
    if (plain(e, ']')) return step(1), true;
    if (plain(e, 'f')) return cycleFilter(), true;
    if (plain(e, 'm')) return (merging = true), true;
    if (plain(e, 'r')) return d && (renaming = d.topic.name), true;
    if (plain(e, 'u') && d && pendingMerge(d.topic.id)) return cancelMerge(d.topic.id), true;
  });

  const entries = $derived(d ? d.chronology.filter((e) => filter === null || e.source === filter) : []);
  const origin = $derived(ui.openedFrom === 'search' ? 'Search' : (TABS.find((t) => t.id === ui.openedFrom)?.label ?? 'Topics'));
</script>

<div class="view">
  {#if ui.openTopicId === null}
    <TopicsIndex />
    <AssistantPane scope="topics" />
  {:else if d}
    {@const cat = catColor(d.topic.category)}
    <div class="content">
      <div class="header" style:background="linear-gradient(90deg, color-mix(in srgb, {cat} 10%, transparent), transparent 55%), var(--bg-panel)">
        <span class="catbar" style:background={cat}></span>
        <div class="head-row">
          <button class="fold" onclick={fold} title="{allFolded ? 'Expand all' : 'Collapse all'} (z)">
            <i class="ph ph-{allFolded ? 'arrows-out-line-vertical' : 'arrows-in-line-vertical'}"></i>
          </button>
          <div class="titles">
            <div class="crumb"><button class="back" onclick={back}>{origin}</button><i class="ph ph-caret-right"></i><span class="catpick"><button style:color={cat} onclick={() => (picking = true)} title="Change category (c)">{category(d.topic.category).label}</button>{#if picking}<CategoryPicker current={d.topic.category} placement={{ top: 'calc(100% + 4px)', left: '0' }} onpick={setCategory} onclose={() => (picking = false)} />{/if}</span></div>
            {#if renaming !== null}
              <input class="name-input" bind:this={renameInput} bind:value={renaming} onblur={() => commitRename(renaming)} />
            {:else}
              <button class="name" class:merged={!!pendingMerge(d.topic.id)} ondblclick={() => d && (renaming = d.topic.name)} title="Rename (r)">{d.topic.name}</button>
            {/if}
            {#if pendingMerge(d.topic.id)}
              <div class="merging">merging into {pendingMerge(d.topic.id)?.intoName} when you leave · <span class="u">u</span> undo</div>
            {:else if merges.err}
              <div class="merging err"><i class="ph ph-warning"></i>{merges.err}</div>
            {:else if renameErr}
              <div class="merging err"><i class="ph ph-warning"></i>{renameErr}</div>
            {/if}
          </div>
          <div class="pills">
            {#each d.topic.pills as p}<Pill pill={p} />{/each}
            {#if d.topic.score !== null}<span class="score tnum">{d.topic.score.toFixed(1)}</span>{/if}
          </div>
        </div>
      </div>

      <div class="cols">
        <DockHost bottom={16} left={16} />
        <div class="left scroll">
          {#if d.upcoming.length}
            <Panel icon="clock-countdown" iconColor="var(--aqua)" label="Upcoming" count={d.upcoming.length} bind:open={open.up} accent={d.upcoming.some((u) => u.today)}>
              {#each d.upcoming as u, i}
                <div class="up-row" class:today={u.today} class:rule={i > 0}>
                  <span class="when nowrap" class:today={u.today}>{u.when}</span>
                  <AppIcon source={u.source} />
                  <div class="min0">
                    <div class="strong">{u.title}</div>
                    {#if u.sub}<div class="sub">{u.sub}</div>{/if}
                  </div>
                  <span class="src">{sourceLabel(u.source)}</span>
                </div>
              {/each}
            </Panel>
          {/if}

          <Panel icon="list-checks" iconColor="var(--orange)" label="Open" count={d.open.length} bind:open={open.open}>
            {#each d.open as o, i}
              <div class="open-row" class:rule={i > 0}>
                <AppIcon source={o.source} />
                <span>{o.title}</span>
                <span class="owner">
                  {#if o.due}<Pill pill={o.due} />{/if}
                  {o.owner ?? ''}
                </span>
              </div>
            {:else}
              <div class="empty">Nothing open.</div>
            {/each}
          </Panel>

          <Panel icon="git-commit" iconColor="var(--blue)" label="Chronology" count={null} bind:open={open.chron}>
            {#snippet right()}
              <span class="filters">
                <button class="chip" class:on={filter === null} onclick={() => (filter = null)}>All {d?.chronology.length}</button>
                {#each d?.filters ?? [] as f}
                  <button class="chip" class:on={filter === f.source} onclick={() => (filter = filter === f.source ? null : f.source)} title={sourceLabel(f.source)}>
                    <AppIcon source={f.source} size={13} />{f.count}
                  </button>
                {/each}
              </span>
            {/snippet}
            <Chronology topicId={d.topic.id} {entries} active={open.chron && !linking && !picking} />
          </Panel>

          <Panel icon="users" iconColor="var(--purple)" label="People" count={d.people.length} bind:open={open.people}>
            <div class="people">
              {#each d.people as p (p.id ?? `${p.name}-${p.account?.id ?? ''}`)}
                <div class="person">
                  <Avatar initials={p.initials} color={personColor(p.color)} size={26} font="0.7692rem" />
                  <div class="min0">
                    {#if !p.known && p.account}
                      <button class="strong unknown ellipsis" style:color={personColor(p.color)} title="No People note: link to a person" onclick={() => (linking = p)}
                        >{p.name}</button
                      >
                    {:else}
                      <div class="strong ellipsis" title={p.name}>{p.name}</div>
                    {/if}
                    {#if p.role}
                      <div class="role ellipsis" title={p.role}>{p.role}</div>
                    {:else if p.signals}
                      <div class="role ellipsis">{p.signals} {p.signals === 1 ? 'signal' : 'signals'}{#if p.last}&nbsp;· {p.last}{/if}</div>
                    {/if}
                  </div>
                </div>
              {/each}
            </div>
            {#if merging && d}
              <Dock>
                <TopicPicker
                  exclude={d.topic.id}
                  excludeIds={mergingAway()}
                  placeholder={`Merge “${d.topic.name}” into…`}
                  onpick={mergeInto}
                  onclose={() => (merging = false)}
                />
              </Dock>
            {/if}
            {#if linking?.account}
              <Dock>
                <PersonPicker
                  account={linking.account}
                  name={linking.name}
                  source={linking.account.kind === 'email' ? 'Email' : sourceLabel(linking.account.kind)}
                  ondone={() => ((linking = null), refresh())}
                  onclose={() => (linking = null)}
                />
              </Dock>
            {/if}
            {#if d.repos.length}
              <div class="repos">
                {#each d.repos as r}
                  <span class="repo"><AppIcon source="github" size={14} />{r}</span>
                {/each}
              </div>
            {/if}
          </Panel>
        </div>

        <div class="right-col">
          {#key d.topic.id}<NotePane topicId={d.topic.id} note={d.note} />{/key}
        </div>
      </div>
    </div>
    <AssistantPane topic={d.topic} />
  {/if}
</div>

<style>
  .view {
    flex: 1;
    min-width: 0;
    display: flex;
  }
  .content {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 14px;
    padding: 14px 22px 0;
  }
  .header {
    flex: none;
    display: flex;
    align-items: stretch;
    border-radius: var(--r-panel);
    border: 1px solid var(--border);
  }
  .catbar {
    width: 5px;
    flex: none;
    border-radius: var(--r-panel) 0 0 var(--r-panel);
  }
  .head-row {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 14px;
    padding: 12px 16px;
  }
  .fold {
    flex: none;
    display: flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    border-radius: var(--r-input);
    border: 1px solid var(--border);
    background: var(--bg-app);
    color: var(--tx-muted);
    font-size: 1.1538rem;
  }
  .titles {
    min-width: 0;
  }
  .catpick {
    position: relative;
  }
  .catpick button:hover {
    text-decoration: underline;
  }
  .back:hover {
    color: var(--tx-2);
    text-decoration: underline;
  }
  .crumb {
    font-size: 0.8846rem;
    color: var(--tx-muted);
    display: flex;
    gap: 6px;
    align-items: center;
  }
  .name {
    display: block;
    width: 100%;
    text-align: left;
    background: none;
    border: none;
    font: inherit;
    font-size: 1.8462rem;
    font-weight: 500;
    letter-spacing: -0.015em;
    margin-top: 3px;
    color: var(--tx-title);
  }
  .name:hover {
    text-decoration: underline;
    text-decoration-color: var(--tx-disabled);
  }
  .name-input {
    display: block;
    width: 100%;
    margin-top: 3px;
    padding: 0 0 1px;
    border: none;
    border-bottom: 1px solid var(--aqua);
    outline: none;
    background: none;
    font: inherit;
    font-size: 1.8462rem;
    font-weight: 500;
    letter-spacing: -0.015em;
    color: var(--tx-title);
  }
  .name.merged {
    color: var(--tx-label);
    text-decoration: line-through;
    text-decoration-color: var(--tx-disabled);
  }
  .merging {
    font-size: 0.9231rem;
    color: var(--tx-label);
  }
  .merging .u {
    color: var(--tx-2);
  }
  .merging.err {
    color: var(--orange);
    display: flex;
    gap: 6px;
    align-items: center;
  }
  .pills {
    margin-left: auto;
    display: flex;
    gap: 5px;
    align-items: center;
  }
  .score {
    font-size: 1.3846rem;
    font-weight: 500;
    margin-left: 10px;
    color: var(--tx-title);
  }
  .cols {
    position: relative;
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: minmax(0, 1.12fr) minmax(0, 1fr);
    gap: 18px;
  }
  .left {
    min-height: 0;
    padding: 0 6px 16px 0;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .min0 {
    min-width: 0;
  }
  .strong {
    font-weight: 500;
  }
  .sub {
    font-size: 0.9231rem;
    color: var(--tx-muted-2);
  }
  .rule {
    border-top: 1px solid var(--border);
  }
  .up-row {
    display: grid;
    grid-template-columns: 78px 22px minmax(0, 1fr) auto;
    gap: 10px;
    align-items: center;
    padding: 9px 12px;
  }
  .up-row.today {
    background: var(--aqua-tint-soft);
  }
  .up-row .when {
    font-size: 0.9231rem;
    color: var(--tx-muted);
  }
  .when.today {
    color: var(--aqua) !important;
    font-weight: 500;
  }
  .src {
    font-size: 0.8462rem;
    color: var(--tx-muted-2);
  }
  .open-row {
    display: grid;
    grid-template-columns: 22px minmax(0, 1fr) auto;
    gap: 10px;
    align-items: center;
    padding: 8px 12px;
  }
  .open-row.rule {
    border-top-color: var(--bg-head);
  }
  .owner {
    display: flex;
    gap: 6px;
    align-items: center;
    font-size: 0.8462rem;
    color: var(--tx-muted);
  }
  .empty {
    padding: 8px 12px;
    color: var(--tx-label);
  }
  .filters {
    display: flex;
    gap: 4px;
    letter-spacing: 0;
    text-transform: none;
    font-size: 0.8462rem;
  }
  .chip {
    padding: 1px 7px;
    border-radius: 9px;
    display: flex;
    gap: 4px;
    align-items: center;
    color: var(--tx-muted);
  }
  .chip.on {
    padding: 1px 8px;
    background: rgba(142, 192, 124, 0.18);
    color: var(--aqua);
    box-shadow: inset 0 0 0 1px rgba(142, 192, 124, 0.5);
  }
  .people {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
  }
  .person {
    display: flex;
    gap: 10px;
    align-items: center;
    padding: 9px 12px;
  }
  .person + .person {
    border-left: 1px solid var(--bg-head);
  }
  .unknown {
    display: block;
    max-width: 100%;
    text-align: left;
    text-decoration: underline dotted var(--tx-label);
    text-underline-offset: 3px;
  }

  .role {
    font-size: 0.8846rem;
    color: var(--tx-muted-2);
  }
  .repos {
    display: flex;
    flex-wrap: wrap;
    gap: 6px 16px;
    padding: 8px 12px 10px;
    border-top: 1px solid var(--bg-head);
    font-size: 0.9231rem;
    color: var(--tx-muted);
  }
  .repo {
    display: flex;
    gap: 6px;
    align-items: center;
  }
  .right-col {
    min-height: 0;
    display: flex;
    flex-direction: column;
    padding-bottom: 16px;
  }
</style>

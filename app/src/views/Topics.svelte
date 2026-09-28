<script lang="ts">
  // Topics (7a): the root object. Left column: Upcoming (only when something is
  // soon), Open, Chronology, People. Right: the topic's Obsidian note. The two
  // columns scroll independently; z folds the left column. e edits the note in
  // place (topics/NotePane), o opens it in Obsidian.
  import { call } from '../lib/api';
  import { catColor, category, TABS, ui } from '../lib/app.svelte';
  import { plain, useKeys } from '../lib/keys.svelte';
  import { SOURCES, sourceLabel } from '../lib/sources';
  import type { TopicDetail, TopicRow } from '../lib/types';
  import AppIcon from '../lib/ui/AppIcon.svelte';
  import Panel from '../lib/ui/Panel.svelte';
  import Pill from '../lib/ui/Pill.svelte';
  import AssistantPane from '../shell/AssistantPane.svelte';
  import NotePane from './topics/NotePane.svelte';

  let topics = $state<TopicRow[]>([]);
  let d = $state<TopicDetail | null>(null);
  let filter = $state<string | null>(null);
  let open = $state({ up: true, open: true, chron: true, people: true });

  $effect(() => {
    void ui.rev;
    call<TopicRow[]>('topics.list').then((t) => {
      topics = t;
      if (ui.openTopicId === null && t.length) ui.openTopicId = t[0].id;
    });
  });

  $effect(() => {
    const id = ui.openTopicId;
    void ui.rev;
    if (id === null) return;
    ui.topicId = id;
    call<TopicDetail>('topics.get', { id }).then((r) => {
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
  useKeys((e) => {
    if (plain(e, 'z')) return fold(), true;
    if (plain(e, '[')) return step(-1), true;
    if (plain(e, ']')) return step(1), true;
    if (plain(e, 'f')) return cycleFilter(), true;
  });

  const entries = $derived(d ? d.chronology.filter((e) => filter === null || e.source === filter) : []);
  // "Now" divider sits between upcoming/undone (open dots) and what happened.
  const nowIndex = $derived(entries.findIndex((e) => !e.open));
  const origin = $derived(TABS.find((t) => t.id === ui.openedFrom)?.label ?? 'Topics');
</script>

<div class="view">
  {#if d}
    {@const cat = catColor(d.topic.category)}
    <div class="content">
      <div class="header" style:background="linear-gradient(90deg, color-mix(in srgb, {cat} 10%, transparent), transparent 55%), var(--bg-panel)">
        <span class="catbar" style:background={cat}></span>
        <div class="head-row">
          <button class="fold" onclick={fold} title="{allFolded ? 'Expand all' : 'Collapse all'} (z)">
            <i class="ph ph-{allFolded ? 'arrows-out-line-vertical' : 'arrows-in-line-vertical'}"></i>
          </button>
          <div class="titles">
            <div class="crumb"><span>{origin}</span><i class="ph ph-caret-right"></i><span style:color={cat}>{category(d.topic.category).label}</span></div>
            <div class="name">{d.topic.name}</div>
          </div>
          <div class="pills">
            {#each d.topic.pills as p}<Pill pill={p} />{/each}
            {#if d.topic.score !== null}<span class="score tnum">{d.topic.score.toFixed(1)}</span>{/if}
          </div>
        </div>
      </div>

      <div class="cols">
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
            <div class="timeline">
              <div class="line"></div>
              {#each entries as e, i}
                {#if i === nowIndex && i > 0}
                  <div class="now-rule"><span class="tick"></span>Now<span class="fade"></span></div>
                {/if}
                <div class="entry">
                  <span
                    class="dot"
                    style:background={e.open ? 'var(--bg-panel)' : SOURCES[e.source]?.dot}
                    style:box-shadow={e.open ? `inset 0 0 0 2px ${SOURCES[e.source]?.dot}` : 'none'}
                  ></span>
                  <span class="when nowrap" class:today={e.today}>{e.when}</span>
                  <span class="icon"><AppIcon source={e.source} /></span>
                  <div class="min0">
                    <div>{e.title}{#if e.range}<span class="range">{e.range}</span>{/if}</div>
                    {#if e.sub}<div class="sub">{e.sub}</div>{/if}
                  </div>
                  <span class="right nowrap">{e.right}</span>
                </div>
              {/each}
            </div>
          </Panel>

          <Panel icon="users" iconColor="var(--purple)" label="People" count={d.people.length} bind:open={open.people}>
            <div class="people">
              {#each d.people as p}
                <div class="person">
                  <span class="avatar">{p.initials}</span>
                  <div class="min0">
                    <div class="strong">{p.name}</div>
                    <div class="role ellipsis">{p.role ?? ''}</div>
                  </div>
                </div>
              {/each}
            </div>
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
    overflow: hidden;
  }
  .catbar {
    width: 5px;
    flex: none;
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
    font-size: 15px;
  }
  .titles {
    min-width: 0;
  }
  .crumb {
    font-size: 11.5px;
    color: var(--tx-muted);
    display: flex;
    gap: 6px;
    align-items: center;
  }
  .name {
    font-size: 24px;
    font-weight: 500;
    letter-spacing: -0.015em;
    margin-top: 3px;
    color: var(--tx-title);
  }
  .pills {
    margin-left: auto;
    display: flex;
    gap: 5px;
    align-items: center;
  }
  .score {
    font-size: 18px;
    font-weight: 500;
    margin-left: 10px;
    color: var(--tx-title);
  }
  .cols {
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
    font-size: 12px;
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
    background: rgba(142, 192, 124, 0.07);
  }
  .up-row .when {
    font-size: 12px;
    color: var(--tx-muted);
  }
  .when.today {
    color: var(--aqua) !important;
    font-weight: 500;
  }
  .src {
    font-size: 11px;
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
    font-size: 11px;
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
    font-size: 11px;
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
  .timeline {
    position: relative;
    padding: 6px 12px 8px 30px;
  }
  .line {
    position: absolute;
    left: 17px;
    top: 16px;
    bottom: 16px;
    width: 2px;
    border-radius: 1px;
    background: linear-gradient(var(--border-strong), var(--border) 85%, transparent);
  }
  .entry {
    display: grid;
    grid-template-columns: 62px 22px minmax(0, 1fr) auto;
    gap: 8px;
    align-items: start;
    padding: 7px 0;
    position: relative;
  }
  .dot {
    position: absolute;
    left: -18px;
    top: 11px;
    width: 10px;
    height: 10px;
    border-radius: 50%;
  }
  .entry .when {
    font-size: 11.5px;
    color: var(--tx-muted-2);
    padding-top: 1px;
  }
  .icon {
    padding-top: 1px;
  }
  .range {
    font-size: 11.5px;
    color: var(--tx-label);
    margin-left: 5px;
  }
  .entry .right {
    font-size: 11px;
    color: var(--tx-label);
  }
  .now-rule {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 4px 0 4px -22px;
    font-size: 10.5px;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--aqua);
  }
  .tick {
    width: 10px;
    height: 2px;
    background: var(--aqua);
  }
  .fade {
    flex: 1;
    height: 1px;
    background: linear-gradient(90deg, rgba(142, 192, 124, 0.5), transparent);
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
  .avatar {
    width: 26px;
    height: 26px;
    flex: none;
    border-radius: 50%;
    background: var(--border-strong);
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 10px;
    color: var(--tx-title);
  }
  .role {
    font-size: 11.5px;
    color: var(--tx-muted-2);
  }
  .repos {
    display: flex;
    flex-wrap: wrap;
    gap: 6px 16px;
    padding: 8px 12px 10px;
    border-top: 1px solid var(--bg-head);
    font-size: 12px;
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

<script lang="ts">
  // Incoming (8a): the triage queue. Left: topics with new signals, Unsorted last.
  // Right: one panel with the selected topic's new signals, grouped by app.
  // Confirm links the signal for good; Wrong topic only records that the link was
  // wrong (the item drops into Unsorted, nothing is deleted, no reason asked).
  import { call, openExternal } from '../lib/api';
  import { catColor, refresh, ui } from '../lib/app.svelte';
  import { plain, useKeys } from '../lib/keys.svelte';
  import { sourceLabel } from '../lib/sources';
  import AppIcon from '../lib/ui/AppIcon.svelte';
  import AssistantPane from '../shell/AssistantPane.svelte';
  import TopicPicker from './incoming/TopicPicker.svelte';
  import type { Queue, QueueKey, Signal } from './incoming/types';

  // Fixed app order so each app's group always sits in the same place.
  const APP_ORDER = ['slack', 'gmail', 'gcal', 'jira', 'github', 'obsidian', 'sheets', 'notion'];

  let queue = $state<Queue | null>(null);
  let selKey = $state<QueueKey | null>(null);
  let selItem = $state<number | null>(null);
  let expanded = $state<Set<number>>(new Set());
  let picking = $state(false);
  let busy = false;
  let err = $state<string | null>(null);

  type Entry = { key: QueueKey; name: string; category: string | null; count: number };
  const entries = $derived<Entry[]>(
    queue
      ? [
          ...queue.topics.map((t) => ({ key: t.id as QueueKey, name: t.name, category: t.category, count: t.signals.length })),
          { key: 'unsorted' as QueueKey, name: 'Unsorted', category: null, count: queue.unsorted.length },
        ]
      : [],
  );

  function signalsFor(q: Queue | null, key: QueueKey | null): Signal[] {
    if (!q || key === null) return [];
    if (key === 'unsorted') return q.unsorted;
    return q.topics.find((t) => t.id === key)?.signals ?? [];
  }

  function grouped(sigs: Signal[]) {
    const rank = (s: string) => (APP_ORDER.indexOf(s) + APP_ORDER.length + 1) % (APP_ORDER.length + 1);
    const groups: { source: string; signals: Signal[] }[] = [];
    for (const s of sigs) {
      const g = groups.find((g) => g.source === s.source);
      if (g) g.signals.push(s);
      else groups.push({ source: s.source, signals: [s] });
    }
    return groups.sort((a, b) => rank(a.source) - rank(b.source));
  }

  const groups = $derived(grouped(signalsFor(queue, selKey)));
  const flat = $derived(groups.flatMap((g) => g.signals));
  const current = $derived(flat.find((s) => s.item_id === selItem) ?? null);
  const unsorted = $derived(selKey === 'unsorted');
  const topicName = $derived(entries.find((e) => e.key === selKey)?.name ?? '');

  $effect(() => {
    void ui.rev;
    call<Queue>('incoming.list').then(apply);
  });

  // Keep the selection where it was: same topic and item if they still exist,
  // otherwise the row that moved into the same place.
  function apply(q: Queue) {
    const prevEntries = entries;
    const prevFlat = flat;
    const topicIdx = Math.max(0, prevEntries.findIndex((e) => e.key === selKey));
    const itemIdx = Math.max(0, prevFlat.findIndex((s) => s.item_id === selItem));
    // Keep the topic order stable while triaging: topics already listed stay put,
    // topics that just got their first new signal go on top.
    const known = queue?.topics.map((t) => t.id) ?? [];
    const pos = (id: number) => {
      const i = known.indexOf(id);
      return i < 0 ? -1 : i;
    };
    q.topics.sort((a, b) => pos(a.id) - pos(b.id));
    queue = q;
    const keys: QueueKey[] = [...q.topics.map((t) => t.id), 'unsorted'];
    if (selKey === null || !keys.includes(selKey)) {
      selKey = keys[Math.min(topicIdx, keys.length - 1)];
      selItem = null;
    }
    const now = grouped(signalsFor(q, selKey)).flatMap((g) => g.signals);
    if (!now.some((s) => s.item_id === selItem)) {
      selItem = now[Math.min(itemIdx, now.length - 1)]?.item_id ?? null;
    }
  }

  $effect(() => {
    ui.topicId = typeof selKey === 'number' ? selKey : null;
  });

  $effect(() => {
    if (selItem === null) return;
    document.querySelector(`[data-item="${selItem}"]`)?.scrollIntoView({ block: 'nearest' });
  });

  function selectTopic(key: QueueKey) {
    if (key === selKey) return;
    selKey = key;
    picking = false;
    selItem = grouped(signalsFor(queue, key)).flatMap((g) => g.signals)[0]?.item_id ?? null;
  }

  function stepTopic(d: number) {
    const i = entries.findIndex((e) => e.key === selKey);
    const next = entries[(i + d + entries.length) % entries.length];
    if (next) selectTopic(next.key);
  }

  function stepSignal(d: number) {
    if (!flat.length) return;
    const i = flat.findIndex((s) => s.item_id === selItem);
    selItem = flat[Math.max(0, Math.min(flat.length - 1, i + d))].item_id;
    picking = false;
  }

  function toggle(id: number) {
    const next = new Set(expanded);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    expanded = next;
  }

  async function act(cmd: string, args: Record<string, unknown>) {
    if (busy) return;
    busy = true;
    try {
      await call(cmd, args);
      err = null;
      refresh();
    } catch (e) {
      err = String((e as Error).message ?? e);
    } finally {
      busy = false;
    }
  }

  function confirm(s: Signal | null) {
    if (!s || typeof selKey !== 'number') return;
    selItem = s.item_id;
    act('incoming.confirm', { item_id: s.item_id, topic_id: selKey });
  }

  function wrong(s: Signal | null) {
    if (!s || typeof selKey !== 'number') return;
    selItem = s.item_id;
    act('incoming.wrong', { item_id: s.item_id, topic_id: selKey });
  }

  function pick(s: Signal | null) {
    if (!s) return;
    selItem = s.item_id;
    picking = true;
  }

  function moveTo(topicId: number) {
    picking = false;
    if (!current) return;
    act('incoming.move', { item_id: current.item_id, topic_id: topicId, from_topic_id: typeof selKey === 'number' ? selKey : null });
  }

  function open(s: Signal | null) {
    if (s?.url) openExternal(s.url);
  }

  useKeys((e) => {
    if (picking) return;
    if (plain(e, 'j') || plain(e, 'ArrowDown')) return stepSignal(1), true;
    if (plain(e, 'k') || plain(e, 'ArrowUp')) return stepSignal(-1), true;
    if (plain(e, ']')) return stepTopic(1), true;
    if (plain(e, '[')) return stepTopic(-1), true;
    if (plain(e, 'Enter')) return current && toggle(current.item_id), true;
    if (plain(e, 'Escape') && current && expanded.has(current.item_id)) return toggle(current.item_id), true;
    if (plain(e, 'c')) return confirm(current), true;
    if (plain(e, 'x')) return wrong(current), true;
    if (plain(e, 't')) return pick(current), true;
    if (plain(e, 'o')) return open(current), true;
  });
</script>

<div class="view">
  <div class="topics scroll">
    <div class="label head">Topics with new signals</div>
    {#each entries as t (t.key)}
      {#if t.key === 'unsorted'}<div class="fade-rule"></div>{/if}
      <button class="topic" class:on={t.key === selKey} class:unsorted={t.key === 'unsorted'} onclick={() => selectTopic(t.key)}>
        <span class="bar" style:background={t.category ? catColor(t.category) : null}></span>
        <span class="name ellipsis">
          {#if t.key === 'unsorted'}<i class="ph ph-shuffle"></i>{/if}{t.name}
        </span>
        <span class="count tnum">{t.count}</span>
      </button>
    {/each}
  </div>

  <div class="main">
    <div class="panel">
      <div class="body scroll">
        {#each groups as g, gi (g.source)}
          <div class="group" class:rule={gi > 0}>
            <AppIcon source={g.source} size={14} />{sourceLabel(g.source)}<span class="gcount">{g.signals.length}</span>
          </div>
          {#each g.signals as s (s.item_id)}
            {@const sel = s.item_id === selItem}
            {@const isOpen = expanded.has(s.item_id)}
            <div class="sig" class:sel data-item={s.item_id} onclick={() => (selItem = s.item_id)} role="presentation">
              <div class="row">
                <button class="title-line" onclick={() => toggle(s.item_id)} title={isOpen ? 'Collapse (↵)' : 'Expand (↵)'}>
                  <span class="title ellipsis">{s.title}</span>
                  <span class="meta tnum">{s.meta}</span>
                  {#if isOpen}<i class="ph ph-caret-up caret"></i>{/if}
                </button>
                <div class="actions">
                  {#if unsorted}
                    <button class="btn primary" onclick={() => pick(s)}>Move to topic</button>
                  {:else}
                    <button class="btn primary" onclick={() => confirm(s)}>Confirm</button>
                    <button class="btn" onclick={() => wrong(s)}>Wrong topic</button>
                  {/if}
                </div>
              </div>
              {#if isOpen}
                <div class="ctx">
                  {#if s.messages.length}
                    {#each s.messages as m}
                      <div class="msg">
                        <span class="t tnum">{m.time}</span>
                        <span class="who ellipsis">{m.author}</span>
                        <span class="txt">{m.text}</span>
                      </div>
                    {/each}
                  {:else if s.summary}
                    <div class="summary">{s.summary}</div>
                  {/if}
                  {#if s.url}
                    <div class="links">
                      {#if s.msg_count && s.msg_count > s.messages.length}
                        <button class="all" onclick={() => open(s)}>All {s.msg_count}</button>
                      {/if}
                      <button class="app" onclick={() => open(s)}>Open in {sourceLabel(s.source)}</button>
                    </div>
                  {/if}
                </div>
              {/if}
              {#if sel && picking}
                <div class="pick">
                  <TopicPicker exclude={typeof selKey === 'number' ? selKey : null} onpick={moveTo} onclose={() => (picking = false)} />
                </div>
              {/if}
            </div>
          {/each}
        {:else}
          {#if queue}
            <div class="empty">{unsorted ? 'Nothing unsorted.' : `Nothing new on ${topicName}.`}</div>
          {/if}
        {/each}
      </div>
      <div class="hints">
        <span>j/k signal</span><span>↵ expand</span>
        {#if unsorted}
          <span>t move to topic</span>
        {:else}
          <span>c confirm</span><span>x wrong topic</span><span>t move to topic</span>
        {/if}
        <span>o open</span><span>[ ] topic</span>
        {#if err}<span class="err"><i class="ph ph-warning"></i>{err}</span>{/if}
      </div>
    </div>
  </div>

  <AssistantPane topic={queue?.topics.find((t) => t.id === selKey) ?? null} />
</div>

<style>
  .view {
    flex: 1;
    min-width: 0;
    display: flex;
    line-height: 1.5;
  }
  .topics {
    width: 260px;
    flex: none;
    display: flex;
    flex-direction: column;
    padding: 14px 0;
  }
  .head {
    padding: 0 14px 8px;
  }
  .topic {
    display: grid;
    grid-template-columns: 4px minmax(0, 1fr) auto;
    gap: 10px;
    align-items: center;
    padding: 8px 12px 8px 0;
    border-radius: 0 6px 6px 0;
    text-align: left;
    transition: background 120ms;
  }
  .topic:hover {
    background: rgba(235, 219, 178, 0.04);
  }
  .topic.on {
    background: var(--aqua-tint);
    box-shadow: inset -2px 0 0 var(--aqua);
  }
  .bar {
    align-self: stretch;
    border-radius: 0 2px 2px 0;
  }
  .unsorted .bar {
    background: repeating-linear-gradient(180deg, var(--tx-label) 0 3px, transparent 3px 6px);
  }
  .name {
    display: flex;
    gap: 8px;
    align-items: center;
  }
  .on .name {
    font-weight: 500;
    color: var(--tx-title);
  }
  .unsorted .name {
    color: var(--tx-muted);
  }
  .unsorted .name i {
    color: var(--tx-label);
  }
  .count {
    font-size: 10.5px;
    padding: 1px 7px;
    border-radius: 9px;
    background: var(--border);
    color: var(--tx-2);
  }
  .on .count {
    background: rgba(142, 192, 124, 0.2);
    color: var(--aqua);
  }
  .fade-rule {
    flex: none;
    margin: 12px 14px 8px;
    height: 1px;
    background: linear-gradient(90deg, transparent, var(--border-strong) 20%, var(--border-strong) 80%, transparent);
  }

  .main {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    padding: 14px 22px 14px 8px;
  }
  .panel {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    border-radius: var(--r-panel);
    background: var(--bg-panel);
    border: 1px solid var(--border);
    overflow: hidden;
  }
  .body {
    flex: 1;
    min-height: 0;
    padding-bottom: 8px;
  }
  .group {
    display: flex;
    gap: 8px;
    align-items: center;
    padding: 8px 16px 6px;
    font-size: 11px;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--tx-muted-2);
  }
  .group.rule {
    border-top: 1px solid var(--border);
    margin-top: 4px;
  }
  .gcount {
    letter-spacing: 0;
    text-transform: none;
    color: var(--tx-label);
  }
  .sig {
    transition: background 120ms;
  }
  .sig:hover:not(.sel) {
    background: rgba(235, 219, 178, 0.03);
  }
  .sig.sel {
    background: var(--aqua-tint-soft);
    box-shadow: inset 3px 0 0 var(--aqua);
  }
  .row {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    gap: 12px;
    align-items: center;
    padding: 9px 16px 9px 38px;
  }
  .title-line {
    min-width: 0;
    display: flex;
    gap: 10px;
    align-items: baseline;
    text-align: left;
  }
  .title {
    font-weight: 500;
    min-width: 0;
  }
  .meta {
    font-size: 12px;
    color: var(--tx-muted-2);
    white-space: nowrap;
  }
  .caret {
    color: var(--tx-muted-2);
    align-self: center;
  }
  .actions {
    display: flex;
    gap: 6px;
  }
  .ctx {
    padding: 4px 16px 12px 38px;
    font-size: 12.5px;
  }
  .msg {
    display: grid;
    grid-template-columns: 44px 64px minmax(0, 1fr);
    gap: 10px;
    padding: 4px 0;
  }
  .t {
    color: var(--tx-label);
  }
  .who {
    color: var(--tx-body);
    font-weight: 500;
  }
  .txt,
  .summary {
    color: var(--tx-2);
  }
  .links {
    display: flex;
    gap: 14px;
    padding-top: 6px;
    font-size: 12px;
  }
  .all {
    color: var(--aqua);
  }
  .app {
    color: var(--blue);
  }
  .links button:hover {
    text-decoration: underline;
  }
  .pick {
    padding: 0 16px 10px 38px;
  }
  .err {
    margin-left: auto;
    color: var(--orange);
    display: flex;
    gap: 6px;
    align-items: center;
  }
  .empty {
    padding: 16px;
    color: var(--tx-label);
  }
  .hints {
    flex: none;
    display: flex;
    gap: 16px;
    padding: 8px 16px;
    border-top: 1px solid var(--border);
    font-size: 11.5px;
    color: var(--tx-label);
    white-space: nowrap;
    overflow: hidden;
  }
</style>

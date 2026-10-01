<script lang="ts">
  // Incoming (8a): the triage queue. Left: topics with new signals, Unsorted last.
  // Right: one panel with the selected topic's new signals, grouped by app.
  // Confirm links the signal for good; Wrong topic only records that the link was
  // wrong (the item drops into Unsorted, nothing is deleted, no reason asked).
  // Unsorted only: ↵ on a signal with several messages enters its message list
  // (j/k move, space selects, t files the selected messages under a topic as a
  // new item, the rest stays; Esc leaves). Triage elsewhere is unchanged.
  import { call, openExternal } from '../lib/api';
  import { untrack } from 'svelte';
  import { catColor, refresh, ui } from '../lib/app.svelte';
  import { plain, useKeys } from '../lib/keys.svelte';
  import { sourceLabel } from '../lib/sources';
  import AppIcon from '../lib/ui/AppIcon.svelte';
  import Markdown from '../lib/ui/Markdown.svelte';
  import AssistantPane from '../shell/AssistantPane.svelte';
  import TopicPicker from './incoming/TopicPicker.svelte';
  import type { Queue, QueueKey, Signal } from './incoming/types';
  import TodoForm from './todos/TodoForm.svelte';
  import type { Draft, JiraTarget, TodosData } from './todos/types';

  // Fixed app order so each app's group always sits in the same place.
  const APP_ORDER = ['slack', 'gmail', 'gcal', 'jira', 'github', 'obsidian', 'sheets', 'notion'];

  let queue = $state<Queue | null>(null);
  let selKey = $state<QueueKey | null>(null);
  let selItem = $state<number | null>(null);
  let expanded = $state<Set<number>>(new Set());
  let fullContext = $state<Set<number>>(new Set());
  let todoFor = $state<number | null>(null);
  let jira = $state<JiraTarget | null>(null);
  call<TodosData>('todos.list').then((d) => (jira = d.jira));
  let picking = $state(false);
  // Message mode (Unsorted): focus is inside the current signal's messages.
  let inMsgs = $state(false);
  let msgIdx = $state(0);
  let chosen = $state<Set<number>>(new Set());
  let dismissed = $state<Set<number>>(new Set());
  let trashed = $state<Set<number>>(new Set());
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
  const splittable = (s: Signal | null) => unsorted && !!s && s.source !== 'gmail' && s.messages.length > 1;

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
    // topics that just got their first new signal go at the end, above Unsorted.
    const known = queue?.topics.map((t) => t.id) ?? [];
    const pos = (id: number) => {
      const i = known.indexOf(id);
      return i < 0 ? known.length : i;
    };
    q.topics.sort((a, b) => pos(a.id) - pos(b.id));
    queue = q;
    const live = new Set([...q.topics.flatMap((t) => t.signals), ...q.unsorted].map((s) => s.item_id));
    dismissed = new Set([...dismissed].filter((id) => live.has(id)));
    const keys: QueueKey[] = [...q.topics.map((t) => t.id), 'unsorted'];
    if (selKey === null || !keys.includes(selKey)) {
      selKey = keys[Math.min(topicIdx, keys.length - 1)];
      selItem = null;
    }
    const now = grouped(signalsFor(q, selKey)).flatMap((g) => g.signals);
    if (!now.some((s) => s.item_id === selItem)) {
      selItem = now[Math.min(itemIdx, now.length - 1)]?.item_id ?? null;
    }
    // A sync may have changed the messages under an open selection.
    const msgs = now.find((s) => s.item_id === selItem)?.messages ?? [];
    if (selKey !== 'unsorted' || msgs.length < 2) leaveMsgs();
    else {
      chosen = new Set([...chosen].filter((id) => msgs.some((m) => m.id === id)));
      msgIdx = Math.min(msgIdx, msgs.length - 1);
    }
  }

  function leaveMsgs() {
    inMsgs = false;
    msgIdx = 0;
    if (chosen.size) chosen = new Set();
  }

  function enterMsgs(s: Signal, idx = 0) {
    if (selItem !== s.item_id) leaveMsgs();
    selItem = s.item_id;
    if (!expanded.has(s.item_id)) toggle(s.item_id);
    inMsgs = true;
    msgIdx = idx;
  }

  function stepMsg(d: number) {
    if (!current) return;
    msgIdx = Math.max(0, Math.min(current.messages.length - 1, msgIdx + d));
  }

  function choose(id: number) {
    const next = new Set(chosen);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    chosen = next;
  }

  function clickMsg(s: Signal, idx: number) {
    if (!splittable(s)) return;
    enterMsgs(s, idx);
    choose(s.messages[idx].id);
  }

  // Unsorted ↵: collapsed → expand and enter the messages; expanded → enter;
  // in the messages → collapse and leave.
  function enter(s: Signal | null) {
    if (!s) return;
    if (!splittable(s)) return toggle(s.item_id);
    if (inMsgs) {
      leaveMsgs();
      if (expanded.has(s.item_id)) toggle(s.item_id);
    } else enterMsgs(s);
  }

  $effect(() => {
    ui.topicId = typeof selKey === 'number' ? selKey : null;
  });

  $effect(() => {
    if (selKey !== null) document.querySelector(`[data-topic-key="${selKey}"]`)?.scrollIntoView({ block: 'nearest' });
  });

  $effect(() => {
    if (selItem === null) return;
    document.querySelector(`[data-item="${selItem}"]`)?.scrollIntoView({ block: 'nearest' });
  });

  $effect(() => {
    if (!inMsgs || !current) return;
    const m = current.messages[msgIdx];
    if (m) document.querySelector(`[data-msg="${m.id}"]`)?.scrollIntoView({ block: 'nearest' });
  });

  function selectTopic(key: QueueKey) {
    if (key === selKey) return;
    selKey = key;
    picking = false;
    leaveMsgs();
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
    leaveMsgs();
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

  $effect(() => {
    const id = selItem;
    if (untrack(() => [...dismissed].some((d) => d !== id))) untrack(refresh);
  });

  async function dismiss(s: Signal | null, on = true, trash = false) {
    if (!s || dismissed.has(s.item_id) === on || (trash && s.source !== 'gmail')) return;
    selItem = s.item_id;
    picking = false;
    const flip = (add: boolean) => {
      const next = new Set(dismissed);
      const t = new Set(trashed);
      if (add) next.add(s.item_id);
      else next.delete(s.item_id);
      if (add && trash) t.add(s.item_id);
      else t.delete(s.item_id);
      dismissed = next;
      trashed = t;
    };
    flip(on);
    try {
      await call(on ? (trash ? 'incoming.trash' : 'incoming.dismiss') : 'incoming.undismiss', { item_id: s.item_id });
      err = null;
    } catch (e) {
      flip(!on);
      err = String((e as Error).message ?? e);
    }
  }

  function confirm(s: Signal | null) {
    if (!s || typeof selKey !== 'number' || dismissed.has(s.item_id)) return;
    selItem = s.item_id;
    act('incoming.confirm', { item_id: s.item_id, topic_id: selKey });
  }

  function wrong(s: Signal | null) {
    if (!s || typeof selKey !== 'number' || dismissed.has(s.item_id)) return;
    selItem = s.item_id;
    act('incoming.wrong', { item_id: s.item_id, topic_id: selKey });
  }

  function pick(s: Signal | null) {
    if (!s || dismissed.has(s.item_id)) return;
    selItem = s.item_id;
    picking = true;
  }

  function moveTo(topicId: number) {
    picking = false;
    if (!current) return;
    if (unsorted && chosen.size) {
      const ids = [...chosen];
      leaveMsgs();
      act('incoming.split', { item_id: current.item_id, message_ids: ids, topic_id: topicId });
      return;
    }
    act('incoming.move', { item_id: current.item_id, topic_id: topicId, from_topic_id: typeof selKey === 'number' ? selKey : null });
  }

  function openTodo(s: Signal | null) {
    if (!s || dismissed.has(s.item_id)) return;
    selItem = s.item_id;
    picking = false;
    todoFor = s.item_id;
  }

  function hold(s: Signal, d: Draft) {
    todoFor = null;
    act('todos.hold', { item_id: s.item_id, target: d.target, text: d.text, due: d.due });
  }

  function info(s: Signal) {
    const shown = new Set(s.meta.split(' · '));
    const extra = (s.summary ?? '').split(' · ').filter((p) => p && !shown.has(p));
    return [s.meta, ...extra].filter(Boolean).join(' · ');
  }

  function open(s: Signal | null) {
    if (s?.url) openExternal(s.url);
  }

  useKeys((e) => {
    if (picking || todoFor !== null) return;
    if (inMsgs && current) {
      if (plain(e, 'j') || plain(e, 'ArrowDown')) return stepMsg(1), true;
      if (plain(e, 'k') || plain(e, 'ArrowUp')) return stepMsg(-1), true;
      if (plain(e, ' ')) return current.messages[msgIdx] && choose(current.messages[msgIdx].id), true;
      if (plain(e, 'Escape')) return leaveMsgs(), true;
    }
    if (plain(e, 'j') || plain(e, 'ArrowDown')) return stepSignal(1), true;
    if (plain(e, 'k') || plain(e, 'ArrowUp')) return stepSignal(-1), true;
    if (plain(e, ']')) return stepTopic(1), true;
    if (plain(e, '[')) return stepTopic(-1), true;
    if (plain(e, 'Enter')) return enter(current), true;
    if (plain(e, 'Escape') && current && expanded.has(current.item_id)) return toggle(current.item_id), true;
    if (plain(e, 'c')) return confirm(current), true;
    if (plain(e, 'x')) return wrong(current), true;
    if (plain(e, 't')) return pick(current), true;
    if (plain(e, 'd')) return dismiss(current), true;
    if (plain(e, 'u')) {
      if (current?.held && !dismissed.has(current.item_id)) act('todos.unhold', { item_id: current.item_id });
      else dismiss(current, false);
      return true;
    }
    if (plain(e, 'n')) return openTodo(current), true;
    if (plain(e, '#')) return dismiss(current, true, true), true;
    if (plain(e, 'o')) return open(current), true;
  });
</script>

<div class="view">
  <div class="topics scroll">
    <div class="label head">Topics with new signals</div>
    {#each entries as t (t.key)}
      {#if t.key === 'unsorted'}<div class="fade-rule"></div>{/if}
      <button class="topic" data-topic-key={t.key} class:on={t.key === selKey} class:unsorted={t.key === 'unsorted'} onclick={() => selectTopic(t.key)}>
        <span class="bar" style:background={t.category ? catColor(t.category) : null}></span>
        <span class="name ellipsis" title={t.name}>
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
            {@const gone = dismissed.has(s.item_id)}
            <div class="sig" class:sel class:gone data-item={s.item_id} onclick={() => (selItem = s.item_id)} role="presentation">
              <div class="row">
                <button class="title-line" onclick={() => toggle(s.item_id)} title={isOpen ? 'Collapse (↵)' : 'Expand (↵)'}>
                  <span class="title" class:ellipsis={!isOpen} title={s.title}>{s.title}</span>
                  {#if !isOpen}<span class="meta tnum">{s.meta}</span>{/if}
                  {#if isOpen}<i class="ph ph-caret-up caret"></i>{/if}
                </button>
                <div class="actions">
                  {#if gone}
                    {#if trashed.has(s.item_id)}
                      <span class="ghost"><i class="ph ph-trash"></i>Trashed</span>
                    {:else}
                      <span class="ghost"><i class="ph ph-eye-slash"></i>{s.source === 'gmail' ? 'Archived' : 'Dismissed'}</span>
                    {/if}
                    <button class="btn" onclick={() => dismiss(s, false)}>Undo <kbd>u</kbd></button>
                  {:else}
                    <span class="src-actions">
                      <button class="btn" onclick={() => openTodo(s)}>Todo… <kbd>n</kbd></button>
                      {#if s.source === 'gmail'}<button class="btn" onclick={() => dismiss(s, true, true)}><i class="ph ph-trash"></i>Trash <kbd>#</kbd></button>{/if}
                    </span>
                    {#if unsorted}
                      <button class="btn primary" onclick={() => pick(s)}>
                        {sel && chosen.size ? `Move ${chosen.size} of ${s.messages.length}` : 'Move to topic'}
                      </button>
                      <button class="ghost" onclick={() => dismiss(s)}><i class="ph ph-eye-slash"></i>Dismiss</button>
                    {:else}
                      <button class="btn primary" onclick={() => confirm(s)}>Confirm{#if s.held}&nbsp;+ create {s.held.target === 'jira' ? 'issue' : 'todo'}{/if}</button>
                      <button class="btn" onclick={() => wrong(s)}>Wrong topic</button>
                      <button class="ghost" onclick={() => dismiss(s)}><i class="ph ph-eye-slash"></i>Dismiss</button>
                    {/if}
                  {/if}
                </div>
              </div>
              {#if todoFor === s.item_id}
                <div class="todo-form">
                  <TodoForm
                    {jira}
                    held
                    topic={unsorted ? null : (queue?.topics.find((t) => t.id === selKey) ?? null)}
                    gmail={s.source === 'gmail'}
                    initial={s.held}
                    onsave={(d) => hold(s, d)}
                    oncancel={() => (todoFor = null)}
                  />
                </div>
              {:else if s.held && !gone}
                <div class="held">
                  <i class="ph ph-hourglass-medium"></i>
                  <span class="ellipsis">
                    {s.held.target === 'jira' ? 'Jira issue' : 'Todo'} held until you {unsorted ? 'file the signal' : 'confirm'} ·
                    <span class="htext">{s.held.text}</span>{#if s.held.due}&nbsp;· due {s.held.due}{/if}{#if s.held.target === 'jira' && jira}&nbsp;· {jira.project} · {jira.issue_type} · you{/if}
                  </span>
                  <button class="ghost" onclick={(e) => (e.stopPropagation(), openTodo(s))}>Edit</button>
                  <button class="ghost" onclick={(e) => (e.stopPropagation(), act('todos.unhold', { item_id: s.item_id }))}>Discard <kbd>u</kbd></button>
                </div>
              {/if}
              {#if isOpen}
                {@const hidden = fullContext.has(s.item_id) ? 0 : Math.max(0, s.context.length - 3)}
                <div class="ctx">
                  <div class="info tnum">{info(s)}</div>
                  {#each s.context as m, ci (m.id)}
                    {#if hidden && ci === 1}
                      <button class="more" onclick={(e) => (e.stopPropagation(), (fullContext = new Set(fullContext).add(s.item_id)))}>
                        … {hidden} earlier
                      </button>
                    {/if}
                    {#if !hidden || ci === 0 || ci > hidden}
                      <div class="msg context">
                        <span class="t tnum">{m.time}</span>
                        <span class="who ellipsis" title={m.author}>{m.author}</span>
                        <div class="txt"><Markdown text={m.text} /></div>
                      </div>
                    {/if}
                  {/each}
                  {#if s.messages.length}
                    {#each s.messages as m, mi (m.id)}
                      {@const canPick = splittable(s)}
                      <div
                        class="msg"
                        class:pickable={canPick}
                        class:focus={sel && inMsgs && mi === msgIdx}
                        class:chosen={sel && chosen.has(m.id)}
                        data-msg={m.id}
                        onclick={(e) => (e.stopPropagation(), clickMsg(s, mi))}
                        role="presentation"
                      >
                        {#if sel && chosen.has(m.id)}<i class="ph ph-check tick"></i>{/if}
                        <span class="t tnum">{m.time}</span>
                        <span class="who ellipsis" title={m.author}>{m.author}</span>
                        <div class="txt"><Markdown text={m.text} /></div>
                      </div>
                      {#each m.replies ?? [] as r (r.id)}
                        <div class="msg reply" class:chosen={sel && chosen.has(m.id)}>
                          <span class="t tnum">{r.time}</span>
                          <span class="who ellipsis" title={r.author}>{r.author}</span>
                          <div class="txt"><Markdown text={r.text} /></div>
                        </div>
                      {/each}
                    {/each}
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
                  <TopicPicker
                    exclude={typeof selKey === 'number' ? selKey : null}
                    placeholder={unsorted && chosen.size ? `Move ${chosen.size} ${chosen.size === 1 ? 'message' : 'messages'} to topic…` : 'Move to topic…'}
                    onpick={moveTo}
                    onclose={() => (picking = false)}
                  />
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
        {#if unsorted && inMsgs}
          <span>j/k message</span><span>space select</span>
          <span>{chosen.size ? `t move ${chosen.size} selected` : 't move whole signal'}</span>
          <span>↵ collapse</span><span>esc done</span>
        {:else if unsorted}
          <span>j/k signal</span><span>{splittable(current) ? '↵ pick messages' : '↵ expand'}</span>
          <span>t move to topic</span><span>n todo</span><span>d dismiss</span>{#if current?.source === 'gmail'}<span># trash</span>{/if}<span>u undo</span>
        {:else}
          <span>j/k signal</span><span>↵ expand</span>
          <span>c confirm</span><span>x wrong topic</span><span>n todo</span><span>d dismiss</span>{#if current?.source === 'gmail'}<span># trash</span>{/if}<span>u undo</span><span>t move to topic</span>
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
    align-items: center;
  }
  .src-actions {
    display: flex;
    gap: 6px;
    align-items: center;
    padding-right: 10px;
    margin-right: 4px;
    border-right: 1px solid var(--border);
  }
  .src-actions .btn {
    display: flex;
    gap: 5px;
    align-items: center;
  }
  .ghost {
    display: flex;
    gap: 6px;
    align-items: center;
    padding: 0 8px;
    color: var(--tx-muted-2);
  }
  button.ghost:hover {
    color: var(--tx-2);
  }
  .sig.gone {
    background: repeating-linear-gradient(135deg, rgba(235, 219, 178, 0.025) 0 6px, transparent 6px 12px);
  }
  .gone .title,
  .gone .meta {
    color: var(--tx-label);
  }
  .gone .title {
    text-decoration: line-through;
  }
  .ctx {
    padding: 4px 16px 12px 38px;
    font-size: 12.5px;
  }
  .msg {
    position: relative;
    display: grid;
    grid-template-columns: 44px 64px minmax(0, 1fr);
    gap: 10px;
    padding: 4px 8px;
    margin: 0 -8px;
    border-radius: 4px;
    transition: background 120ms;
  }
  .msg.context {
    opacity: 0.5;
  }
  .todo-form {
    padding: 0 16px 4px 38px;
  }
  .held {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 16px 8px 38px;
    font-size: 12px;
    color: var(--tx-muted-2);
  }
  .held .htext {
    color: var(--tx-2);
  }
  .held .ghost {
    flex: none;
    font-size: 12px;
  }
  .more {
    padding: 2px 0 2px 54px;
    font-size: 12px;
    color: var(--tx-muted-2);
  }
  .more:hover {
    color: var(--aqua);
  }
  .msg.reply {
    margin-left: 10px;
    padding-left: 16px;
    border-left: 1px solid var(--border);
  }
  .msg.pickable {
    cursor: pointer;
  }
  .msg.pickable:hover:not(.chosen) {
    background: rgba(235, 219, 178, 0.03);
  }
  .msg.focus {
    box-shadow: inset 2px 0 0 var(--aqua);
  }
  .msg.chosen {
    background: var(--aqua-tint);
  }
  .msg.chosen .txt {
    color: var(--tx-body);
  }
  .tick {
    position: absolute;
    left: -14px;
    top: 7px;
    font-size: 11px;
    color: var(--aqua);
  }
  .t {
    color: var(--tx-label);
  }
  .who {
    color: var(--tx-body);
    font-weight: 500;
  }
  .txt {
    color: var(--tx-2);
  }
  .info {
    padding-bottom: 6px;
    font-size: 12px;
    color: var(--tx-muted-2);
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

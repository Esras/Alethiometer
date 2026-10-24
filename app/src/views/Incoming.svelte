<script lang="ts">
  // Incoming (8a): the triage queue. Left: topics with new signals, Unsorted last.
  // Right: one panel with the selected topic's new signals, grouped by app.
  // Confirm links the signal for good; Wrong topic only records that the link was
  // wrong (the item drops into Unsorted, nothing is deleted, no reason asked).
  // Unsorted only: ↵ on a signal with several messages enters its message list
  // (j/k move, space selects, shift+j/k selects a range like text, t files the selected messages under a topic as a
  // new item, the rest stays; Esc leaves). Triage elsewhere is unchanged.
  // A signal the AI split across topics has a `split` pill and ⋯ where its
  // other parts came in; `s` (or a click on either) opens the split review
  // (15g/15h). `s` also opens it on any Slack signal with more than one
  // message, split or not (user decision 2026-10-01), so a mixed DM can be
  // split under its topic without going through Unsorted. Names are in their person colour (16c);
  // someone without a People note is underlined and clicking them links an
  // account to a person (17b); a frequent unknown sender gets a New person
  // strip under the messages (17a: p create note, l link, Not now).
  // Agents (2a–2c, app/docs/agents.md): agent signals are one more app group,
  // after GitHub, oldest first; Agents waiting lists the ones waiting on you
  // across topics. Open, 1–9 pick the agent's answers, r replies, o opens it
  // in Agents and dismisses it here. An answered row keeps a check until you move off it.
  // Answering (app/src-tauri/src/queries/respond.rs) never triages: an
  // invite's second line answers it (⌥Y accept, ⌥N decline, ⌥⇧N decline with
  // a note; 14a), and `r` on a Slack signal writes a reply (⌥T thread or
  // conversation, ⌘↵ sends).
  import { call, openExternal } from '../lib/api';
  import { live as liveQuery } from '../lib/live';
  import { onDestroy, tick, untrack } from 'svelte';
  import { catColor, category, refresh, setTab, ui } from '../lib/app.svelte';
  import { alt, chord, plain, useKeys } from '../lib/keys.svelte';
  import { extend, shiftStep, type Range } from '../lib/range';
  import { isChat, sourceLabel } from '../lib/sources';
  import { messageUrl, newest } from '../lib/slackLink';
  import { toast } from '../lib/toast.svelte';
  import type { Account } from '../lib/people';
  import AppIcon from '../lib/ui/AppIcon.svelte';
  import Dock from '../lib/ui/Dock.svelte';
  import DockHost from '../lib/ui/DockHost.svelte';
  import KeySheet, { type KeySection } from '../lib/ui/KeySheet.svelte';
  import { popover } from '../lib/ui/popover';
  import Markdown from '../lib/ui/Markdown.svelte';
  import PersonName from '../lib/ui/PersonName.svelte';
  import AssistantPane from '../shell/AssistantPane.svelte';
  import { agentColor } from './agents/look';
  import { ag, look, openThread } from './agents/state.svelte';
  import StatusMark from '../lib/ui/StatusMark.svelte';
  import type { Request as AgentRequest, Session as AgentSession, Tree as AgentTree } from './agents/types';
  import AgentPart from './incoming/AgentPart.svelte';
  import PersonPicker from './incoming/PersonPicker.svelte';
  import RsvpLine, { answerable as canAnswer } from './incoming/RsvpLine.svelte';
  import ChatReply from './incoming/ChatReply.svelte';
  import SplitReview from './incoming/SplitReview.svelte';
  import TopicPicker from './incoming/TopicPicker.svelte';
  import { nextKey, putBack, stillDismissed } from './incoming/nextKey';
  import type { Bodies, IgnoreSuggestion, Message, Queue, QueueKey, Signal } from './incoming/types';
  import TodoForm from './todos/TodoForm.svelte';
  import type { Draft, JiraTarget, TodosData } from './todos/types';

  // Fixed app order so each app's group always sits in the same place.
  const APP_ORDER = ['slack', 'teams', 'gmail', 'gcal', 'jira', 'github', 'agents', 'obsidian', 'sheets', 'notion'];

  let queue = $state<Queue | null>(null);
  let selKey = $state<QueueKey | null>(null);
  let selItem = $state<number | null>(null);
  let expanded = $state<Set<number>>(new Set());
  // Expanded rows' messages, each its own live query: the list leaves them out
  // (an Inbox of Gmail threads is megabytes of text nobody has opened).
  let bodies = $state<Map<number, Bodies>>(new Map());
  let fullContext = $state<Set<number>>(new Set());
  let todoFor = $state<number | null>(null);
  let jira = $state<JiraTarget | null>(null);
  call<TodosData>('todos.list').then((d) => (jira = d.jira));
  let picking = $state(false);
  // Split review (15g/15h) open on this item. Until it closes the row stays put
  // and the family's other parts stay out of the list (`holdReview`): the split
  // happens in the review, and only its confirm files the parts.
  let reviewing = $state<{ item: number; root: number } | null>(null);
  let lastList: Queue | null = null;
  // Link to person (17b) open on this signal for this account.
  let linking = $state<{ item: number; account: Account; name: string } | null>(null);
  // Message mode (Unsorted): focus is inside the current signal's messages.
  let inMsgs = $state(false);
  let msgIdx = $state(0);
  let chosen = $state<Set<number>>(new Set());
  let dismissed = $state<Set<number>>(new Set());
  let trashed = $state<Set<number>>(new Set());
  let busy = false;
  // Agents: the live sessions and requests behind agent signals, and rows answered here.
  let live = $state<{ sessions: AgentSession[]; requests: AgentRequest[] } | null>(null);
  let answered = $state<Map<number, string>>(new Map());
  let parts = $state<Record<number, AgentPart>>({});
  let rsvpLines = $state<Record<number, RsvpLine>>({});
  let replies = $state<Record<number, ChatReply>>({});
  /** An invite that can still be answered here. */
  const answerable = (s: Signal | null) => !!s && !dismissed.has(s.item_id) && canAnswer(s.rsvp);

  type Entry = { key: QueueKey; name: string; category: string | null; count: number };
  const entries = $derived<Entry[]>(
    queue
      ? [
          ...queue.topics.map((t) => ({
            key: t.id as QueueKey,
            name: t.name,
            category: t.category,
            count: t.signals.length,
          })),
          { key: 'unsorted' as QueueKey, name: 'Unsorted', category: null, count: queue.unsorted.length },
          ...(queue.agents.length || selKey === 'agents'
            ? [{ key: 'agents' as QueueKey, name: 'Agents waiting', category: null, count: queue.agents.length }]
            : []),
        ]
      : [],
  );

  function signalsFor(q: Queue | null, key: QueueKey | null): Signal[] {
    if (!q || key === null) return [];
    if (key === 'unsorted') return q.unsorted;
    if (key === 'agents') return q.agents;
    return q.topics.find((t) => t.id === key)?.signals ?? [];
  }

  function grouped(sigs: Signal[], key: QueueKey | null = selKey) {
    // Agents waiting: by project, the way a topic groups by app.
    if (key === 'agents') {
      const byProject: { source: string; label: string; signals: Signal[] }[] = [];
      for (const s of sigs) {
        const label = s.agent?.project || s.agent?.cwd || 'Agents';
        const g = byProject.find((g) => g.label === label);
        if (g) g.signals.push(s);
        else byProject.push({ source: 'agents', label, signals: [s] });
      }
      return byProject;
    }
    const rank = (s: string) => (APP_ORDER.indexOf(s) + APP_ORDER.length + 1) % (APP_ORDER.length + 1);
    const groups: { source: string; label: string; signals: Signal[] }[] = [];
    for (const s of sigs) {
      const g = groups.find((g) => g.source === s.source);
      if (g) g.signals.push(s);
      else groups.push({ source: s.source, label: sourceLabel(s.source), signals: [s] });
    }
    // Agents read oldest first, like a conversation.
    for (const g of groups) if (g.source === 'agents') g.signals.reverse();
    return groups.sort((a, b) => rank(a.source) - rank(b.source));
  }

  /** A signal as shown: with its messages once its row is expanded. */
  const shown = (s: Signal): Signal => {
    const b = bodies.get(s.item_id);
    return b ? { ...s, messages: b.messages, context: b.context, field_changes: b.field_changes } : s;
  };
  const groups = $derived(grouped(signalsFor(queue, selKey).map(shown)));
  const flat = $derived(groups.flatMap((g) => g.signals));
  const current = $derived(flat.find((s) => s.item_id === selItem) ?? null);
  const unsorted = $derived(selKey === 'unsorted');
  const topicName = $derived(entries.find((e) => e.key === selKey)?.name ?? '');
  const splittable = (s: Signal | null) =>
    unsorted && !!s && s.source !== 'gmail' && s.source !== 'jira' && s.n_messages > 1;

  // Kept current by the core (lib/live.ts): syncs, agents and writes here
  // all show up on their own.
  $effect(() => liveQuery<Queue>('incoming.list', {}, apply));

  // One live query per expanded row still listed (agent signals come with
  // theirs); dropped when the row closes or leaves.
  const bodySubs = new Map<number, () => void>();
  $effect(() => {
    const q = queue;
    const listed = new Set(
      q
        ? [...q.topics.flatMap((t) => t.signals), ...q.unsorted]
            .filter((s) => s.source !== 'agents')
            .map((s) => s.item_id)
        : [],
    );
    const want = new Set([...expanded].filter((id) => listed.has(id)));
    untrack(() => {
      for (const [id, off] of bodySubs) {
        if (want.has(id)) continue;
        off();
        bodySubs.delete(id);
        if (bodies.has(id)) {
          const next = new Map(bodies);
          next.delete(id);
          bodies = next;
        }
      }
      for (const id of want) {
        if (bodySubs.has(id)) continue;
        const off = liveQuery<Bodies>('incoming.messages', { item_id: id }, (b) => {
          bodies = new Map(bodies).set(id, b);
          keepMsgs(b);
        });
        bodySubs.set(id, off);
      }
    });
  });
  onDestroy(() => bodySubs.forEach((off) => off()));

  const hasAgents = (q: Queue | null) =>
    !!q && (q.agents.length > 0 || [...q.unsorted, ...q.topics.flatMap((t) => t.signals)].some((s) => s.agent));
  function loadLive() {
    call<AgentTree>('agents.tree')
      .then((t) => (live = { sessions: t.sessions, requests: t.requests }))
      .catch(() => {});
  }
  // While an agent row is open its request can still change (Explained arrives): keep it fresh.
  $effect(() => {
    if (!current?.agent || !expanded.has(current.item_id)) return;
    const t = setInterval(loadLive, 1500);
    return () => clearInterval(t);
  });

  function openInAgents(s: Signal | null) {
    const a = s?.agent;
    if (!a) return false;
    const key = a.thread?.startsWith('r:') ? a.thread : `s:${a.session}`;
    setTab('agents');
    ag.sel = key;
    openThread(key);
    dismiss(s);
    return true;
  }

  function answer(s: Signal, label: string) {
    answered = new Map(answered).set(s.item_id, label);
    dismissed = new Set(dismissed).add(s.item_id);
    selItem = s.item_id;
    // Out of Incoming now; the check stays until you move off it.
    call('incoming.dismiss', { item_id: s.item_id, ...saw(s) }).catch(() => {});
    loadLive();
  }

  // Keep the selection where it was: same topic and item if they still exist,
  // otherwise the next topic left and the row that moved into the same place.
  function apply(q: Queue) {
    lastList = structuredClone(q);
    const prevEntries = entries;
    let prevFlat = flat;
    // Keep the topic order stable while triaging: topics already listed stay put,
    // topics that just got their first new signal go at the end, above Unsorted.
    const known = queue?.topics.map((t) => t.id) ?? [];
    const pos = (id: number) => {
      const i = known.indexOf(id);
      return i < 0 ? known.length : i;
    };
    holdReview(q);
    keepDismissed(q);
    q.topics.sort((a, b) => pos(a.id) - pos(b.id));
    queue = q;
    if (hasAgents(q)) loadLive();
    const listed = new Map(
      [...q.topics.flatMap((t) => t.signals), ...q.unsorted, ...q.agents].map((s) => [s.item_id, s.rev]),
    );
    dismissed = stillDismissed(dismissed, dismissedRev, listed);
    const present = new Set(listed.keys());
    if ([...ignoreHint.keys()].some((id) => !present.has(id)))
      ignoreHint = new Map([...ignoreHint].filter(([id]) => present.has(id)));
    if ([...answered.keys()].some((id) => !present.has(id)))
      answered = new Map([...answered].filter(([id]) => present.has(id)));
    const keys: QueueKey[] = [
      ...q.topics.map((t) => t.id),
      'unsorted',
      ...(q.agents.length || selKey === 'agents' ? ['agents' as QueueKey] : []),
    ];
    if (selKey === null || !keys.includes(selKey)) {
      selKey = nextKey(
        prevEntries.map((e) => e.key),
        selKey,
        keys,
      )!;
      selItem = null;
      prevFlat = [];
    }
    const now = grouped(signalsFor(q, selKey), selKey).flatMap((g) => g.signals);
    if (!now.some((s) => s.item_id === selItem)) {
      const before = prevFlat.map((s) => s.item_id);
      const left = now.map((s) => s.item_id).filter((id) => before.includes(id));
      selItem =
        (prevFlat.length ? nextKey(before, selItem, left) : now[0]?.item_id) ?? now[now.length - 1]?.item_id ?? null;
    }
    // A sync may have changed the messages under an open selection.
    const sig = now.find((s) => s.item_id === selItem);
    if (selKey !== 'unsorted' || !sig || sig.n_messages < 2) leaveMsgs();
    else keepMsgs(bodies.get(sig.item_id));
  }

  /** Keep the message selection to messages the selected row still shows. */
  function keepMsgs(b: Bodies | undefined) {
    if (!b || b.item_id !== selItem) return;
    chosen = new Set([...chosen].filter((id) => b.messages.some((m) => m.id === id)));
    msgIdx = Math.max(0, Math.min(msgIdx, b.messages.length - 1));
  }

  function holdReview(q: Queue) {
    if (!reviewing) return;
    const { item: id, root } = reviewing;
    const out = (l: Signal[]) => l.filter((s) => s.item_id === id || s.root !== root);
    for (const t of q.topics) t.signals = out(t.signals);
    q.topics = q.topics.filter((t) => t.signals.length);
    q.unsorted = out(q.unsorted);
    keepRow(q, id);
  }

  function keepDismissed(q: Queue) {
    const id = selItem;
    if (!queue || id === null || !dismissed.has(id)) return;
    if (selKey === 'agents') {
      if (!q.agents.some((s) => s.item_id === id) && queue.agents.some((s) => s.item_id === id)) {
        putBack(q.agents, queue.agents, id);
      }
      return;
    }
    keepRow(q, id);
  }

  /** Put a row that left the list back where it was. */
  function keepRow(q: Queue, id: number) {
    if (!queue) return;
    if ([...q.topics.flatMap((t) => t.signals), ...q.unsorted].some((s) => s.item_id === id)) return;
    const put = (list: Signal[], from: Signal[]) => putBack(list, from, id);
    if (queue.unsorted.some((s) => s.item_id === id)) return put(q.unsorted, queue.unsorted);
    const t = queue.topics.find((t) => t.signals.some((s) => s.item_id === id));
    if (!t) return;
    const nt = q.topics.find((x) => x.id === t.id);
    if (nt) put(nt.signals, t.signals);
    else q.topics.push({ ...t, signals: t.signals.filter((s) => s.item_id === id) });
  }

  function leaveMsgs() {
    inMsgs = false;
    msgIdx = 0;
    run = null;
    if (chosen.size) chosen = new Set();
  }

  // A shift+j/k run in Unsorted's message mode; any other key ends it.
  let run: Range | null = null;

  function enterMsgs(s: Signal, idx = 0) {
    if (selItem !== s.item_id) leaveMsgs();
    selItem = s.item_id;
    if (!expanded.has(s.item_id)) toggle(s.item_id);
    inMsgs = true;
    msgIdx = idx;
  }

  function stepMsg(d: number, extending = false) {
    if (!current) return;
    const to = Math.max(0, Math.min(current.messages.length - 1, msgIdx + d));
    if (extending) {
      const ids = current.messages.map((m) => m.id);
      const r = extend(ids, run, ids[msgIdx], ids[to], chosen);
      run = r.run;
      chosen = r.chosen;
    } else run = null;
    msgIdx = to;
  }

  function choose(id: number) {
    run = null;
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

  /** The split review applies: an AI-split signal, or any Slack signal with several messages. */
  const reviewable = (s: Signal | null) => !!s && (s.split || (isChat(s.source) && s.n_messages > 1));

  function review(s: Signal | null) {
    if (!s || !reviewable(s) || dismissed.has(s.item_id)) return;
    selItem = s.item_id;
    picking = false;
    linking = null;
    leaveMsgs();
    reviewing = { item: s.item_id, root: s.root };
  }

  $effect(() => {
    if (reviewing && !flat.some((s) => s.item_id === reviewing!.item)) untrack(closeReview);
  });

  function closeReview() {
    reviewing = null;
    if (lastList) apply(lastList);
  }

  function endReview() {
    closeReview();
    refresh();
  }

  function link(s: Signal, account: Account, name: string) {
    selItem = s.item_id;
    picking = false;
    linking = { item: s.item_id, account, name };
  }

  function linkFrom(s: Signal, m: Message) {
    if (m.who.account) link(s, m.who.account, m.author);
  }

  function newPersonNote(s: Signal | null) {
    const np = s?.new_person;
    if (!s || !np || !expanded.has(s.item_id)) return;
    // The note is named after the profile's real name when the source has one.
    act('people.create', { name: np.real_name ?? np.name, alias: np.name, kind: np.kind, account: np.account });
  }

  function newPersonLink(s: Signal | null) {
    const np = s?.new_person;
    if (!s || !np || !expanded.has(s.item_id)) return;
    link(s, { kind: np.kind, id: np.account }, np.name);
  }

  function selectTopic(key: QueueKey) {
    if (key === selKey) return;
    selKey = key;
    picking = false;
    leaveMsgs();
    selItem = grouped(signalsFor(queue, key), key).flatMap((g) => g.signals)[0]?.item_id ?? null;
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

  async function jumpEnd(s: Signal | null, end: boolean) {
    if (!s) return;
    if (!expanded.has(s.item_id)) toggle(s.item_id);
    if (inMsgs && s.messages.length) msgIdx = end ? s.messages.length - 1 : 0;
    await tick();
    document
      .querySelector(`[data-item="${s.item_id}"]`)
      ?.scrollIntoView({ block: end ? 'end' : 'start', behavior: 'smooth' });
  }

  function toggle(id: number) {
    const next = new Set(expanded);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    expanded = next;
  }

  // The last Confirm, Move or Wrong, kept for `u` until the next one: the row has left the list, and the view may have moved on.
  type Filed = {
    text: string;
    itemId: number;
    key: QueueKey | null;
    moved: unknown;
    from: number | null;
    to: number;
    gmail: boolean;
    /** The triage event the filing recorded (`queries/ignore.rs`), forgotten on `u`. */
    event: number | null;
  };
  let filed = $state<Filed | null>(null);
  // The last dismissal, so `u` still undoes it once its row has left the list.
  let lastDis = $state<{ id: number; title: string; key: QueueKey | null } | null>(null);
  const disGone = $derived(lastDis && !flat.some((s) => s.item_id === lastDis!.id) ? lastDis : null);

  async function undoGone() {
    const d = disGone;
    if (!d || busy) return;
    lastDis = null;
    dismissedRev.delete(d.id);
    await act('incoming.undismiss', {
      item_id: d.id,
      unread: unreadBefore.get(d.id) ?? [],
      event: dismissEvent.get(d.id) ?? null,
    });
    dismissEvent.delete(d.id);
    if (d.key !== null) selKey = d.key;
    selItem = d.id;
  }
  const topicNamed = (id: number) => entries.find((e) => e.key === id)?.name ?? 'topic';

  async function unfile() {
    const f = filed;
    if (!f || busy) return;
    filed = null;
    await act('incoming.unfile', {
      moved: f.moved,
      from_topic_id: f.from,
      to_topic_id: f.to,
      filed: f.gmail,
      item_id: f.itemId,
      event: f.event,
    });
    if (f.key !== null) selKey = f.key;
    selItem = f.itemId;
  }

  type Acted = { moved: unknown; event?: number | null };
  async function act(cmd: string, args: Record<string, unknown>, after?: (r: Acted) => void) {
    if (busy) return;
    busy = true;
    try {
      const r = await call<Acted>(cmd, args);
      after?.(r);
      refresh();
    } catch (e) {
      toast(e);
    } finally {
      busy = false;
    }
  }

  $effect(() => {
    const id = selItem;
    if (untrack(() => [...dismissed].some((d) => d !== id)))
      untrack(() => {
        refresh();
        if (lastList) apply(structuredClone(lastList));
      });
  });

  // The unread messages a dismissal marked read, so `u` makes the same ones unread.
  const unreadBefore = new Map<number, string[]>();
  const dismissedRev = new Map<number, number>();
  // The triage event a dismissal recorded, forgotten on `u`, and what it suggests ignoring.
  const dismissEvent = new Map<number, number | null>();
  let ignoreHint = $state<Map<number, IgnoreSuggestion>>(new Map());
  let ignoreDone = $state<Map<number, string>>(new Map());
  async function ignoreAct(id: number, h: IgnoreSuggestion) {
    const o = h.offer;
    const done = (text: string) => (ignoreDone = new Map(ignoreDone).set(id, text));
    try {
      if (o.type === 'copy') {
        await navigator.clipboard?.writeText(o.text);
        done(`Copied ${o.text} for a Gmail filter`);
      } else if (o.type === 'mute') {
        await call('settings.mute', { kind: h.source_kind, name: h.source_name, entry: o.entry });
        done(`Muted in ${h.source_name}`);
      } else openExternal(o.url);
    } catch (e) {
      toast(e);
    }
  }
  function notNow(id: number, h: IgnoreSuggestion) {
    setHint(id, null);
    call('incoming.ignore_not_now', { source_id: h.source_id, kind: h.kind, key: h.key }).catch(() => {});
  }
  const setHint = (id: number, s: IgnoreSuggestion | null) => {
    const next = new Map(ignoreHint);
    if (s) next.set(id, s);
    else next.delete(id);
    ignoreHint = next;
  };

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
    if (on) dismissedRev.set(s.item_id, s.rev);
    if (on) lastDis = { id: s.item_id, title: s.title, key: selKey };
    else if (lastDis?.id === s.item_id) lastDis = null;
    else dismissedRev.delete(s.item_id);
    try {
      const r = await call<{ unread?: string[]; event?: number | null; suggestion?: IgnoreSuggestion | null }>(
        on ? (trash ? 'incoming.trash' : 'incoming.dismiss') : 'incoming.undismiss',
        {
          item_id: s.item_id,
          ...(on ? saw(s) : { unread: unreadBefore.get(s.item_id) ?? [], event: dismissEvent.get(s.item_id) ?? null }),
        },
      );
      if (on) {
        unreadBefore.set(s.item_id, r.unread ?? []);
        dismissEvent.set(s.item_id, r.event ?? null);
      } else dismissEvent.delete(s.item_id);
      setHint(s.item_id, on ? (r.suggestion ?? null) : null);
    } catch (e) {
      flip(!on);
      toast(e);
    }
  }

  /** What you saw of a signal: triage applies to that, and newer messages stay new. */
  // What the row showed: an expanded row's own messages once they're in
  // (they can be older or newer than the list's), else the list's.
  const saw = (s: Signal) => {
    const b = expanded.has(s.item_id) ? bodies.get(s.item_id) : undefined;
    return b ? { rev: b.rev, keys: b.keys } : { rev: s.rev, keys: s.keys };
  };

  /** The topic a signal is listed under: the selected one, or (Agents waiting) its own. */
  const topicOf = (s: Signal) =>
    typeof selKey === 'number' ? selKey : selKey === 'agents' ? (s.topic_id ?? null) : null;

  function confirm(s: Signal | null) {
    if (!s || dismissed.has(s.item_id)) return;
    const topic = topicOf(s);
    // Agents waiting, not filed yet: confirming means picking its topic.
    if (topic === null) return selKey === 'agents' ? pick(s) : undefined;
    selItem = s.item_id;
    const key = selKey;
    act('incoming.confirm', { item_id: s.item_id, topic_id: topic, ...saw(s) }, (r) => {
      filed = {
        text: `Filed to ${topicNamed(topic)}`,
        itemId: s.item_id,
        key,
        moved: r.moved,
        from: null,
        to: topic,
        gmail: true,
        event: r.event ?? null,
      };
    });
  }

  function wrong(s: Signal | null) {
    if (!s || dismissed.has(s.item_id)) return;
    const topic = topicOf(s);
    if (topic === null) return;
    selItem = s.item_id;
    const key = selKey;
    act('incoming.wrong', { item_id: s.item_id, topic_id: topic }, (r) => {
      filed = {
        text: `Not ${topicNamed(topic)}`,
        itemId: s.item_id,
        key,
        moved: r.moved,
        from: null,
        to: topic,
        gmail: false,
        event: null,
      };
    });
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
    const s = current;
    const from = topicOf(s);
    const key = selKey;
    act('incoming.move', { item_id: s.item_id, topic_id: topicId, from_topic_id: from, ...saw(s) }, (r) => {
      filed = {
        text: `Filed to ${topicNamed(topicId)}`,
        itemId: s.item_id,
        key,
        moved: r.moved,
        from,
        to: topicId,
        gmail: true,
        event: r.event ?? null,
      };
    });
  }

  function openTodo(s: Signal | null) {
    if (!s || dismissed.has(s.item_id)) return;
    selItem = s.item_id;
    picking = false;
    todoFor = s.item_id;
  }

  function hold(s: Signal, d: Draft) {
    todoFor = null;
    act('todos.hold', { item_id: s.item_id, target: d.target, text: d.text, due: d.due, sched: d.sched });
  }

  /** `meta`-style text split at the author's name, so the name can be a PersonName. */
  function byAuthor(s: Signal, text: string): string | null {
    const a = s.author;
    return a && text.startsWith(`${a.name} · `) ? text.slice(a.name.length) : null;
  }

  function info(s: Signal) {
    const shown = new Set(s.meta.split(' · '));
    const extra = (s.summary ?? '').split(' · ').filter((p) => p && !shown.has(p));
    return [s.meta, ...extra].filter(Boolean).join(' · ');
  }

  /** `r` on a chat signal: its messages open, the reply field under them. */
  function replyTo(s: Signal) {
    selItem = s.item_id;
    if (!expanded.has(s.item_id)) toggle(s.item_id);
    tick().then(() => replies[s.item_id]?.open());
  }

  function open(s: Signal | null) {
    if (!s?.url) return;
    const [m, reply] = inMsgs ? [s.messages[msgIdx], undefined] : newest(s.messages);
    openExternal(messageUrl(s.url, m, reply));
  }

  let keysOpen = $state(false);
  let acting = $state<number | null>(null);
  const conversationOf = (s: Signal) =>
    [' · thread', ' · mention'].reduce((t, cut) => (t.includes(cut) ? t.slice(0, t.indexOf(cut)) : t), s.title);

  async function ignore(s: Signal) {
    acting = null;
    try {
      await call('settings.mute_signal', { item_id: s.item_id });
      toast(
        `Ignoring ${conversationOf(s)}: its new messages won't come to Incoming. Settings › ${sourceLabel(s.source)} › Ignored undoes it.`,
        true,
      );
    } catch (e) {
      toast(e);
    }
  }
  const KEYS: KeySection[] = [
    {
      title: 'Everywhere',
      keys: [
        [['[', ']'], 'Previous or next topic'],
        [[chord('⌘K', 'Ctrl+K')], 'Search everything'],
        [['?'], 'This sheet'],
      ],
    },
    {
      title: 'Signals',
      keys: [
        [['J', 'K'], 'Move'],
        [['G', '⇧G'], 'Top or end of the signal'],
        [['↵'], 'Expand; in Unsorted, pick its messages'],
        [['Esc'], 'Collapse'],
        [['O'], 'Open in its app (agents: in Agents, and dismiss)'],
      ],
    },
    {
      title: 'Triage',
      keys: [
        [['C'], 'Confirm the topic'],
        [['X'], 'Wrong topic'],
        [['T'], 'Move to a topic'],
        [['D'], 'Dismiss'],
        [['U'], 'Undo'],
        [['N'], 'Hold a todo, made when it’s filed'],
      ],
    },
    {
      title: 'Unsorted messages',
      keys: [
        [['J', 'K'], 'Move'],
        [['Space'], 'Select'],
        [['⇧J', '⇧K'], 'Select a range'],
        [['T'], 'Move the selected, or the whole signal'],
        [['↵', 'Esc'], 'Collapse, or done'],
      ],
    },
    {
      title: 'Slack and Teams',
      keys: [
        [['R'], 'Reply'],
        [[chord('⌥T', 'Alt+T')], 'Thread or conversation'],
        [['S'], 'Split review'],
        [['.'], 'More: ignore the channel or DM'],
      ],
    },
    {
      title: 'Gmail and Calendar',
      keys: [
        [['#'], 'Trash (Gmail)'],
        [[chord('⌥Y', 'Alt+Y')], 'Accept'],
        [[chord('⌥N', 'Alt+N')], 'Decline'],
        [[chord('⌥⇧N', 'Alt+Shift+N')], 'Decline with a note'],
      ],
    },
    {
      title: 'Agents',
      keys: [
        [['1', '…', '9'], 'Answer (open)'],
        [['R'], 'Reply'],
      ],
    },
    {
      title: 'New person',
      keys: [
        [['P'], 'Create their note'],
        [['L'], 'Link to someone'],
      ],
    },
  ];

  useKeys((e) => {
    if (picking || todoFor !== null || linking || reviewing !== null) return;
    if (acting !== null) {
      const s = flat.find((x) => x.item_id === acting);
      if (plain(e, 'i') && s) return (ignore(s), true);
      acting = null;
      if (plain(e, '.')) return true;
    }
    if (plain(e, '.') && current && isChat(current.source)) return ((acting = current.item_id), true);
    if (plain(e, '?')) return ((keysOpen = true), true);
    if (inMsgs && current) {
      const shift = shiftStep(e);
      if (shift) return (stepMsg(shift, true), true);
      if (plain(e, 'j') || plain(e, 'ArrowDown')) return (stepMsg(1), true);
      if (plain(e, 'k') || plain(e, 'ArrowUp')) return (stepMsg(-1), true);
      if (plain(e, ' ')) return (current.messages[msgIdx] && choose(current.messages[msgIdx].id), true);
      if (plain(e, 'Escape')) return (leaveMsgs(), true);
    }
    if (plain(e, 'G')) return (jumpEnd(current, true), true);
    if (plain(e, 'g')) return (jumpEnd(current, false), true);
    if (plain(e, 'j') || plain(e, 'ArrowDown')) return (stepSignal(1), true);
    if (plain(e, 'k') || plain(e, 'ArrowUp')) return (stepSignal(-1), true);
    if (plain(e, ']')) return (stepTopic(1), true);
    if (plain(e, '[')) return (stepTopic(-1), true);
    if (plain(e, 'Enter')) return (enter(current), true);
    // An open agent signal: its answers, a reply, the session in Agents.
    if (current?.agent && !dismissed.has(current.item_id)) {
      const part = parts[current.item_id];
      if (expanded.has(current.item_id) && /^[1-9]$/.test(e.key) && plain(e, e.key) && part?.choose(e.key)) return true;
      if (plain(e, 'r')) {
        if (!expanded.has(current.item_id)) toggle(current.item_id);
        tick().then(() => parts[current.item_id]?.reply());
        return true;
      }
      if (plain(e, 'o')) return (openInAgents(current), true);
    }
    // Answering, which never triages.
    if (current && answerable(current)) {
      const line = rsvpLines[current.item_id];
      if (alt(e, 'y')) return (line?.accept(), true);
      if (alt(e, 'n')) return (line?.decline(), true);
      if (alt(e, 'n', true)) return (line?.declineWithNote(), true);
    }
    if (current && isChat(current.source) && !dismissed.has(current.item_id) && plain(e, 'r'))
      return (replyTo(current), true);
    if (plain(e, 'Escape') && current && expanded.has(current.item_id)) return (toggle(current.item_id), true);
    if (plain(e, 'c')) return (confirm(current), true);
    if (plain(e, 'x')) return (wrong(current), true);
    if (plain(e, 't')) return (pick(current), true);
    if (plain(e, 'd')) return (dismiss(current), true);
    if (plain(e, 'u')) {
      if (filed && !current?.held && !(current && dismissed.has(current.item_id))) unfile();
      else if (current?.held && !dismissed.has(current.item_id)) act('todos.unhold', { item_id: current.item_id });
      else if (disGone && !(current && dismissed.has(current.item_id))) undoGone();
      else dismiss(current, false);
      return true;
    }
    if (plain(e, 'n')) return (openTodo(current), true);
    if (plain(e, '#')) return (dismiss(current, true, true), true);
    if (plain(e, 'o')) return (open(current), true);
    if (plain(e, 's')) return (review(current), true);
    if (plain(e, 'p')) return (newPersonNote(current), true);
    if (plain(e, 'l')) return (newPersonLink(current), true);
  });
</script>

{#snippet metaText(s: Signal, text: string)}
  {@const rest = byAuthor(s, text)}
  {#if rest !== null && s.author}<span class="who-meta"
      ><PersonName
        who={s.author.who}
        name={s.author.name}
        variant="tertiary"
        onlink={s.author.who.account ? () => link(s, s.author!.who.account!, s.author!.name) : undefined}
      /></span
    >{rest}{:else}{text}{/if}
{/snippet}

{#snippet changesAt(s: Signal, at: number)}
  {#each (s.field_changes ?? []).filter((c) => c.after === at) as c (c.key)}
    <div class="msg change" class:context={c.read}>
      <span class="t tnum">{c.time}</span>
      <PersonName who={c.who} name={c.author} />
      <div class="txt">{c.text}</div>
    </div>
  {/each}
{/snippet}

<div class="view">
  <div class="topics scroll">
    <div class="label head">Topics</div>
    {#each entries as t (t.key)}
      {#if t.key === 'unsorted'}<div class="fade-rule"></div>{/if}
      <button
        class="topic"
        data-topic-key={t.key}
        class:on={t.key === selKey}
        class:unsorted={t.key === 'unsorted' || t.key === 'agents'}
        onclick={() => selectTopic(t.key)}
      >
        <span class="bar" style:background={t.category ? catColor(t.category) : null}></span>
        <span class="name ellipsis" title={t.name}>
          {#if t.key === 'unsorted'}<i class="ph ph-shuffle"></i>{:else if t.key === 'agents'}<i class="ph ph-robot"
            ></i>{/if}{t.name}
        </span>
        <span class="count tnum" class:on={t.key === selKey}>{t.count}</span>
      </button>
    {/each}
  </div>

  <div class="main">
    <div class="panel col">
      <div class="body scroll">
        {#if filed}
          <div class="filed">
            <i class="ph ph-check"></i>{filed.text}<button class="btn" onclick={unfile}>Undo <kbd>u</kbd></button>
          </div>
        {:else if disGone}
          <div class="filed">
            <i class="ph ph-check"></i>Dismissed {disGone.title}<button class="btn" onclick={undoGone}
              >Undo <kbd>u</kbd></button
            >
          </div>
        {/if}
        {#each groups as g, gi (g.source + g.label)}
          <div class="group panel-head" class:rule={gi > 0}>
            <AppIcon source={g.source} size={14} />{g.label}<span class="n">{g.signals.length}</span>
          </div>
          {#each g.signals as s (s.item_id)}
            {@const sel = s.item_id === selItem}
            {@const isOpen = expanded.has(s.item_id)}
            {@const gone = dismissed.has(s.item_id)}
            {@const ag_ = s.agent}
            {@const agSession = ag_ ? live?.sessions.find((x) => x.id === ag_.session) : undefined}
            <div
              class="sig"
              class:sel
              class:inside={sel && (inMsgs || reviewing?.item === s.item_id)}
              class:gone={gone && !answered.has(s.item_id)}
              data-item={s.item_id}
              onclick={() => (selItem = s.item_id)}
              role="presentation"
            >
              {#if reviewing?.item === s.item_id}
                <SplitReview itemId={s.item_id} onclose={endReview} />
              {:else}
                <div class="row">
                  {#if ag_}<span class="agent-mark"
                      ><StatusMark status={agSession?.status ?? (ag_.waiting ? 'waiting' : 'idle')} /></span
                    >{/if}
                  <button
                    class="title-line"
                    onclick={() => toggle(s.item_id)}
                    title={isOpen ? 'Collapse (↵)' : 'Expand (↵)'}
                  >
                    {#if ag_}
                      <span class="title" class:ellipsis={!isOpen} title={s.title}
                        ><span
                          class="agent-name"
                          style:color={ag_.name.startsWith('#') ? 'var(--tx-2)' : agentColor(look(), agSession)}
                          >{ag_.name}</span
                        >
                        {ag_.reason}</span
                      >
                    {:else}
                      <span class="title" class:ellipsis={!isOpen} title={s.title}>{s.title}</span>
                    {/if}
                    {#if !isOpen}<span class="meta tnum">{@render metaText(s, s.meta)}</span>{/if}
                    {#if s.change}
                      <span
                        class="pill change"
                        class:red={s.change === 'Cancelled'}
                        class:green={s.change.startsWith('New')}
                        class:yellow={s.change !== 'Cancelled' && !s.change.startsWith('New')}>{s.change}</span
                      >
                    {/if}
                    {#if s.category}
                      <span
                        class="pill cat"
                        style:background={`color-mix(in oklch, ${catColor(s.category)} 22%, transparent)`}
                        style:color={catColor(s.category)}
                        title="Labeled {category(s.category).label} in Gmail">{category(s.category).label}</span
                      >
                    {/if}
                    {#if s.split}
                      <span
                        class="count split"
                        role="button"
                        tabindex="-1"
                        title="Split across topics: review it (s)"
                        onclick={(e) => (e.stopPropagation(), review(s))}
                        onkeydown={() => {}}><i class="ph ph-git-fork"></i>split</span
                      >
                    {/if}
                    {#if isOpen}<i class="ph ph-caret-up caret"></i>{/if}
                  </button>
                  <div class="actions">
                    {#if answered.has(s.item_id)}
                      <span class="answered"><i class="ph ph-check"></i>{answered.get(s.item_id)}</span>
                    {:else if gone}
                      {#if trashed.has(s.item_id)}
                        <span class="ghost"><i class="ph ph-trash"></i>Trashed</span>
                      {:else}
                        <span class="ghost"
                          ><i class="ph ph-eye-slash"></i>{s.source === 'gmail' ? 'Archived' : 'Dismissed'}</span
                        >
                      {/if}
                      <button class="btn" onclick={() => dismiss(s, false)}>Undo <kbd>u</kbd></button>
                    {:else}
                      <span class="src-actions">
                        {#if ag_ && (ag_.kind === 'wrote' || ag_.kind === 'turn')}
                          <button
                            class="btn"
                            onclick={(e) => (
                              e.stopPropagation(),
                              (selItem = s.item_id),
                              expanded.has(s.item_id) || toggle(s.item_id),
                              tick().then(() => parts[s.item_id]?.reply())
                            )}>Reply <kbd>r</kbd></button
                          >
                        {/if}
                        {#if ag_?.kind === 'done' && ag_.facts?.pr}
                          <button
                            class="btn"
                            onclick={(e) => (e.stopPropagation(), ag_.facts?.pr?.url && openExternal(ag_.facts.pr.url))}
                            ><i class="ph ph-git-pull-request"></i>{ag_.facts.pr.label}</button
                          >
                        {/if}
                        {#if isChat(s.source)}
                          <button class="btn" onclick={(e) => (e.stopPropagation(), replyTo(s))}
                            >Reply <kbd>r</kbd></button
                          >
                        {/if}
                        <button class="btn" onclick={() => openTodo(s)}>Todo… <kbd>n</kbd></button>
                        {#if s.source === 'gmail'}<button class="btn" onclick={() => dismiss(s, true, true)}
                            ><i class="ph ph-trash"></i>Trash <kbd>#</kbd></button
                          >{/if}
                      </span>
                      {#if unsorted || (selKey === 'agents' && s.topic_id == null)}
                        <button class="btn" onclick={() => pick(s)}>
                          {sel && chosen.size ? `Move ${chosen.size} of ${s.n_messages}` : 'Move to topic'}
                        </button>
                        <button class="btn" onclick={() => dismiss(s)}><i class="ph ph-eye-slash"></i>Dismiss</button>
                      {:else}
                        <button class="btn" onclick={() => confirm(s)}
                          >Confirm{#if s.held}&nbsp;+ create {s.held.target === 'jira' ? 'issue' : 'todo'}{/if}</button
                        >
                        <button class="btn" onclick={() => wrong(s)}>Wrong topic</button>
                        <button class="btn" onclick={() => dismiss(s)}><i class="ph ph-eye-slash"></i>Dismiss</button>
                      {/if}
                    {/if}
                  </div>
                </div>
                {#if s.rsvp && !gone}
                  <div class="answer">
                    <RsvpLine bind:this={rsvpLines[s.item_id]} itemId={s.item_id} rsvp={s.rsvp} />
                  </div>
                {/if}
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
                      {s.held.target === 'jira' ? 'Jira issue' : 'Todo'} held until you {unsorted
                        ? 'file the signal'
                        : 'confirm'} ·
                      <span class="htext">{s.held.text}</span>{#if s.held.due}&nbsp;· due {s.held
                          .due}{/if}{#if s.held.target === 'jira' && jira}&nbsp;· {jira.project} · {jira.issue_type} · you{/if}
                    </span>
                    <button class="btn" onclick={(e) => (e.stopPropagation(), openTodo(s))}>Edit</button>
                    <button
                      class="btn"
                      onclick={(e) => (e.stopPropagation(), act('todos.unhold', { item_id: s.item_id }))}
                      >Discard <kbd>u</kbd></button
                    >
                  </div>
                {/if}
                {#if gone && ignoreHint.has(s.item_id)}
                  {@const h = ignoreHint.get(s.item_id)!}
                  {@const done = ignoreDone.get(s.item_id)}
                  <div class="held ignore">
                    <i class="ph ph-funnel"></i>
                    <span class="ellipsis"
                      ><span class="htext">{h.label}</span>: you dismissed {h.dismissed} of {h.total} in the last {h.days}
                      days</span
                    >
                    {#if done}
                      <span class="htext">{done}</span>
                    {:else}
                      <button class="btn" onclick={(e) => (e.stopPropagation(), ignoreAct(s.item_id, h))}
                        >{#if h.offer.type === 'copy'}<i class="ph ph-copy"></i>Copy filter{:else if h.offer.type === 'mute'}Add
                          to muted{:else}<i class="ph ph-arrow-square-out"></i>Open {h.kind}{/if}</button
                      >
                      <button class="btn" onclick={(e) => (e.stopPropagation(), notNow(s.item_id, h))}>Not now</button>
                    {/if}
                  </div>
                {/if}
                {#if isOpen}
                  {@const hidden = fullContext.has(s.item_id) ? 0 : Math.max(0, s.context.length - 3)}
                  <div class="ctx">
                    {#if !(isChat(s.source) && s.n_messages <= 1)}
                      <div class="info tnum">{@render metaText(s, info(s))}</div>
                    {/if}
                    {#if s.description}<div class="desc"><Markdown text={s.description} /></div>{/if}
                    {#each s.context as m, ci (m.id)}
                      {#if hidden && ci === 1}
                        <button
                          class="link-toggle more"
                          onclick={(e) => (e.stopPropagation(), (fullContext = new Set(fullContext).add(s.item_id)))}
                        >
                          … {hidden} earlier
                        </button>
                      {/if}
                      {#if !hidden || ci === 0 || ci > hidden}
                        <div class="msg context" class:reply={s.thread && ci > 0}>
                          <span class="t tnum">{m.time}</span>
                          <PersonName who={m.who} name={m.author} />
                          <div class="txt"><Markdown text={m.text} tags={s.source !== 'slack'} /></div>
                        </div>
                      {/if}
                    {/each}
                    {#if s.messages.length}
                      {#each s.messages as m, mi (m.id)}
                        {@const canPick = splittable(s)}
                        {@render changesAt(s, mi)}
                        {#if m.gap}
                          <button
                            class="gap"
                            title="Other topics' messages came in here: review the split (s)"
                            onclick={(e) => (e.stopPropagation(), review(s))}>⋯</button
                          >
                        {/if}
                        <div
                          class="msg"
                          class:reply={s.thread}
                          class:context={m.read}
                          class:pickable={canPick}
                          class:focus={sel && inMsgs && mi === msgIdx}
                          class:chosen={sel && chosen.has(m.id)}
                          data-msg={m.id}
                          onclick={(e) => (e.stopPropagation(), clickMsg(s, mi))}
                          role="presentation"
                        >
                          {#if sel && chosen.has(m.id)}<i class="ph ph-check tick"></i>{/if}
                          <span class="t tnum">{m.time}</span>
                          <PersonName who={m.who} name={m.author} onlink={() => linkFrom(s, m)} />
                          <div class="txt"><Markdown text={m.text} tags={s.source !== 'slack'} /></div>
                        </div>
                        {@const replies = m.replies ?? []}
                        {@const readReplies = replies.filter((r) => r.read).length}
                        {@const hiddenReplies = fullContext.has(s.item_id) || readReplies <= 3 ? 0 : readReplies - 2}
                        {#if hiddenReplies}
                          <button
                            class="link-toggle more reply-more"
                            onclick={(e) => (e.stopPropagation(), (fullContext = new Set(fullContext).add(s.item_id)))}
                          >
                            … {hiddenReplies} earlier {hiddenReplies === 1 ? 'reply' : 'replies'}
                          </button>
                        {/if}
                        {#each replies.filter((r, ri) => !r.read || ri >= hiddenReplies) as r (r.id)}
                          <div class="msg reply" class:context={r.read} class:chosen={sel && chosen.has(m.id)}>
                            <span class="t tnum">{r.time}</span>
                            <PersonName who={r.who} name={r.author} onlink={() => linkFrom(s, r)} />
                            <div class="txt"><Markdown text={r.text} tags={s.source !== 'slack'} /></div>
                          </div>
                        {/each}
                      {/each}
                    {/if}
                    {@render changesAt(s, s.messages.length)}
                    {#if ag_ && !answered.has(s.item_id)}
                      <AgentPart
                        bind:this={parts[s.item_id]}
                        {s}
                        {live}
                        onDone={(label) => answer(s, label)}
                        onOpen={() => openInAgents(s)}
                      />
                    {/if}
                    {#if s.new_person && !gone}
                      {@const np = s.new_person}
                      <div class="new-person">
                        <i class="ph ph-user-plus"></i>
                        <span class="q">New person?</span>
                        <PersonName who={{ color: np.color, known: false, me: false }} name={np.real_name ?? np.name} />
                        <span class="np-info ellipsis">
                          {sourceLabel(np.source)}{#if np.email}&nbsp;· {np.email}{:else if np.kind === 'github'}&nbsp;· {np.account}{/if}
                          · in {np.signals} signals this week
                        </span>
                        <span class="np-actions">
                          <button class="btn primary" onclick={(e) => (e.stopPropagation(), newPersonNote(s))}
                            >Create note <kbd>p</kbd></button
                          >
                          <button class="btn" onclick={(e) => (e.stopPropagation(), newPersonLink(s))}
                            >Link to… <kbd>l</kbd></button
                          >
                          <button
                            class="btn"
                            onclick={(e) => (
                              e.stopPropagation(),
                              act('people.snooze', { kind: np.kind, account: np.account })
                            )}>Not now</button
                          >
                        </span>
                      </div>
                    {/if}
                    {#if s.url}
                      <div class="links">
                        {#if s.msg_count && s.msg_count > s.n_messages}
                          <button class="all" onclick={() => open(s)}>All {s.msg_count}</button>
                        {/if}
                        <button class="app" onclick={() => open(s)}>Open in {sourceLabel(s.source)}</button>
                      </div>
                    {/if}
                  </div>
                {/if}
                {#if isChat(s.source) && !gone}
                  <div class="answer">
                    <ChatReply
                      bind:this={replies[s.item_id]}
                      {s}
                      msg={sel && inMsgs ? s.messages[msgIdx] : undefined}
                    />
                  </div>
                {/if}
              {/if}
            </div>
          {/each}
        {:else}
          {#if queue}
            <div class="empty-pane">{unsorted ? 'Nothing unsorted.' : `Nothing new on ${topicName}.`}</div>
          {/if}
        {/each}
      </div>
      {#if linking && current?.item_id === linking.item}
        <Dock under=".sig.sel">
          <PersonPicker
            account={linking.account}
            name={linking.name}
            source={sourceLabel(current.source)}
            ondone={() => ((linking = null), refresh())}
            onclose={() => (linking = null)}
          />
        </Dock>
      {/if}
      {#if acting !== null && current?.item_id === acting}
        <Dock under=".sig.sel">
          <div class="more popover flush" role="menu" use:popover={{ onclose: () => (acting = null) }}>
            <button role="menuitem" onclick={() => current && ignore(current)}
              ><i class="ph ph-bell-slash"></i>Ignore {conversationOf(current)}<kbd>i</kbd></button
            >
          </div>
        </Dock>
      {/if}
      {#if picking && current}
        <Dock under=".sig.sel">
          <TopicPicker
            exclude={typeof selKey === 'number' ? selKey : null}
            placeholder={unsorted && chosen.size
              ? `Move ${chosen.size} ${chosen.size === 1 ? 'message' : 'messages'} to topic…`
              : 'Move to topic…'}
            onpick={moveTo}
            onclose={() => (picking = false)}
          />
        </Dock>
      {/if}
      <DockHost />
      <div class="hint-bar">
        {#if reviewing !== null}
          <span><kbd>j</kbd> <kbd>k</kbd> message (skips ⋯)</span><span><kbd>Space</kbd> select</span><span
            ><kbd>⇧j</kbd> <kbd>⇧k</kbd> select range</span
          ><span><kbd>t</kbd> move</span><span><kbd>d</kbd> dismiss</span><span><kbd>u</kbd> undo</span>
          <span><kbd>c</kbd> confirm groups</span><span><kbd>g</kbd> by topic / in order</span><span
            ><kbd>Esc</kbd> back</span
          >
        {:else if unsorted && inMsgs}
          <span><kbd>j</kbd> <kbd>k</kbd> message</span><span><kbd>g</kbd> <kbd>G</kbd> first / last</span><span
            ><kbd>Space</kbd> select</span
          ><span><kbd>⇧j</kbd> <kbd>⇧k</kbd> select range</span>
          <span><kbd>t</kbd> {chosen.size ? `move ${chosen.size} selected` : 'move whole signal'}</span>
          <span><kbd>↵</kbd> collapse</span><span><kbd>Esc</kbd> done</span>
        {:else if unsorted}
          <span><kbd>j</kbd> <kbd>k</kbd> signal</span><span><kbd>t</kbd> move to topic</span><span
            ><kbd>d</kbd> dismiss</span
          ><span><kbd>u</kbd> undo</span><span><kbd>?</kbd> keys</span>
        {:else}
          <span><kbd>j</kbd> <kbd>k</kbd> signal</span><span><kbd>c</kbd> confirm</span><span
            ><kbd>t</kbd> move to topic</span
          ><span><kbd>d</kbd> dismiss</span><span><kbd>u</kbd> undo</span><span><kbd>?</kbd> keys</span>
        {/if}
      </div>
    </div>
  </div>

  <AssistantPane topic={queue?.topics.find((t) => t.id === selKey) ?? null} />
</div>

{#if keysOpen}<KeySheet sections={KEYS} onclose={() => (keysOpen = false)} />{/if}

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
    padding: var(--row-pad-comfortable) 12px var(--row-pad-comfortable) 0;
    min-height: var(--row-min-comfortable);
    border-radius: 0 var(--radius-md) var(--radius-md) 0;
    text-align: left;
    transition: background 120ms;
  }
  .topic:hover {
    background: var(--row-hover);
  }
  /* The topic whose signals are open. The keyboard is in the signals, never here. */
  .topic.on {
    background: var(--bg-head);
  }
  .bar {
    align-self: stretch;
    border-radius: 0 var(--radius-sm) var(--radius-sm) 0;
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
    position: relative;
    flex: 1;
    min-height: 0;
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
  }
  .group.rule {
    border-top: 1px solid var(--border);
    margin-top: 4px;
  }
  .sig {
    transition: background 120ms;
  }
  .sig:hover:not(.sel) {
    background: var(--row-hover);
  }
  .sig.sel {
    background: var(--aqua-tint);
  }
  /* The keyboard is in its messages: the signal is the open row, the message the cursor. */
  .sig.sel.inside {
    background: var(--bg-head);
  }
  .row {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    gap: 12px;
    align-items: center;
    padding: var(--row-pad-comfortable) 16px var(--row-pad-comfortable) 38px;
    min-height: var(--row-min-comfortable);
    position: relative;
  }
  /* An agent's status mark, as in the tree, in the 38px gutter. */
  .agent-mark {
    position: absolute;
    left: 16px;
    top: 0;
    bottom: 0;
    width: 12px;
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .agent-name {
    font-weight: 500;
  }
  .filed {
    display: flex;
    gap: 8px;
    align-items: center;
    padding: 8px 16px;
    color: var(--status-done);
    border-bottom: 1px solid var(--border);
  }
  .answered {
    display: flex;
    gap: 6px;
    align-items: center;
    color: var(--status-done);
    white-space: nowrap;
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
    font-size: 0.9231rem;
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
  .ghost {
    display: flex;
    gap: 6px;
    align-items: center;
    padding: 0 8px;
    color: var(--tx-muted-2);
  }
  .sig.gone {
    background: var(--hatch);
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
    font-size: calc(var(--fs-read) * 12.5 / 14);
  }
  .msg {
    position: relative;
    display: grid;
    grid-template-columns: 44px 64px minmax(0, 1fr);
    gap: 10px;
    padding: var(--row-pad-compact) 8px;
    min-height: var(--row-min-compact);
    margin: 0 -8px;
    border-radius: var(--radius-sm);
    transition: background 120ms;
  }
  .msg.context {
    opacity: 0.5;
  }
  .msg.change .txt {
    color: var(--tx-muted-2);
  }
  .todo-form {
    padding: 0 16px 4px 38px;
  }
  .answer {
    padding: 0 16px 0 38px;
  }
  .answer:has(> :global(*)) {
    padding-bottom: 10px;
  }
  .held {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 16px 8px 38px;
    font-size: 0.9231rem;
    color: var(--tx-muted-2);
  }
  .held .htext {
    color: var(--tx-2);
  }
  .held .btn {
    flex: none;
  }
  .held.ignore {
    margin-top: -2px;
  }
  .more {
    padding: 2px 0 2px 54px;
    font-size: 0.9231rem;
    color: var(--tx-muted-2);
  }
  .more:hover {
    color: var(--aqua);
  }
  .reply-more {
    margin-left: 10px;
    padding-left: 70px;
  }
  .msg.reply {
    margin-left: 10px;
    padding-left: 16px;
    border-left: 1px solid var(--border);
  }
  .msg.pickable {
    cursor: pointer;
  }
  .msg.pickable:hover:not(.focus) {
    background: var(--row-hover);
  }
  .msg.focus {
    background: var(--aqua-tint);
  }
  .msg.chosen .txt {
    color: var(--tx-body);
  }
  .tick {
    position: absolute;
    left: -14px;
    top: 7px;
    font-size: 0.8462rem;
    color: var(--aqua);
  }
  .t {
    color: var(--tx-label);
  }
  .pill.cat {
    align-self: center;
    flex: none;
  }
  .pill.change {
    align-self: center;
    flex: none;
  }
  .desc {
    font-size: var(--fs-read);
    color: var(--tx-2);
    margin: 4px 0;
  }
  .count.split {
    align-self: center;
    flex: none;
    display: flex;
    gap: 4px;
    align-items: center;
    cursor: pointer;
  }
  .count.split i {
    font-size: 0.8462rem;
  }
  .gap {
    display: block;
    height: 18px;
    padding: 0 8px 0 22px;
    margin: 0 -8px;
    font-size: 1.25rem;
    line-height: 18px;
    letter-spacing: 2px;
    color: var(--tx-muted-2);
  }
  .gap:hover {
    color: var(--aqua);
  }
  .new-person {
    display: flex;
    gap: 10px;
    align-items: center;
    flex-wrap: wrap;
    margin: 8px 0 4px;
    padding: 8px 10px;
    border-radius: var(--radius-md);
    background: var(--bg-panel);
    border: 1px dashed var(--border-strong);
    font-size: 0.9231rem;
  }
  .new-person > i {
    font-size: 1.0769rem;
    color: var(--tx-muted-2);
  }
  .new-person .q {
    color: var(--tx-body);
  }
  .np-info {
    color: var(--tx-label);
    min-width: 0;
  }
  .np-actions {
    margin-left: auto;
    display: flex;
    gap: 6px;
  }
  .txt {
    color: var(--tx-2);
  }
  .info {
    padding-bottom: 6px;
    font-size: 0.9231rem;
    color: var(--tx-muted-2);
  }
  .links {
    display: flex;
    gap: 14px;
    padding-top: 6px;
    font-size: 0.9231rem;
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
  .more {
    width: 300px;
    border-radius: var(--radius-md);
    display: flex;
    flex-direction: column;
  }
  .more button {
    display: flex;
    gap: 8px;
    align-items: center;
    padding: 5px 12px;
    text-align: left;
    color: var(--tx-2);
  }
  .more button:hover {
    background: var(--hover-tint);
  }
  .more button kbd {
    margin-left: auto;
  }
  .empty-pane {
    display: block;
    padding: 16px;
    font: inherit;
    text-align: left;
  }
</style>

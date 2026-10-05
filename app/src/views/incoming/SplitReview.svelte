<script lang="ts">
  // Split review (15g by topic, 15h a channel with a thread). Opens on a signal
  // the AI split across topics and shows every part still in Incoming.
  // - By topic: each topic's messages together in time order, ⋯ where another
  //   group's messages came in between; groups ordered by first message.
  //   In order: the conversation as it happened, in runs per group. `g` flips
  //   (a 1:1 DM opens In order; everything else By topic). Not remembered.
  // - Bars cycle through the Gruvbox brights for this signal only; they're
  //   not topic colours. Names are in their person colour.
  // - j/k move (⋯ isn't a row), space selects, shift+j/k selects a range
  //   (like text: back toward where it started shrinks it), t moves the selection (or the
  //   current message) to a topic's group straight away (a new topic makes a
  //   new group; the picker's Unsorted row moves it there), d dismisses in place, u undoes, c confirms every group that
  //   has a topic (Unsorted stays in Incoming), Esc goes back.
  // - `filed` (18c chronology, 18d search): reviewing one filed signal. Only
  //   its own part and the parts this review makes are shown; a move files
  //   straight away (confirmed), so c is just Done.
  import { untrack } from 'svelte';
  import { call } from '../../lib/api';
  import { useKeys, plain } from '../../lib/keys.svelte';
  import { extend, shiftStep, type Range } from '../../lib/range';
  import Dock from '../../lib/ui/Dock.svelte';
  import Markdown from '../../lib/ui/Markdown.svelte';
  import PersonName from '../../lib/ui/PersonName.svelte';
  import TopicPicker from './TopicPicker.svelte';
  import type { Family, Message, Part } from './types';

  let { itemId, filed = false, onclose }: { itemId: number; filed?: boolean; onclose: (changed: boolean) => void } = $props();

  // Filed review: the parts it works on (the signal, plus parts it makes).
  let scope = $state<number[]>(untrack(() => [itemId]));
  const scoped = () => (filed ? { parts: scope } : {});
  let changed = false;

  const BARS = ['#fabd2f', '#83a598', '#d3869b', '#fe8019', '#b8bb26', '#8ec07c', '#fb4934'];
  const BIN = -1;

  type Meta = { topic: Part['topic']; color: string };
  type Mark = { kind: 'moved' | 'dismissed'; from: string; undo: number | null };

  let fam = $state<Family | null>(null);
  let mode = $state<'topic' | 'order'>('topic');
  let focus = $state<number | null>(null);
  let chosen = $state<Set<number>>(new Set());
  let picking = $state(false);
  let busy = false;
  let err = $state<string | null>(null);
  // Per review, kept across reloads: each part's topic and bar colour (a part
  // pruned after its messages left still names the group they came from),
  // where a dismissed message was, and what can be undone.
  let meta = $state<Map<number, Meta>>(new Map());
  let origin = $state<Map<number, number>>(new Map());
  let marks = $state<Map<number, Mark>>(new Map());
  let history: number[] = [];

  async function load(first = false) {
    const f = await call<Family>('incoming.family', { item_id: itemId, ...scoped() });
    const m = new Map(meta);
    for (const id of [...new Set(f.messages.map((x) => x.part!))]) {
      const p = f.parts.find((p) => p.item_id === id)!;
      if (p.dismissed) continue;
      const known = m.get(id);
      m.set(id, { topic: p.topic, color: known?.color ?? BARS[[...m.values()].filter((v) => v.color !== 'var(--border-strong)').length % BARS.length] });
    }
    meta = m;
    fam = f;
    if (first) {
      mode = f.conv === 'dm' ? 'order' : 'topic';
      focus = f.messages[0]?.id ?? null;
    }
  }
  load(true);

  const bin = $derived(fam?.parts.find((p) => p.dismissed)?.item_id ?? null);
  const isGone = (m: Message) => m.part === bin;
  /** The group a message shows in: where it was, for one dismissed this review. */
  const groupOf = (m: Message) => (isGone(m) ? (origin.get(m.id) ?? BIN) : m.part!);
  const label = (g: number) => (g === BIN ? 'Dismissed' : (meta.get(g)?.topic?.name ?? 'Unsorted'));
  const barOf = (g: number) => (g === BIN ? 'var(--border-strong)' : (meta.get(g)?.color ?? 'var(--border-strong)'));

  type Row = { kind: 'msg'; m: Message } | { kind: 'gap' };
  type Block = { group: number; rows: Row[] };

  const blocks = $derived.by<Block[]>(() => {
    if (!fam) return [];
    const msgs = fam.messages;
    if (mode === 'order') {
      const out: Block[] = [];
      for (const m of msgs) {
        const g = groupOf(m);
        const last = out[out.length - 1];
        if (last && last.group === g) last.rows.push({ kind: 'msg', m });
        else out.push({ group: g, rows: [{ kind: 'msg', m }] });
      }
      return out;
    }
    const order: number[] = [];
    for (const m of msgs) if (!order.includes(groupOf(m))) order.push(groupOf(m));
    return order.map((g) => {
      const rows: Row[] = [];
      let prev = -1;
      msgs.forEach((m, i) => {
        if (groupOf(m) !== g) return;
        if (prev >= 0 && msgs.slice(prev + 1, i).some((o) => groupOf(o) !== g)) rows.push({ kind: 'gap' });
        rows.push({ kind: 'msg', m });
        prev = i;
      });
      return { group: g, rows };
    });
  });

  const flat = $derived(blocks.flatMap((b) => b.rows.flatMap((r) => (r.kind === 'msg' ? [r.m] : []))));
  const current = $derived(flat.find((m) => m.id === focus) ?? null);

  const legend = $derived.by(() => {
    const out: { group: number; count: number }[] = [];
    for (const b of blocks) {
      if (b.group === BIN) continue;
      const n = b.rows.reduce((a, r) => a + (r.kind === 'msg' && !isGone(r.m) ? 1 + (r.m.replies?.length ?? 0) : 0), 0);
      const e = out.find((x) => x.group === b.group);
      if (e) e.count += n;
      else out.push({ group: b.group, count: n });
    }
    return out.filter((x) => x.count > 0);
  });
  const confirmable = $derived(legend.filter((g) => meta.get(g.group)?.topic).length);

  const metaLine = $derived.by(() => {
    if (!fam) return '';
    const live = fam.messages.filter((m) => !isGone(m));
    const n = live.reduce((a, m) => a + 1 + (m.replies?.length ?? 0), 0);
    const threads = live.filter((m) => m.replies?.length).length;
    const span = live.length ? `${live[0].time}–${live[live.length - 1].time}` : '';
    return [String(n), span, threads ? `${threads} thread${threads > 1 ? 's' : ''}` : mode === 'topic' ? 'grouped by topic' : 'in order']
      .filter(Boolean)
      .join(' · ');
  });

  $effect(() => {
    if (focus !== null) document.querySelector(`[data-review-msg="${focus}"]`)?.scrollIntoView({ block: 'nearest' });
  });

  // A shift+j/k run in progress; any other key ends it.
  let run: Range | null = null;

  function step(d: number, extending = false) {
    if (!flat.length) return;
    const i = flat.findIndex((m) => m.id === focus);
    const to = flat[Math.max(0, Math.min(flat.length - 1, (i < 0 ? 0 : i) + d))].id;
    if (extending && focus !== null) {
      const r = extend(
        flat.map((m) => m.id),
        run,
        focus,
        to,
        chosen,
      );
      run = r.run;
      chosen = r.chosen;
    } else run = null;
    focus = to;
  }

  function toggleChosen(id: number) {
    run = null;
    const next = new Set(chosen);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    chosen = next;
  }

  /** The selection, or the current message; by the part each is in now. */
  function targets(): Map<number, Message[]> {
    const picked = chosen.size ? flat.filter((m) => chosen.has(m.id)) : current ? [current] : [];
    const out = new Map<number, Message[]>();
    for (const m of picked) out.set(m.part!, [...(out.get(m.part!) ?? []), m]);
    return out;
  }

  async function act(run: () => Promise<void>) {
    if (busy) return;
    busy = true;
    try {
      await run();
      err = null;
      changed = true;
      await load();
    } catch (e) {
      err = String((e as Error).message ?? e);
    } finally {
      busy = false;
    }
  }

  function mark(m: Message, kind: Mark['kind']) {
    const g = groupOf(m);
    const next = new Map(marks);
    next.set(m.id, { kind, from: label(g), undo: meta.get(g)?.topic?.id ?? null });
    marks = next;
    history = [...history.filter((id) => id !== m.id), m.id];
  }

  /** `topicId` null: Unsorted. */
  function moveTo(topicId: number | null) {
    picking = false;
    const by = targets();
    if (!by.size) return;
    act(async () => {
      for (const [part, ms] of by) {
        const live = ms.filter((m) => !isGone(m));
        if (!live.length) continue;
        for (const m of live) mark(m, 'moved');
        const r = await call<{ item_id: number }>('incoming.review_move', { item_id: part, message_ids: live.map((m) => m.id), topic_id: topicId, ...scoped() });
        if (!scope.includes(r.item_id)) scope = [...scope, r.item_id];
      }
      chosen = new Set();
      run = null;
    });
  }

  function dismiss() {
    const by = targets();
    if (!by.size) return;
    act(async () => {
      for (const [part, ms] of by) {
        const live = ms.filter((m) => !isGone(m));
        if (!live.length) continue;
        const o = new Map(origin);
        for (const m of live) {
          mark(m, 'dismissed');
          o.set(m.id, groupOf(m));
        }
        origin = o;
        await call('incoming.review_dismiss', { item_id: part, message_ids: live.map((m) => m.id) });
      }
      chosen = new Set();
      run = null;
    });
  }

  function undo() {
    const id = current && marks.has(current.id) ? current.id : history[history.length - 1];
    const m = fam?.messages.find((x) => x.id === id);
    const k = id === undefined ? undefined : marks.get(id);
    if (!m || !k) return;
    act(async () => {
      const r = await call<{ item_id: number }>('incoming.review_move', { item_id: m.part, message_ids: [m.id], topic_id: k.undo, ...scoped() });
      if (!scope.includes(r.item_id)) scope = [...scope, r.item_id];
      const next = new Map(marks);
      next.delete(m.id);
      marks = next;
      const o = new Map(origin);
      o.delete(m.id);
      origin = o;
      history = history.filter((x) => x !== m.id);
      focus = m.id;
    });
  }

  function confirmAll() {
    act(async () => {
      await call('incoming.review_confirm', { item_id: itemId });
      onclose(true);
    });
  }

  useKeys((e) => {
    if (picking) return false;
    const shift = shiftStep(e);
    if (shift) return step(shift, true), true;
    if (plain(e, 'j') || plain(e, 'ArrowDown')) return step(1), true;
    if (plain(e, 'k') || plain(e, 'ArrowUp')) return step(-1), true;
    if (plain(e, ' ')) return focus !== null && toggleChosen(focus), true;
    if (plain(e, 't')) return (picking = !!(chosen.size || current)), true;
    if (plain(e, 'd')) return dismiss(), true;
    if (plain(e, 'u')) return undo(), true;
    if (plain(e, 'g')) return (mode = mode === 'topic' ? 'order' : 'topic'), true;
    if (plain(e, 'c')) return filed ? onclose(changed) : confirmable && confirmAll(), true;
    if (plain(e, 'Enter')) return true;
    if (plain(e, 'Escape')) return onclose(changed), true;
    // Everything else (tab switches, [ ]) waits until the review is closed.
    return plain(e, e.key) && e.key.length === 1;
  });
</script>

{#if fam}
  <div class="row head">
    <div class="title-line">
      <span class="title ellipsis">{fam.title}</span>
      <span class="meta tnum">{metaLine}</span>
      <span class="count split"><i class="ph ph-git-fork"></i>split</span>
    </div>
    <span class="modes">
      <span class="seg">
        <button class:on={mode === 'topic'} onclick={() => (mode = 'topic')}>By topic</button>
        <button class:on={mode === 'order'} onclick={() => (mode = 'order')}>In order</button>
      </span>
      <kbd>g</kbd>
      <button class="key" onclick={() => onclose(changed)}>· <kbd>Esc</kbd> back</button>
    </span>
  </div>

  <div class="groups">
    {#each blocks as b, bi (`${b.group}-${bi}`)}
      <div class="group" class:rule={bi > 0}>
        <span class="bar" style:background={barOf(b.group)}></span>
        <div>
          {#each b.rows as r, ri (r.kind === 'msg' ? r.m.id : `gap-${ri}`)}
            {#if r.kind === 'gap'}
              <div class="gap"><span>⋯</span></div>
            {:else}
              {@const m = r.m}
              {@const k = marks.get(m.id)}
              {@const gone = isGone(m)}
              <div
                class="msg"
                class:focus={m.id === focus}
                class:chosen={chosen.has(m.id)}
                class:moved={k?.kind === 'moved' && !gone}
                class:gone
                style:--tint={barOf(b.group)}
                data-review-msg={m.id}
                onclick={() => ((focus = m.id), toggleChosen(m.id))}
                role="presentation"
              >
                {#if chosen.has(m.id)}<i class="ph ph-check tick"></i>{/if}
                <span class="t tnum">{m.time}</span>
                <PersonName who={m.who} name={m.author} dim={gone} />
                <div class="txt"><Markdown text={m.text} /></div>
                <span class="side">
                  {#if gone}
                    <i class="ph ph-eye-slash"></i>dismissed{#if k}&nbsp;· <kbd>u</kbd>&nbsp;undo{/if}
                  {:else if k?.kind === 'moved'}
                    moved from {k.from} · <kbd>u</kbd>&nbsp;undo
                  {:else if m.replies?.length}
                    <i class="ph ph-chat-circle-dots"></i>thread · {m.replies.length} {m.replies.length === 1 ? 'reply' : 'replies'}
                  {/if}
                </span>
              </div>
              {#if m.replies?.length}
                <div class="replies" class:gone>
                  {#each m.replies as rep (rep.id)}
                    <div class="msg reply">
                      <span class="t tnum">{rep.time}</span>
                      <PersonName who={rep.who} name={rep.author} />
                      <div class="txt"><Markdown text={rep.text} /></div>
                      <span></span>
                    </div>
                  {/each}
                </div>
              {/if}
            {/if}
          {/each}
        </div>
      </div>
    {/each}
  </div>

  {#if picking}
    <Dock under=".msg.focus">
      <TopicPicker
        placeholder={chosen.size > 1 ? `Move ${chosen.size} messages to…` : 'Move to topic…'}
        onpick={moveTo}
        onunsorted={() => moveTo(null)}
        onclose={() => (picking = false)}
      />
    </Dock>
  {/if}

  <div class="legend inset">
    {#each legend as g (g.group)}
      <span class="lg">
        <span class="sw" style:background={barOf(g.group)}></span>
        <span class:unsorted={!meta.get(g.group)?.topic}>{label(g.group)}</span>
        <span class="n">{g.count}</span>
      </span>
    {/each}
    <span class="btns">
      <button class="btn" onclick={() => (picking = !!(chosen.size || current))}>Move… <kbd>t</kbd></button>
      <button class="btn" onclick={dismiss}>Dismiss <kbd>d</kbd></button>
      {#if filed}
        <button class="btn primary" onclick={() => onclose(changed)}>Done <kbd>c</kbd></button>
      {:else}
        <button class="btn primary" onclick={confirmAll} disabled={!confirmable}>
          Confirm {confirmable} {confirmable === 1 ? 'group' : 'groups'} <kbd>c</kbd>
        </button>
      {/if}
    </span>
    {#if err}<span class="err"><i class="ph ph-warning"></i>{err}</span>{/if}
  </div>
{/if}

<style>
  .row.head {
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
  .count.split {
    align-self: center;
    display: flex;
    gap: 4px;
    align-items: center;
  }
  .count.split i {
    font-size: 0.8462rem;
  }
  .modes {
    display: flex;
    gap: 10px;
    align-items: center;
  }
  .seg {
    display: flex;
    border: 1px solid var(--border);
    border-radius: 6px;
    font-size: 0.8846rem;
    background: var(--bg-app);
  }
  .seg button {
    padding: 2px 9px;
    color: var(--tx-muted);
    border-radius: 5px;
    white-space: nowrap;
  }
  .seg button.on {
    color: var(--bg-app);
    background: var(--aqua);
  }
  .key {
    font-size: 0.8846rem;
    color: var(--tx-label);
    white-space: nowrap;
  }
  .groups {
    padding: 4px 16px 0 26px;
    font-size: calc(var(--fs-read) * 12.5 / 14);
    display: flex;
    flex-direction: column;
  }
  .group {
    display: grid;
    grid-template-columns: 4px minmax(0, 1fr);
    gap: 12px;
  }
  .group.rule {
    margin-top: 8px;
    padding-top: 8px;
    border-top: 1px solid var(--border);
  }
  .bar {
    border-radius: 2px;
  }
  .msg {
    position: relative;
    display: grid;
    grid-template-columns: 44px 56px minmax(0, 1fr) auto;
    gap: 10px;
    align-items: baseline;
    padding: var(--row-pad-compact) 8px;
    min-height: var(--row-min-compact);
    margin: 0 -8px 0 0;
    border-radius: 4px;
    cursor: pointer;
  }
  .msg:hover:not(.focus):not(.chosen) {
    background: var(--row-hover);
  }
  .msg.gone {
    background: var(--hatch);
  }
  .msg.moved {
    background: color-mix(in srgb, var(--tint) 8%, transparent);
  }
  .msg.focus {
    background: var(--aqua-tint);
  }
  .msg.chosen .txt {
    color: var(--tx-body);
  }
  .tick {
    position: absolute;
    left: -26px;
    top: 7px;
    font-size: 0.8462rem;
    color: var(--aqua);
  }
  .t {
    color: var(--tx-label);
  }
  .txt {
    color: var(--tx-2);
    min-width: 0;
  }
  .msg.gone .txt {
    color: var(--tx-label);
    text-decoration: line-through;
    text-decoration-color: var(--tx-disabled);
  }
  .side {
    font-size: 0.8846rem;
    color: var(--tx-label);
    white-space: nowrap;
    display: flex;
    gap: 5px;
    align-items: center;
  }
  .replies {
    margin: 2px 0 2px 54px;
    padding-left: 12px;
    border-left: 1px solid var(--border-strong);
    display: flex;
    flex-direction: column;
  }
  .replies.gone {
    opacity: 0.5;
  }
  .msg.reply {
    cursor: default;
  }
  .gap {
    display: grid;
    grid-template-columns: 44px minmax(0, 1fr);
    gap: 10px;
    height: 14px;
    align-items: center;
    font-size: 0.9231rem;
    line-height: 1;
    letter-spacing: 2px;
    color: var(--tx-disabled);
  }
  .legend {
    margin: 10px 16px 12px 38px;
    display: flex;
    gap: 10px 20px;
    align-items: center;
    flex-wrap: wrap;
    font-size: 0.9231rem;
  }
  .lg {
    display: flex;
    gap: 8px;
    align-items: center;
    white-space: nowrap;
    color: var(--tx-body);
  }
  .lg .unsorted {
    color: var(--tx-muted);
  }
  .sw {
    width: 14px;
    height: 4px;
    border-radius: 2px;
  }
  .n {
    color: var(--tx-label);
  }
  .btns {
    margin-left: auto;
    display: flex;
    gap: 6px;
    align-items: center;
  }
  .err {
    flex-basis: 100%;
    display: flex;
    gap: 6px;
    align-items: center;
  }
</style>

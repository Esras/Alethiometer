<script lang="ts">
  // A topic's chronology (7a), with 18c: j/k select an entry (selecting adds
  // nothing), ↵ or a click expands it in place (messages, sender colours, one
  // faded context message, Open in the app) and Esc collapses it. While it's
  // expanded, t moves the whole signal to another topic (filed straight away)
  // and s opens the split review for this one signal, to move some of its
  // messages (only with more than one). A moved signal leaves one quiet line,
  // "moved to X · u undo", where it was until you leave the topic.
  // An unanswered invite shows a `no reply` pill; selected, it gets the RSVP
  // line (⌥Y accept, ⌥N decline, ⌥⇧N decline with a note; 14b).
  import { untrack } from 'svelte';
  import { refresh } from '../../lib/app.svelte';
  import { alt, plain, useKeys } from '../../lib/keys.svelte';
  import { moveSignal, undoMove, type Detail, type Move } from '../../lib/reclassify';
  import { SOURCES } from '../../lib/sources';
  import type { TopicDetail } from '../../lib/types';
  import AppIcon from '../../lib/ui/AppIcon.svelte';
  import Dock from '../../lib/ui/Dock.svelte';
  import RsvpLine, { answerable } from '../incoming/RsvpLine.svelte';
  import SplitReview from '../incoming/SplitReview.svelte';
  import TopicPicker from '../incoming/TopicPicker.svelte';
  import SignalDetail from '../shared/SignalDetail.svelte';

  type Entry = TopicDetail['chronology'][number];

  let { topicId, entries, active }: { topicId: number; entries: Entry[]; active: boolean } = $props();

  let sel = $state<number | null>(null);
  let expanded = $state<number | null>(null);
  let detail = $state<Detail | null>(null);
  let picking = $state(false);
  let reviewing = $state<number | null>(null);
  let err = $state<string | null>(null);
  let line = $state<RsvpLine>();
  // Moved signals keep their line (and place) until the topic changes.
  let moves = $state<{ move: Move; entry: Entry; index: number }[]>([]);

  // A different topic starts fresh (a reload of the same one doesn't).
  let shown = untrack(() => topicId);
  $effect(() => {
    const id = topicId;
    if (id === shown) return;
    shown = id;
    untrack(() => {
      sel = null;
      expanded = null;
      reviewing = null;
      picking = false;
      moves = [];
    });
  });

  const rows = $derived.by(() => {
    const out: { entry: Entry; move: Move | null }[] = entries
      .filter((e) => !moves.some((m) => m.entry.item_id === e.item_id))
      .map((e) => ({ entry: e, move: null }));
    for (const m of [...moves].sort((a, b) => a.index - b.index))
      out.splice(Math.min(m.index, out.length), 0, { entry: m.entry, move: m.move });
    return out;
  });
  // "Now" divider between upcoming/undone (open dots) and what happened.
  const nowIndex = $derived(rows.findIndex((r) => !r.entry.open));
  const multi = $derived(!!detail && detail.item_id === expanded && detail.messages.length > 1);

  $effect(() => {
    if (sel !== null) document.querySelector(`[data-chron="${sel}"]`)?.scrollIntoView({ block: 'nearest' });
  });

  function step(d: number) {
    if (!rows.length) return;
    const i = rows.findIndex((r) => r.entry.item_id === sel);
    sel = rows[Math.max(0, Math.min(rows.length - 1, i < 0 ? 0 : i + d))].entry.item_id;
    if (expanded !== sel) expanded = null;
    picking = false;
  }

  function toggle(id: number) {
    if (moves.some((m) => m.entry.item_id === id)) return;
    sel = id;
    picking = false;
    expanded = expanded === id ? null : id;
  }

  async function moveTo(to: number, name: string) {
    picking = false;
    const id = expanded;
    if (id === null) return;
    const index = rows.findIndex((r) => r.entry.item_id === id);
    const entry = rows[index]?.entry;
    try {
      const move = await moveSignal(id, topicId, to, name);
      moves = [...moves, { move, entry, index }];
      expanded = null;
      err = null;
      refresh();
    } catch (e) {
      err = String((e as Error).message ?? e);
    }
  }

  async function undo() {
    const m = moves.find((x) => x.entry.item_id === sel) ?? moves[moves.length - 1];
    if (!m) return;
    try {
      await undoMove(m.move);
      moves = moves.filter((x) => x !== m);
      sel = m.entry.item_id;
      err = null;
      refresh();
    } catch (e) {
      err = String((e as Error).message ?? e);
    }
  }

  function endReview(changed: boolean) {
    reviewing = null;
    if (changed) refresh();
  }

  useKeys((e) => {
    if (!active || picking || reviewing !== null) return;
    if (plain(e, 'j') || plain(e, 'ArrowDown')) return (step(1), true);
    if (plain(e, 'k') || plain(e, 'ArrowUp')) return (step(-1), true);
    if (plain(e, 'Enter')) return (sel !== null && toggle(sel), true);
    if (plain(e, 'Escape') && expanded !== null) return ((expanded = null), true);
    if (plain(e, 't') && expanded !== null) return ((picking = true), true);
    if (plain(e, 's') && expanded !== null && multi) return ((reviewing = expanded), true);
    if (plain(e, 'u') && moves.length) return (undo(), true);
    const r = entries.find((x) => x.item_id === sel)?.rsvp;
    if (r && answerable(r) && line) {
      if (alt(e, 'y')) return (line.accept(), true);
      if (alt(e, 'n')) return (line.decline(), true);
      if (alt(e, 'n', true)) return (line.declineWithNote(), true);
    }
  });
</script>

<div class="timeline">
  <div class="line"></div>
  {#each rows as { entry: e, move }, i (e.item_id)}
    {#if i === nowIndex && i > 0}
      <div class="now-rule label"><span class="tick"></span>Now<span class="fade"></span></div>
    {/if}
    {#if reviewing === e.item_id}
      <div class="review"><SplitReview itemId={e.item_id} filed onclose={endReview} /></div>
    {:else}
      <div
        class="entry"
        class:sel={sel === e.item_id}
        class:open={expanded === e.item_id}
        class:moved={!!move}
        data-chron={e.item_id}
        onclick={() => toggle(e.item_id)}
        role="presentation"
      >
        <span
          class="dot"
          style:background={e.open ? 'var(--bg-panel)' : SOURCES[e.source]?.dot}
          style:box-shadow={e.open ? `inset 0 0 0 2px ${SOURCES[e.source]?.dot}` : 'none'}
        ></span>
        <span class="when nowrap" class:today={e.today}>{e.when}</span>
        <span class="icon"><AppIcon source={e.source} /></span>
        {#if move}
          <div class="min0 quiet">moved to {move.toName} · <kbd>u</kbd> undo</div>
          <span></span>
        {:else}
          <div class="min0">
            <div>
              {e.title}{#if expanded === e.item_id}<i class="ph ph-caret-up caret"></i>{/if}{#if e.range}<span
                  class="range">{e.range}</span
                >{/if}
            </div>
            {#if e.sub}<div class="sub">{e.sub}</div>{/if}
            {#if e.rsvp && sel === e.item_id && expanded !== e.item_id}<RsvpLine
                bind:this={line}
                itemId={e.item_id}
                rsvp={e.rsvp}
                small
              />{/if}
          </div>
          <span class="right nowrap"
            >{#if e.rsvp && answerable(e.rsvp) && e.rsvp.status === 'needs_action'}<span class="pill quiet"
                >no reply</span
              >{/if}{e.right}</span
          >
          {#if expanded === e.item_id}
            <div class="expanded" onclick={(ev) => ev.stopPropagation()} role="presentation">
              <SignalDetail itemId={e.item_id} ondetail={(d) => (detail = d)}>
                {#snippet hints()}<kbd>t</kbd> move · {#if multi}<kbd>s</kbd> split{' · '}{/if}<kbd>Esc</kbd> collapse{/snippet}
              </SignalDetail>
            </div>
          {/if}
        {/if}
      </div>
    {/if}
  {/each}
  {#if err}<div class="err"><i class="ph ph-warning"></i>{err}</div>{/if}
</div>

{#if picking && expanded !== null}
  <Dock under=".entry.open">
    <TopicPicker
      exclude={topicId}
      placeholder="Move this signal to…"
      onpick={moveTo}
      onclose={() => (picking = false)}
    />
  </Dock>
{/if}

<style>
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
    padding: var(--row-pad-cozy) 8px var(--row-pad-cozy) 0;
    min-height: var(--row-min-cozy);
    margin-right: -4px;
    border-radius: var(--radius-md);
    position: relative;
    cursor: pointer;
  }
  .entry.open {
    background: var(--bg-head);
    padding-bottom: 10px;
    cursor: default;
  }
  .entry.sel {
    background: var(--aqua-tint);
  }
  .pill.quiet {
    background: var(--bg-head);
    color: var(--tx-muted-2);
    margin-right: 8px;
  }
  .entry.moved {
    cursor: default;
  }
  .dot {
    position: absolute;
    left: -18px;
    top: 11px;
    width: 10px;
    height: 10px;
    border-radius: 50%;
  }
  .when {
    font-size: 0.8846rem;
    color: var(--tx-muted-2);
    padding-top: 1px;
  }
  .when.today {
    color: var(--aqua);
    font-weight: 500;
  }
  .icon {
    padding-top: 1px;
  }
  .caret {
    font-size: 0.8462rem;
    color: var(--tx-muted-2);
    margin-left: 4px;
  }
  .range {
    font-size: 0.8846rem;
    color: var(--tx-label);
    margin-left: 5px;
  }
  .sub {
    font-size: 0.9231rem;
    color: var(--tx-muted-2);
  }
  .right {
    font-size: 0.8462rem;
    color: var(--tx-label);
  }
  .expanded {
    grid-column: 3 / -1;
  }
  .quiet {
    font-size: 0.9231rem;
    color: var(--tx-label);
    padding-top: 1px;
  }
  .moved .dot {
    opacity: 0.4;
  }
  .review {
    margin: 4px -12px 4px -30px;
    border-radius: var(--radius-md);
    background: var(--aqua-tint-soft);
    box-shadow: inset 3px 0 0 var(--aqua);
  }
  .now-rule {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 4px 0 4px -22px;
  }
  .tick {
    width: 10px;
    height: 2px;
    background: var(--aqua);
  }
  .fade {
    flex: 1;
    height: 1px;
    background: linear-gradient(90deg, var(--aqua-line), transparent);
  }
  .err {
    padding-top: 6px;
    font-size: 0.9231rem;
    display: flex;
    gap: 6px;
    align-items: center;
  }
</style>

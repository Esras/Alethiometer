<script lang="ts">
  // Sources (hi-fi 3a): floats over the view, anchored to the Assistant's
  // left edge, on the lighter surface so it reads as part of the Assistant.
  // Queries first, each with its mark and "returned → kept"; then Kept and
  // Dropped (⇥). Hovering a query dims every source it didn't return and
  // lights the sentences they support. j/k move, ↵ opens, p pins, x rejects
  // (for this topic). The left edge drags wider.
  import { openExternal } from '../../lib/api';
  import { openTopic } from '../../lib/app.svelte';
  import { feedback } from '../../lib/hydrated';
  import { isTyping, useKeys } from '../../lib/keys.svelte';
  import AppIcon from '../../lib/ui/AppIcon.svelte';
  import { markColor, type Reply, type Source } from './types';

  let {
    reply,
    topicId,
    hoverQuery = $bindable(null),
    hoverSource = $bindable(null),
    onchanged,
  }: {
    reply: Reply;
    topicId: number | null;
    hoverQuery?: number | null;
    hoverSource?: string | null;
    onchanged: (note: string) => void;
  } = $props();

  let tab = $state<'kept' | 'dropped'>('kept');
  let sel = $state(0);
  let width = $state(440);

  const kept = $derived(reply.sources.filter((s) => s.kept));
  const dropped = $derived(reply.sources.filter((s) => !s.kept));
  const list = $derived(tab === 'kept' ? kept : dropped);
  const gleanCount = $derived(reply.queries.filter((q) => !q.local).reduce((a, q) => a + q.returned, 0));
  const localCount = $derived(reply.queries.find((q) => q.local)?.returned ?? 0);

  function open(s: Source | undefined) {
    if (!s) return;
    if (s.topic) return openTopic(s.topic.id);
    if (s.url) openExternal(s.url);
  }

  async function verdict(s: Source | undefined, v: 'pin' | 'reject') {
    if (!s) return;
    if (topicId === null) return onchanged('Pins and rejects are per topic: ask from a topic.');
    await feedback(topicId, s, v);
    onchanged(v === 'pin' ? `Pinned for this topic: ${s.title}` : `Rejected for this topic: ${s.title} (ask again to drop it)`);
  }

  useKeys((e) => {
    if (isTyping(e) || e.metaKey || e.ctrlKey) return false;
    if (e.key === 'Tab') tab = tab === 'kept' ? 'dropped' : 'kept';
    else if (e.key === 'j') sel = Math.min(sel + 1, list.length - 1);
    else if (e.key === 'k') sel = Math.max(sel - 1, 0);
    else if (e.key === 'Enter') open(list[sel]);
    else if (e.key === 'p') verdict(list[sel], 'pin');
    else if (e.key === 'x') verdict(list[sel], 'reject');
    else return false;
    return true;
  });

  $effect(() => {
    void tab;
    sel = 0;
  });

  function drag(e: PointerEvent) {
    const start = e.clientX;
    const w0 = width;
    const move = (m: PointerEvent) => (width = Math.max(360, Math.min(900, w0 + (start - m.clientX))));
    const up = () => {
      window.removeEventListener('pointermove', move);
      window.removeEventListener('pointerup', up);
    };
    window.addEventListener('pointermove', move);
    window.addEventListener('pointerup', up);
  }

  const dim = (s: Source) => (hoverQuery !== null && !s.queries.includes(hoverQuery)) || (hoverSource !== null && hoverSource !== s.key);
</script>

<div class="sheet" style:width="{width}px" role="complementary" aria-label="Sources">
  <div class="edge" onpointerdown={drag} role="separator" aria-orientation="vertical"></div>
  <div class="head">
    <i class="ph ph-stack"></i><span class="title">Sources</span>
    <span class="tally tnum">{gleanCount} Glean + {localCount} local → {kept.length} kept</span>
  </div>
  <div class="label">Queries</div>
  <div class="queries">
    {#each reply.queries as q, i}
      <div class="query" class:on={hoverQuery === i} onmouseenter={() => (hoverQuery = i)} onmouseleave={() => (hoverQuery = null)} role="presentation">
        <span class="mark" style:color={markColor(i, q)}>{q.mark}</span>
        <span class="qn mono">{q.local ? 'L' : `Q${i + 1}`}</span>
        <span class="qt mono ellipsis" title={q.error ?? q.text}>{q.text}</span>
        {#if q.error}<span class="qerr" title={q.error}><i class="ph ph-warning"></i></span>{:else}<span class="qc tnum">{q.returned} → {q.kept}</span>{/if}
      </div>
    {/each}
  </div>
  <div class="tabs">
    <button class:on={tab === 'kept'} onclick={() => (tab = 'kept')}>Kept <span class="n">{kept.length}</span></button>
    <button class:on={tab === 'dropped'} onclick={() => (tab = 'dropped')}>Dropped <span class="n warn">{dropped.length}</span></button>
    <kbd>⇥</kbd>
  </div>
  <div class="rows scroll">
    {#each list as s, i (s.key)}
      <button class="row" class:sel={i === sel} class:dim={dim(s)} onclick={() => open(s)} onmouseenter={() => ((sel = i), (hoverSource = s.key))} onmouseleave={() => (hoverSource = null)}>
        <span class="num">{s.n ?? ''}</span>
        <AppIcon source={s.icon} size={16} />
        <span class="body">
          <span class="t ellipsis">{s.title}</span>
          <span class="sub ellipsis">{s.system}{s.container ? ` › ${s.container}` : ''}{s.reason ? ` · ${s.reason}` : ''}</span>
        </span>
        <span class="marks">
          {#each s.queries as qi}<span style:color={markColor(qi, reply.queries[qi])}>{reply.queries[qi]?.mark}</span>{/each}
        </span>
      </button>
    {:else}
      <div class="empty">{tab === 'kept' ? 'Nothing kept.' : 'Nothing dropped.'}</div>
    {/each}
  </div>
  <div class="foot"><span>hover a query to trace</span><span><kbd>↵</kbd> open</span><span><kbd>p</kbd> pin</span><span><kbd>x</kbd> reject</span><span>drag edge to widen</span></div>
</div>

<style>
  .sheet {
    position: absolute;
    top: 0;
    bottom: 0;
    right: 100%;
    z-index: 20;
    display: flex;
    flex-direction: column;
    background: var(--bg-head);
    border-left: 1px solid var(--border-strong);
    box-shadow: -14px 0 32px rgba(0, 0, 0, 0.45);
  }
  .edge {
    position: absolute;
    left: -3px;
    top: 0;
    bottom: 0;
    width: 6px;
    cursor: col-resize;
  }
  .head {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 12px 14px;
  }
  .head i {
    color: var(--tx-muted-2);
  }
  .title {
    color: var(--tx-title);
  }
  .tally {
    margin-left: auto;
    color: var(--tx-muted-2);
    font-size: 0.9231rem;
  }
  .label {
    padding: 0 14px 4px;
  }
  .queries {
    padding: 0 8px 8px;
  }
  .query {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: var(--row-pad-compact) 6px;
    min-height: var(--row-min-compact);
    border-radius: var(--radius-md);
    border: 1px solid transparent;
    font-size: 0.9231rem;
  }
  .query.on {
    border-color: var(--orange);
    background: rgba(254, 128, 25, 0.08);
  }
  .qn {
    color: var(--tx-label);
    min-width: 18px;
  }
  .qt {
    flex: 1;
    color: var(--tx-2);
  }
  .qc {
    color: var(--tx-label);
    white-space: nowrap;
  }
  .qerr {
    color: var(--orange);
  }
  .tabs {
    display: flex;
    align-items: center;
    gap: 14px;
    padding: 6px 14px;
    border-top: 1px solid var(--border);
    border-bottom: 1px solid var(--border);
  }
  .tabs button {
    color: var(--tx-muted-2);
    padding-bottom: 2px;
    border-bottom: 1.5px solid transparent;
  }
  .tabs button.on {
    color: var(--tx-title);
    border-color: var(--aqua);
  }
  .n {
    color: var(--tx-label);
  }
  .n.warn {
    color: var(--orange);
  }
  .tabs kbd {
    margin-left: auto;
  }
  .rows {
    flex: 1;
    min-height: 0;
  }
  .row {
    width: 100%;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: var(--row-pad-cozy) 14px;
    min-height: var(--row-min-cozy);
    text-align: left;
    border-bottom: 1px solid var(--border);
    transition: opacity 120ms;
  }
  .row.sel {
    background: rgba(254, 128, 25, 0.1);
    box-shadow: inset 2px 0 0 var(--orange);
  }
  .row.dim {
    opacity: 0.35;
  }
  .num {
    width: 16px;
    color: var(--tx-label);
    font-size: 0.8462rem;
    text-align: center;
  }
  .body {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
  .t {
    color: var(--tx-title);
  }
  .sub {
    color: var(--tx-muted-2);
    font-size: 0.9231rem;
  }
  .marks {
    display: flex;
    gap: 3px;
    font-size: 0.8462rem;
  }
  .empty {
    padding: 12px 14px;
    color: var(--tx-label);
  }
  .foot {
    display: flex;
    flex-wrap: wrap;
    gap: 12px;
    padding: 8px 14px;
    border-top: 1px solid var(--border);
    color: var(--tx-label);
    font-size: 0.8846rem;
  }
</style>

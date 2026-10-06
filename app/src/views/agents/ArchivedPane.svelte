<script lang="ts">
  // An archived session or group opened read-only from the tree filter (Agents Archive 04).
  import { tick } from 'svelte';
  import { chord } from '../../lib/keys.svelte';
  import StatusMark from '../../lib/ui/StatusMark.svelte';
  import { items } from './items';
  import RowView from './RowView.svelte';
  import { ag, closeArchived, openArchived } from './state.svelte';
  import type { ArchivedOpen, Fact } from './types';

  let { a }: { a: ArchivedOpen } = $props();

  const it = $derived(a.item);
  const d = $derived(a.data);
  const day = $derived(new Date(it.at).toLocaleDateString(undefined, { day: 'numeric', month: 'short' }));
  const isGroup = $derived(it.kind === 'group');
  const all = $derived(new Set(a.rows.map((r) => `row:${r.id}`)));
  const list = $derived(items(a.rows, new Set(), all, false));
  const title = $derived(isGroup ? d.goal || `#${d.room?.name ?? d.name}` : d.title || d.name || '');
  const where = $derived(isGroup ? [d.room ? `${d.room.hub || 'Scion Hub'} · project ${d.project}` : d.project, d.room ? 'group thread' : 'group'].filter(Boolean).join(' · ') : (d.context ?? ''));
  const facts = $derived<[string, string, Fact | null | undefined][]>(
    isGroup ? [] : [['ph-ticket', 'ticket', d.facts?.ticket], ['ph-git-pull-request', 'PR', d.facts?.pr], ['ph-git-branch', 'branch', d.facts?.branch], ['ph-check-square', 'todo', d.facts?.todo]],
  );
  const kept = $derived(
    `${it.source === 'scion' ? 'Agents deleted on the Hub. ' : ''}${a.kept.messages} message${a.kept.messages === 1 ? '' : 's'} and ${a.kept.transcripts} transcript${a.kept.transcripts === 1 ? '' : 's'} kept.`,
  );

  let stream = $state<HTMLDivElement>();
  let ranges: Range[] = [];
  let at = $state(0);
  let count = $state(0);

  const supported = typeof CSS !== 'undefined' && 'highlights' in CSS;

  $effect(() => {
    const q = ag.archivedQ.toLowerCase();
    void list;
    tick().then(() => {
      ranges = [];
      if (supported) CSS.highlights.delete('archive-match');
      if (!q || !stream) return void (count = 0);
      const walk = document.createTreeWalker(stream, NodeFilter.SHOW_TEXT);
      for (let n = walk.nextNode(); n; n = walk.nextNode()) {
        const t = (n.textContent ?? '').toLowerCase();
        for (let i = t.indexOf(q); i >= 0; i = t.indexOf(q, i + q.length)) {
          const r = new Range();
          r.setStart(n, i);
          r.setEnd(n, i + q.length);
          ranges.push(r);
        }
      }
      count = ranges.length;
      at = 0;
      if (supported && count) CSS.highlights.set('archive-match', new Highlight(...ranges));
      show();
    });
    return () => {
      if (supported) {
        CSS.highlights.delete('archive-match');
        CSS.highlights.delete('archive-current');
      }
    };
  });

  function show() {
    const r = ranges[at];
    if (!r) return;
    if (supported) CSS.highlights.set('archive-current', new Highlight(r));
    r.startContainer.parentElement?.scrollIntoView({ block: 'center' });
  }

  /** ⌘G / ⇧⌘G. */
  export function nextMatch(d: 1 | -1) {
    if (!count) return;
    at = (at + d + count) % count;
    show();
  }
</script>

<div class="pane">
  <div class="head">
    <div class="left">
      <div class="ctx">
        {#if isGroup}<i class="ph ph-hash hash"></i>{d.room?.name ?? d.name}{:else}<StatusMark status={d.status ?? 'idle'} /><span>{d.name}</span>{/if}
        <span class="c">{where}</span>
        <span class="chip arch"><i class="ph ph-archive"></i>archived {day}</span>
      </div>
      <div class="title">{title}</div>
      {#if isGroup && a.members.length}
        <div class="members">
          {#each a.members as m (m.id)}<button class="btn" onclick={() => openArchived(m.id)}>{m.name}</button>{/each}
        </div>
      {/if}
    </div>
    {#if facts.length}
      <div class="facts inset">
        {#each facts as [icon, label, f] (label)}
          <i class="ph {icon}"></i>
          <span class="fl">{label}</span>
          <span class="fv" class:none={!f}>{f ? `${f.label}${f.detail ? ` ${f.detail}` : ''}` : '—'}</span>
        {/each}
      </div>
    {/if}
  </div>

  <div class="panel">
    {#if ag.archivedQ}
      <div class="bar">
        <span>{count ? `${at + 1} of ${count} match${count === 1 ? '' : 'es'}` : 'No matches'} for “{ag.archivedQ}”</span>
        <kbd>{chord('⌘G', 'Ctrl+G')}</kbd>
      </div>
    {/if}
    <div class="stream" bind:this={stream}>
      <div class="rows">
        {#each list as x (x.key)}
          <div data-row={x.row.id}><RowView row={x.row} own={isGroup ? null : it.id} open={true} /></div>
        {:else}
          <div class="quiet">{isGroup ? 'No group thread. Open a session above.' : 'Nothing was stored for this session.'}</div>
        {/each}
      </div>
    </div>
    <div class="bottom hint-bar">
      <span class="ro">Archived {day} · read-only</span>
      <span class="kept">{kept}</span>
      <button class="btn" onclick={closeArchived}>Back<kbd>Esc</kbd></button>
    </div>
  </div>
</div>

<style>
  .pane {
    display: flex;
    flex-direction: column;
    min-height: 0;
    min-width: 0;
  }
  .head {
    flex: none;
    padding: 14px 20px 0;
    display: grid;
    grid-template-columns: minmax(0, 1fr) min(560px, 45%);
    gap: 24px;
    align-items: center;
  }
  .left {
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-width: 0;
  }
  .ctx {
    display: flex;
    gap: 8px;
    align-items: center;
    font: 0.923rem var(--font-mono);
    color: var(--tx-2);
    min-width: 0;
  }
  .ctx .c {
    color: var(--tx-label);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .hash {
    color: var(--msg);
  }
  .arch {
    flex: none;
    color: var(--tx-muted-2);
  }
  .title {
    font-size: 1.4615rem;
    font-weight: 500;
    line-height: 1.3;
    color: var(--tx-title);
  }
  .members {
    display: flex;
    gap: 6px;
    flex-wrap: wrap;
  }
  .facts {
    display: grid;
    grid-template-columns: 14px auto minmax(0, 1fr) 14px auto minmax(0, 1fr);
    gap: 3px 8px;
    align-items: baseline;
    font: 0.885rem/1.55 var(--font-mono);
  }
  .facts i,
  .fl,
  .fv.none {
    color: var(--tx-disabled);
  }
  .fv {
    color: var(--tx-2);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .panel {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    margin: 12px 16px 14px;
    border-radius: var(--radius-md);
    background: var(--bg-app);
    border: 1px solid var(--border);
    overflow: hidden;
  }
  .bar {
    flex: none;
    display: flex;
    gap: 8px;
    align-items: center;
    justify-content: flex-end;
    padding: 7px 14px;
    font-size: 0.885rem;
    color: var(--tx-label);
    border-bottom: 1px solid var(--bg-head);
  }
  .stream {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 0 22px;
    scrollbar-width: thin;
    scrollbar-color: var(--border-strong) transparent;
  }
  .rows {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 12px 0 14px;
  }
  .quiet {
    color: var(--tx-disabled);
    font-size: 0.923rem;
    padding: 20px 0;
  }
  .bottom {
    align-items: center;
  }
  .ro {
    color: var(--tx-2);
  }
  .kept {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  :global(::highlight(archive-match)) {
    background: color-mix(in srgb, var(--yellow) 30%, transparent);
    color: var(--tx-title);
  }
  :global(::highlight(archive-current)) {
    background: var(--yellow);
    color: var(--bg-app);
  }
</style>

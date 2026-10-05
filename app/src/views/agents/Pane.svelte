<script lang="ts">
  // The right side: header with the facts box, then the thread panel (legend
  // or tabs, the stream newest at the bottom, the pinned request, the composer
  // or the new-agent form, the status line).
  import { openExternal } from '../../lib/api';
  import ApprovalCard from './ApprovalCard.svelte';
  import AskCard from './AskCard.svelte';
  import Composer from './Composer.svelte';
  import { items, LEGEND, type Item } from './items';
  import { agentColor } from './look';
  import RowView from './RowView.svelte';
  import { act, ag, look, pinned, setTab, threadOf } from './state.svelte';
  import StatusLine from './StatusLine.svelte';
  import StatusMark from '../../lib/ui/StatusMark.svelte';
  import type { Fact } from './types';

  let { composerEl = $bindable(), approval = $bindable(), ask = $bindable(), denying = $bindable(false) }: {
    composerEl?: HTMLTextAreaElement;
    approval?: ApprovalCard;
    ask?: AskCard;
    denying?: boolean;
  } = $props();

  const key = $derived(ag.open);
  const t = $derived(threadOf(key));
  const s = $derived(t.session);
  const g = $derived(t.group);
  const isRoom = $derived(!!key?.startsWith('r:'));
  const kind = $derived<'local' | 'scion' | 'room'>(isRoom ? 'room' : s?.source === 'scion' ? 'scion' : 'local');
  const lk = $derived(look());
  const req = $derived(pinned(key));
  const list = $derived<Item[]>(items(ag.rows, ag.hidden, ag.expanded, lk.boxes.fold_pairs));
  const firstMine = $derived(ag.rows.find((r) => r.kind === 'you')?.text ?? '');

  // Header: room → its group's goal; session → its title.
  const title = $derived(isRoom ? g?.goal || `#${g?.room?.name}` : s?.title || s?.name || '');
  const subtitle = $derived(isRoom ? firstMine.split('\n')[0] : s?.subtitle || '');
  const facts = $derived<[string, string, Fact | null | undefined, string][]>(
    !s && !isRoom
      ? []
      : [
          ['ph-ticket', 'ticket', s?.facts.ticket, '—'],
          ['ph-git-pull-request', 'PR', s?.facts.pr, 'none yet'],
          ['ph-git-branch', 'branch', s?.facts.branch, '—'],
          ['ph-check-square', 'todo', s?.facts.todo, '—'],
        ],
  );
  // A room has no facts of its own: show its members' (the first that has each).
  const roomFacts = $derived.by(() => {
    if (!isRoom || !g) return facts;
    const members = (ag.tree?.sessions ?? []).filter((x) => x.group_id === g.id);
    const pick = (k: 'ticket' | 'pr' | 'branch' | 'todo') => members.map((m) => m.facts[k]).find(Boolean) ?? null;
    return [
      ['ph-ticket', 'ticket', pick('ticket'), '—'],
      ['ph-git-pull-request', 'PR', pick('pr'), 'none yet'],
      ['ph-git-branch', 'branch', pick('branch'), '—'],
      ['ph-check-square', 'todo', pick('todo'), '—'],
    ] as [string, string, Fact | null, string][];
  });

  let scroller = $state<HTMLDivElement>();
  let following = $state(true);
  function onScroll() {
    // column-reverse: 0 is the bottom.
    following = Math.abs(scroller?.scrollTop ?? 0) < 6;
  }
  export function latest() {
    scroller?.scrollTo({ top: 0 });
    ag.rowSel = null;
    following = true;
  }
  $effect(() => {
    if (ag.rowSel === null) return;
    const el = scroller?.querySelector(`[data-row="${ag.rowSel}"]`);
    el?.scrollIntoView({ block: 'nearest' });
  });

  function toggleKind(kinds: string[]) {
    const on = kinds.every((k) => ag.hidden.has(k as never));
    for (const k of kinds) {
      if (on) ag.hidden.delete(k as never);
      else ag.hidden.add(k as never);
    }
    ag.hidden = new Set(ag.hidden);
  }

  function toggleRow(id: number) {
    const k = `row:${id}`;
    if (ag.expanded.has(k)) ag.expanded.delete(k);
    else ag.expanded.add(k);
    ag.expanded = new Set(ag.expanded);
  }

  const waitingRows = $derived(new Set((ag.tree?.requests ?? []).map((r) => r.id)));
  const messageCount = $derived(ag.tab === 'messages' ? ag.rows.length : 0);

  function delivery(rowId: number, action: string) {
    if (!key) return;
    if (action === 'edit') {
      const row = ag.rows.find((r) => r.id === rowId);
      if (row) ag.drafts[key] = row.text;
    }
    act('delivery', { thread: key, row: rowId, action }).catch(() => {});
  }

  function open(how: 'app' | 'terminal') {
    const id = isRoom ? key : s?.id;
    if (id) act('open', { session: id, how }).catch(() => {});
  }

</script>

{#if !key || (!s && !g)}
  <div class="empty">
    <div class="empty-pane hello">
      <i class="ph ph-robot"></i>
      <p>Open a session or room from the tree, or start a local session.</p>
      <button class="btn md primary" onclick={() => (ag.form ={ kind: 'local', group: null })}><i class="ph ph-plus"></i>New agent<kbd>N</kbd></button>
    </div>
  </div>
{:else}
  <div class="pane">
    <div class="head">
      <div class="left">
        <div class="ctx">
          {#if isRoom}<i class="ph ph-hash hash"></i>{g?.room?.name}{:else if s}<StatusMark status={s.status} /><span style:color={agentColor(lk, s)}>{s.name}</span>{/if}
          <span class="c">{isRoom ? `${g?.room?.hub ?? 'Scion Hub'} · project ${g?.project} · group thread` : s?.context}</span>
        </div>
        <div class="pane-title title">{title}</div>
        {#if subtitle}<div class="subtitle">{subtitle}</div>{/if}
      </div>
      <div class="facts inset">
        {#each roomFacts as [icon, label, f, none] (label)}
          <i class="ph {icon}"></i>
          <span class="fl">{label}</span>
          {#if f}
            <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
            <span class="fv" class:link={!!f.url} onclick={() => f.url && openExternal(f.url)} title={f.url || undefined}
              ><span class:key={label === 'ticket'}>{f.label}</span>{#if f.detail} <span class="fd">{f.detail}</span>{/if}</span
            >
          {:else}<span class="fv none">{none}</span>{/if}
        {/each}
      </div>
    </div>

    <div class="panel">
      {#if kind === 'scion' && ag.tab === 'messages'}
        <div class="tabs">
          <button class="tab" onclick={() => setTab('transcript')}>Transcript<span class="n">{s?.stats.turns ?? 0} turns</span></button>
          <button class="tab on">Messages<span class="n">{messageCount}</span></button>
          <span class="tabnote">You ↔ {s?.name}, plus mentions of {s?.name} in rooms</span>
          <span class="tk"><kbd>T</kbd><kbd>M</kbd></span>
        </div>
      {:else}
        <div class="legend">
          {#each LEGEND[kind] as c (c.label)}
            {@const off = c.kinds.every((k) => ag.hidden.has(k as never))}
            <button class="chip {c.box ? `c-${c.box}` : ''}" class:off onclick={() => toggleKind(c.kinds)}><i class="ph {c.icon}" style:color={c.color}></i>{c.label}</button>
          {/each}
          {#if kind === 'scion'}<button class="chip plain" onclick={() => setTab('messages')}>Messages tab<kbd>M</kbd></button>{/if}
          <button class="follow" class:paused={!following} onclick={latest}>
            <i class="ph ph-arrow-down"></i>{#if following}Following latest{:else}Jump to latest · <kbd>G</kbd>{/if}
          </button>
        </div>
      {/if}

      <div class="stream" bind:this={scroller} onscroll={onScroll}>
        <div class="rows">
          {#each list as it (it.key)}
            {#if it.run && !it.folded}
              <div class="runbox" class:sel={ag.rowSel === it.row.id} data-row={it.row.id}>
                {#each it.run as r (r.id)}<RowView row={r} own={s?.id ?? null} nested={true} />{/each}
              </div>
            {:else}
              <div data-row={it.row.id}>
                <RowView
                  row={it.row}
                  own={s?.id ?? null}
                  sel={ag.rowSel === it.row.id}
                  open={ag.expanded.has(`row:${it.row.id}`)}
                  earlier={it.run ? it.run.length - 1 : 0}
                  waiting={!!it.row.request && waitingRows.has(it.row.request)}
                  onDelivery={(a) => delivery(it.row.id, a)}
                  onToggle={() => toggleRow(it.row.id)}
                />
              </div>
            {/if}
          {:else}
            <div class="quiet">{ag.threadKey ? 'Nothing here yet.' : 'Loading…'}</div>
          {/each}
        </div>
      </div>

      <div class="bottom">
        {#if req && req.kind === 'approval'}
          <ApprovalCard bind:this={approval} bind:denying {req} compact={!!ag.form} />
        {:else if req && req.kind === 'ask'}
          <AskCard bind:this={ask} {req} compact={!!ag.form} onReply={() => ((ag.replying = req.id), composerEl?.focus())} />
        {/if}
        {#if key}
          {#key key}
            <Composer
              bind:el={composerEl}
              thread={key}
              session={s}
              group={g}
              onOpen={open}
              onNewAgent={() => g && (ag.form = { kind: 'scion', group: g.id, room: g.room?.id ?? null })}
            />
          {/key}
        {/if}
        {#if ag.error}<div class="err">{ag.error}</div>{/if}
        <StatusLine session={s} group={g} room={isRoom} />
      </div>
    </div>
  </div>
{/if}

<style>
  .pane,
  .empty {
    display: flex;
    flex-direction: column;
    min-height: 0;
    min-width: 0;
  }
  .empty {
    justify-content: flex-end;
  }
  .hello {
    margin: auto;
    gap: 10px;
    padding: 0;
    font: inherit;
  }
  /* Reaches the + in the button too (it was 2rem and grey before): kept. */
  .hello i {
    font-size: 2rem;
    color: var(--tx-disabled);
  }
  .hello p {
    margin: 0;
  }
  .btn {
    border-radius: var(--radius-md);
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
  .ctx > span:not(.c) {
    white-space: nowrap;
  }
  .ctx .c {
    color: var(--tx-label);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .hash {
    font-size: 1rem;
    color: var(--msg);
  }
  .title {
    line-height: 1.3;
    text-wrap: pretty;
  }
  .subtitle {
    font-size: 1rem;
    color: var(--tx-muted-2);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .facts {
    display: grid;
    grid-template-columns: 14px auto minmax(0, 1fr) 14px auto minmax(0, 1fr);
    gap: 3px 8px;
    align-items: baseline;
    font: 0.885rem/1.55 var(--font-mono);
  }
  .facts i {
    font-size: 0.923rem;
    color: var(--tx-disabled);
    line-height: 18px;
  }
  .fl {
    color: var(--tx-disabled);
  }
  .fv {
    color: var(--tx-2);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .fv.link {
    cursor: pointer;
  }
  .fv.link:hover {
    text-decoration: underline;
  }
  .fv.none {
    color: var(--tx-disabled);
  }
  .key {
    color: var(--blue);
  }
  .fd {
    color: var(--tx-label);
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
    box-shadow: inset 0 1px 0 rgba(0, 0, 0, 0.25);
    overflow: hidden;
    position: relative;
  }
  .legend {
    flex: none;
    display: flex;
    gap: 5px;
    align-items: center;
    padding: 8px 14px 7px;
    font-size: 0.885rem;
    border-bottom: 1px solid var(--bg-head);
  }
  .chip i {
    font-size: 0.923rem;
  }
  .chip.c-you {
    background: var(--you-fill);
    border-color: var(--you-border);
  }
  .chip.c-tool {
    background: var(--tool-fill);
    border-color: var(--tool-border);
  }
  .chip.c-msg {
    background: var(--msg-fill);
    border-color: var(--msg-border);
  }
  .chip.c-hub {
    background: var(--hub-fill);
    border-color: var(--hub-border);
  }
  .chip.c-need {
    background: var(--need-fill);
    border-color: var(--need-border);
  }
  .chip.off {
    opacity: 0.4;
    text-decoration: line-through;
  }
  .chip.plain {
    border-style: dashed;
    color: var(--tx-label);
  }
  .follow {
    margin-left: auto;
    color: var(--tx-label);
    display: flex;
    gap: 5px;
    align-items: center;
  }
  .follow.paused {
    color: var(--aqua);
  }
  .follow i {
    font-size: 0.846rem;
  }
  .tabs {
    flex: none;
    display: flex;
    gap: 4px;
    align-items: center;
    padding: 6px 10px;
    font-size: 0.9615rem;
    border-bottom: 1px solid var(--bg-head);
  }
  .tab {
    display: flex;
    gap: 6px;
    align-items: center;
    padding: 3px 10px;
    border-radius: var(--radius-md);
    color: var(--tx-muted-2);
  }
  .tab.on {
    background: var(--border);
    color: var(--tx-title);
  }
  .tab .n {
    font: 0.846rem var(--font-mono);
    color: var(--tx-disabled);
  }
  .tab.on .n {
    color: var(--tx-2);
  }
  .tabnote {
    margin-left: 10px;
    font-size: 0.885rem;
    color: var(--tx-label);
  }
  .tk {
    margin-left: auto;
    display: flex;
    gap: 4px;
  }
  .stream {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    overscroll-behavior: contain;
    display: flex;
    flex-direction: column-reverse;
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
  .runbox {
    margin: 0 -10px;
    padding: 6px 10px;
    border-radius: var(--radius-md);
    background: var(--msg-fill);
    border: 1px solid var(--msg-border);
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .runbox.sel {
    background: var(--aqua-tint);
  }
  .quiet {
    color: var(--tx-disabled);
    font-size: 0.923rem;
    padding: 20px 0;
  }
  .bottom {
    flex: none;
    border-top: 1px solid var(--bg-head);
    padding: 10px 12px 12px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .err {
    font-size: 0.923rem;
  }
</style>

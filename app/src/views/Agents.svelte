<script lang="ts">
  // Agents (design "Agents Final" 01–08, app/docs/agents.md): local ACP
  // sessions and Scion agents in one tree, a thread on the right.
  // ⏎ goes in, Esc comes out, Tab cycles tree → thread → composer.
  import { isTyping, mod, useKeys } from '../lib/keys.svelte';
  import type ApprovalCard from './agents/ApprovalCard.svelte';
  import type AskCard from './agents/AskCard.svelte';
  import { items } from './agents/items';
  import Keys from './agents/Keys.svelte';
  import { lookVars } from './agents/look';
  import Pane from './agents/Pane.svelte';
  import { act, ag, look, nodes, openThread, pinned, selectable, setTab, startPolling, stopPolling, threadOf } from './agents/state.svelte';
  import Tree from './agents/Tree.svelte';

  $effect(() => {
    startPolling();
    return stopPolling;
  });

  let filterEl = $state<HTMLInputElement>();
  let composerEl = $state<HTMLTextAreaElement>();
  let pane = $state<Pane>();
  let approval = $state<ApprovalCard>();
  let ask = $state<AskCard>();
  let denying = $state(false);

  const vars = $derived(lookVars(look()));

  function moveTree(d: 1 | -1) {
    const list = nodes().filter(selectable);
    if (!list.length) return;
    const i = list.findIndex((n) => n.key === ag.sel);
    const next = list[Math.max(0, Math.min(list.length - 1, i < 0 ? 0 : i + d))];
    ag.sel = next.key;
    if (next.type !== 'group') openThread(next.key);
  }

  function fold(open: boolean) {
    const n = nodes().find((x) => x.key === ag.sel);
    if (!n) return;
    const gid = n.type === 'group' ? n.group.id : n.type === 'session' ? n.s.group_id : n.type === 'room' ? n.group.id : null;
    if (!gid) return;
    if (open) ag.folded.delete(gid);
    else {
      ag.folded.add(gid);
      ag.sel = `g:${gid}`;
    }
    ag.folded = new Set(ag.folded);
  }

  function newAgent() {
    const { group, session } = threadOf(ag.sel?.startsWith('g:') ? null : ag.sel);
    const gid = ag.sel?.startsWith('g:') ? ag.sel.slice(2) : (group?.id ?? session?.group_id ?? null);
    const g = ag.tree?.groups.find((x) => x.id === gid);
    if (g?.source === 'scion') {
      if (g.room) {
        ag.sel = `r:${g.room.id}`;
        openThread(`r:${g.room.id}`);
      }
      ag.form = { kind: 'scion', group: g.id, room: g.room?.id ?? null };
    } else {
      ag.form = { kind: 'local', group: gid };
    }
    ag.zone = 'thread';
  }

  function threadItems() {
    return items(ag.rows, ag.hidden, ag.expanded, look().boxes.fold_pairs);
  }

  function moveRow(d: 1 | -1) {
    const list = threadItems();
    if (!list.length) return;
    const i = ag.rowSel === null ? list.length : list.findIndex((it) => it.row.id === ag.rowSel);
    const j = Math.max(0, Math.min(list.length - 1, i + d));
    ag.rowSel = list[j].row.id;
  }

  function selectedItem() {
    return threadItems().find((it) => it.row.id === ag.rowSel);
  }

  function toggleExpand() {
    const it = selectedItem() ?? threadItems().at(-1);
    if (!it) return;
    const k = it.run ? it.key : `row:${it.row.id}`;
    if (ag.expanded.has(k)) ag.expanded.delete(k);
    else ag.expanded.add(k);
    ag.expanded = new Set(ag.expanded);
  }

  function reply() {
    const it = selectedItem();
    if (!it) return composerEl?.focus();
    const r = it.row;
    if (r.kind === 'tool' || r.kind === 'thinking') return toggleExpand();
    if (r.request && ag.tree?.requests.some((q) => q.id === r.request) && r.kind === 'ask') {
      ag.replying = r.request;
    } else if (ag.open?.startsWith('r:') && r.who?.kind === 'agent' && ag.open) {
      const k = ag.open;
      ag.drafts[k] = `@${r.who.name} ${ag.drafts[k] ?? ''}`;
    }
    composerEl?.focus();
  }

  function copyRow() {
    const r = selectedItem()?.row;
    if (!r) return;
    const t = r.tool ? [r.tool.name, r.tool.detail, r.tool.result].filter(Boolean).join('\n') : r.text;
    navigator.clipboard?.writeText(t);
  }

  function interrupt() {
    const { session } = threadOf(ag.open);
    if (session) act('interrupt', { session: session.id }).catch(() => {});
  }

  function deliveryKey(e: KeyboardEvent): boolean {
    const r = selectedItem()?.row;
    const st = r?.delivery?.state;
    if (!r || (st !== 'unreachable' && st !== 'failed') || !ag.open) return false;
    const map: Record<string, string> =
      st === 'unreachable' ? { Enter: 'start_and_send', f: 'forward', Escape: 'discard' } : { Enter: 'retry', e: 'edit', Escape: 'cancel' };
    const action = map[e.key];
    if (!action) return false;
    if (action === 'edit') ag.drafts[ag.open] = r.text;
    act('delivery', { thread: ag.open, row: r.id, action }).catch(() => {});
    return true;
  }

  useKeys((e) => {
    if (ag.keys) {
      if (e.key === 'Escape' || e.key === '?') ag.keys = false;
      return true;
    }
    if (mod(e) && e.key === '.') {
      interrupt();
      return true;
    }
    if (isTyping(e) || e.altKey || mod(e)) return false;
    if (ag.form) return false;
    const k = e.key;
    if (k === '?') {
      ag.keys = true;
      return true;
    }
    if (k === '/') {
      filterEl?.focus();
      ag.zone = 'tree';
      return true;
    }
    if (k === 'Tab') {
      ag.zone = ag.zone === 'tree' ? (ag.open ? 'thread' : 'tree') : ag.zone === 'thread' ? 'composer' : 'tree';
      if (ag.zone === 'composer') composerEl?.focus();
      return true;
    }
    // Needs you keys, whenever a request is pinned.
    const req = pinned(ag.open);
    if (req && !denying) {
      if (req.kind === 'approval' && /^[1-9]$/.test(k) && approval?.choose(k)) return true;
      if (req.kind === 'ask' && ['1', '2', '3'].includes(k) && ask?.choose(k)) return true;
      if (req.kind === 'ask' && k === 'r') {
        ag.replying = req.id;
        composerEl?.focus();
        return true;
      }
    }
    if (ag.zone === 'thread' && deliveryKey(e)) return true;
    if (k === 'Escape') {
      if (req && ag.zone !== 'tree') {
        ag.later.add(req.id);
        ag.later = new Set(ag.later);
        return true;
      }
      if (ag.rowSel !== null) ag.rowSel = null;
      ag.zone = 'tree';
      return true;
    }
    if (k === 'n') {
      newAgent();
      return true;
    }
    if (ag.zone === 'tree') {
      if (k === 'j' || k === 'ArrowDown') moveTree(1);
      else if (k === 'k' || k === 'ArrowUp') moveTree(-1);
      else if (k === 'h' || k === 'ArrowLeft') fold(false);
      else if (k === 'l' || k === 'ArrowRight') fold(true);
      else if (k === 'Enter') {
        if (ag.sel?.startsWith('g:')) fold(ag.folded.has(ag.sel.slice(2)));
        else if (ag.sel) {
          openThread(ag.sel);
          ag.zone = 'thread';
        }
      } else return false;
      return true;
    }
    if (ag.zone === 'thread') {
      if (k === 'j' || k === 'ArrowDown') moveRow(1);
      else if (k === 'k' || k === 'ArrowUp') moveRow(-1);
      else if (k === 'e') toggleExpand();
      else if (k === 'Enter') reply();
      else if (k === 'c') copyRow();
      else if (k === 'g') pane?.latest(); else if (k === 't' && threadOf(ag.open).session?.source === 'scion') setTab('transcript');
      else if (k === 'm' && threadOf(ag.open).session?.source === 'scion') setTab('messages');
      else return false;
      return true;
    }
    return false;
  });
</script>

<div class="agents" style={vars}>
  <Tree bind:filterEl onNew={newAgent} />
  <Pane bind:this={pane} bind:composerEl bind:approval bind:ask bind:denying />
  {#if ag.keys}<Keys />{/if}
</div>

<style>
  .agents {
    flex: 1;
    min-width: 0;
    min-height: 0;
    display: grid;
    grid-template-columns: 450px minmax(0, 1fr);
    background: var(--bg-panel);
    overflow: hidden;
    position: relative;
    border-top: 1px solid var(--bg-head);
  }
</style>

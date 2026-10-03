<script lang="ts">
  // An open agent signal in Incoming (2a–2c), under its messages: why it's
  // here, then the agent's answers on number keys (only while open). They act
  // on the agent; the row's own actions (c x t d) only ever touch the signal.
  import { tick } from 'svelte';
  import { call } from '../../lib/api';
  import { clockHM, ui } from '../../lib/app.svelte';
  import { mod } from '../../lib/keys.svelte';
  import Panels from '../agents/Panels.svelte';
  import { ag } from '../agents/state.svelte';
  import type { Request, Session } from '../agents/types';
  import type { AgentInfo, Signal } from './types';

  let {
    s,
    live,
    onDone,
    onOpen,
  }: { s: Signal; live: { sessions: Session[]; requests: Request[] } | null; onDone: (label: string) => void; onOpen: () => void } = $props();

  type Answer = { key: string; label: string; detail?: string; primary?: boolean; run: () => void };

  const a = $derived(s.agent as AgentInfo);
  const req = $derived(a.request ? live?.requests.find((r) => r.id === a.request) : undefined);
  const session = $derived(live?.sessions.find((x) => x.id === a.session));
  let writing = $state<'reply' | 'deny' | 'forward' | null>(null);
  let text = $state('');
  let denyWith = $state<string | null>(null);
  let to = $state('');
  let err = $state<string | null>(null);
  let busy = false;
  let box = $state<HTMLTextAreaElement | HTMLInputElement | HTMLSelectElement>();

  const hm = () => clockHM(ui.nowMs);

  async function run(cmd: string, args: Record<string, unknown>, label: string) {
    if (busy) return;
    busy = true;
    try {
      await call(`agents.${cmd}`, args);
      err = null;
      writing = null;
      onDone(`${label} · ${hm()}`);
    } catch (e) {
      err = String((e as Error).message ?? e);
    } finally {
      busy = false;
    }
  }

  async function write(kind: 'reply' | 'deny' | 'forward') {
    writing = kind;
    text = '';
    await tick();
    box?.focus();
  }

  const delivery = (action: string, label: string, extra: Record<string, unknown> = {}) => run('delivery', { thread: a.thread, row: a.row, action, ...extra }, label);
  const thread = $derived(a.thread ?? `s:${a.session}`);
  // Who else could take an undelivered message: the group's other running Scion agents.
  const others = $derived((live?.sessions ?? []).filter((x) => x.id !== a.session && x.source === 'scion' && !x.parent_id && x.group_id === session?.group_id && x.status !== 'done' && x.status !== 'failed'));

  const answers = $derived.by<Answer[]>(() => {
    switch (a.kind) {
      case 'approval':
        return (req?.options ?? []).map((o, i) => ({
          key: String(i + 1),
          label: o.name || o.kind.replace('_', ' '),
          primary: i === 0 && o.kind === 'allow_once',
          run: () => {
            if (o.kind.startsWith('reject')) {
              denyWith = o.id;
              write('deny');
            } else run('respond', { request: req!.id, option: o.id }, o.kind === 'allow_once' ? 'Allowed once' : o.name);
          },
        }));
      case 'ask':
        return (req?.choices ?? []).map((c) => ({
          key: c.key,
          label: c.title,
          detail: c.detail,
          run: () => run('answer', req!.form ? { request: req!.id, choice: c.key } : { request: req!.id, text: c.title }, `Answered: ${c.title}`),
        }));
      case 'undelivered':
        return [
          { key: '1', label: 'Start it again and send', primary: true, run: () => delivery('start_and_send', 'Started again and sent') },
          { key: '2', label: 'Send to another agent', run: () => write('forward') },
          { key: '3', label: 'Discard', run: () => delivery('discard', 'Discarded') },
        ];
      case 'failed_delivery':
        return [
          { key: '1', label: 'Retry', primary: true, run: () => delivery('retry', 'Retried') },
          {
            key: '2',
            label: 'Edit',
            run: () => {
              // As in Agents: the text goes back to the composer, then the message is cancelled.
              if (a.thread) ag.drafts[a.thread] = s.messages.at(-1)?.text ?? '';
              delivery('edit', 'Back in the composer').then(onOpen);
            },
          },
          { key: '3', label: 'Cancel', run: () => delivery('cancel', 'Cancelled') },
        ];
      case 'failed':
        return [
          { key: '1', label: 'Open in Agents', primary: true, run: onOpen },
          { key: '2', label: session?.source === 'scion' ? 'Open on Hub' : 'Open in Zed', run: () => call('agents.open', { session: a.session, how: 'app' }) },
        ];
      default:
        return [];
    }
  });

  const canReply = $derived(a.kind === 'wrote' || a.kind === 'turn' || (a.kind === 'ask' && !!req));

  /** Number keys: one of the agent's answers. */
  export function choose(key: string): boolean {
    if (writing) return false;
    const x = answers.find((x) => x.key === key);
    if (!x) return false;
    x.run();
    return true;
  }

  /** `r`: a reply under the messages. */
  export function reply(): boolean {
    if (!canReply || writing) return false;
    write('reply');
    return true;
  }

  function send() {
    const t = text.trim();
    if (writing === 'deny') return run('respond', { request: req!.id, option: denyWith, reason: t }, 'Denied');
    if (writing === 'forward') return to && delivery('forward', `Sent to ${live?.sessions.find((x) => x.id === to)?.name ?? 'another agent'}`, { to });
    if (!t) return;
    if (a.kind === 'ask') return run('answer', { request: req!.id, text: t }, 'Replied');
    return run('send', { thread, text: t }, 'Replied');
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      writing = null;
      e.preventDefault();
    } else if ((mod(e) || writing !== 'reply') && e.key === 'Enter') {
      send();
      e.preventDefault();
    }
    e.stopPropagation();
  }
</script>

<div class="part">
  {#if a.why}<div class="why">{a.why}</div>{/if}
  {#if a.kind === 'approval'}
    {#if req}
      {#if req.command}<div class="command">{req.command}</div>{/if}
      {#if req.message}<div class="said"><i class="ph ph-chat-text"></i>{req.message}</div>{/if}
      {#if req.parsed || req.explained}<Panels {req} />{/if}
    {:else}
      <div class="why">No longer waiting.</div>
    {/if}
  {:else if a.kind === 'done' && a.facts}
    {@const f = a.facts}
    <div class="facts">
      {#if f.task}<span><b>task</b> {f.task}</span>{/if}
      {#if f.ran}<span><b>ran</b> {f.ran} · {f.turns} turns</span>{/if}
      {#if f.pr}<span><b>PR</b> {f.pr.label}{#if f.pr.detail}&nbsp;{f.pr.detail}{/if}</span>{/if}
      {#if f.ticket}<span><b>ticket</b> {f.ticket}</span>{/if}
    </div>
  {:else if a.kind === 'turn' && session && !session.live}
    <div class="why">Not attached since a restart: a reply starts the harness again and resumes the session.</div>
  {/if}

  {#if writing}
    <div class="write">
      {#if writing === 'forward'}
        <select bind:this={box} bind:value={to} onkeydown={onKey}>
          <option value="">Send to…</option>
          {#each others as o (o.id)}<option value={o.id}>{o.name}</option>{/each}
        </select>
        {#if !others.length}<span class="note">No other running agent in this group.</span>{/if}
      {:else if writing === 'deny'}
        <input bind:this={box} bind:value={text} placeholder="Why not? Sent as your next message (optional)" onkeydown={onKey} />
      {:else}
        <textarea bind:this={box} bind:value={text} rows="2" placeholder={a.kind === 'turn' ? 'Your next prompt…' : 'Reply…'} onkeydown={onKey}></textarea>
      {/if}
      <span class="note">{writing === 'reply' ? '⌘⏎ send' : '⏎ send'} · esc</span>
    </div>
  {:else if answers.length || canReply}
    <div class="answers">
      {#each answers as x (x.key)}
        <button class="ans" class:primary={x.primary} class:card={!!x.detail} onclick={(e) => (e.stopPropagation(), x.run())}>
          <span class="lab">{x.label}{#if x.detail}<span class="det">{x.detail}</span>{/if}</span><kbd>{x.key}</kbd>
        </button>
      {/each}
      {#if canReply}<button class="ans" onclick={(e) => (e.stopPropagation(), reply())}><span class="lab">Reply…</span><kbd>r</kbd></button>{/if}
      <span class="note">{answers.length ? 'Number keys answer while it’s open' : ''}{a.kind === 'approval' ? ' · the agent keeps any Always rule' : ''}</span>
    </div>
  {/if}
  {#if err}<div class="err"><i class="ph ph-warning"></i>{err}</div>{/if}
</div>

<style>
  .part {
    display: flex;
    flex-direction: column;
    gap: 8px;
    margin-top: 4px;
  }
  .why {
    font-size: 0.923rem;
    color: var(--tx-label);
  }
  .command {
    padding: 5px 10px;
    border-radius: 5px;
    background: var(--bg-app);
    border: 1px solid var(--border);
    font: 0.923rem/1.55 var(--font-mono);
    color: var(--tx-body);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    max-height: 160px;
    overflow-y: auto;
  }
  .said {
    display: flex;
    gap: 8px;
    align-items: baseline;
    font-size: 0.962rem;
    color: var(--tx-2);
    white-space: pre-wrap;
  }
  .said i {
    color: var(--tx-label);
  }
  .facts {
    display: flex;
    flex-wrap: wrap;
    gap: 4px 16px;
    font: 0.885rem var(--font-mono);
    color: var(--tx-2);
  }
  .facts b {
    font-weight: 400;
    color: var(--tx-label);
  }
  .answers {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    align-items: stretch;
  }
  .ans {
    display: flex;
    gap: 10px;
    align-items: center;
    padding: 4px 10px;
    border-radius: var(--r-input);
    border: 1px solid var(--border-strong);
    color: var(--tx-2);
    text-align: left;
  }
  .ans:hover {
    border-color: var(--tx-label);
  }
  .ans.primary {
    background: rgba(142, 192, 124, 0.14);
    border-color: var(--aqua);
    color: var(--aqua);
  }
  .ans.card {
    flex: 1 1 220px;
    align-items: flex-start;
    justify-content: space-between;
    background: var(--bg-panel);
  }
  .lab {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .det {
    font-size: 0.885rem;
    color: var(--tx-label);
  }
  kbd {
    font: 0.808rem var(--font-mono);
    color: var(--tx-label);
  }
  .note {
    align-self: center;
    font-size: 0.846rem;
    color: var(--tx-disabled);
  }
  .write {
    display: flex;
    gap: 8px;
    align-items: center;
  }
  .write textarea,
  .write input,
  .write select {
    flex: 1;
    min-width: 0;
    padding: 5px 10px;
    border-radius: var(--r-input);
    background: var(--bg-app);
    border: 1px solid var(--aqua);
    color: var(--tx-body);
    font: inherit;
    outline: none;
    resize: vertical;
  }
  .err {
    display: flex;
    gap: 6px;
    align-items: center;
    font-size: 0.923rem;
    color: var(--orange);
  }
</style>

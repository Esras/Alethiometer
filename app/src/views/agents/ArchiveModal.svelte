<script lang="ts">
  // The archive confirm (Agents Archive 02): the tree as archiving will change it.
  import { chord, mod } from '../../lib/keys.svelte';
  import Modal from '../../lib/ui/Modal.svelte';
  import StatusMark from '../../lib/ui/StatusMark.svelte';
  import { ag, confirmArchive } from './state.svelte';
  import type { Plan } from './types';

  let { plan }: { plan: Plan } = $props();

  const close = () => (ag.plan = null);
  const blocked = $derived(plan.state === 'blocked');
  const notDone = $derived(plan.state === 'not_done');
  const whole = $derived(plan.kind === 'group');
  const agents = $derived(plan.hub + plan.local);
  const n = (k: number, one: string, many: string) => (k === 1 ? one : many.replace('#', String(k)));
  const unpushed = $derived(plan.reasons.some((r) => r.note === 'lost with its clone'));

  const consequence = $derived.by(() => {
    const parts: string[] = [];
    if (plan.hub) parts.push(`${n(plan.hub, whole ? '1 agent and its clone' : 'Its agent and clone', '# agents and their clones')} will be deleted on the Hub${unpushed ? ', with the unpushed commits' : ''}.`);
    if (plan.sandboxes) parts.push(`${n(plan.sandboxes, 'Its OpenShell sandbox', '# OpenShell sandboxes')} will be deleted, with ${plan.sandboxes === 1 ? 'its' : 'their'} files.`);
    if (plan.local) parts.push(`${n(plan.local, 'Its local session stops', '# local sessions stop')}. Transcripts stay searchable.`);
    return parts.join(' ');
  });

  const STATUS: Record<string, string> = { running: 'running', waiting: 'waiting for you', idle: 'stopped', done: 'completed', failed: 'failed' };

  function onkeys(e: KeyboardEvent): boolean {
    if (!mod(e)) return false;
    if (e.key === 'Enter' && plan.state === 'ok') confirmArchive(false);
    else if (e.key === 'Backspace' && notDone) confirmArchive(true);
    else return false;
    return true;
  }
</script>

<Modal label="Archive" onclose={close} {onkeys} inset="32px 0 0 0" isolate>
  <div class="box modal agent-form">
    <div class="top">
      {#if blocked}<i class="ph ph-prohibit no"></i><span class="t">Can't archive</span>{:else}<i class="ph ph-archive"></i><span class="t">Archive</span>{/if}
      <span class="kind">{plan.kind}</span>
      <span class="name">{plan.name}</span>
      <span class="sub">{plan.place}</span>
      <kbd>Esc</kbd>
    </div>
    <div class="body">
      {#if blocked}
        <p class="why">{plan.name} still has {n(plan.going, '1 agent', '# agents')} going. Stop {plan.going === 1 ? 'it' : 'them'}, then archive it.</p>
      {:else if notDone}
        <div class="notdone">
          <div class="nd-head"><i class="ph ph-warning"></i>Not done</div>
          {#each plan.reasons as r (r.text)}
            <div class="reason"><span>{r.text}</span>{#if r.note}<span class="note">{r.note}</span>{/if}</div>
          {/each}
        </div>
      {/if}
      <div class="tree inset" class:whole={whole && !blocked}>
        {#each plan.rows as r, i (i)}
          <div class="row {r.kind}" class:lit={r.lit && !blocked} class:dim={!r.lit || (blocked && r.tag === 'ready')} style:--indent="{14 * r.depth}px">
            <span class="mark">
              {#if r.kind === 'group'}<i class="ph ph-caret-down"></i>{:else if r.kind === 'room'}<i class="ph ph-hash hash"></i>{:else if r.status}<StatusMark status={r.status} />{/if}
            </span>
            <span class="rname">{r.name}</span>
            <span class="meta">{r.kind === 'session' && r.status ? STATUS[r.status] : r.kind}</span>
            <span class="tag" class:will={r.tag === 'will archive'} class:running={r.tag === 'running'} class:waiting={r.tag === 'waiting for you'}>{r.tag}</span>
          </div>
        {/each}
      </div>
      {#if !blocked && consequence}<p class="what"><i class="ph ph-trash"></i>{consequence}</p>{/if}
    </div>
    <div class="foot">
      {#if blocked}
        <button class="btn md" onclick={close}>Close<kbd>Esc</kbd></button>
      {:else if notDone}
        <button class="btn md danger" onclick={() => confirmArchive(true)}>Archive anyway<kbd>{chord('⌘⌫', 'Ctrl+⌫')}</kbd></button>
        <button class="btn md" onclick={close}>Cancel<kbd>Esc</kbd></button>
        <span class="undo"><kbd>u</kbd> undoes for 10s</span>
      {:else}
        <button class="btn md primary" onclick={() => confirmArchive(false)}
          >{whole ? `Archive group${agents ? ` and ${n(agents, '1 agent', '# agents')}` : ''}` : 'Archive session'}<kbd>{chord('⌘⏎', 'Ctrl+⏎')}</kbd></button
        >
        <button class="btn md" onclick={close}>Cancel<kbd>Esc</kbd></button>
        <span class="undo"><kbd>u</kbd> undoes for 10s</span>
      {/if}
    </div>
  </div>
</Modal>

<style>
  .box {
    width: min(560px, calc(100vw - 48px));
    max-height: calc(100vh - 80px);
    border-radius: var(--radius-md);
    padding: 0;
    display: flex;
    flex-direction: column;
  }
  .top .no {
    color: var(--status-attention);
  }
  .kind {
    font: 0.808rem var(--font-mono);
    text-transform: uppercase;
    color: var(--tx-label);
    border: 1px solid var(--tx-disabled);
    border-radius: var(--radius-sm);
    padding: 0 5px;
  }
  .name {
    color: var(--tx-body);
    font-weight: 500;
  }
  .body {
    padding: 12px;
    display: flex;
    flex-direction: column;
    gap: 10px;
    overflow-y: auto;
    min-height: 0;
  }
  .why,
  .what {
    margin: 0;
    color: var(--tx-2);
  }
  .what {
    display: flex;
    gap: 8px;
    align-items: baseline;
    color: var(--tx-muted-2);
  }
  .notdone {
    border-radius: var(--radius-md);
    border: 1px solid color-mix(in srgb, var(--status-headsup) 45%, transparent);
    background: color-mix(in srgb, var(--status-headsup) 8%, transparent);
    padding: 7px 10px;
    display: flex;
    flex-direction: column;
    gap: 3px;
  }
  .nd-head {
    display: flex;
    gap: 6px;
    align-items: center;
    color: var(--status-headsup);
    font-weight: 500;
  }
  .reason {
    display: flex;
    gap: 10px;
    color: var(--tx-2);
  }
  .reason .note {
    color: var(--tx-label);
  }
  .tree {
    padding: 4px 0;
  }
  .tree.whole {
    background: var(--aqua-tint);
  }
  .row {
    display: grid;
    grid-template-columns: calc(22px + var(--indent)) minmax(0, 1fr) auto 104px;
    gap: 6px;
    align-items: center;
    padding: var(--row-pad-compact) 10px;
    min-height: var(--row-min-compact);
  }
  .row.lit {
    background: var(--aqua-tint);
  }
  .tree.whole .row.lit {
    background: none;
  }
  .row.dim {
    opacity: 0.5;
  }
  .mark {
    display: flex;
    justify-content: flex-end;
    color: var(--tx-label);
  }
  .hash {
    color: var(--msg, #9a9fd6);
  }
  .rname {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    color: var(--tx-body);
  }
  .row.group .rname {
    font-weight: 500;
  }
  .meta,
  .tag {
    font: 0.846rem var(--font-mono);
    color: var(--tx-label);
    white-space: nowrap;
  }
  .tag {
    text-align: right;
  }
  .tag.will {
    color: var(--aqua);
  }
  .tag.running {
    color: var(--aqua);
  }
  .tag.waiting {
    color: var(--status-attention);
  }
  .foot {
    border-top: 1px solid var(--border);
  }
  .danger {
    border-color: var(--red);
    color: var(--red);
    background: var(--pill-red-fill);
    font-weight: 500;
  }
  .danger kbd {
    color: var(--red);
    background: transparent;
    border-color: color-mix(in srgb, var(--red) 45%, transparent);
  }
  .undo {
    margin-left: auto;
    color: var(--tx-label);
    font-size: 0.885rem;
  }
</style>

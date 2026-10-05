<script lang="ts">
  // A pinned approval (02, 03): the command, the deterministic parse, the small
  // model's explanation, and the agent's own options. "Always" is whatever the
  // agent offers; the agent keeps that rule (app/docs/agents.md).
  import Panels from './Panels.svelte';
  import { act } from './state.svelte';
  import type { Request } from './types';
  import NeedCard from './NeedCard.svelte';

  let { req, compact = false, denying = $bindable(false) }: { req: Request; compact?: boolean; denying?: boolean } = $props();

  let reason = $state('');
  let reasonEl = $state<HTMLInputElement>();
  $effect(() => {
    if (denying) reasonEl?.focus();
  });

  // Long pipelines read better one stage per line, as the design shows them.
  const pretty = $derived(req.command.length > 70 && req.command.includes(' | ') ? req.command.replace(/\s+\|\s+/g, ' \\\n  | ') : req.command);
  type Option = Request['options'][number];
  const isDeny = (o: Option) => o.kind.startsWith('reject');
  // The first plain allow is the default; a connector that marks every choice allow_once gets none.
  const primary = $derived(req.options.filter((o) => o.kind === 'allow_once').length === 1 ? req.options.find((o) => o.kind === 'allow_once') : undefined);
  let denyOpt = $state<Option>();

  export function choose(key: string) {
    const o = req.options.find((o) => o.key === key);
    if (!o) return false;
    if (isDeny(o)) {
      denyOpt = o;
      denying = true;
      return true;
    }
    act('respond', { request: req.id, option: o.id }).catch(() => {});
    return true;
  }

  function sendDeny() {
    const o = denyOpt ?? req.options.find(isDeny);
    if (!o) return;
    act('respond', { request: req.id, option: o.id, reason }).catch(() => {});
    denying = false;
    reason = '';
  }

  // The agent's own words: a connector that gets the kinds wrong can't make "No" read as "Allow once".
  const label = (o: Option) => o.name.trim() || (isDeny(o) ? 'Deny' : o.kind === 'allow_once' ? 'Allow once' : 'Always');
</script>

{#snippet title()}{req.title}{/snippet}
{#snippet meta()}
  <span class="tool">{req.tool}</span>
  <button class="btn copy" onclick={() => navigator.clipboard?.writeText(req.command)}><i class="ph ph-copy"></i>Copy</button>
{/snippet}
{#snippet summary()}
  <span class="c">{req.command}</span>
{/snippet}

<NeedCard icon="shield-warning" {compact} {title} {meta} {summary} maxHeight="55vh">
    {#if req.command}<div class="command command-block">{pretty}</div>{/if}
    {#if req.message}<div class="said" title="Sent by the agent with the request; the explainer never sees it"><i class="ph ph-chat-text"></i><span>{req.message}</span></div>{/if}
    {#if req.parsed || req.explained}<div class="pw"><Panels {req} /></div>{/if}
    {#if denying}
      <div class="deny">
        <input
          bind:this={reasonEl}
          bind:value={reason}
          placeholder="Why not? Sent as your next message (optional)"
          onkeydown={(e) => {
            if (e.key === 'Enter') {
              sendDeny();
              e.preventDefault();
            } else if (e.key === 'Escape') {
              denying = false;
              e.preventDefault();
            }
            e.stopPropagation();
          }}
        />
        <button class="btn md" onclick={sendDeny}>Deny<span class="k">⏎</span></button>
        <button class="btn md" onclick={() => (denying = false)}>Back<span class="k">Esc</span></button>
      </div>
    {:else}
      <div class="btns">
        {#each req.options as o (o.id)}
          <button class="btn md" class:primary={o === primary} title={o.kind.replace('_', ' ')} onclick={() => choose(o.key)}>{label(o)}{#if isDeny(o)}…{/if}{#if o.key}<span class="k">{o.key}</span>{/if}</button>
        {/each}
        <span class="right">Esc later</span>
      </div>
    {/if}
</NeedCard>

<style>
  .tool {
    font: 0.885rem var(--font-mono);
    color: var(--tx-label);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .copy {
    margin-left: auto;
  }
  .command {
    flex: none;
    margin: 0 12px;
  }
  .said {
    margin: 6px 12px 0;
    display: flex;
    gap: 8px;
    align-items: baseline;
    font-size: 0.962rem;
    line-height: 1.5;
    color: var(--tx-2);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    max-height: 120px;
    overflow-y: auto;
  }
  .said i {
    color: var(--tx-label);
  }
  .pw {
    margin: 8px 12px 0;
    min-height: 0;
    overflow-y: auto;
  }
  .btns {
    flex-wrap: wrap;
  }
  .btns,
  .deny {
    display: flex;
    gap: 8px;
    align-items: center;
    padding: 9px 12px;
  }
  .btns .btn {
    max-width: 420px;
    text-align: left;
    white-space: normal;
    overflow-wrap: anywhere;
  }
  .k {
    flex: none;
    font: 0.808rem var(--font-mono);
    color: var(--tx-label);
  }
  .primary .k {
    color: var(--aqua);
    opacity: 0.75;
  }
  .right {
    margin-left: auto;
    font-size: 0.885rem;
    color: var(--tx-label);
  }
  .deny input {
    flex: 1;
    min-width: 0;
    padding: 5px 10px;
    border-radius: var(--r-input);
    background: var(--bg-app);
    border: 1px solid var(--aqua);
    outline: none;
    font-size: 1rem;
  }
  .c {
    flex: 1;
    min-width: 0;
    font: 0.923rem var(--font-mono);
    color: var(--tx-2);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
</style>

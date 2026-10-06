<script lang="ts">
  // A pinned approval (02, 03): the command, the deterministic parse, the small
  // model's explanation, and the agent's own options. "Always" is whatever the
  // agent offers; the agent keeps that rule (app/docs/agents.md).
  import Panels from './Panels.svelte';
  import { act } from './state.svelte';
  import type { Request } from './types';
  import NeedCard from './NeedCard.svelte';
  import { fieldsLine, oneLine, splitMessage } from './approvalMessage';

  let { req, compact = false, denying = $bindable(false) }: { req: Request; compact?: boolean; denying?: boolean } = $props();

  // What came with the request (a gate's prompt or free text): the parts of a
  // field list that the card doesn't already show go in the header, free text gets
  // a line of its own that opens on a click.
  const said = $derived(splitMessage(req.message ?? '', req.command, req.tool.split(' · ')[0]));
  let sayOpen = $state(false);
  $effect(() => {
    req.id;
    sayOpen = false;
  });

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
  {#if said.fields.length}<span class="tool gate" title={req.message}>{fieldsLine(said.fields)}</span>{/if}
  <button class="btn copy" onclick={() => navigator.clipboard?.writeText(req.command)}><i class="ph ph-copy"></i>Copy</button>
{/snippet}
{#snippet summary()}
  <span class="c">{req.command}</span>
{/snippet}

<NeedCard icon="shield-warning" {compact} {title} {meta} {summary} capped>
    {#if req.command}<div class="command inset command-block">{pretty}</div>{/if}
    {#if said.text}
      <button class="said" aria-expanded={sayOpen} class:open={sayOpen} onclick={() => (sayOpen = !sayOpen)} title={sayOpen ? 'Sent with the request; the explainer never sees it. Click to fold' : said.text}>
        <i class="ph ph-chat-text"></i><span>{sayOpen ? said.text : oneLine(said.text)}</span><i class="ph ph-caret-{sayOpen ? 'up' : 'down'} caret"></i>
      </button>
    {/if}
    {#if req.parsed || req.explained}<div class="pw"><Panels {req} fill /></div>{/if}
    {#if denying}
      <div class="deny">
        <input
          class="input"
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
        <button class="btn md" onclick={sendDeny}>Deny<kbd>⏎</kbd></button>
        <button class="btn md" onclick={() => (denying = false)}>Back<kbd>Esc</kbd></button>
      </div>
    {:else}
      <div class="btns">
        {#each req.options as o (o.id)}
          <button class="btn md" class:primary={o === primary} title={o.kind.replace('_', ' ')} onclick={() => choose(o.key)}>{label(o)}{#if isDeny(o)}…{/if}{#if o.key}<kbd>{o.key}</kbd>{/if}</button>
        {/each}
        <span class="right"><kbd>Esc</kbd> later</span>
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
  /* What a gate's field list adds to the card (`rule *`), beside the tool. */
  .gate {
    color: var(--tx-2);
  }
  .copy {
    margin-left: auto;
    flex: none;
  }
  .command {
    flex: none;
    margin: 0 12px;
  }
  /* The agent's text: one line until clicked, then as sent (wrapped, 120px, scrolling). */
  .said {
    flex: none;
    margin: 6px 12px 0;
    padding: 0;
    border: 0;
    background: none;
    font: inherit;
    text-align: left;
    cursor: pointer;
    display: flex;
    gap: 8px;
    align-items: baseline;
    font-size: 0.962rem;
    line-height: 1.5;
    color: var(--tx-2);
    min-width: 0;
  }
  .said span {
    flex: 1;
    min-width: 0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .said.open span {
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    overflow-y: auto;
    max-height: 120px;
  }
  .said i {
    color: var(--tx-label);
  }
  .said .caret {
    flex: none;
    font-size: 0.846rem;
  }
  /* The part of the card that gives way when the pane is short (the transcript keeps
     its own minimum in Pane): Parsed and Explained scroll separately inside it. */
  .pw {
    margin: 8px 12px 0;
    flex: 0 1 auto;
    min-height: 80px;
    display: flex;
    flex-direction: column;
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
    /* If the pane is so short that the card itself scrolls, the answers stay in reach. */
    position: sticky;
    bottom: 0;
    margin-top: auto;
    flex: none;
    background: linear-gradient(var(--need-soft), var(--need-soft)), var(--bg-app);
    border-radius: 0 0 var(--radius-md) var(--radius-md);
  }
  .btns .btn {
    max-width: 420px;
    text-align: left;
    white-space: normal;
    overflow-wrap: anywhere;
  }
  .btns kbd {
    flex: none;
  }
  .right {
    margin-left: auto;
    font-size: 0.885rem;
    color: var(--tx-label);
  }
  .deny input {
    flex: 1;
    min-width: 0;
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

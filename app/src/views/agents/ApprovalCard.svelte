<script lang="ts">
  // A pinned approval (02, 03): the command, the deterministic parse, the small
  // model's explanation, and the agent's own options. "Always" is whatever the
  // agent offers; the agent keeps that rule (app/docs/agents.md).
  import { act } from './state.svelte';
  import type { Request } from './types';

  let { req, compact = false, denying = $bindable(false) }: { req: Request; compact?: boolean; denying?: boolean } = $props();

  let reason = $state('');
  let reasonEl = $state<HTMLInputElement>();
  $effect(() => {
    if (denying) reasonEl?.focus();
  });

  // Long pipelines read better one stage per line, as the design shows them.
  const pretty = $derived(req.command.length > 70 && req.command.includes(' | ') ? req.command.replace(/\s+\|\s+/g, ' \\\n  | ') : req.command);
  const allow = $derived(req.options.filter((o) => o.kind.startsWith('allow')));
  const deny = $derived(req.options.find((o) => o.kind.startsWith('reject')));
  const verdictClass = (v: string) => (v === 'read-only' ? 'green' : v === 'unknown' ? 'yellow' : 'orange');
  const tagClass = (t: string) =>
    ['reads files', 'reads git'].includes(t) ? 'blue' : ['local bin'].includes(t) ? 'green' : ['shell per file', 'runs shell', 'env', 'unknown command'].includes(t) ? 'yellow' : 'orange';

  export function choose(key: string) {
    const o = req.options.find((o) => o.key === key);
    if (!o) return false;
    if (o.kind.startsWith('reject')) {
      denying = true;
      return true;
    }
    act('respond', { request: req.id, option: o.id }).catch(() => {});
    return true;
  }

  function sendDeny() {
    if (!deny) return;
    act('respond', { request: req.id, option: deny.id, reason }).catch(() => {});
    denying = false;
    reason = '';
  }

  const label = (o: Request['options'][number]) => {
    if (o.kind === 'allow_once') return 'Allow once';
    if (o.key === 's') return 'This session';
    return `Always: ${o.name}`;
  };
</script>

{#if compact}
  <div class="one">
    <i class="ph ph-shield-warning"></i>
    <span class="t">{req.title}</span>
    <span class="c">{req.command}</span>
    <span class="hint">back after Esc</span>
  </div>
{:else}
  <div class="card">
    <div class="top">
      <i class="ph ph-shield-warning big"></i>
      <span class="t">{req.title}</span>
      <span class="tool">{req.tool}</span>
      <button class="copy" onclick={() => navigator.clipboard?.writeText(req.command)}><i class="ph ph-copy"></i>Copy</button>
    </div>
    {#if req.command}<div class="command">{pretty}</div>{/if}
    {#if req.parsed}
      <div class="panels" class:solo={!req.explained}>
        <div class="panel">
          <div class="ph-head">
            <i class="ph ph-tree-structure" style:color="var(--blue)"></i>Parsed<span class="sub">shell parser · same answer every time</span>
            <span class="badge-wrap"><span class="tag {verdictClass(req.parsed.verdict)}">{req.parsed.verdict}</span></span>
          </div>
          {#each req.parsed.steps as s (s.n)}
            <div class="step" class:deep={s.depth > 0}>
              <span class="n">{s.n}</span>
              <span class="st">{s.text}</span>
              <span class="tags">{#each s.tags as t (t)}<span class="tag {tagClass(t)}">{t}</span>{/each}</span>
            </div>
          {/each}
          {#if req.parsed.checks.length}
            <div class="checks">
              {#each req.parsed.checks as c (c.text)}
                <span class:warn={!c.ok}><i class="ph {c.ok ? 'ph-check' : 'ph-warning'}"></i>{c.text}</span>
              {/each}
            </div>
          {/if}
        </div>
        {#if req.explained}
          <div class="panel">
            <div class="ph-head"><i class="ph ph-sparkle" style:color="var(--purple)"></i>Explained<span class="sub">{req.explained.model}</span></div>
            {#if req.explained.state === 'pending'}<div class="ex muted">Asking…</div>
            {:else if req.explained.state === 'ok'}<div class="ex">{req.explained.text}</div>
            {:else}<div class="ex muted">{req.explained.state === 'off' ? 'Off: ' : 'Couldn’t explain: '}{req.explained.text}</div>{/if}
          </div>
        {/if}
      </div>
    {/if}
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
        <button class="b" onclick={sendDeny}>Deny<span class="k">⏎</span></button>
        <button class="b" onclick={() => (denying = false)}>Back<span class="k">Esc</span></button>
      </div>
    {:else}
      <div class="btns">
        {#each allow as o (o.id)}
          <button class="b" class:primary={o.kind === 'allow_once'} title={o.name} onclick={() => choose(o.key)}>{label(o)}{#if o.key}<span class="k">{o.key.toUpperCase()}</span>{/if}</button>
        {/each}
        {#if deny}<button class="b" title={deny.name} onclick={() => choose(deny.key)}>Deny…<span class="k">D</span></button>{/if}
        <span class="right">Esc later</span>
      </div>
    {/if}
  </div>
{/if}

<style>
  .card {
    border-radius: var(--r-panel);
    background: var(--need-soft);
    border: 1px solid var(--need-soft-border);
    display: flex;
    flex-direction: column;
  }
  .top {
    display: flex;
    gap: 10px;
    align-items: center;
    padding: 8px 12px 6px;
  }
  .big {
    font-size: 1.231rem;
    color: var(--need);
  }
  .t {
    color: var(--tx-title);
    font-weight: 500;
    white-space: nowrap;
  }
  .tool {
    font: 0.885rem var(--font-mono);
    color: var(--tx-label);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .copy {
    margin-left: auto;
    font-size: 0.923rem;
    color: var(--tx-label);
    display: flex;
    gap: 5px;
    align-items: center;
  }
  .copy:hover {
    color: var(--tx-2);
  }
  .command {
    margin: 0 12px;
    border-radius: 5px;
    background: var(--bg-app);
    border: 1px solid var(--border);
    padding: 5px 10px;
    font: 0.923rem/1.55 var(--font-mono);
    color: var(--tx-body);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    max-height: 160px;
    overflow-y: auto;
  }
  .panels {
    margin: 8px 12px 0;
    display: grid;
    grid-template-columns: minmax(0, 1.35fr) minmax(0, 1fr);
    gap: 10px;
  }
  .panels.solo {
    grid-template-columns: minmax(0, 1fr);
  }
  .panel {
    padding: 7px 10px;
    border-radius: var(--r-input);
    background: var(--bg-panel);
    border: 1px solid var(--border);
    display: flex;
    flex-direction: column;
    gap: 3px;
    min-width: 0;
  }
  .ph-head {
    display: flex;
    gap: 7px;
    align-items: center;
    font-size: 0.846rem;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--tx-label);
    margin-bottom: 2px;
  }
  .ph-head i {
    font-size: 0.923rem;
  }
  .sub {
    letter-spacing: 0;
    text-transform: none;
    font: 0.846rem var(--font-mono);
    color: var(--tx-disabled);
  }
  .badge-wrap {
    margin-left: auto;
    letter-spacing: 0;
    text-transform: none;
  }
  .step {
    display: grid;
    grid-template-columns: 20px minmax(0, 1fr) auto;
    gap: 8px;
    align-items: center;
    padding: 2px 0;
  }
  .step.deep {
    grid-template-columns: 34px minmax(0, 1fr) auto;
  }
  .n {
    font: 0.846rem var(--font-mono);
    color: var(--tx-disabled);
    text-align: right;
  }
  .st {
    font: 0.923rem var(--font-mono);
    color: var(--tx-2);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .tags {
    display: flex;
    gap: 4px;
  }
  .tag {
    font: 0.808rem var(--font-mono);
    border: 1px solid currentColor;
    border-radius: var(--r-block);
    padding: 0 5px;
    white-space: nowrap;
    opacity: 0.9;
  }
  .tag.green {
    color: var(--green);
  }
  .tag.blue {
    color: var(--blue);
  }
  .tag.yellow {
    color: var(--yellow);
  }
  .tag.orange {
    color: var(--orange);
  }
  .checks {
    display: flex;
    gap: 4px 14px;
    flex-wrap: wrap;
    border-top: 1px solid var(--bg-head);
    padding-top: 5px;
    margin-top: 2px;
  }
  .checks span {
    display: flex;
    gap: 5px;
    align-items: center;
    font-size: 0.923rem;
    color: var(--tx-muted);
  }
  .checks i {
    font-size: 0.923rem;
    color: var(--green);
  }
  .checks .warn,
  .checks .warn i {
    color: var(--yellow);
  }
  .ex {
    font-size: 1rem;
    line-height: 1.5;
    color: var(--tx-body);
    text-wrap: pretty;
  }
  .ex.muted {
    color: var(--tx-label);
  }
  .btns,
  .deny {
    display: flex;
    gap: 8px;
    align-items: center;
    padding: 9px 12px;
  }
  .b {
    display: flex;
    gap: 8px;
    align-items: center;
    padding: 5px 12px;
    border-radius: var(--r-input);
    border: 1px solid var(--border-strong);
    color: var(--tx-2);
    white-space: nowrap;
    max-width: 320px;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .b:hover {
    border-color: var(--tx-disabled);
  }
  .b.primary {
    background: rgba(142, 192, 124, 0.14);
    border-color: var(--aqua);
    color: var(--aqua);
    font-weight: 500;
  }
  .k {
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
  .one {
    display: flex;
    gap: 10px;
    align-items: center;
    padding: 6px 12px;
    border-radius: var(--r-panel);
    background: var(--need-soft);
    border: 1px solid var(--need-soft-border);
    min-width: 0;
  }
  .one i {
    font-size: 1.077rem;
    color: var(--need);
  }
  .one .c {
    flex: 1;
    min-width: 0;
    font: 0.923rem var(--font-mono);
    color: var(--tx-2);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .hint {
    font: 0.846rem var(--font-mono);
    color: var(--tx-label);
    white-space: nowrap;
  }
</style>

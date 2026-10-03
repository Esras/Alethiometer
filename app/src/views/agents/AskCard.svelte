<script lang="ts">
  // A pinned ask (Scion input-needed): the question, the choices read from the
  // message as option cards (1–3), Write a reply… (R) and Later (Esc).
  import { agentColor } from './look';
  import { act, ag, age, look, openThread, sessionById } from './state.svelte';
  import type { Request } from './types';

  let { req, compact = false, onReply }: { req: Request; compact?: boolean; onReply: () => void } = $props();

  const s = $derived(sessionById(req.session_id));

  export function choose(key: string): boolean {
    const c = req.choices.find((c) => c.key === key);
    if (!c) return false;
    act('answer', { request: req.id, text: c.title }).catch(() => {});
    return true;
  }
</script>

{#if compact}
  <div class="one">
    <i class="ph ph-question"></i>
    <span class="t"><span style:color={agentColor(look(), s)}>{s?.name}</span> is waiting for you</span>
    <span class="c">{req.question}</span>
    <span class="hint">back after Esc</span>
  </div>
{:else}
  <div class="card">
    <div class="top">
      <i class="ph ph-question big"></i>
      <span class="t">{s?.name ?? 'An agent'} is waiting for you</span>
      <span class="via">{req.via || 'input-needed'} · {age(req.at, Date.now())}</span>
      {#if req.thread !== `s:${req.session_id}`}
        <button class="link" onclick={() => openThread(`s:${req.session_id}`)}><i class="ph ph-arrow-square-out"></i>Open session</button>
      {/if}
    </div>
    <div class="q">{req.question}</div>
    {#if req.choices.length}
      <div class="choices" style:grid-template-columns="repeat({Math.min(req.choices.length, 3)}, minmax(0, 1fr))">
        {#each req.choices as c (c.key)}
          <button class="choice" onclick={() => choose(c.key)}>
            <span class="key">{c.key}</span>
            <span class="ct">{c.title}</span>
            <span></span>
            {#if c.detail}<span class="cd">{c.detail}</span>{/if}
          </button>
        {/each}
      </div>
    {/if}
    <div class="btns">
      <button class="b" class:on={ag.replying === req.id} onclick={onReply}><span class="k">R</span>Write a reply…</button>
      <button class="b" onclick={() => ag.later.add(req.id)}><span class="k">Esc</span>Later</button>
      {#if req.choices.length}<span class="right">Choices read from {s?.name ?? 'the agent'}’s message</span>{/if}
    </div>
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
    padding: 8px 12px 4px;
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
  .via {
    font: 0.885rem var(--font-mono);
    color: var(--tx-label);
  }
  .link {
    margin-left: auto;
    font-size: 0.923rem;
    color: var(--tx-label);
    display: flex;
    gap: 5px;
    align-items: center;
  }
  .link:hover {
    color: var(--tx-2);
  }
  .q {
    padding: 0 12px 0 38px;
    font-size: 1.077rem;
    line-height: 1.5;
    color: var(--tx-title);
    text-wrap: pretty;
  }
  .choices {
    margin: 8px 12px 0;
    display: grid;
    gap: 10px;
  }
  .choice {
    text-align: left;
    padding: 7px 10px;
    border-radius: var(--r-input);
    background: var(--bg-panel);
    border: 1px solid var(--border);
    display: grid;
    grid-template-columns: auto minmax(0, 1fr);
    gap: 2px 9px;
    align-items: baseline;
  }
  .choice:hover {
    border-color: var(--border-strong);
  }
  .key {
    font: 0.846rem var(--font-mono);
    color: var(--tx-body);
    background: var(--bg-head);
    border: 1px solid var(--border-strong);
    border-radius: var(--r-block);
    padding: 0 5px;
  }
  .ct {
    color: var(--tx-body);
  }
  .cd {
    font-size: 0.923rem;
    line-height: 1.45;
    color: var(--tx-muted-2);
    text-wrap: pretty;
  }
  .btns {
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
  }
  .b.on {
    border-color: var(--aqua);
  }
  .k {
    font: 0.808rem var(--font-mono);
    color: var(--tx-label);
  }
  .right {
    margin-left: auto;
    font-size: 0.885rem;
    color: var(--tx-label);
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
  .one .t {
    font-weight: 500;
  }
  .one .c {
    flex: 1;
    min-width: 0;
    font-size: 1rem;
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

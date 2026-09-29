<script lang="ts">
  // Collapsible ask pane. Suggests and answers only; never writes your words.
  // Questions go only to the allow-listed provider; failures are shown, not hidden.
  import { call } from '../lib/api';
  import { catColor, ui } from '../lib/app.svelte';
  import type { TopicRef } from '../lib/types';
  import Markdown from '../lib/ui/Markdown.svelte';

  let { topic = null, scope = null }: { topic?: TopicRef | null; scope?: 'topics' | null } = $props();
  let question = $state('');
  let log = $state<{ q: string; a?: string; err?: string }[]>([]);
  let busy = $state(false);

  async function ask(e: SubmitEvent) {
    e.preventDefault();
    const q = question.trim();
    if (!q || busy) return;
    question = '';
    busy = true;
    const entry = { q } as { q: string; a?: string; err?: string };
    log.push(entry);
    try {
      const r = await call<{ answer: string }>('ai.ask', { question: q, topic_id: topic?.id ?? null, scope });
      log[log.length - 1] = { ...entry, a: r.answer };
    } catch (err) {
      log[log.length - 1] = { ...entry, err: String((err as Error).message ?? err) };
    } finally {
      busy = false;
    }
  }
</script>

{#if ui.assistantOpen}
  <aside class="pane">
    <div class="head">
      <i class="ph ph-sparkle"></i>Assistant
      <button class="close" onclick={() => (ui.assistantOpen = false)} title="Close (⌘I)"><i class="ph ph-caret-double-right"></i></button>
    </div>
    {#if topic}
      <div class="ctx"><span class="sq" style:background={catColor(topic.category)}></span>{topic.name}</div>
    {:else if scope === 'topics'}
      <div class="ctx"><i class="ph ph-stack"></i>All topics</div>
    {/if}
    <div class="provider" title="Questions go only to this allow-listed provider">{ui.info?.ai_label}</div>
    <div class="log scroll">
      {#each log as m}
        <div class="q">{m.q}</div>
        {#if m.a}<div class="a"><Markdown text={m.a} /></div>{:else if m.err}<div class="err"><i class="ph ph-warning"></i>{m.err}</div>{:else}<div class="a pending">Thinking…</div>{/if}
      {/each}
    </div>
    <form class="ask" onsubmit={ask}>
      <input bind:value={question} placeholder="Ask, or @ to reference…" disabled={busy} />
    </form>
  </aside>
{:else}
  <button class="rail" onclick={() => (ui.assistantOpen = true)} title="Assistant (⌘I)">
    <i class="ph ph-caret-double-left"></i><i class="ph ph-sparkle spark"></i>
  </button>
{/if}

<style>
  .pane {
    width: 300px;
    flex: none;
    display: flex;
    flex-direction: column;
    background: var(--bg-panel);
    border-left: 1px solid var(--border);
  }
  .head {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 12px 14px;
    font-weight: 500;
  }
  .head > i {
    color: var(--aqua);
  }
  .close {
    margin-left: auto;
    color: var(--tx-muted-2);
    font-size: 15px;
    display: flex;
  }
  .ctx {
    margin: 0 14px;
    padding: 6px 8px;
    border-radius: var(--r-input);
    border: 1px solid var(--border);
    font-size: 12px;
    color: var(--tx-muted);
    display: flex;
    gap: 6px;
    align-items: center;
  }
  .sq {
    width: 8px;
    height: 8px;
    border-radius: 3px;
  }
  .provider {
    margin: 8px 14px 0;
    font-size: 11px;
    color: var(--tx-label);
  }
  .log {
    flex: 1;
    min-height: 0;
    padding: 10px 14px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    font-size: 12.5px;
    line-height: 1.5;
  }
  .q {
    color: var(--tx-title);
  }
  .a {
    color: var(--tx-2);
  }
  .a.pending {
    opacity: 0.6;
    font-style: italic;
  }
  .err {
    color: var(--orange);
    display: flex;
    gap: 6px;
  }
  .ask {
    padding: 10px 12px 12px;
  }
  .ask input {
    width: 100%;
    padding: 9px 11px;
    border-radius: var(--r-panel);
    border: 1px solid var(--border);
    background: var(--bg-app);
    font-size: 13px;
    outline: none;
  }
  .ask input:focus {
    border-color: var(--aqua);
  }
  .rail {
    width: 40px;
    flex: none;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 10px;
    padding-top: 12px;
    background: var(--bg-panel);
    border-left: 1px solid var(--border);
    color: var(--tx-muted-2);
    font-size: 15px;
  }
  .spark {
    font-size: 17px;
    color: var(--aqua);
  }
</style>

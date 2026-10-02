<script lang="ts">
  // Collapsible ask pane. Suggests and answers only; never writes your words.
  // Questions go only to the allow-listed provider; failures are shown, not hidden.
  //
  // With a Glean source on (hi-fi 3a): Glean searches, the large model answers
  // from what the local check kept, citing it by number. "Sent with your
  // question" chips say what context goes along; removing one (or adding the
  // note) asks again without it. `e` opens Sources over the view.
  import { call } from '../lib/api';
  import { catColor, ui } from '../lib/app.svelte';
  import { isTyping, useKeys } from '../lib/keys.svelte';
  import type { TopicRef } from '../lib/types';
  import AppIcon from '../lib/ui/AppIcon.svelte';
  import Markdown from '../lib/ui/Markdown.svelte';
  import GleanAnswer from './assistant/GleanAnswer.svelte';
  import SourcesPanel from './assistant/SourcesPanel.svelte';
  import type { Chip, Reply } from './assistant/types';

  let { topic = null, scope = null }: { topic?: TopicRef | null; scope?: 'topics' | null } = $props();
  type Entry = { q: string; a?: string; reply?: Reply; err?: string };
  let question = $state('');
  let log = $state<Entry[]>([]);
  let busy = $state(false);
  let chips = $state<Chip[]>([]);
  let off = $state<string[]>([]);
  let on = $state<string[]>([]);
  let sources = $state(false);
  let hoverQuery = $state<number | null>(null);
  let hoverSource = $state<string | null>(null);
  let hoverPerson = $state<string | null>(null);
  let note = $state<string | null>(null);

  const glean = $derived(!!ui.info?.glean);
  const last = $derived([...log].reverse().find((m) => m.reply)?.reply ?? null);

  // What would be sent, before asking too.
  $effect(() => {
    const id = topic?.id ?? null;
    if (!glean || !ui.assistantOpen) return;
    call<Chip[]>('ai.chips', { topic_id: id, off, on }).then((c) => (chips = c));
  });

  // A different topic starts over.
  let lastTopic: number | null | undefined = undefined;
  $effect(() => {
    const id = topic?.id ?? null;
    if (lastTopic !== undefined && id !== lastTopic) {
      log = [];
      off = [];
      on = [];
      sources = false;
    }
    lastTopic = id;
  });

  async function run(q: string) {
    if (!q || busy) return;
    busy = true;
    note = null;
    log.push({ q });
    const i = log.length - 1;
    try {
      if (glean) {
        const r = await call<Reply>('ai.ask_glean', { question: q, topic_id: topic?.id ?? null, off, on });
        log[i] = { q, reply: r };
        chips = r.chips;
      } else {
        const r = await call<{ answer: string }>('ai.ask', { question: q, topic_id: topic?.id ?? null, scope });
        log[i] = { q, a: r.answer };
      }
    } catch (err) {
      log[i] = { q, err: String((err as Error).message ?? err) };
    } finally {
      busy = false;
    }
  }

  function ask(e: SubmitEvent) {
    e.preventDefault();
    const q = question.trim();
    question = '';
    run(q);
  }

  /** Remove (or add back) a chip; with an answer on screen, ask again without it. */
  function toggle(c: Chip) {
    if (c.on) {
      off = [...off.filter((x) => x !== c.id), c.id];
      on = on.filter((x) => x !== c.id);
    } else {
      on = [...on.filter((x) => x !== c.id), c.id];
      off = off.filter((x) => x !== c.id);
    }
    const prev = [...log].reverse().find((m) => m.reply);
    if (prev) run(prev.q);
  }

  useKeys((e) => {
    if (!ui.assistantOpen || !glean || isTyping(e) || e.metaKey || e.ctrlKey || e.altKey) return false;
    if (e.key === 'e' && last) {
      sources = !sources;
      return true;
    }
    if (e.key === 'Escape' && sources) {
      sources = false;
      return true;
    }
    return false;
  });
</script>

{#if ui.assistantOpen}
  <aside class="pane" class:wide={glean}>
    {#if sources && last}
      <SourcesPanel reply={last} topicId={topic?.id ?? null} bind:hoverQuery bind:hoverSource onchanged={(n) => (note = n)} />
    {/if}
    <div class="head">
      <i class="ph ph-sparkle"></i>Assistant
      {#if glean}
        <span class="route mono" title="Glean searches; the large model answers from what's kept"><AppIcon source="glean" size={12} />{last?.route ?? 'search → model'}</span>
      {/if}
      <button class="close" onclick={() => (ui.assistantOpen = false)} title="Close (⌘I)"><i class="ph ph-caret-double-right"></i></button>
    </div>
    {#if topic}
      <div class="ctx"><span class="sq" style:background={catColor(topic.category)}></span>{topic.name}</div>
    {:else if scope === 'topics'}
      <div class="ctx"><i class="ph ph-stack"></i>All topics</div>
    {/if}
    {#if !glean}<div class="provider" title="Questions go only to this allow-listed provider">{ui.info?.ai_label}</div>{/if}
    <div class="log scroll">
      {#each log as m}
        <div class="q" class:bubble={glean}>{m.q}</div>
        {#if m.reply}
          <GleanAnswer reply={m.reply} {hoverQuery} {hoverSource} bind:hoverPerson />
        {:else if m.a}<div class="a"><Markdown text={m.a} /></div>{:else if m.err}<div class="err"><i class="ph ph-warning"></i>{m.err}</div>{:else}<div class="a pending">{glean ? 'Searching and reading…' : 'Thinking…'}</div>{/if}
      {/each}
    </div>
    {#if glean}
      {#if last}
        {@const kept = last.sources.filter((s) => s.kept).length}
        {@const dropped = last.sources.length - kept}
        <div class="count">
          <span>{kept} sources</span>
          <span class="warn">{dropped} dropped</span>
          <span>{last.queries.length - 1} {last.queries.length === 2 ? 'query' : 'queries'}</span>
          {#if last.glean.state !== 'ok'}<span class="warn" title={last.glean.message ?? ''}>Glean unavailable</span>{/if}
          <button class="btn srcbtn" class:on={sources} onclick={() => (sources = !sources)}>
            <i class="ph {sources ? 'ph-caret-right' : 'ph-caret-left'}"></i>{sources ? 'Hide sources' : 'Sources'} <span class="k">e</span>
          </button>
        </div>
      {/if}
      {#if note}<div class="note">{note}</div>{/if}
      {#if chips.length}
        <div class="sent">
          <div class="slabel">Sent with your question</div>
          <div class="chips">
            {#each chips as c (c.id)}
              <button class="chip" class:off={!c.on} onclick={() => toggle(c)} title={c.on ? 'Remove (asks again without it)' : 'Add'}>
                <span class:mono={c.id === 'keys'}>{c.label}</span><i class="ph {c.on ? 'ph-x' : 'ph-plus'}"></i>
              </button>
            {/each}
          </div>
        </div>
      {/if}
    {/if}
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
    position: relative;
    width: 300px;
    flex: none;
    display: flex;
    flex-direction: column;
    background: var(--bg-panel);
    border-left: 1px solid var(--border);
  }
  .pane.wide {
    width: 440px;
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
  .route {
    margin-left: auto;
    display: inline-flex;
    align-items: center;
    gap: 5px;
    font-size: 0.8462rem;
    font-weight: 400;
    color: #9a9df5;
  }
  .close {
    margin-left: auto;
    color: var(--tx-muted-2);
    font-size: 1.1538rem;
    display: flex;
  }
  .route + .close {
    margin-left: 8px;
  }
  .ctx {
    margin: 0 14px;
    padding: 6px 8px;
    border-radius: var(--r-input);
    border: 1px solid var(--border);
    font-size: 0.9231rem;
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
    font-size: 0.8462rem;
    color: var(--tx-label);
  }
  .log {
    flex: 1;
    min-height: 0;
    padding: 10px 14px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    font-size: calc(var(--fs-read) * 12.5 / 14);
    line-height: 1.5;
  }
  .wide .log {
    font-size: calc(var(--fs-read) * 13.5 / 14);
    gap: 12px;
  }
  .q {
    color: var(--tx-title);
  }
  .q.bubble {
    align-self: flex-end;
    max-width: 85%;
    padding: 7px 12px;
    border-radius: 10px;
    background: var(--bg-head);
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
  .count {
    display: flex;
    align-items: center;
    gap: 12px;
    margin: 0 14px;
    padding: 8px 0;
    border-top: 1px solid var(--border);
    color: var(--tx-2);
    font-size: 0.9231rem;
  }
  .count .warn {
    color: var(--orange);
  }
  .srcbtn {
    margin-left: auto;
    display: inline-flex;
    align-items: center;
    gap: 4px;
  }
  .srcbtn.on {
    border-color: var(--orange);
  }
  .k {
    color: var(--tx-label);
  }
  .note {
    margin: 0 14px 4px;
    color: var(--tx-muted-2);
    font-size: 0.8846rem;
  }
  .sent {
    margin: 0 14px;
  }
  .slabel {
    font-size: 0.8077rem;
    letter-spacing: 0.1em;
    text-transform: uppercase;
    color: var(--tx-label);
    margin: 4px 0 6px;
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  .chip {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 2px 8px;
    border-radius: var(--r-input);
    background: var(--bg-head);
    color: var(--tx-2);
    font-size: 0.9231rem;
  }
  .chip i {
    color: var(--tx-label);
    font-size: 0.7692rem;
  }
  .chip.off {
    background: none;
    border: 1px dashed var(--border-strong);
    color: var(--tx-label);
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
    font-size: 1rem;
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
    font-size: 1.1538rem;
  }
  .spark {
    font-size: 1.3077rem;
    color: var(--aqua);
  }
</style>

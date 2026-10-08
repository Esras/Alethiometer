<script lang="ts">
  // An entry expanded in place (18c chronology, 18d search): its messages
  // with sender colours and your replies, one faded context message before
  // them, Open in the app, and the actions as small hint text (nothing is
  // offered until it's expanded).
  import type { Snippet } from 'svelte';
  import { call, openExternal } from '../../lib/api';
  import type { Detail } from '../../lib/reclassify';
  import { messageUrl } from '../../lib/slackLink';
  import { sourceLabel } from '../../lib/sources';
  import Markdown from '../../lib/ui/Markdown.svelte';
  import PersonName from '../../lib/ui/PersonName.svelte';

  /** `hints` is the key legend at the foot: the site's own keys, as <kbd> and words. */
  let { itemId, hints, ondetail }: { itemId: number; hints: Snippet; ondetail?: (d: Detail) => void } = $props();

  let d = $state<Detail | null>(null);
  $effect(() => {
    const id = itemId;
    call<Detail>('incoming.detail', { item_id: id }).then((r) => {
      if (id === itemId) {
        d = r;
        ondetail?.(r);
      }
    });
  });
</script>

{#if d}
  <div class="detail">
    {#if d.context}
      <div class="msg faded">
        <span class="t tnum">{d.context.time}</span>
        <PersonName who={d.context.who} name={d.context.author} />
        <div class="txt">(earlier) <Markdown text={d.context.text} tags={d.source !== 'slack'} /></div>
      </div>
    {/if}
    {#each d.messages as m (m.id)}
      <div class="msg">
        <span class="t tnum">{m.time}</span>
        <PersonName who={m.who} name={m.author} />
        <div class="txt"><Markdown text={m.text} tags={d.source !== 'slack'} /></div>
      </div>
      {#if m.replies?.length}
        <div class="replies">
          {#each m.replies as r (r.id)}
            <div class="msg">
              <span class="t tnum">{r.time}</span>
              <PersonName who={r.who} name={r.author} />
              <div class="txt"><Markdown text={r.text} tags={d.source !== 'slack'} /></div>
            </div>
          {/each}
        </div>
      {/if}
    {/each}
    <div class="foot">
      {#if d.url}<button class="app" onclick={() => openExternal(messageUrl(d!.url!, d!.messages.at(-1), d!.messages.at(-1)?.replies?.at(-1)))}>Open in {sourceLabel(d.source)}</button>{/if}
      <span class="hints">{@render hints()}</span>
    </div>
  </div>
{/if}

<style>
  .detail {
    display: flex;
    flex-direction: column;
    margin-top: 6px;
    padding-top: 6px;
    border-top: 1px solid var(--border);
  }
  .msg {
    display: grid;
    grid-template-columns: 40px 56px minmax(0, 1fr);
    gap: 10px;
    align-items: baseline;
    padding: var(--row-pad-compact) 0;
    min-height: var(--row-min-compact);
    font-size: calc(var(--fs-read) * 12.5 / 14);
  }
  .msg.faded {
    opacity: 0.55;
  }
  .t {
    font-size: 0.9231rem;
    color: var(--tx-label);
  }
  .txt {
    color: var(--tx-2);
    min-width: 0;
  }
  .replies {
    margin: 2px 0 2px 50px;
    padding-left: 12px;
    border-left: 1px solid var(--border-strong);
  }
  .foot {
    display: flex;
    gap: 14px;
    align-items: center;
    padding-top: 6px;
    font-size: 0.8846rem;
  }
  .app {
    color: var(--blue);
  }
  .app:hover {
    text-decoration: underline;
  }
  .hints {
    margin-left: auto;
    color: var(--tx-label);
  }
</style>

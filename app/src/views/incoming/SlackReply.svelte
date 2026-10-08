<script lang="ts">
  // A Slack reply from Incoming. `r` opens the field under the signal; ⌘↵
  // sends it as you (`respond.slack`, through the outbox), Esc closes it and
  // keeps the draft. It goes in the thread for a thread signal, else in the
  // conversation; ⌥T switches (user, 2026-10-07). In a thread it answers the
  // message picked in the list, else the newest. Replying never triages: your
  // message comes back into the signal like anyone's.
  import { tick } from 'svelte';
  import { call } from '../../lib/api';
  import { chord, mod } from '../../lib/keys.svelte';
  import type { Message, Signal } from './types';

  let { s, msg }: { s: Signal; msg?: Message } = $props();

  let writing = $state(false);
  let text = $state('');
  let thread = $state(false);
  // The failed reply being edited: sending replaces it.
  let editing = $state(false);
  let err = $state<string | null>(null);
  let box = $state<HTMLTextAreaElement>();
  let busy = false;

  const a = $derived(s.reply);
  const target = $derived(msg ?? s.messages.at(-1));
  const where = $derived(
    !thread
      ? `in ${s.title}`
      : s.thread
        ? 'in the thread'
        : target
          ? `in the thread of ${target.author} ${target.time}`
          : 'in the thread of the newest message',
  );

  export async function open(failed = false) {
    if (!writing) {
      thread = !!s.thread;
      if (failed && a?.text) text = a.text;
    }
    editing = failed;
    writing = true;
    await tick();
    box?.focus();
    return true;
  }

  async function send() {
    const t = text.trim();
    if (!t || busy) return;
    busy = true;
    try {
      await call('respond.slack', {
        item_id: s.item_id,
        text: t,
        thread,
        message_key: thread && !s.thread ? msg?.key : undefined,
      });
      err = null;
      text = '';
      writing = false;
      editing = false;
    } catch (e) {
      err = String((e as Error).message ?? e);
    } finally {
      busy = false;
    }
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      writing = false;
      e.preventDefault();
    } else if (mod(e) && e.key === 'Enter') {
      send();
      e.preventDefault();
    } else if (e.altKey && e.code === 'KeyT' && !e.metaKey && !e.ctrlKey) {
      thread = !thread;
      e.preventDefault();
    }
    e.stopPropagation();
  }

  const retry = () => a && call('outbox.retry', { id: a.outbox_id }).catch((e) => (err = String(e.message ?? e)));
</script>

{#if writing || a || err}
  <div class="reply" onclick={(e) => e.stopPropagation()} role="presentation">
    {#if a && !(writing && editing)}
      <div class="status">
        {#if a.state === 'confirmed'}
          <span class="ok"><i class="ph ph-check"></i>Replied · {a.at}</span>
        {:else if a.state === 'failed'}
          <span class="failed" title={a.error}
            ><i class="ph ph-warning"></i>Your reply didn't send{#if a.error}: {a.error}{/if}</span
          >
          <button class="btn" onclick={retry}>Retry</button>
          <button class="btn" onclick={() => open(true)}>Edit</button>
        {:else}
          <span class="quiet"><i class="ph ph-circle-notch"></i>Sending reply…</span>
        {/if}
      </div>
    {/if}
    {#if writing}
      <div class="write">
        <textarea class="input" bind:this={box} bind:value={text} rows="2" placeholder="Reply as you…" onkeydown={onKey}
        ></textarea>
        <div class="foot">
          <button
            class="link-toggle"
            onclick={() => (thread = !thread)}
            title="Switch between the thread and the conversation"
          >
            <i class="ph ph-{thread ? 'chats-circle' : 'hash'}"></i>{where} <kbd>{chord('⌥T', 'Alt+T')}</kbd>
          </button>
          <span class="note"
            ><kbd>{chord('⌘⏎', 'Ctrl+⏎')}</kbd>
            {editing ? 'send in place of the failed one' : 'send'} · <kbd>Esc</kbd></span
          >
        </div>
      </div>
    {/if}
    {#if err}<span class="failed"><i class="ph ph-warning"></i>{err}</span>{/if}
  </div>
{/if}

<style>
  .reply {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .status,
  .foot {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .foot {
    justify-content: space-between;
  }
  .write textarea {
    width: 100%;
    font: inherit;
    font-size: var(--fs-read);
    resize: vertical;
  }
  .ok,
  .failed,
  .quiet {
    display: flex;
    align-items: center;
    gap: 5px;
  }
  .ok {
    color: var(--green);
  }
  .failed {
    color: var(--red);
  }
  .quiet,
  .note {
    color: var(--tx-muted-2);
  }
  .note {
    font-size: 0.846rem;
  }
</style>

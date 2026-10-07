<script lang="ts" module>
  import type { Rsvp as RsvpT } from './types';
  /** Accept/Decline can still be sent: nothing sending or done from here. */
  export const answerable = (r: RsvpT | undefined) => !!r && !['pending', 'sent', 'confirmed'].includes(r.answer?.state ?? '');
</script>

<script lang="ts">
  // An invite's RSVP line (14a Incoming, 14b Topic). Answering never triages:
  // the row stays where it is until you confirm, move or dismiss it. Answers
  // go through the outbox (`respond.rsvp`); the line shows the latest one from
  // sending to done, then "Accepted · 10:58" and a link to change it in Google
  // Calendar (it can't be changed here). ⌥Y accept, ⌥N decline, ⌥⇧N decline
  // with a note (the owner's view registers the keys and calls these).
  // `small` (a topic's panels) leaves the `no reply` pill to the row.
  import { tick } from 'svelte';
  import { call, openExternal } from '../../lib/api';
  import { chord, mod } from '../../lib/keys.svelte';
  import type { Rsvp } from './types';

  let { itemId, rsvp, small = false }: { itemId: number; rsvp: Rsvp; small?: boolean } = $props();

  let noting = $state(false);
  let note = $state('');
  let err = $state<string | null>(null);
  let box = $state<HTMLInputElement>();
  let busy = false;

  const a = $derived(rsvp.answer);
  const sending = $derived(a?.state === 'pending' || a?.state === 'sent');
  // Answered from here: no buttons until it fails.
  const done = $derived(a?.state === 'confirmed');
  const open = $derived(!sending && !done);

  async function send(response: 'accepted' | 'declined', comment?: string) {
    if (busy || !open) return false;
    busy = true;
    try {
      await call('respond.rsvp', { item_id: itemId, response, comment });
      err = null;
      noting = false;
      note = '';
    } catch (e) {
      err = String((e as Error).message ?? e);
    } finally {
      busy = false;
    }
    return true;
  }

  export const accept = () => send('accepted');
  export const decline = () => send('declined');
  export async function declineWithNote() {
    if (!open) return false;
    noting = true;
    await tick();
    box?.focus();
    return true;
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      noting = false;
      e.preventDefault();
    } else if (mod(e) && e.key === 'Enter') {
      send('declined', note);
      e.preventDefault();
    }
    e.stopPropagation();
  }

  const retry = () => a && call('outbox.retry', { id: a.outbox_id }).catch((e) => (err = String(e.message ?? e)));
  const alt = (k: string) => chord(`⌥${k}`, `Alt+${k}`);
</script>

<div class="rsvp" class:small onclick={(e) => e.stopPropagation()} role="presentation">
  {#if done && a}
    {#if a.response === 'accepted'}
      <span class="answered ok"><i class="ph ph-check"></i>Accepted · {a.at}</span>
    {:else}
      <span class="answered no"><i class="ph ph-x"></i>Declined · {a.at}</span>
    {/if}
    {#if rsvp.url}<button class="link-out" onclick={() => rsvp.url && openExternal(rsvp.url)}>Change in Google Calendar</button>{/if}
  {:else if sending && a}
    <span class="answered no"><i class="ph ph-circle-notch"></i>{a.response === 'accepted' ? 'Accepting…' : 'Declining…'}</span>
  {:else if noting}
    <input class="input sm" bind:this={box} bind:value={note} placeholder="Note to the organiser (optional)" onkeydown={onKey} />
    <button class="btn" onclick={() => send('declined', note)}>Send decline <kbd>{chord('⌘⏎', 'Ctrl+⏎')}</kbd></button>
    <span class="note">Esc</span>
  {:else}
    {#if a?.state === 'failed'}
      <span class="failed" title={a.error}><i class="ph ph-warning"></i>{a.response === 'accepted' ? 'Accept' : 'Decline'} didn't send{#if a.error}: {a.error}{/if}</span>
      <button class="btn" onclick={retry}>Retry</button>
    {:else if !small}
      <span class="pill quiet">no reply</span>
    {/if}
    <button class="btn" onclick={accept}><i class="ph ph-check"></i>Accept <kbd>{alt('Y')}</kbd></button>
    <button class="btn" onclick={decline}><i class="ph ph-x"></i>Decline <kbd>{alt('N')}</kbd></button>
    <button class="link-act" onclick={declineWithNote}>{small ? 'with note…' : 'Decline with note…'} <kbd>{chord('⌥⇧N', 'Alt+Shift+N')}</kbd></button>
    {#if rsvp.overlaps.length}
      <span class="overlap"><i class="ph ph-warning-circle"></i>Overlaps {rsvp.overlaps.join(', ')}</span>
    {/if}
  {/if}
  {#if err}<span class="failed"><i class="ph ph-warning"></i>{err}</span>{/if}
</div>

<style>
  .rsvp {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px 10px;
  }
  .rsvp.small {
    font-size: 0.885rem;
    gap: 4px 8px;
  }
  .rsvp.small .btn {
    font-size: 0.885rem;
  }
  .pill.quiet {
    background: var(--bg-head);
    color: var(--tx-muted-2);
  }
  .answered {
    display: flex;
    align-items: center;
    gap: 5px;
  }
  .answered.ok {
    color: var(--green);
  }
  .answered.no {
    color: var(--tx-muted-2);
  }
  .overlap,
  .failed {
    display: flex;
    align-items: center;
    gap: 5px;
  }
  .overlap {
    color: var(--yellow);
  }
  .failed {
    color: var(--red);
  }
  .input.sm {
    flex: 1;
    min-width: 200px;
    width: auto;
  }
  .note {
    color: var(--tx-disabled);
    font-size: 0.846rem;
  }
</style>

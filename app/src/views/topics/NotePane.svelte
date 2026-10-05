<script lang="ts">
  // The topic's Obsidian note: the markdown preview, or (e) the note as plain
  // text to edit anywhere: add, change, move things around. ⌘S or leaving the
  // editor (esc, click away) saves the whole note, provided it still holds the
  // text the edit started from. With a vault configured that goes through the
  // running Obsidian app (an outbox write we follow here); otherwise straight
  // to the file. A conflict never merges: reload theirs (r) or keep mine (k).
  import { onDestroy, tick, untrack } from 'svelte';
  import { call, openExternal } from '../../lib/api';
  import { refresh } from '../../lib/app.svelte';
  import { chord, mod, plain, useKeys } from '../../lib/keys.svelte';
  import AppIcon from '../../lib/ui/AppIcon.svelte';
  import NoteText from '../../lib/ui/NoteText.svelte';
  import type { SaveState, TopicNote } from './types';

  let { topicId, note }: { topicId: number; note: TopicNote | null } = $props();

  type Status = 'idle' | 'editing' | 'saving' | 'saved' | 'conflict' | 'offline' | 'failed';
  let editing = $state(false);
  let status = $state<Status>('idle');
  let draft = $state('');
  // What the next save starts from: the loaded note, then each text we sent.
  let base = $state<string | null>(untrack(() => (note ? (note.base === undefined ? note.text : note.base) : null)));
  // The last text handed to a save, and (until the file on disk shows it) the
  // text the preview should show.
  let sent = $state<string | null>(null);
  let pending = $state<string | null>(null);
  let outboxId = $state<number | null>(null);
  let disk = $state<string | null>(null);
  let error = $state('');
  let area: HTMLTextAreaElement | undefined = $state();
  let poller: ReturnType<typeof setTimeout> | undefined;

  const busy = $derived(status === 'saving' || status === 'conflict' || status === 'offline' || status === 'failed');

  // A refreshed note: once the file shows what we saved, it is the truth again.
  $effect(() => {
    const n = note;
    untrack(() => {
      if (!n) return;
      if (pending !== null && n.text === pending) pending = null;
      if (!editing && !busy && pending === null) base = n.base === undefined ? n.text : n.base;
    });
  });

  // Not saved yet (or refused): the preview shows your text, not the file's.
  const shown = $derived(editing ? draft : (pending ?? (busy ? (sent ?? draft) : (note?.text ?? ''))));

  async function edit() {
    if (!note || editing) return;
    // While a save is out (or refused) keep editing your text; else the note's.
    if (!busy) {
      draft = pending ?? note.text;
      status = 'editing';
    }
    editing = true;
    await tick();
    area?.focus();
  }

  function openInObsidian() {
    if (note) openExternal(`obsidian://open?path=${encodeURIComponent(note.abs_path)}`);
  }

  /** `force`: keep mine after a direct-write conflict (base is already theirs). */
  async function save(force = false) {
    if (!note || (status === 'conflict' && !force)) return;
    const text = draft;
    if (text === (sent ?? note.text) && !force) return;
    sent = text;
    status = 'saving';
    try {
      const r = await call<SaveState>('topics.save_note', { id: topicId, text, base });
      apply(r, text);
    } catch (e) {
      error = String((e as Error).message ?? e);
      status = 'failed';
    }
  }

  function apply(r: SaveState, text?: string) {
    clearTimeout(poller);
    if (r.state === 'saving' && r.outbox_id !== undefined && text !== undefined) {
      // Queued: the next save continues from this text.
      outboxId = r.outbox_id;
      base = text;
      pending = text;
    }
    switch (r.state) {
      case 'saved':
        if (text !== undefined) {
          base = text;
          pending = text;
        }
        status = editing && draft !== sent ? 'editing' : 'saved';
        refresh();
        break;
      case 'saving':
        status = 'saving';
        poller = setTimeout(poll, 400);
        break;
      case 'conflict':
        disk = r.disk;
        status = 'conflict';
        area?.blur();
        break;
      case 'offline':
        status = 'offline';
        poller = setTimeout(poll, 2000);
        break;
      case 'failed':
        error = r.error;
        status = 'failed';
        poller = setTimeout(poll, 2000);
        break;
    }
  }

  async function poll() {
    if (outboxId === null) return;
    try {
      apply(await call<SaveState>('topics.note_status', { outbox_id: outboxId }));
    } catch {
      poller = setTimeout(poll, 2000);
    }
  }

  function typed() {
    if (status === 'saved' || status === 'idle') status = 'editing';
  }

  /** Esc / click away: save and show the preview again. */
  function done() {
    if (!editing) return;
    editing = false;
    if (status === 'editing') status = 'idle';
    save();
  }

  async function reload() {
    if (status !== 'conflict') return;
    clearTimeout(poller);
    draft = disk ?? '';
    base = disk;
    sent = pending = outboxId = null;
    status = 'idle';
    editing = false;
    refresh();
  }

  async function keepMine() {
    if (status !== 'conflict') return;
    if (outboxId === null) {
      // Written directly (no vault configured): save again over what's there now.
      base = disk;
      await save(true);
      return;
    }
    status = 'saving';
    apply(await call<SaveState>('topics.keep_mine', { outbox_id: outboxId }));
  }

  async function retry() {
    if (outboxId === null) return;
    await call('outbox.retry', { id: outboxId });
    status = 'saving';
    poll();
  }

  useKeys((e) => {
    if (editing && mod(e) && e.key === 's') return save(), true;
    if (editing && e.key === 'Escape') return done(), true;
    if (status === 'offline' && mod(e) && e.key === 'r') return retry(), true;
    if (status === 'conflict' && plain(e, 'r')) return reload(), true;
    if (status === 'conflict' && plain(e, 'k')) return keepMine(), true;
    if (plain(e, 'e')) return edit(), true;
    if (plain(e, 'o')) return openInObsidian(), true;
  });

  onDestroy(() => {
    clearTimeout(poller);
    // Switching topic or tab mid-edit still saves.
    if (note && editing && status !== 'conflict' && draft !== (sent ?? note.text)) {
      call('topics.save_note', { id: topicId, text: draft, base }).then(() => refresh());
    }
  });

  const label = $derived(
    {
      idle: '',
      editing: 'editing',
      saving: 'saving…',
      saved: 'saved',
      conflict: '',
      offline: '',
      failed: `not saved: ${error}`,
    }[status],
  );
</script>

<div class="note panel col" class:editing>
  <div class="note-head">
    <AppIcon source="obsidian" /><span class="path ellipsis">{note?.path ?? 'No note linked'}</span>
    {#if status === 'conflict'}
      <span class="state warn nowrap">
        conflict: changed in Obsidian ·
        <button class="act" onclick={reload}>reload <kbd>r</kbd></button> /
        <button class="act" onclick={keepMine}>keep mine <kbd>k</kbd></button>
      </span>
    {:else if status === 'offline'}
      <span class="state warn nowrap">Obsidian isn't open · <button class="act" onclick={retry}><kbd>{chord('⌘R', 'Ctrl+R')}</kbd> retry</button></span>
    {:else if label}
      <span class="state nowrap ellipsis" class:warn={status === 'failed'} class:ok={status === 'saved'} title={status === 'failed' ? error : undefined}>{label}{#if status === 'editing'}{' · '}<kbd>{chord('⌘S', 'Ctrl+S')}</kbd> save · <kbd>Esc</kbd> done{/if}</span>
    {/if}
    {#if note}
      <span class="keys nowrap">
        {#if !editing}<button class="edit" onclick={edit} title="Edit here"><kbd>e</kbd> edit</button>{/if}
        <button class="edit" onclick={openInObsidian} title="Open in Obsidian"><kbd>o</kbd> open</button>
      </span>
    {/if}
  </div>
  {#if editing}
    <textarea class="note-body scroll" bind:this={area} bind:value={draft} spellcheck="true" oninput={typed} onblur={done}></textarea>
  {:else}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="note-body scroll" ondblclick={edit}>
      {#if shown}<NoteText text={shown} />{:else}<span class="empty">Empty note.</span>{/if}
    </div>
  {/if}
</div>

<style>
  .note {
    flex: 1;
    min-height: 0;
  }
  .note.editing {
    border-color: rgba(142, 192, 124, 0.55);
  }
  .note-head {
    display: flex;
    gap: 8px;
    align-items: center;
    padding: 7px 12px;
    background: var(--bg-head);
    font-size: 0.8846rem;
    color: var(--tx-2);
    white-space: nowrap;
  }
  .path {
    min-width: 0;
    flex: 0 1 auto;
  }
  .state {
    margin-left: auto;
    min-width: 0;
    font-size: 0.8462rem;
    color: var(--tx-muted-2);
    display: flex;
    gap: 4px;
    align-items: center;
  }
  .state.ok {
    color: var(--aqua);
  }
  .state.warn {
    color: var(--status-attention);
  }
  .act {
    color: inherit;
    font-size: 0.8462rem;
    text-decoration: underline;
    text-decoration-color: rgba(254, 128, 25, 0.35);
    text-underline-offset: 2px;
  }
  .act:hover {
    color: var(--tx-title);
  }
  .keys {
    display: flex;
    gap: 10px;
    margin-left: auto;
  }
  .state + .keys {
    margin-left: 10px;
  }
  .edit {
    font-size: 0.8462rem;
    color: var(--tx-muted-2);
  }
  .edit:hover {
    color: var(--tx-2);
  }
  .note-body {
    flex: 1;
    min-height: 0;
    padding: 12px 16px 16px;
    font-size: calc(var(--fs-read) * 13 / 14);
    line-height: 1.75;
    color: var(--tx-2);
  }
  textarea.note-body {
    display: block;
    width: 100%;
    resize: none;
    border: 0;
    outline: none;
    background: transparent;
    font-family: inherit;
    caret-color: var(--aqua);
    white-space: pre-wrap;
  }
  textarea.note-body::selection {
    background: rgba(142, 192, 124, 0.25);
  }
  .empty {
    color: var(--tx-label);
  }
</style>

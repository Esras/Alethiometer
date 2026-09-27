<script lang="ts">
  // The reflection note: the user's own words, saved to their Obsidian vault.
  // A note without headings (the day) is one plain textarea. A note with
  // `## ` headings (week and up, from a template) shows each heading in orange
  // with its own textarea; the file text is exactly the lines joined back.
  //
  // Autosave is debounced. A save only lands if the file on disk still matches
  // what we loaded; otherwise we show the conflict and write nothing.
  import { onDestroy, tick, untrack } from 'svelte';
  import { call, openExternal } from '../../lib/api';
  import { refresh } from '../../lib/app.svelte';
  import AppIcon from '../../lib/ui/AppIcon.svelte';
  import type { ReflectNote, SaveResult, Scope } from './types';

  let {
    note,
    scope,
    period,
    lens,
    placeholder = '',
  }: { note: ReflectNote; scope: Scope; period: string; lens: string; placeholder?: string } = $props();

  // Fixed for this editor's lifetime (the parent re-keys it per note).
  const target = untrack(() => ({ scope, period, lens, path: note.path, abs: note.abs_path }));

  type Section = { head: string | null; body: string; trail: number };

  function parse(t: string): Section[] {
    const out: { head: string | null; lines: string[] }[] = [{ head: null, lines: [] }];
    for (const l of t.split('\n')) {
      if (/^#{1,6} /.test(l)) out.push({ head: l, lines: [] });
      else out[out.length - 1].lines.push(l);
    }
    return out.map(({ head, lines }) => {
      let trail = 0;
      while (trail < lines.length && lines[lines.length - 1 - trail] === '') trail++;
      return { head, body: lines.slice(0, lines.length - trail).join('\n'), trail };
    });
  }

  function serialize(secs: Section[]): string {
    const lines: string[] = [];
    for (const s of secs) {
      if (s.head !== null) lines.push(s.head);
      if (s.body !== '') lines.push(...s.body.split('\n'));
      // An edited empty body still needs its line.
      for (let i = 0; i < s.trail; i++) lines.push('');
    }
    return lines.join('\n');
  }

  let sections = $state(untrack(() => parse(note.text)));
  let base = $state<string | null>(untrack(() => note.base));
  const structured = $derived(sections.some((s) => s.head !== null));
  type Status = 'new' | 'saved' | 'unsaved' | 'saving' | 'conflict' | 'error';
  let status = $state<Status>(untrack(() => (note.base === null ? 'new' : 'saved')));
  let disk = $state<string | null>(null);
  let error = $state('');
  let timer: ReturnType<typeof setTimeout> | undefined;
  let areas: HTMLTextAreaElement[] = $state([]);

  function edited() {
    if (status === 'conflict') return;
    status = 'unsaved';
    clearTimeout(timer);
    timer = setTimeout(flush, 700);
  }

  async function flush() {
    clearTimeout(timer);
    if (status !== 'unsaved') return;
    status = 'saving';
    const text = serialize(sections);
    try {
      const r = await call<SaveResult>('reflect.save', { scope: target.scope, period: target.period, lens: target.lens, text, base });
      if ('conflict' in r) {
        disk = r.disk;
        status = 'conflict';
        return;
      }
      base = r.base;
      if (serialize(sections) === text) {
        status = 'saved';
        refresh();
      } else {
        status = 'unsaved';
        timer = setTimeout(flush, 700);
      }
    } catch (e) {
      error = String((e as Error).message ?? e);
      status = 'error';
    }
  }

  function loadTheirs() {
    sections = parse(disk ?? '');
    base = disk;
    status = disk === null ? 'new' : 'saved';
    tick().then(sizeAll);
  }

  function keepMine() {
    // The user chose this; still guarded by the new base.
    base = disk;
    status = 'unsaved';
    flush();
  }

  onDestroy(() => {
    if (status === 'unsaved') flush();
  });

  function autosize(el: HTMLTextAreaElement) {
    el.style.height = 'auto';
    el.style.height = `${el.scrollHeight}px`;
  }
  function sizeAll() {
    if (structured) areas.forEach((a) => a && autosize(a));
  }
  $effect(() => {
    void structured;
    tick().then(sizeAll);
  });

  export function focus() {
    const a = areas.filter(Boolean);
    const el = structured ? a.find((x) => x.value.trim() === '' || /^\d\.\s*$/m.test(x.value)) ?? a[0] : a[0];
    if (!el) return;
    el.focus();
    if (!structured) el.setSelectionRange(el.value.length, el.value.length);
  }

  export function openInObsidian() {
    openExternal(`obsidian://open?path=${encodeURIComponent(target.abs)}`);
  }

  const statusLabel = $derived(
    {
      new: 'not created yet · saves as you type',
      saved: 'saved',
      unsaved: 'editing',
      saving: 'saving…',
      conflict: 'not saved',
      error: 'not saved',
    }[status],
  );
</script>

<div class="note">
  <div class="head">
    <AppIcon source="obsidian" size={15} />
    <button class="path ellipsis" onclick={openInObsidian} title="Open in Obsidian (o)">{target.path}</button>
    <span class="status nowrap" class:warn={status === 'conflict' || status === 'error'}>{statusLabel}</span>
  </div>
  {#if status === 'conflict'}
    <div class="conflict">
      <i class="ph-bold ph-warning"></i>
      <span class="ellipsis">Changed outside Helm since you opened it. Nothing was overwritten.</span>
      <span class="acts">
        <button class="btn" onclick={loadTheirs}>Load the file</button>
        <button class="btn" onclick={keepMine}>Keep mine</button>
      </span>
    </div>
  {:else if status === 'error'}
    <div class="conflict"><i class="ph-bold ph-warning"></i><span class="ellipsis">Couldn't save: {error}</span></div>
  {/if}

  {#if structured}
    <div class="body scroll structured">
      {#each sections as s, i}
        {#if s.head !== null}
          <div class="h">{s.head.replace(/^#+\s*/, '')}</div>
        {/if}
        {#if s.head !== null || s.body !== ''}
          <textarea
            bind:this={areas[i]}
            bind:value={s.body}
            rows="1"
            spellcheck="true"
            oninput={(e) => {
              if (s.trail === 0 && s.head !== null && i < sections.length - 1) s.trail = 1;
              autosize(e.currentTarget);
              edited();
            }}
          ></textarea>
        {/if}
      {/each}
    </div>
  {:else}
    <textarea
      class="body plain scroll"
      bind:this={areas[0]}
      bind:value={sections[0].body}
      {placeholder}
      spellcheck="true"
      oninput={() => {
        if (sections[0].trail === 0) sections[0].trail = 1;
        edited();
      }}
    ></textarea>
  {/if}
</div>

<style>
  .note {
    min-height: 0;
    min-width: 0;
    display: flex;
    flex-direction: column;
    border-radius: var(--r-panel);
    background: var(--bg-panel);
    border: 1px solid var(--border);
    overflow: hidden;
  }
  .head {
    flex: none;
    display: flex;
    gap: 8px;
    align-items: center;
    padding: 8px 14px;
    background: var(--bg-head);
    font-size: 11.5px;
    color: var(--tx-2);
  }
  .path {
    min-width: 0;
    text-align: left;
    color: var(--tx-2);
  }
  .path:hover {
    color: var(--tx-title);
  }
  .status {
    margin-left: auto;
    font-size: 11px;
    color: var(--tx-muted-2);
  }
  .status.warn {
    color: var(--orange);
  }
  .conflict {
    flex: none;
    display: flex;
    gap: 8px;
    align-items: center;
    padding: 7px 14px;
    border-bottom: 1px solid var(--border);
    font-size: 12px;
    color: var(--tx-2);
  }
  .conflict i {
    color: var(--orange);
  }
  .acts {
    margin-left: auto;
    display: flex;
    gap: 6px;
  }
  .body {
    flex: 1;
    min-height: 0;
  }
  textarea {
    display: block;
    width: 100%;
    resize: none;
    border: 0;
    outline: none;
    background: transparent;
    font-size: 15px;
    line-height: 1.8;
    color: var(--tx-2);
    caret-color: var(--aqua);
    padding: 0;
  }
  textarea::placeholder {
    color: var(--tx-disabled);
  }
  .plain {
    padding: 20px 24px;
  }
  .structured {
    padding: 4px 26px 18px 26px;
    padding-top: 18px;
  }
  .structured textarea {
    overflow: hidden;
  }
  .h {
    color: var(--orange);
    font-weight: 500;
    font-size: 13px;
    letter-spacing: 0.02em;
    margin-top: 14px;
    line-height: 1.8;
  }
</style>

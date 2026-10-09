<script lang="ts">
  // Notes saved in the stand-in vault before `obsidian_vault` was set, which
  // the app no longer reads (`standin.rs`). Shown once per start while any are
  // left. Nothing is ticked at first: the stand-in also holds sample notes.
  import { call } from '../lib/api';
  import { chord, mod } from '../lib/keys.svelte';
  import { listNav, moveSel } from '../lib/ui/listNav';
  import Modal from '../lib/ui/Modal.svelte';

  type File = { path: string; state: 'missing' | 'differs'; modified: string | null; preview: string };
  type Listing = { standin: string; vault: string; files: File[] };

  let { listing, onclose }: { listing: Listing; onclose: () => void } = $props();

  let picked = $state<Set<string>>(new Set());
  let sel = $state(0);
  let busy = $state(false);
  let err = $state<string | null>(null);
  let rows = $state<HTMLDivElement>();

  // The ticked-or-not rows come first in `rows`, so `sel` indexes them.
  $effect(() => rows?.children[sel]?.scrollIntoView({ block: 'nearest' }));

  const missing = $derived(listing.files.filter((f) => f.state === 'missing'));
  const differs = $derived(listing.files.filter((f) => f.state === 'differs'));
  const n = $derived(picked.size);

  function toggle(f: File) {
    const next = new Set(picked);
    if (!next.delete(f.path)) next.add(f.path);
    picked = next;
  }

  async function resolve() {
    if (busy) return;
    busy = true;
    err = null;
    try {
      const out = await call<{ copied: number; failed: { path: string; error: string }[] }>('standin.resolve', { copy: [...picked] });
      if (out.failed.length) err = out.failed.map((f) => `${f.path}: ${f.error}`).join(' · ');
      else onclose();
    } catch (e) {
      err = String((e as Error).message ?? e);
    } finally {
      busy = false;
    }
  }

  function onkeys(e: KeyboardEvent): boolean {
    const d = listNav(e, { bare: true });
    if (d) sel = moveSel(sel, d, missing.length);
    else if ((e.key === ' ' || (e.key === 'Enter' && !mod(e))) && missing[sel]) toggle(missing[sel]);
    else if (e.key === 'Enter' && mod(e)) resolve();
    else return false;
    return true;
  }
</script>

<Modal label="Notes left in the stand-in vault" {onclose} {onkeys}>
  <div class="standin modal">
    <header>
      <i class="ph ph-folder-simple-dashed"></i>
      <span class="title">Notes the app no longer reads</span>
      <button class="esc" onclick={onclose}><kbd>Esc</kbd> <i class="ph ph-x"></i></button>
    </header>
    <p class="why">
      These were saved in the stand-in vault before an Obsidian vault was set, and nothing shows them now. Tick the ones to copy into your vault.
      With sample data on, the stand-in also holds sample notes: leave those unticked.
    </p>
    <div class="rows scroll" bind:this={rows}>
      {#each missing as f, i (f.path)}
        <button class="row" class:sel={i === sel} onclick={() => toggle(f)}>
          <i class="ph {picked.has(f.path) ? 'ph-check-square on' : 'ph-square'}"></i>
          <span class="path">{f.path}</span>
          <span class="muted tnum">{f.modified ?? ''}</span>
          <span class="preview muted">{f.preview}</span>
        </button>
      {/each}
      {#if differs.length}
        <div class="label sub">In your vault with different text · compare by hand</div>
        {#each differs as f (f.path)}
          <div class="row other">
            <i class="ph ph-git-diff"></i>
            <span class="path">{f.path}</span>
            <span class="muted tnum">{f.modified ?? ''}</span>
            <span class="preview muted">{f.preview}</span>
          </div>
        {/each}
      {/if}
    </div>
    {#if err}<div class="warn">{err}</div>{/if}
    <footer>
      <span class="muted where" title={listing.standin}>Unticked notes stay in {listing.standin} and aren't offered again.</span>
      <button class="btn md" onclick={onclose}>Later<kbd>Esc</kbd></button>
      <button class="btn md primary" disabled={busy} onclick={resolve}
        >{n ? `Copy ${n === 1 ? '1 note' : `${n} notes`}` : 'Copy none'}<kbd>{chord('⌘⏎', 'Ctrl+⏎')}</kbd></button
      >
    </footer>
  </div>
</Modal>

<style>
  .standin {
    position: fixed;
    top: 90px;
    left: 50%;
    transform: translateX(-50%);
    width: 680px;
    max-height: calc(100vh - 180px);
    display: flex;
    flex-direction: column;
    border-radius: var(--radius-md);
    padding: 0;
    overflow: hidden;
  }
  header {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 10px 16px;
    background: var(--bg-head);
    border-bottom: 1px solid var(--border);
  }
  .title {
    color: var(--tx-title);
  }
  .esc {
    margin-left: auto;
    color: var(--tx-label);
  }
  .why {
    margin: 0;
    padding: 10px 16px;
    color: var(--tx-2);
    border-bottom: 1px solid var(--border);
  }
  .rows {
    flex: 1;
    min-height: 80px;
  }
  .row {
    display: grid;
    grid-template-columns: 20px minmax(0, 1fr) auto;
    column-gap: 12px;
    align-items: center;
    width: 100%;
    padding: var(--row-pad-compact) 16px;
    min-height: var(--row-min-compact);
    text-align: left;
  }
  .row.sel {
    background: var(--aqua-tint);
  }
  .row i {
    color: var(--tx-label);
  }
  .row i.on {
    color: var(--aqua);
  }
  .path {
    color: var(--tx-2);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .preview {
    grid-column: 2 / 4;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .muted {
    color: var(--tx-label);
  }
  .tnum {
    white-space: nowrap;
  }
  .sub {
    padding: 10px 16px 4px;
  }
  .warn {
    padding: 8px 16px;
    color: var(--status-attention);
  }
  footer {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 16px;
    border-top: 1px solid var(--border);
  }
  .where {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 0.8846rem;
  }
</style>

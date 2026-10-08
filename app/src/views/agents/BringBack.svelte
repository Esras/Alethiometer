<script lang="ts">
  // Bring an OpenShell agent's changes back into the directory its sandbox
  // was made from (agents/openshell/sync.rs): the sandbox's directory is
  // downloaded and compared with what was uploaded and what's here now. Only
  // what the agent changed and you didn't is copied; a file both changed is
  // listed and left as it is, unless the sandbox should win (⌘⌫).
  import { onMount } from 'svelte';
  import { call } from '../../lib/api';
  import { chord, mod } from '../../lib/keys.svelte';
  import Modal from '../../lib/ui/Modal.svelte';
  import { ag } from './state.svelte';

  let { session }: { session: string } = $props();

  type Change = { path: string; what: 'added' | 'modified' | 'deleted' };
  type Look = { sandbox: string; local: string; changes: Change[]; conflicts: Change[]; same: number; ready: boolean };
  let look = $state<Look | null>(null);
  let err = $state<string | null>(null);
  let busy = $state(true);
  let done = $state<string | null>(null);
  const s = $derived(ag.tree?.sessions.find((x) => x.id === session));
  const files = (n: number) => `${n} file${n === 1 ? '' : 's'}`;
  const ICON = { added: 'ph-plus', modified: 'ph-pencil-simple', deleted: 'ph-minus' } as const;

  onMount(() => {
    call<Look>('agents.sandbox_changes', { session })
      .then((l) => (look = l))
      .catch((e) => (err = String((e as Error).message ?? e)))
      .finally(() => (busy = false));
  });

  function close() {
    if (look?.ready && !done) call('agents.sandbox_discard', { session }).catch(() => {});
    ag.bringBack = null;
  }

  async function copy(force: boolean) {
    if (!look?.ready || busy || done) return;
    busy = true;
    try {
      const r = await call<{ applied: Change[]; conflicts: Change[] }>('agents.sandbox_copy_back', { session, force });
      done = `Brought back ${files(r.applied.length)}${r.conflicts.length ? `; ${files(r.conflicts.length)} left as they are here` : ''}.`;
    } catch (e) {
      err = String((e as Error).message ?? e);
    } finally {
      busy = false;
    }
  }

  function onkeys(e: KeyboardEvent): boolean {
    if (!mod(e)) return false;
    if (e.key === 'Enter') done ? close() : copy(false);
    else if (e.key === 'Backspace' && look?.conflicts.length) copy(true);
    else return false;
    return true;
  }
</script>

<Modal label="Bring back changes" onclose={close} {onkeys} inset="32px 0 0 0" isolate>
  <div class="box modal agent-form">
    <div class="top">
      <i class="ph ph-download-simple"></i><span class="t">Bring back changes</span>
      <span class="sub"
        >{s?.name ?? ''} · sandbox {s?.sandbox?.name ?? ''} → {look?.local ?? s?.sandbox?.upload ?? ''}</span
      >
      <kbd>Esc</kbd>
    </div>
    <div class="body">
      {#if busy && !look}
        <p class="muted">Downloading from the sandbox…</p>
      {:else if look}
        {#if done}
          <p>{done}</p>
        {:else if !look.changes.length}
          <p class="muted">
            Nothing new to bring back{look.same ? ` (${files(look.same)} already the same here)` : ''}.
          </p>
        {:else}
          <div class="head">
            {files(look.changes.length)} to copy back{look.same ? ` · ${look.same} already the same here` : ''}
          </div>
          <div class="list inset">
            {#each look.changes as c (c.path)}<div class="f {c.what}">
                <i class="ph {ICON[c.what]}"></i><span class="p">{c.path}</span><span class="w">{c.what}</span>
              </div>{/each}
          </div>
        {/if}
        {#if look.conflicts.length && !done}
          <div class="head warn"><i class="ph ph-warning"></i>Changed here too: left as they are</div>
          <div class="list inset">
            {#each look.conflicts as c (c.path)}<div class="f">
                <i class="ph {ICON[c.what]}"></i><span class="p">{c.path}</span><span class="w"
                  >{c.what} in the sandbox</span
                >
              </div>{/each}
          </div>
        {/if}
      {/if}
      {#if err}<div class="err">{err}</div>{/if}
    </div>
    <div class="foot">
      {#if done || !look?.ready}
        <button class="btn md" onclick={close}>Close<kbd>Esc</kbd></button>
      {:else}
        <button class="btn md primary" disabled={busy} onclick={() => copy(false)}
          >Copy {files(look.changes.length)} back<kbd>{chord('⌘⏎', 'Ctrl+⏎')}</kbd></button
        >
        {#if look.conflicts.length}<button class="btn md danger" disabled={busy} onclick={() => copy(true)}
            >Sandbox wins, conflicts too<kbd>{chord('⌘⌫', 'Ctrl+⌫')}</kbd></button
          >{/if}
        <button class="btn md" onclick={close}>Cancel<kbd>Esc</kbd></button>
      {/if}
    </div>
  </div>
</Modal>

<style>
  .box {
    width: min(620px, calc(100vw - 48px));
    max-height: calc(100vh - 80px);
    border-radius: var(--radius-md);
    padding: 0;
    display: flex;
    flex-direction: column;
  }
  .top .t {
    white-space: nowrap;
  }
  .top .sub {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .body {
    padding: 12px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    overflow-y: auto;
    min-height: 0;
  }
  p {
    margin: 0;
    color: var(--tx-2);
  }
  .muted {
    color: var(--tx-muted-2);
  }
  .head {
    font: 0.808rem var(--font-mono);
    color: var(--tx-label);
    display: flex;
    gap: 6px;
    align-items: center;
  }
  .head.warn {
    color: var(--status-headsup);
  }
  .list {
    display: flex;
    flex-direction: column;
    max-height: 240px;
    overflow-y: auto;
    padding: 4px 0;
    border-radius: var(--radius-sm);
  }
  .f {
    display: flex;
    gap: 8px;
    align-items: baseline;
    padding: 1px 10px;
    font: 0.846rem var(--font-mono);
  }
  .f .p {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--tx-body);
  }
  .f .w {
    color: var(--tx-muted-2);
  }
  .f.added i {
    color: var(--status-done);
  }
  .f.deleted i {
    color: var(--status-attention);
  }
  .foot {
    padding: 12px;
    border-top: 1px solid var(--border);
    display: flex;
    gap: 8px;
    align-items: center;
  }
</style>

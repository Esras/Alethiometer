<script lang="ts">
  import { call } from '../../lib/api';
  import FilterField from '../../lib/ui/FilterField.svelte';
  import { listNav, moveSel } from '../../lib/ui/listNav';
  import Modal from '../../lib/ui/Modal.svelte';
  import { KIND_ICON, type Conversations } from './MutedList.svelte';
  import type { SourceRow } from './model';

  let {
    source,
    muted,
    onclose,
    onchange,
  }: { source: SourceRow; muted: string[]; onclose: () => void; onchange: (v: string[]) => void } = $props();

  let data = $state<Conversations | null>(null);
  let err = $state<string | null>(null);
  let q = $state('');
  let sel = $state(0);
  let input = $state<HTMLInputElement | null>(null);

  $effect(() => {
    call<Conversations>('settings.chat_conversations', { source: source.status?.id })
      .then((d) => (data = d))
      .catch((e) => (err = String(e.message ?? e)));
  });
  $effect(() => input?.focus());

  const shown = $derived.by(() => {
    const n = q.trim().toLowerCase().replace(/^#/, '');
    return (data?.choices ?? []).filter((c) => !muted.includes(c.value) && (!n || c.name.toLowerCase().includes(n)));
  });

  function add(i: number) {
    const c = shown[i];
    if (!c) return;
    onchange([...muted, c.value]);
    sel = Math.min(i, shown.length - 2);
  }

  function onkeys(e: KeyboardEvent): boolean {
    const d = listNav(e);
    if (d) sel = moveSel(sel, d, shown.length);
    else if (e.key === 'Enter') add(sel);
    else return false;
    return true;
  }
</script>

<Modal label="Ignore a conversation" {onclose} {onkeys}>
  <div class="pick modal">
    <header>
      <span class="title">Ignore in {source.name}</span>
      <span class="muted">still read, never new in Incoming</span>
      <button class="esc" onclick={onclose}><kbd>Esc</kbd> <i class="ph ph-x"></i></button>
    </header>
    <div class="field">
      <FilterField bind:value={q} bind:el={input} placeholder="Filter channels and DMs" oninput={() => (sel = 0)} />
    </div>
    <div class="rows scroll">
      {#if err}<div class="warn">{err}</div>{:else if data && !shown.length}<div class="none">
          Nothing else seen{q ? ` matching “${q}”` : ''}.
        </div>{/if}
      {#each shown as c, i (c.value)}
        <button class="row" class:sel={i === sel} onclick={() => add(i)}>
          <i class="ph {KIND_ICON[c.kind]}"></i>
          <span class="name ellipsis">{c.name.replace(/^#/, '')}</span>
          <span class="kind">{c.kind === 'channel' ? 'channel' : c.kind === 'dm' ? 'DM' : 'group DM'}</span>
        </button>
      {/each}
    </div>
    <footer>
      <span><kbd>↑</kbd> <kbd>↓</kbd> move</span><span><kbd>↵</kbd> ignore</span><span
        >Conversations Alethiometer has seen, most recent first.</span
      ><span class="last"><kbd>Esc</kbd> done</span>
    </footer>
  </div>
</Modal>

<style>
  .pick {
    position: fixed;
    top: 90px;
    left: 50%;
    transform: translateX(-50%);
    width: 560px;
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
    gap: 12px;
    padding: 10px 16px;
    background: var(--bg-head);
    border-bottom: 1px solid var(--border);
  }
  .title {
    color: var(--tx-title);
  }
  .muted,
  .kind {
    color: var(--tx-label);
  }
  .esc {
    margin-left: auto;
    color: var(--tx-label);
  }
  .field {
    padding: 8px 16px;
    border-bottom: 1px solid var(--border);
  }
  .rows {
    flex: 1;
    min-height: 120px;
  }
  .row {
    display: grid;
    grid-template-columns: 20px minmax(0, 1fr) auto;
    gap: 12px;
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
  .name {
    color: var(--tx-2);
  }
  .none,
  .warn {
    padding: 12px 16px;
    color: var(--tx-label);
  }
  .warn {
    color: var(--status-attention);
  }
  footer {
    display: flex;
    gap: 14px;
    padding: 7px 16px;
    border-top: 1px solid var(--border);
    color: var(--tx-label);
    font-size: 0.8846rem;
  }
  footer .last {
    margin-left: auto;
    color: var(--tx-2);
  }
</style>

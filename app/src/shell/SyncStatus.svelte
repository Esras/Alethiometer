<script lang="ts">
  // Freshness per source ("synced 2m ago") and writeback state, in the tab bar.
  // A dot summarises: orange when a source errored or a write failed.
  import { call } from '../lib/api';
  import { refresh, ui } from '../lib/app.svelte';
  import { useKeys } from '../lib/keys.svelte';
  import AppIcon from '../lib/ui/AppIcon.svelte';

  type Source = { id: number; kind: string; tag: string; name: string; last_sync_at: string | null; last_error: string | null };
  type Write = { id: number; source: string; op: string; state: 'pending' | 'sent' | 'confirmed' | 'failed'; error: string | null };

  let sources = $state<Source[]>([]);
  let writes = $state<Write[]>([]);
  let open = $state(false);
  let wrap: HTMLElement;

  function onWindowClick(e: MouseEvent) {
    if (open && e.target instanceof Node && !wrap.contains(e.target)) open = false;
  }

  let synced = '';
  async function load() {
    [sources, writes] = await Promise.all([call<Source[]>('sync.status'), call<Write[]>('outbox.list')]);
    const key = sources.map((s) => `${s.id}:${s.last_sync_at}`).join();
    if (synced && key !== synced) refresh();
    synced = key;
  }

  $effect(() => {
    void ui.rev;
    load();
    const t = setInterval(load, 30_000);
    return () => clearInterval(t);
  });

  // Sample-data sources never sync; only show ones that have synced or errored.
  const live = $derived(sources.filter((s) => s.last_sync_at || s.last_error));
  const failed = $derived(writes.filter((w) => w.state === 'failed'));
  const pending = $derived(writes.filter((w) => w.state === 'pending' || w.state === 'sent'));
  const problem = $derived(live.some((s) => s.last_error) || failed.length > 0);

  function ago(ts: string | null): string {
    if (!ts) return 'never';
    const mins = Math.round((ui.nowMs - new Date(ts.replace(' ', 'T')).getTime()) / 60000);
    if (mins < 1) return 'just now';
    if (mins < 60) return `${mins}m ago`;
    const h = Math.round(mins / 60);
    return h < 24 ? `${h}h ago` : `${Math.round(h / 24)}d ago`;
  }

  const newest = $derived(live.map((s) => s.last_sync_at).filter(Boolean).sort().at(-1) ?? null);

  async function retryAll() {
    await call('outbox.retry', {});
    refresh();
  }

  // Google needs a one-time consent in the browser; Alethiometer saves the token itself.
  let connectMsg = $state<string | null>(null);
  async function connect(id: number) {
    try {
      const r = await call<{ url: string; opened: boolean }>('gcal.auth_start', { source_id: id });
      connectMsg = r.opened ? 'Approve in your browser…' : `Open this link to approve: ${r.url}`;
      for (;;) {
        await new Promise((ok) => setTimeout(ok, 1500));
        const s = await call<{ state: string; error: string | null }>('gcal.auth_status', { source_id: id });
        if (s.state === 'failed') throw new Error(s.error ?? 'consent failed');
        if (s.state !== 'waiting') break;
      }
      connectMsg = 'Connected. Syncing…';
      await syncNow(id);
      connectMsg = null;
    } catch (e) {
      connectMsg = String((e as Error).message ?? e);
    }
  }

  let syncing = $state<number | null>(null);
  async function syncNow(id: number) {
    syncing = id;
    try {
      await call('sync.now', { source_id: id });
    } finally {
      syncing = null;
      refresh();
    }
  }

  useKeys((e) => {
    if ((e.metaKey || e.ctrlKey) && e.key === 'r' && failed.length) {
      retryAll();
      return true;
    }
    if (open && e.key === 'Escape') {
      open = false;
      return true;
    }
  });
</script>

<svelte:window onclick={onWindowClick} />

<span class="wrap" bind:this={wrap}>
  <button class="chip" onclick={() => (open = !open)} aria-expanded={open} title="Sources and writes">
    <span class="dot" class:bad={problem}></span>
    {#if live.length}synced {ago(newest)}{:else}sample data{/if}
    {#if pending.length}<span class="n">· {pending.length} sending</span>{/if}
    {#if failed.length}<span class="n bad">· {failed.length} failed</span>{/if}
  </button>
  {#if open}
    <div class="pop">
      <div class="label">Sources</div>
      {#each live as s (s.id)}
        <div class="row">
          <AppIcon source={s.kind} size={14} />
          <span class="ellipsis">{s.name}</span>
          <span class="when" class:bad={!!s.last_error} title={s.last_error ?? ''}>{s.last_error ? 'error' : ago(s.last_sync_at)}</span>
          {#if (s.kind === 'gcal' || s.kind === 'gmail') && s.last_error}
            <button class="btn primary" onclick={() => connect(s.id)}>Connect</button>
          {:else}
            <button class="btn" onclick={() => syncNow(s.id)} disabled={syncing === s.id}>{syncing === s.id ? 'Syncing…' : 'Sync'}</button>
          {/if}
        </div>
        {#if s.last_error}<div class="err">{s.last_error}</div>{/if}
      {:else}
        <div class="muted">No live sources yet. Add them under "sources" in settings.json.</div>
      {/each}
      {#if connectMsg}<div class="muted connect">{connectMsg}</div>{/if}
      {#if writes.length}
        <div class="label">Writes</div>
        {#each writes.slice(0, 8) as w (w.id)}
          <div class="row w">
            <span class="state {w.state}">{w.state}</span>
            <span class="ellipsis">{w.op}</span>
            <span class="when ellipsis" title={w.error ?? ''}>{w.error ?? w.source}</span>
          </div>
        {/each}
        {#if failed.length}
          <button class="btn primary retry" onclick={retryAll}>Retry failed <kbd>⌘R</kbd></button>
        {/if}
      {/if}
    </div>
  {/if}
</span>

<style>
  .wrap {
    position: relative;
    display: flex;
    align-items: center;
  }
  .chip {
    display: flex;
    gap: 7px;
    align-items: center;
    font-size: 11.5px;
    color: var(--tx-label);
    white-space: nowrap;
  }
  .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--aqua);
  }
  .dot.bad {
    background: var(--orange);
  }
  .n.bad,
  .when.bad {
    color: var(--orange);
  }
  .pop {
    position: absolute;
    z-index: 40;
    right: 0;
    top: calc(100% + 10px);
    width: 340px;
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 10px 12px 12px;
    border-radius: var(--r-panel);
    background: var(--bg-panel);
    border: 1px solid var(--border-strong);
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.45);
    font-size: 12.5px;
  }
  .label {
    margin-top: 4px;
  }
  .row {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr) auto auto;
    gap: 8px;
    align-items: center;
  }
  .row.w {
    grid-template-columns: auto minmax(0, 1fr) auto;
  }
  .when {
    font-size: 11.5px;
    color: var(--tx-muted-2);
    max-width: 150px;
  }
  .err {
    font-size: 11.5px;
    color: var(--orange);
    padding-left: 22px;
  }
  .muted {
    color: var(--tx-label);
  }
  .connect {
    word-break: break-all;
  }
  .state {
    font-size: 10.5px;
    padding: 1px 6px;
    border-radius: 9px;
    background: var(--border);
    color: var(--tx-2);
  }
  .state.failed {
    background: rgba(254, 128, 25, 0.16);
    color: var(--orange);
  }
  .state.confirmed {
    background: rgba(184, 187, 38, 0.16);
    color: var(--green);
  }
  .retry {
    align-self: flex-start;
    margin-top: 4px;
  }
</style>

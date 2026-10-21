<script lang="ts">
  // Freshness per source ("synced 2m ago") and writeback state, in the tab bar.
  // A dot summarises: orange when a source errored or a write failed.
  import { call } from '../lib/api';
  import { clockFull, refresh, ui } from '../lib/app.svelte';
  import { chord, mod, useKeys } from '../lib/keys.svelte';
  import AppIcon from '../lib/ui/AppIcon.svelte';
  import { popover } from '../lib/ui/popover';
  import StatusMark from '../lib/ui/StatusMark.svelte';

  type Source = {
    id: number;
    kind: string;
    tag: string;
    name: string;
    last_sync_at: string | null;
    last_error: string | null;
    syncing: boolean;
    live?: boolean | null;
  };
  type Write = {
    id: number;
    source: string;
    op: string;
    target: string | null;
    state: 'pending' | 'sent' | 'confirmed' | 'failed';
    error: string | null;
    created_at: string;
  };

  type Tokens = { calls: number; input: number; output: number; cost: number | null };
  type Usage = { tier: string; today: Tokens; week: Tokens };
  type Run = {
    status: 'ok' | 'ai_off' | 'failed';
    message: string | null;
    suggested: number;
    at: string;
    last_ok_at: string | null;
  };

  let sources = $state<Source[]>([]);
  let usage = $state<Usage[]>([]);
  let writes = $state<Write[]>([]);
  let suggest = $state<{ run: Run | null; waiting: number; running?: boolean }>({ run: null, waiting: 0 });
  let open = $state(false);
  let chip = $state<HTMLElement>();

  let synced = '';
  async function load() {
    [sources, writes, usage, suggest] = await Promise.all([
      call<Source[]>('sync.status'),
      call<Write[]>('outbox.list'),
      call<Usage[]>('ai.usage'),
      call<{ run: Run | null; waiting: number; running?: boolean }>('ai.suggestions'),
    ]);
    const key = sources.map((s) => `${s.id}:${s.last_sync_at}`).join();
    if (synced && key !== synced) refresh();
    synced = key;
  }

  $effect(() => {
    void ui.rev;
    load();
    const t = setInterval(load, open ? 2_000 : 30_000);
    return () => clearInterval(t);
  });

  // Sample-data sources never sync; only show ones that have synced or errored.
  const live = $derived(sources.filter((s) => s.last_sync_at || s.last_error || s.syncing));
  const failed = $derived(writes.filter((w) => w.state === 'failed'));
  const pending = $derived(writes.filter((w) => w.state === 'pending' || w.state === 'sent'));
  const suggestFailed = $derived(suggest.run?.status === 'failed');
  const problem = $derived(live.some((s) => s.last_error) || failed.length > 0 || suggestFailed);
  const order = { failed: 0, sent: 1, pending: 1, confirmed: 2 } as const;
  const shownWrites = $derived([...writes].sort((a, b) => order[a.state] - order[b.state]).slice(0, 8));

  function ago(ts: string | null): string {
    if (!ts) return 'never';
    const mins = Math.round((ui.nowMs - new Date(ts).getTime()) / 60000);
    if (mins < 1) return 'just now';
    if (mins < 60) return `${mins}m ago`;
    const h = Math.round(mins / 60);
    return h < 24 ? `${h}h ago` : `${Math.round(h / 24)}d ago`;
  }

  const k = (n: number) => (n >= 1000 ? `${(n / 1000).toFixed(n >= 10_000 ? 0 : 1)}k` : `${n}`);
  const tokens = (t: Tokens) =>
    `${t.calls} calls · ${k(t.input)} in · ${k(t.output)} out${t.cost === null ? '' : ` · $${t.cost.toFixed(2)}`}`;

  const newest = $derived(
    live
      .map((s) => s.last_sync_at)
      .filter(Boolean)
      .sort()
      .at(-1) ?? null,
  );

  function runState(r: Run | null): string {
    if (!r) return 'not run yet';
    if (r.status === 'failed') return `failed ${ago(r.at)}`;
    if (r.status === 'ai_off') return 'off';
    return r.suggested ? `ok · ${r.suggested} suggested · ${ago(r.at)}` : 'ok · nothing new';
  }

  let suggesting = $state(false);
  async function retrySuggest() {
    suggesting = true;
    try {
      await call('ai.suggest_topics', { limit: 10 });
    } catch {
    } finally {
      suggesting = false;
      await load();
      refresh();
    }
  }

  async function retryAll() {
    await call('outbox.retry', {});
    refresh();
  }

  // Google and Microsoft need a one-time consent in the browser; Alethiometer saves the token itself.
  // Gmail signs in through the calendar's calls.
  const SIGN_IN: Record<string, string> = { gcal: 'gcal', gmail: 'gcal', teams: 'teams' };
  let connectMsg = $state<string | null>(null);
  async function connect(id: number, kind: string) {
    const via = SIGN_IN[kind];
    try {
      const r = await call<{ url: string; opened: boolean }>(`${via}.auth_start`, { source_id: id });
      connectMsg = r.opened ? 'Approve in your browser…' : `Open this link to approve: ${r.url}`;
      for (;;) {
        await new Promise((ok) => setTimeout(ok, 1500));
        const s = await call<{ state: string; error: string | null }>(`${via}.auth_status`, { source_id: id });
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

  let clicked = $state<Set<number>>(new Set());
  async function syncNow(id: number) {
    clicked = new Set(clicked).add(id);
    try {
      await call('sync.now', { source_id: id });
      refresh();
      await call('sync.suggest', { source_id: id });
    } finally {
      const next = new Set(clicked);
      next.delete(id);
      clicked = next;
      refresh();
    }
  }

  useKeys((e) => {
    if (mod(e) && e.key === 'r' && failed.length) {
      retryAll();
      return true;
    }
  });
</script>

<span class="wrap">
  <button class="sync" bind:this={chip} onclick={() => (open = !open)} aria-expanded={open} title="Sources and writes">
    <StatusMark status={problem ? 'problem' : 'running'} />
    {#if live.length}synced {ago(newest)}{:else}sample data{/if}
    {#if pending.length}<span class="n">· {pending.length} sending</span>{/if}
    {#if failed.length}<span class="n bad">· {failed.length} failed</span>{/if}
    {#if suggestFailed}<span class="n bad">· suggestions failed</span>{/if}
  </button>
  {#if open}
    <div class="pop popover" use:popover={{ onclose: () => (open = false), trigger: chip }}>
      <div class="label">Sources</div>
      {#each live as s (s.id)}
        <div class="row">
          <AppIcon source={s.kind} size={14} />
          <span class="ellipsis">{s.name}</span>
          <span class="when" class:broken={!!s.last_error} title={s.last_error ?? ''}
            >{#if s.live != null}<span
                class="mode"
                class:on={s.live}
                title={s.live
                  ? 'Live socket connected: new messages arrive within seconds; a full catch-up runs every few hours'
                  : 'Socket down: polling on a timer'}>{s.live ? 'live' : 'polling'}</span
              > ·
            {/if}{s.last_error ? 'error' : ago(s.last_sync_at)}</span
          >
          {#if SIGN_IN[s.kind] && s.last_error}
            <button class="btn" onclick={() => connect(s.id, s.kind)}>Connect</button>
          {:else}
            {@const busy = s.syncing || clicked.has(s.id)}
            <button class="btn" onclick={() => syncNow(s.id)} disabled={busy}>{busy ? 'Syncing…' : 'Sync'}</button>
          {/if}
        </div>
        {#if s.last_error}<div class="err broken">{s.last_error}</div>{/if}
      {:else}
        <div class="empty-row">No live sources yet. Add them under "sources" in settings.json.</div>
      {/each}
      {#if connectMsg}<div class="muted connect">{connectMsg}</div>{/if}
      <div class="label">AI · Bedrock</div>
      <div class="row">
        <i class="ph ph-sparkle spark"></i>
        <span class="ellipsis">Topic suggestions</span>
        <span class="when" class:bad={suggestFailed && !suggest.running}
          >{suggest.running ? 'running…' : runState(suggest.run)}</span
        >
        <button class="btn primary" onclick={retrySuggest} disabled={suggesting || suggest.running}
          >{suggesting || suggest.running ? 'Running…' : 'Retry'}</button
        >
      </div>
      {#if suggest.run?.message && suggest.run.status !== 'ok'}<div class="err" class:off={!suggestFailed}>
          {suggest.run.message}
        </div>{/if}
      {#if suggestFailed}
        <div class="err sub">
          {suggest.waiting} signals waiting in Unsorted · last success {ago(suggest.run?.last_ok_at ?? null)}
        </div>
      {/if}
      {#if usage.length}
        <div class="label">AI tokens</div>
        {#each usage as u (u.tier)}
          <div class="row u">
            <span>{u.tier}</span>
            <span class="when tok" title="last 7 days: {tokens(u.week)}">today {tokens(u.today)}</span>
          </div>
        {/each}
      {/if}
      {#if writes.length}
        <div class="label">Writes</div>
        {#each shownWrites as w (w.id)}
          <div class="row w">
            <span class="count {w.state}">{w.state}</span>
            <span class="ellipsis" title={w.target ?? w.op}
              >{w.op}{#if w.target}<span class="target"> · {w.target}</span>{/if}</span
            >
            <span class="when ellipsis" title={clockFull(w.created_at)}
              >{w.error ? `error · ${ago(w.created_at)}` : w.source}</span
            >
          </div>
          {#if w.error}<div class="err">{w.error}</div>{/if}
        {/each}
        {#if failed.length}
          <button class="btn primary retry" onclick={retryAll}>Retry failed <kbd>{chord('⌘R', 'Ctrl+R')}</kbd></button>
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
  .sync {
    display: flex;
    gap: 7px;
    align-items: center;
    font-size: 0.8846rem;
    color: var(--tx-label);
    white-space: nowrap;
  }
  .n.bad,
  .when.bad {
    color: var(--status-attention);
  }
  .when.broken {
    color: var(--status-broken);
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
    border-radius: var(--radius-md);
    font-size: 0.9615rem;
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
  .target {
    color: var(--tx-2);
  }
  .row.u {
    grid-template-columns: minmax(0, 1fr) auto;
  }
  .tok {
    max-width: none;
  }
  .mode.on {
    color: var(--aqua);
  }
  .when {
    font-size: 0.8846rem;
    color: var(--tx-muted-2);
    max-width: 150px;
  }
  .err {
    font-size: 0.8846rem;
    padding-left: 22px;
  }
  .muted {
    color: var(--tx-label);
  }
  .empty-row {
    padding: 0;
    font: inherit;
  }
  .spark {
    font-size: 1.0769rem;
    color: var(--aqua);
  }
  .err.off,
  .err.sub {
    color: var(--tx-muted-2);
  }
  .connect {
    word-break: break-all;
  }
  .count.failed {
    background: rgba(254, 128, 25, 0.16);
    color: var(--status-attention);
  }
  .count.confirmed {
    background: rgba(184, 187, 38, 0.16);
    color: var(--status-done);
  }
  .retry {
    align-self: flex-start;
    margin-top: 4px;
  }
</style>

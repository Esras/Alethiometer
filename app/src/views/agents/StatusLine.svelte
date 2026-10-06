<script lang="ts">
  // Sessions: harness, model, host; then context, cache, turns, elapsed,
  // cost. Rooms: Hub, thread id, mode; then loop cap, messages, live, ? keys.
  import { act, ag } from './state.svelte';
  import type { Group, Session } from './types';
  import StatusMark from '../../lib/ui/StatusMark.svelte';

  let { session, group, room = false }: { session?: Session; group?: Group; room?: boolean } = $props();

  let now = $state(Date.now());
  $effect(() => {
    const t = setInterval(() => (now = Date.now()), 5000);
    return () => clearInterval(t);
  });

  const r = $derived(group?.room);
  const s = $derived(session);
  const pct = $derived(s && s.stats.ctx_size ? Math.round((100 * s.stats.ctx_used) / s.stats.ctx_size) : null);
  const k = (n: number) => (n >= 1000 ? `${Math.round(n / 1000)}k` : String(n));
  const elapsed = $derived.by(() => {
    if (!s) return '';
    const from = new Date(s.stats.started_at || s.created_at).getTime();
    const to = s.live ? now : new Date(s.active_at).getTime();
    const m = Math.max(0, Math.round((to - from) / 60000));
    return m < 60 ? `${m}m` : `${Math.floor(m / 60)}h ${String(m % 60).padStart(2, '0')}m`;
  });
  // The agent rarely says; the cache lasts the harness's cache_ttl_min after a turn (Claude Code: 5 unless set).
  const ttl = $derived(ag.tree?.harnesses.find((h) => h.key === s?.template)?.cache_ttl_min ?? (s && /claude/i.test(s.harness) ? 5 : null));
  const cache = $derived.by(() => {
    if (!s) return null;
    if (s.stats.cache) return { label: s.stats.cache, cls: 'aqua', fill: 60 };
    if (!ttl || !s.stats.turns) return null;
    const left = ttl - (now - new Date(s.active_at).getTime()) / 60000;
    if (s.status === 'running') return { label: 'warm', cls: 'aqua', fill: 100 };
    if (left > 1) return { label: `warm ~${Math.round(left)}m`, cls: 'aqua', fill: Math.round((left * 100) / ttl) };
    if (left > 0) return { label: 'cooling', cls: 'yellow', fill: 15 };
    return { label: 'cold', cls: 'muted', fill: 0 };
  });
</script>

<div class="line">
  {#if room && r}
    <span>{r.hub}</span>
    {#if r.thread_ref}<span>{r.thread_ref}</span>{/if}
    {#if r.mode}<span>mode: {r.mode}</span>{/if}
    <span class="right">
      {#if r.loop_cap}<span>{r.loop_cap}</span>{/if}
      <span>{r.messages} messages</span>
      <span class="live"><StatusMark status={r.live ? 'running' : 'offline'} />{r.live ? 'live' : 'offline'}</span>
      <button class="keys" onclick={() => (ag.keys = true)}><kbd>?</kbd>keys</button>
    </span>
  {:else if s}
    <span>{s.harness}</span>
    {#if s.model}<span>{s.model}</span>{/if}
    {#if s.thinking}
      <span class="g">Thinking:
      <select class="input sm think" title="Thinking" value={s.thinking.current} disabled={!s.live} onchange={(e) => {
          const el = e.currentTarget;
          act('set_thinking', { session: s.id, value: el.value }).catch(() => (el.value = s.thinking?.current ?? ''));
        }}>
        {#each s.thinking.options as o (o.value)}<option value={o.value}>{o.name.replace(/^thinking:?\s*/i, '').toLowerCase()}</option>{/each}
      </select></span>
    {/if}
    <span>{s.source === 'acp' ? (ag.tree?.host ?? s.host) : s.host}</span>
    {#if s.problem}<span class="problem" title={s.problem}>{s.problem}</span>{/if}
    <span class="right">
      {#if pct !== null}
        <span class="g">ctx <span class="v">{pct}%</span><span class="bar" style:background="linear-gradient(90deg,var(--tx-muted-2) {pct}%,var(--border) {pct}%)"></span><span class="dim">{k(s.stats.ctx_used)}/{k(s.stats.ctx_size)}</span></span>
      {/if}
      {#if cache}
        <span class="g">cache <span class={cache.cls}>{cache.label}</span>{#if cache.fill}<span class="bar short" style:background="linear-gradient(90deg,var(--aqua) {cache.fill}%,var(--border) {cache.fill}%)"></span>{/if}</span>
      {/if}
      <span>{s.stats.turns} turn{s.stats.turns === 1 ? '' : 's'}</span>
      <span>{elapsed}</span>
      {#if s.stats.cost_usd != null}<span>${s.stats.cost_usd.toFixed(2)}</span>{/if}
    </span>
  {/if}
</div>

<style>
  /* Keeps the status line's own size and height; only the field look is the system's. */
  .think {
    width: auto;
    font: inherit;
    border-radius: var(--radius-md);
    padding: 0 4px;
  }
  .think:disabled {
    color: var(--tx-disabled);
  }
  .line {
    display: flex;
    gap: 6px 14px;
    flex-wrap: wrap;
    align-items: center;
    margin: 0 -12px -12px;
    padding: 7px 12px 8px;
    border-top: 1px solid var(--bg-head);
    font: 0.846rem var(--font-mono);
    color: var(--tx-label);
  }
  .right {
    margin-left: auto;
    display: flex;
    gap: 14px;
    align-items: center;
  }
  .g {
    display: flex;
    gap: 6px;
    align-items: center;
  }
  .v {
    color: var(--tx-muted);
  }
  .dim {
    color: var(--tx-disabled);
  }
  .bar {
    display: inline-block;
    width: 40px;
    height: 5px;
    border-radius: 3px;
  }
  .bar.short {
    width: 32px;
  }
  .aqua {
    color: var(--aqua);
  }
  .yellow {
    color: var(--status-headsup);
  }
  .muted {
    color: var(--tx-disabled);
  }
  .problem {
    color: var(--status-attention);
    max-width: 380px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .live {
    display: flex;
    gap: 6px;
    align-items: center;
  }
  .keys {
    display: flex;
    gap: 6px;
    align-items: center;
    color: var(--tx-label);
  }
</style>

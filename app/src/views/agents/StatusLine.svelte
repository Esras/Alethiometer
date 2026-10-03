<script lang="ts">
  // Sessions: harness, model, host · cwd; then context, cache, turns, elapsed,
  // cost. Rooms: Hub, thread id, mode; then loop cap, messages, live, ? keys.
  import { ag } from './state.svelte';
  import type { Group, Session } from './types';

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
  // The agent rarely says; for Claude the prompt cache lasts about five minutes after a turn.
  const cache = $derived.by(() => {
    if (!s) return null;
    if (s.stats.cache) return { label: s.stats.cache, cls: 'aqua', fill: 60 };
    if (!/claude/i.test(s.harness) || !s.stats.turns) return null;
    const left = 5 - (now - new Date(s.active_at).getTime()) / 60000;
    if (s.status === 'running') return { label: 'warm', cls: 'aqua', fill: 100 };
    if (left > 1) return { label: `warm ~${Math.round(left)}m`, cls: 'aqua', fill: Math.round(left * 20) };
    if (left > 0) return { label: 'cooling', cls: 'yellow', fill: 15 };
    return { label: 'cold', cls: 'muted', fill: 0 };
  });
  const tilde = (p: string) => p.replace(/^\/(Users|home)\/[^/]+/, '~');
</script>

<div class="line">
  {#if room && r}
    <span>{r.hub}</span>
    {#if r.thread_ref}<span>{r.thread_ref}</span>{/if}
    {#if r.mode}<span>mode: {r.mode}</span>{/if}
    <span class="right">
      {#if r.loop_cap}<span>{r.loop_cap}</span>{/if}
      <span>{r.messages} messages</span>
      <span class="live"><span class="dot" class:off={!r.live}></span>{r.live ? 'live' : 'offline'}</span>
      <button class="keys" onclick={() => (ag.keys = true)}><span class="kbd">?</span>keys</button>
    </span>
  {:else if s}
    <span>{s.harness}</span>
    {#if s.model}<span>{s.model}</span>{/if}
    <span>{s.source === 'acp' ? `${s.host} · ${tilde(s.cwd)}` : s.host}</span>
    {#if s.problem}<span class="problem" title={s.problem}>{s.problem}</span>{/if}
    <span class="right">
      {#if pct !== null}
        <span class="g">ctx <span class="v">{pct}%</span><span class="bar" style:background="linear-gradient(90deg,#a89984 {pct}%,#3c3836 {pct}%)"></span><span class="dim">{k(s.stats.ctx_used)}/{k(s.stats.ctx_size)}</span></span>
      {/if}
      {#if cache}
        <span class="g">cache <span class={cache.cls}>{cache.label}</span>{#if cache.fill}<span class="bar short" style:background="linear-gradient(90deg,#8ec07c {cache.fill}%,#3c3836 {cache.fill}%)"></span>{/if}</span>
      {/if}
      <span>{s.stats.turns} turn{s.stats.turns === 1 ? '' : 's'}</span>
      <span>{elapsed}</span>
      {#if s.stats.cost_usd != null}<span>${s.stats.cost_usd.toFixed(2)}</span>{/if}
    </span>
  {/if}
</div>

<style>
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
    color: var(--yellow);
  }
  .muted {
    color: var(--tx-disabled);
  }
  .problem {
    color: var(--orange);
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
  .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--aqua);
  }
  .dot.off {
    background: var(--tx-disabled);
  }
  .keys {
    display: flex;
    gap: 6px;
    align-items: center;
    color: var(--tx-label);
  }
  .kbd {
    font: 0.846rem var(--font-mono);
    color: var(--tx-body);
    background: var(--bg-head);
    border: 1px solid var(--border-strong);
    border-radius: var(--r-block);
    padding: 0 5px;
  }
</style>

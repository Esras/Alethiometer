<script lang="ts">
  // Review… (26b): every channel you're in, searchable and sortable, with a
  // checkbox, last activity and messages per day. In All mode, unchecking one
  // switches to Only these with everything else still checked.
  import { call } from '../../lib/api';
  import { ui } from '../../lib/app.svelte';
  import { useKeys } from '../../lib/keys.svelte';
  import { ago, type SourceRow } from './model';

  type Chan = { id: string; name: string; private: boolean; last: string | null; per_day: number };

  let { source, channels, onclose, onchange }: { source: SourceRow; channels: unknown; onclose: () => void; onchange: (v: unknown) => void } = $props();

  let all = $state<Chan[]>([]);
  let err = $state<string | null>(null);
  let loading = $state(true);
  let q = $state('');
  let sort = $state<'name' | 'last' | 'per_day'>('per_day');
  let sel = $state(0);
  let input: HTMLInputElement;

  $effect(() => {
    call<Chan[]>('settings.slack_channels', { source: source.status?.id })
      .then((c) => (all = c))
      .catch((e) => (err = String(e.message ?? e)))
      .finally(() => (loading = false));
  });

  $effect(() => input?.focus());

  const allMode = $derived(channels === 'all');
  const list = $derived(Array.isArray(channels) ? (channels as string[]) : []);
  const follows = (c: Chan) => allMode || list.some((x) => x === c.id || x.replace(/^#/, '') === c.name);

  const shown = $derived.by(() => {
    const n = q.trim().toLowerCase().replace(/^#/, '');
    const rows = all.filter((c) => !n || c.name.includes(n));
    const by = {
      name: (a: Chan, b: Chan) => a.name.localeCompare(b.name),
      last: (a: Chan, b: Chan) => (b.last ?? '').localeCompare(a.last ?? ''),
      per_day: (a: Chan, b: Chan) => b.per_day - a.per_day || a.name.localeCompare(b.name),
    }[sort];
    return [...rows].sort(by);
  });
  const followed = $derived(all.filter(follows).length);

  function toggle(c: Chan) {
    if (allMode) {
      onchange(all.filter((x) => x.id !== c.id).map((x) => `#${x.name}`));
      return;
    }
    if (follows(c)) onchange(list.filter((x) => x !== c.id && x.replace(/^#/, '') !== c.name));
    else onchange([...list, `#${c.name}`]);
  }

  useKeys((e) => {
    if (e.key === 'Escape') onclose();
    else if (e.key === 'ArrowDown' || (e.ctrlKey && e.key === 'j')) sel = Math.min(sel + 1, shown.length - 1);
    else if (e.key === 'ArrowUp' || (e.ctrlKey && e.key === 'k')) sel = Math.max(sel - 1, 0);
    else if (e.key === 'Enter' && shown[sel]) toggle(shown[sel]);
    else if (e.key === 'Tab') sort = sort === 'per_day' ? 'last' : sort === 'last' ? 'name' : 'per_day';
    else return false;
    return true;
  });
</script>

<div class="scrim" onclick={onclose} role="presentation"></div>
<div class="review" role="dialog" aria-label="Slack channels">
  <header>
    <span class="title">Channels in {source.name}</span>
    <span class="muted">{allMode ? `All I'm in · ${all.length}` : `Only these · ${followed} of ${all.length}`}</span>
    <button class="esc" onclick={onclose}>esc <i class="ph ph-x"></i></button>
  </header>
  <div class="field"><i class="ph ph-magnifying-glass"></i><input bind:this={input} bind:value={q} placeholder="Filter channels" oninput={() => (sel = 0)} /></div>
  <div class="thead">
    <span></span>
    <button class:on={sort === 'name'} onclick={() => (sort = 'name')}>Channel</button>
    <button class:on={sort === 'last'} onclick={() => (sort = 'last')}>Last activity</button>
    <button class:on={sort === 'per_day'} onclick={() => (sort = 'per_day')}>Msgs/day</button>
  </div>
  <div class="rows scroll">
    {#if loading}
      <div class="empty">Asking Slack…</div>
    {:else if err}
      <div class="empty warn">{err}</div>
    {/if}
    {#each shown as c, i (c.id)}
      <button class="row" class:sel={i === sel} onclick={() => toggle(c)}>
        <i class="ph {follows(c) ? 'ph-check-square' : 'ph-square'}" class:on={follows(c)}></i>
        <span class="name">{c.private ? '🔒' : '#'}{c.name}</span>
        <span class="muted tnum">{c.last ? `${ago(c.last, ui.nowMs)} ago` : '—'}</span>
        <span class="muted tnum r">{c.per_day ? c.per_day : '—'}</span>
      </button>
    {/each}
  </div>
  <footer>
    <span>↑↓ channel</span><span>↵ follow / unfollow</span><span>⇥ sort</span><span>Activity is what Alethiometer saw in the last 14 days.</span><span class="last">esc done</span>
  </footer>
</div>

<style>
  .scrim {
    position: fixed;
    inset: 0;
    z-index: 70;
    background: rgba(0, 0, 0, 0.3);
  }
  .review {
    position: fixed;
    z-index: 71;
    top: 90px;
    left: 50%;
    transform: translateX(-50%);
    width: 640px;
    max-height: calc(100vh - 180px);
    display: flex;
    flex-direction: column;
    border-radius: var(--r-panel);
    border: 1px solid var(--border-strong);
    background: var(--bg-panel);
    box-shadow: 0 14px 40px rgba(0, 0, 0, 0.55);
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
  .esc {
    margin-left: auto;
    color: var(--tx-label);
  }
  .field {
    display: flex;
    gap: 8px;
    align-items: center;
    padding: 8px 16px;
    border-bottom: 1px solid var(--border);
    color: var(--tx-label);
  }
  .field input {
    flex: 1;
    background: none;
    border: 0;
    outline: none;
  }
  .thead,
  .row {
    display: grid;
    grid-template-columns: 20px minmax(0, 1fr) 110px 80px;
    gap: 12px;
    align-items: center;
    padding: 6px 16px;
    text-align: left;
  }
  .thead {
    font-size: 0.8077rem;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--tx-label);
    border-bottom: 1px solid var(--border);
  }
  .thead button {
    text-align: left;
    color: inherit;
    letter-spacing: inherit;
    text-transform: inherit;
  }
  .thead button:last-child {
    text-align: right;
  }
  .thead button.on {
    color: var(--tx-2);
  }
  .rows {
    flex: 1;
    min-height: 120px;
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
  .name {
    color: var(--tx-2);
  }
  .muted {
    color: var(--tx-label);
  }
  .r {
    text-align: right;
  }
  .empty {
    padding: 12px 16px;
    color: var(--tx-label);
  }
  .warn {
    color: var(--orange);
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

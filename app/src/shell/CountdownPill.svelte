<script lang="ts">
  // Reflect has no day strip; only this pill, and only when something is near.
  import { call } from '../lib/api';
  import { nowMinute, ui, urgencyColor } from '../lib/app.svelte';
  import type { DayStripData } from '../lib/types';

  const NEAR_MIN = 30;
  let data = $state<DayStripData | null>(null);
  $effect(() => {
    void ui.rev;
    call<DayStripData>('daystrip.get', {}).then((d) => (data = d));
  });
  const now = $derived.by(() => {
    void ui.nowMs;
    return nowMinute();
  });
  const next = $derived(data?.blocks.find((b) => b.start_min > now && b.category !== 'self') ?? null);
  const left = $derived(next ? Math.ceil(next.start_min - now) : null);
</script>

{#if next && left !== null && left <= NEAR_MIN}
  <span class="pill-cd">
    <span class="dot" style:background={urgencyColor(left)}></span>
    {next.life ? 'Life' : next.title}
    <span class="tnum" style:color={urgencyColor(left)} style:font-weight="500">in {left} min</span>
  </span>
{/if}

<style>
  .pill-cd {
    display: flex;
    gap: 8px;
    align-items: center;
    padding: 4px 12px;
    border-radius: 14px;
    border: 1px solid var(--border);
    background: var(--bg-panel);
    font-size: 0.9231rem;
    color: var(--tx-muted);
    white-space: nowrap;
  }
  .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
  }
</style>

<script lang="ts">
  // Reflect has no day strip; only this pill, and only when something is near.
  import { live } from '../lib/live';
  import { nowMinute, ui, urgencyColor } from '../lib/app.svelte';
  import type { DayStripData } from '../lib/types';
  import Pill from '../lib/ui/CountdownPill.svelte';

  const NEAR_MIN = 30;
  let data = $state<DayStripData | null>(null);
  $effect(() => live<DayStripData>('daystrip.get', {}, (d) => (data = d)));
  const now = $derived.by(() => {
    void ui.nowMs;
    return nowMinute();
  });
  const next = $derived(data?.blocks.find((b) => b.start_min > now && b.category !== 'self') ?? null);
  const left = $derived(next ? Math.ceil(next.start_min - now) : null);
</script>

{#if next && left !== null && left <= NEAR_MIN}
  <Pill title={next.life ? 'Life' : next.title} {left} color={urgencyColor(left)} />
{/if}

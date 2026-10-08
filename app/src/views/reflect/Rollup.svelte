<script lang="ts">
  // One row per child period (days in a week, weeks in a month, months in a
  // quarter): its one-line summary from the reflection below, and a small
  // stacked bar of where the time went, in category colors.
  import { catColor, category } from '../../lib/app.svelte';
  import Panel from '../../lib/ui/Panel.svelte';
  import type { ChildRow } from './types';

  let { rows, label, icon, open }: { rows: ChildRow[]; label: string; icon: string; open: (row: ChildRow) => void } =
    $props();

  const hm = (m: number) => (m >= 60 ? `${Math.floor(m / 60)}h${m % 60 ? `${m % 60}m` : ''}` : `${m}m`);
</script>

<Panel {icon} iconColor="var(--blue)" {label} count={rows.length} collapsible={false}>
  {#each rows as r (r.period)}
    <button class="row" onclick={() => open(r)} title="Open {r.label}">
      <span class="lbl nowrap">{r.label}</span>
      {#if r.summary}
        <span class="sum">{r.summary}</span>
      {:else}
        <span class="sum none">no entry</span>
      {/if}
      <span class="bar" title={r.cats.map((c) => `${category(c.category).label} ${hm(c.minutes)}`).join(' · ')}>
        {#each r.cats as c}
          <span style:flex={c.minutes} style:background={catColor(c.category)}></span>
        {/each}
      </span>
    </button>
  {/each}
</Panel>

<style>
  .row {
    width: 100%;
    display: grid;
    grid-template-columns: 52px minmax(0, 1fr) 160px;
    gap: 12px;
    align-items: center;
    padding: var(--row-pad-cozy) 12px;
    min-height: var(--row-min-cozy);
    border-top: 1px solid var(--bg-head);
    text-align: left;
    transition: background 120ms;
  }
  .row:hover {
    background: var(--row-hover);
  }
  .lbl {
    color: var(--tx-muted-2);
  }
  .sum {
    color: var(--tx-2);
    min-width: 0;
    overflow: hidden;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
  }
  .sum.none {
    color: var(--tx-label);
  }
  .bar {
    display: flex;
    height: 8px;
    gap: 2px;
  }
  .bar span {
    border-radius: 2px;
  }
</style>

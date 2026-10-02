<script lang="ts">
  // One row per child period (days in a week, weeks in a month, months in a
  // quarter): its one-line summary from the reflection below, and a small
  // stacked bar of where the time went, in category colors.
  import { catColor, category } from '../../lib/app.svelte';
  import type { ChildRow } from './types';

  let {
    rows,
    label,
    icon,
    open,
  }: { rows: ChildRow[]; label: string; icon: string; open: (row: ChildRow) => void } = $props();

  const hm = (m: number) => (m >= 60 ? `${Math.floor(m / 60)}h${m % 60 ? `${m % 60}m` : ''}` : `${m}m`);
</script>

<section class="panel">
  <header>
    <i class="ph-bold ph-{icon}" style:color="var(--blue)"></i>{label}
    <span class="count">{rows.length}</span>
  </header>
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
</section>

<style>
  .panel {
    flex: none;
    border-radius: var(--r-panel);
    background: var(--bg-panel);
    border: 1px solid var(--border);
    overflow: hidden;
  }
  header {
    display: flex;
    gap: 8px;
    align-items: center;
    padding: 7px 12px;
    background: var(--bg-head);
    font-size: 0.8462rem;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--tx-2);
  }
  header i {
    font-size: 1rem;
  }
  .count {
    margin-left: auto;
    letter-spacing: 0;
    color: var(--tx-muted-2);
  }
  .row {
    width: 100%;
    display: grid;
    grid-template-columns: 52px minmax(0, 1fr) 160px;
    gap: 12px;
    align-items: center;
    padding: 10px 12px;
    border-top: 1px solid var(--bg-head);
    text-align: left;
    transition: background 120ms;
  }
  .row:hover {
    background: rgba(235, 219, 178, 0.03);
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

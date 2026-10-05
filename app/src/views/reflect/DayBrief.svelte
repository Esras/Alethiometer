<script lang="ts">
  // "The day, briefly" (9b): four fact tiles, plan vs did to scale, and two
  // folded sections behind aqua text links.
  import { catColor } from '../../lib/app.svelte';
  import Panel from '../../lib/ui/Panel.svelte';
  import type { Bar, DayBriefData } from './types';

  let {
    brief,
    showWhere = $bindable(false),
    showDecided = $bindable(false),
    life = false,
  }: { brief: DayBriefData; showWhere?: boolean; showDecided?: boolean; life?: boolean } = $props();

  const span = $derived(brief.range.end_min - brief.range.start_min);
  const pct = (m: number) => `${Math.max(0, Math.min(100, ((m - brief.range.start_min) / span) * 100))}%`;
  const width = (b: Bar) => `${Math.max(0, ((Math.min(b.end_min, brief.range.end_min) - Math.max(b.start_min, brief.range.start_min)) / span) * 100)}%`;
  const hours = $derived.by(() => {
    const out: number[] = [];
    for (let h = Math.ceil(brief.range.start_min / 60); h * 60 <= brief.range.end_min; h += 2) out.push(h);
    return out;
  });
  const maxWhere = $derived(Math.max(1, ...brief.where.map((w) => w.minutes)));
</script>

<Panel icon="sun-horizon" iconColor="var(--yellow)" label="The day, briefly" collapsible={false}>
  <div class="inner">
    <div class="tiles">
      {#each brief.tiles as t}
        <div class="tile inset">
          <div class="v tnum">{t.value}</div>
          <div class="l ellipsis">{t.label}</div>
        </div>
      {/each}
    </div>

    <div class="bars">
      <div class="row">
        <span title="Your calendar for the day">plan</span>
        <div class="track">
          {#each brief.plan as b}
            <span class="blk" style:left={pct(b.start_min)} style:width={width(b)} style:background={catColor(b.category)} style:opacity="0.5"></span>
          {/each}
        </div>
      </div>
      <div class="row">
        <span title={brief.did_note}>did</span>
        <div class="track">
          {#each brief.did as b}
            <span class="blk" style:left={pct(b.start_min)} style:width={width(b)} style:background={b.over ? 'var(--status-attention)' : catColor(b.category)}></span>
          {/each}
        </div>
      </div>
      <div class="axis tnum">
        {#each hours as h}<span style:left={pct(h * 60)}>{String(h).padStart(2, '0')}</span>{/each}
      </div>
    </div>

    <div class="links">
      <button class="link-toggle" onclick={() => (showWhere = !showWhere)} title="T">
        <i class="ph ph-caret-{showWhere ? 'down' : 'right'}"></i>Where time went
      </button>
      {#if !life}
        <button class="link-toggle" onclick={() => (showDecided = !showDecided)} title="D">
          <i class="ph ph-caret-{showDecided ? 'down' : 'right'}"></i>Decided · {brief.decided.length}
        </button>
      {/if}
    </div>

    {#if showWhere}
      <div class="fold">
        {#each brief.where as w}
          <div class="where">
            <span class="ellipsis">{w.label}</span>
            <span class="meter"><span style:width="{(w.minutes / maxWhere) * 100}%" style:background={catColor(w.category)}></span></span>
            <span class="tnum dur">{w.dur}</span>
          </div>
        {:else}
          <div class="empty-row">Nothing on the calendar.</div>
        {/each}
      </div>
    {/if}
    {#if showDecided && !life}
      <div class="fold">
        {#each brief.decided as d}
          <div class="decided">
            <i class="ph-bold ph-check"></i>
            <span class="ellipsis">{d.title}</span>
            {#if d.topic}<span class="topic nowrap">{d.topic}</span>{/if}
          </div>
        {:else}
          <div class="empty-row">Nothing recorded as decided.</div>
        {/each}
      </div>
    {/if}
  </div>
</Panel>

<style>
  .inner {
    padding: 12px;
    display: flex;
    flex-direction: column;
    gap: 14px;
  }
  .tiles {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 8px;
  }
  .tile {
    min-width: 0;
  }
  .v {
    font-size: 1.6923rem;
    font-weight: 500;
    color: var(--tx-title);
  }
  .l {
    font-size: 0.8846rem;
    color: var(--tx-muted-2);
  }
  .bars {
    display: flex;
    flex-direction: column;
    gap: 5px;
  }
  .row {
    display: grid;
    grid-template-columns: 44px minmax(0, 1fr);
    gap: 10px;
    align-items: center;
    font-size: 0.8462rem;
    color: var(--tx-label);
  }
  .track {
    position: relative;
    height: 10px;
  }
  .blk {
    position: absolute;
    top: 0;
    bottom: 0;
    border-radius: 2px;
    box-shadow: 1px 0 0 var(--bg-panel);
  }
  .axis {
    position: relative;
    height: 12px;
    margin-left: 54px;
    font-size: 0.7692rem;
    color: var(--tx-disabled);
  }
  .axis span {
    position: absolute;
    transform: translateX(-50%);
  }
  .axis span:first-child {
    transform: none;
  }
  .links {
    display: flex;
    gap: 16px;
    font-size: 0.9231rem;
  }
  .link-toggle {
    white-space: nowrap;
  }
  .link-toggle:hover {
    text-decoration: underline;
  }
  .fold {
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin-top: -4px;
    font-size: 0.9231rem;
  }
  .where {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 90px 48px;
    gap: 10px;
    align-items: center;
    color: var(--tx-2);
  }
  .meter {
    height: 6px;
    border-radius: 2px;
    background: var(--bg-head);
    overflow: hidden;
  }
  .meter span {
    display: block;
    height: 100%;
    border-radius: 2px;
  }
  .dur {
    text-align: right;
    color: var(--tx-muted-2);
  }
  .decided {
    display: flex;
    gap: 8px;
    align-items: center;
    color: var(--tx-2);
    min-width: 0;
  }
  .decided i {
    color: var(--aqua);
    flex: none;
  }
  .topic {
    margin-left: auto;
    color: var(--tx-label);
    font-size: 0.8846rem;
  }
  .empty-row {
    padding: 0;
    font: inherit;
  }
</style>

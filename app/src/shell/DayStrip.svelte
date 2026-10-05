<script lang="ts">
  // Day strip (DayStripGruv, 7a/7b/10a/11a): countdown card + to-scale blocks.
  // Every block is faded; meetings linked to the current topic return to full
  // strength with an aqua ring. Life blocks are untitled blue hatching.
  import { call } from '../lib/api';
  import { live } from '../lib/live';
  import { catColor, hm, nowMinute, ui, urgencyColor } from '../lib/app.svelte';
  import { useKeys } from '../lib/keys.svelte';
  import { popover as popoverBehaviour } from '../lib/ui/popover';
  import type { Block, DayStripData } from '../lib/types';

  let data = $state<DayStripData | null>(null);
  let popover = $state<Block | null>(null);
  let popTrigger = $state<Element | null>(null);
  let editingBuffer = $state(false);
  let bufferInput = $state('');

  $effect(() => live<DayStripData>('daystrip.get', { topic_id: ui.topicId }, (d) => (data = d)));

  const span = $derived(data ? data.day_end_min - data.day_start_min : 660);
  const pct = (min: number) => (data ? Math.max(0, Math.min(100, ((min - data.day_start_min) / span) * 100)) : 0);
  const now = $derived.by(() => {
    void ui.nowMs;
    return nowMinute();
  });

  // Next thing to count down to: the next block that starts after now. Self-holds
  // (Lunch, Walk) don't count; Life blocks do.
  const next = $derived(data?.blocks.find((b) => b.start_min > now && b.category !== 'self') ?? null);
  const left = $derived(next ? Math.ceil(next.start_min - now) : null);
  const color = $derived(left === null ? 'var(--tx-label)' : urgencyColor(left));

  const hl = (b: Block) => ui.topicId !== null && b.topic_ids.includes(ui.topicId);

  // Upcoming leave-by ticks get a label; hour labels they would cover are dropped.
  const leaves = $derived(data ? data.blocks.filter((b) => b.life && b.buffer_min && b.start_min > now).map((b) => b.start_min - (b.buffer_min ?? 0)) : []);
  const hours = $derived.by(() => {
    if (!data) return [];
    const out: number[] = [];
    for (let m = data.day_start_min; m < data.day_end_min; m += 60) {
      if (!leaves.some((l) => m >= l - 50 && m <= l + 25)) out.push(m);
    }
    return out;
  });

  const label = (b: Block) => b.end_min - b.start_min >= 30;

  const BH = 44;
  const MAX_LANES = 3;
  type Lane = { lane: number; lanes: number };
  const overlaps = $derived.by(() => {
    const lanes = new Map<number, Lane>();
    const hidden: { start: number; blocks: Block[]; lanes: number }[] = [];
    const bands: { start: number; end: number; depth: number }[] = [];
    if (!data) return { lanes, hidden, bands };
    const work = data.blocks.filter((b) => !b.life).sort((a, b) => a.start_min - b.start_min || b.end_min - a.end_min);
    let cluster: Block[] = [];
    let clusterEnd = 0;
    const flush = () => {
      const ends: number[] = [];
      const lane = new Map<number, number>();
      for (const b of cluster) {
        let i = ends.findIndex((e) => e <= b.start_min);
        if (i < 0) i = ends.push(0) - 1;
        ends[i] = b.end_min;
        lane.set(b.id, i);
      }
      const n = Math.min(ends.length, MAX_LANES);
      for (const b of cluster) lanes.set(b.id, { lane: lane.get(b.id)!, lanes: n });
      const over = cluster.filter((b) => lane.get(b.id)! >= MAX_LANES);
      if (over.length) hidden.push({ start: Math.min(...over.map((b) => b.start_min)), blocks: over, lanes: n });
      const cuts = [...new Set(cluster.flatMap((b) => [b.start_min, b.end_min]))].sort((a, b) => a - b);
      for (let i = 0; i + 1 < cuts.length; i++) {
        const depth = Math.min(cluster.filter((b) => b.start_min <= cuts[i] && b.end_min >= cuts[i + 1]).length, MAX_LANES);
        const last = bands.at(-1);
        if (depth < 2) continue;
        if (last && last.end === cuts[i] && last.depth === depth) last.end = cuts[i + 1];
        else bands.push({ start: cuts[i], end: cuts[i + 1], depth });
      }
    };
    for (const b of work) {
      if (cluster.length && b.start_min >= clusterEnd) {
        flush();
        cluster = [];
      }
      cluster.push(b);
      clusterEnd = cluster.length === 1 ? b.end_min : Math.max(clusterEnd, b.end_min);
    }
    if (cluster.length) flush();
    return { lanes, hidden, bands };
  });

  const laneH = (n: number) => Math.floor((BH - 2 * (n - 1)) / n);
  function laneStyle(b: Block): string {
    const l = overlaps.lanes.get(b.id);
    if (!l || l.lanes < 2) return '';
    const h = laneH(l.lanes);
    return `top:${l.lane * (h + 2)}px;height:${h}px;line-height:${h - (b.tentative ? 2 : 0)}px;padding-top:0;padding-bottom:0;font-size:${h >= 18 ? 10.5 / 13 : 10 / 13}rem;border-radius:3px;`;
  }
  const laned = (b: Block) => (overlaps.lanes.get(b.id)?.lanes ?? 1) > 1;
  const shown = (b: Block) => (overlaps.lanes.get(b.id)?.lane ?? 0) < MAX_LANES;
  const edge = (b: Block) => (laned(b) ? `inset 2px 0 0 ${catColor(b.category)}` : `inset 0 2px 0 ${catColor(b.category)}`);

  function tint(key: string, pctAlpha: number) {
    return `color-mix(in srgb, ${catColor(key)} ${pctAlpha}%, transparent)`;
  }

  const focus = (el: HTMLInputElement) => el.focus();

  function openLife(b: Block, trigger: Element) {
    popover = popover?.id === b.id ? null : b;
    popTrigger = trigger;
    editingBuffer = false;
  }

  async function saveBuffer() {
    const minutes = parseInt(bufferInput, 10);
    if (!popover || isNaN(minutes)) return;
    await call('daystrip.set_buffer', { event_id: popover.id, minutes });
    popover = { ...popover, buffer_min: minutes };
    editingBuffer = false;
  }

  useKeys((e) => {
    if (!popover) return;
    if (e.key === 'b' && !editingBuffer) {
      editingBuffer = true;
      bufferInput = String(popover.buffer_min ?? '');
      return true;
    }
  });
</script>

<div class="strip">
  <div class="countdown">
    <span class="next nowrap">Next · {next ? (next.life ? 'Life' : next.title) : 'nothing left today'}</span>
    <span class="left tnum" style:color>{left === null ? '—' : left <= 0 ? 'now' : `in ${left} min`}</span>
    <div class="bar"><div style:width="{left === null ? 0 : Math.round(Math.max(0, 1 - left / 30) * 100)}%" style:background={color}></div></div>
  </div>

  <div class="track">
    {#if data}
      <div class="layer">
        {#each overlaps.bands as o (o.start)}
          <div class="band" class:triple={o.depth >= 3} style:left="{pct(o.start)}%" style:width="{pct(o.end) - pct(o.start)}%"></div>
        {/each}
      </div>
      <div class="layer faded">
        {#each data.blocks.filter((b) => !b.life && !hl(b) && shown(b)) as b (b.id)}
          <div
            class="block"
            class:tentative={b.tentative}
            style={laneStyle(b)}
            style:left="{pct(b.start_min)}%"
            style:width="{pct(b.end_min) - pct(b.start_min)}%"
            style:background={b.tentative ? 'transparent' : tint(b.category, 17)}
            style:box-shadow={b.tentative ? 'none' : edge(b)}
            style:border-color={catColor(b.category)}
            title="{b.title} · {hm(b.start_min)}–{hm(b.end_min)}"
          >
            {#if label(b)}{b.title}{/if}
          </div>
        {/each}
      </div>
      <div class="layer">
        {#each overlaps.hidden as x (x.start)}
          {@const h = laneH(x.lanes)}
          <div
            class="more tnum"
            style:left="{pct(x.start)}%"
            style:top="{(MAX_LANES - 1) * (h + 2)}px"
            style:height="{h}px"
            style:line-height="{h}px"
            title={x.blocks.map((b) => `${b.title} · ${hm(b.start_min)}–${hm(b.end_min)}`).join('\n')}
          >
            +{x.blocks.length}
          </div>
        {/each}
        {#each data.blocks.filter((b) => !b.life && hl(b) && shown(b)) as b (b.id)}
          <div
            class="block lit"
            class:tentative={b.tentative}
            style={laneStyle(b)}
            style:left="{pct(b.start_min)}%"
            style:width="{pct(b.end_min) - pct(b.start_min)}%"
            style:background="color-mix(in srgb, {catColor(b.category)} 12%, var(--bg-panel))"
            style:box-shadow="{b.tentative ? '' : `${edge(b)},`} 0 0 0 1.5px var(--aqua)"
            style:border-color={catColor(b.category)}
            title="{b.title} · {hm(b.start_min)}–{hm(b.end_min)}"
          >
            {#if label(b)}{b.title}{/if}
          </div>
        {/each}
        {#each data.blocks.filter((b) => b.life) as b (b.id)}
          {@const leave = b.start_min - (b.buffer_min ?? 0)}
          {#if b.buffer_min}
            <div class="leave-tick" style:left="{pct(leave)}%"></div>
          {/if}
          <button
            class="block life"
            class:open={popover?.id === b.id}
            style:left="{pct(b.start_min)}%"
            style:width="{pct(b.end_min) - pct(b.start_min)}%"
            onclick={(e) => openLife(b, e.currentTarget)}
            aria-label="Life, busy {hm(b.start_min)}–{hm(b.end_min)}"
          >
            {#if label(b)}<i class="ph ph-house-simple"></i>Life{/if}
          </button>
        {/each}
      </div>
      {#if now >= data.day_start_min && now <= data.day_end_min}
        <div class="now" style:left="{pct(now)}%" style:background={color}></div>
      {/if}
      <div class="axis tnum">
        {#each hours as h}
          <span style:left="{pct(h)}%">{String(h / 60).padStart(2, '0')}</span>
        {/each}
        {#each leaves as l}
          <span class="leave" style:left="{pct(l) - 3.4}%">leave {hm(l)}</span>
        {/each}
      </div>
    {/if}

    {#if popover}
      <div class="popover day-pop" use:popoverBehaviour={{ onclose: () => (popover = null), trigger: popTrigger }} style:left="min({pct(popover.start_min)}%, calc(100% - 290px))">
        <div class="pop-head">
          <i class="ph ph-house-simple"></i>
          <span class="pop-title">Life · {hm(popover.start_min)}–{hm(popover.end_min)}</span>
          <span class="busy">busy</span>
        </div>
        <div class="pop-body">
          That's all this laptop knows. Travel buffer
          {#if editingBuffer}
            <input
              class="input sm buf tnum"
              bind:value={bufferInput}
              onkeydown={(e) => {
                if (e.key === 'Enter') saveBuffer();
                if (e.key === 'Escape') editingBuffer = false;
                e.stopPropagation();
              }}
              use:focus
            /> min.
          {:else}
            <span class="em">{popover.buffer_min} min</span>, so you'll get a nudge at {hm(popover.start_min - (popover.buffer_min ?? 0))}.
          {/if}
        </div>
        <div class="pop-foot">
          <span><kbd>b</kbd> buffer</span><span><kbd>Esc</kbd></span>
        </div>
      </div>
    {/if}
  </div>

  <div class="note" class:empty={!data?.note}>
    {#if data?.note}<i class="ph ph-calendar-x"></i><span>{data.note}</span>{/if}
  </div>
</div>

<style>
  .strip {
    --bh: 44px;
    position: relative;
    flex: none;
    display: flex;
    gap: 16px;
    align-items: center;
    padding: 8px 16px 6px;
    background: var(--bg-app);
    border-bottom: 1px solid var(--bg-head);
  }
  .countdown {
    width: 156px;
    flex: none;
    display: flex;
    flex-direction: column;
    gap: 3px;
    padding: 5px 10px 6px;
    border-radius: var(--radius-md);
    border: 1px solid var(--border);
    background: var(--bg-panel);
  }
  .next {
    font-size: 0.8077rem;
    color: var(--tx-muted-2);
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .left {
    font-size: 1.3077rem;
    font-weight: 500;
  }
  .bar {
    height: 3px;
    border-radius: 2px;
    background: var(--border);
    overflow: hidden;
  }
  .bar div {
    height: 100%;
  }
  .track {
    flex: 1;
    min-width: 0;
    position: relative;
    height: calc(var(--bh) + 14px);
  }
  .layer {
    position: absolute;
    inset: 0 0 auto 0;
    height: var(--bh);
    pointer-events: none;
  }
  .layer > * {
    pointer-events: auto;
  }
  .layer.faded {
    opacity: 0.5;
  }
  .block {
    position: absolute;
    top: 0;
    height: var(--bh);
    border-radius: var(--radius-sm);
    font-size: 0.8077rem;
    padding: calc(var(--bh) / 2 - 6px) 5px 0;
    overflow: hidden;
    white-space: nowrap;
    color: var(--tx-body);
    text-align: left;
    line-height: normal;
  }
  .block.tentative {
    border: 1px dashed;
    padding: calc(var(--bh) / 2 - 7px) 4px 0;
  }
  .band {
    position: absolute;
    top: 0;
    height: var(--bh);
    background: rgba(254, 128, 25, 0.12);
  }
  .band::before {
    content: '';
    position: absolute;
    inset: -6px 0 auto 0;
    height: 3px;
    border-radius: 1px;
    background: var(--orange);
  }
  .band.triple {
    background: rgba(251, 73, 52, 0.14);
  }
  .band.triple::before {
    background: var(--red);
  }
  .more {
    position: absolute;
    padding: 0 5px;
    border-radius: var(--radius-sm);
    font-size: 0.7692rem;
    color: var(--tx-body);
    background: var(--bg-head);
    border: 1px solid var(--border-strong);
  }
  .block.lit {
    font-weight: 500;
  }
  .block.life {
    display: flex;
    gap: 4px;
    background: repeating-linear-gradient(
      135deg,
      color-mix(in srgb, var(--cat-life) 26%, transparent) 0 3px,
      color-mix(in srgb, var(--cat-life) 8%, transparent) 3px 6px
    );
    box-shadow: inset 0 2px 0 var(--cat-life);
    cursor: pointer;
  }
  .block.life.open {
    box-shadow: inset 0 2px 0 var(--cat-life), 0 0 0 1.5px var(--aqua);
  }
  .block.life i {
    font-size: 0.8462rem;
  }
  .leave-tick {
    position: absolute;
    top: -4px;
    height: calc(var(--bh) + 8px);
    width: 0;
    border-left: 1.5px dashed var(--cat-life);
  }
  .now {
    position: absolute;
    top: -4px;
    height: calc(var(--bh) + 8px);
    width: 2px;
  }
  .axis {
    position: absolute;
    inset: auto 0 0 0;
    height: 11px;
    font-size: 0.7692rem;
    color: var(--tx-label);
  }
  .axis span {
    position: absolute;
    padding-left: 3px;
    border-left: 1px solid var(--border-strong);
  }
  .axis .leave {
    color: var(--cat-life);
    background: var(--bg-app);
    padding-right: 3px;
  }
  .note {
    width: 150px;
    flex: none;
    align-self: stretch;
    display: flex;
    gap: 6px;
    align-items: center;
    padding: 4px 10px;
    border-radius: var(--radius-md);
    border: 1px dashed var(--border-strong);
    font-size: 0.8462rem;
    line-height: 1.25;
    color: var(--tx-muted);
  }
  .note.empty {
    border-color: transparent;
  }
  .note i {
    font-size: 1.1538rem;
    color: var(--aqua);
  }
  .day-pop {
    position: absolute;
    z-index: 10;
    top: calc(var(--bh) + 12px);
    width: 290px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    border-radius: var(--radius-md);
    font-size: 0.9615rem;
  }
  .pop-head {
    display: flex;
    gap: 8px;
    align-items: center;
  }
  .pop-head i {
    font-size: 1.0769rem;
    color: var(--cat-life);
  }
  .pop-title {
    color: var(--tx-title);
    font-weight: 500;
  }
  .busy {
    margin-left: auto;
    font-size: 0.8462rem;
    color: var(--tx-label);
  }
  .pop-body {
    color: var(--tx-muted);
    line-height: 1.5;
  }
  .em {
    color: var(--tx-body);
  }
  /* In running text: it keeps the line's size and height. */
  .buf {
    display: inline-block;
    width: 44px;
    border-radius: var(--radius-md);
    padding: 0 4px;
    font: inherit;
  }
  .pop-foot {
    display: flex;
    gap: 14px;
    padding-top: 7px;
    border-top: 1px solid var(--border);
    font-size: 0.8846rem;
    color: var(--tx-muted-2);
  }
</style>

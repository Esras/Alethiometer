<script lang="ts">
  // The facet bar (hi-fi 1c/3b): one chip per route with the sources it
  // returned. Hovering a route or a source highlights its rows; clicking
  // filters; ⌥click excludes. Rows themselves show only their source.
  import { chord } from '../keys.svelte';
  import { ROUTE_COLOR, ROUTE_LABEL, routeNote, type Facet, type Lens } from '../hydrated';
  import AppIcon from './AppIcon.svelte';

  let {
    facets,
    total,
    hover = $bindable(null),
    filter = $bindable(null),
    exclude = $bindable(null),
  }: {
    facets: Facet[];
    total: number;
    hover?: Lens | null;
    filter?: Lens | null;
    exclude?: Lens | null;
  } = $props();

  const isMac = navigator.platform.toLowerCase().includes('mac');
  const same = (a: Lens | null, b: Lens) => !!a && a.route === b.route && a.icon === b.icon;

  function pick(e: MouseEvent, l: Lens) {
    e.stopPropagation();
    if (e.altKey) {
      exclude = same(exclude, l) ? null : l;
      filter = null;
    } else {
      filter = same(filter, l) ? null : l;
      exclude = null;
    }
  }
</script>

<div class="bar" onmouseleave={() => (hover = null)} role="toolbar" tabindex="-1" aria-label="Routes">
  {#each facets as f (f.route)}
    {@const l = { route: f.route }}
    <div
      class="chip"
      class:on={same(filter, l)}
      class:off={same(exclude, l)}
      class:hot={hover?.route === f.route}
      style:--rc={ROUTE_COLOR[f.route]}
      onmouseenter={() => (hover = l)}
      role="group">
      <button class="route" onclick={(e) => pick(e, l)} title="Click filters · {isMac ? '⌥' : 'Alt+'}click excludes">
        {#if f.route === 'glean'}<AppIcon source="glean" size={12} />{:else}<span class="sq"></span>{/if}
        {ROUTE_LABEL[f.route]}
        {#if f.status.state === 'ok'}<span class="n">{f.count}</span>{/if}
      </button>
      {#if f.status.state === 'ok' && f.sources.length}
        <span class="div"></span>
        {#each f.sources as s (s.icon)}
          {@const sl = { route: f.route, icon: s.icon }}
          <button
            class="src"
            class:on={same(filter, sl)}
            class:off={same(exclude, sl)}
            onmouseenter={(e) => {
              e.stopPropagation();
              hover = sl;
            }}
            onmouseleave={() => (hover = l)}
            onclick={(e) => pick(e, sl)}
            title="{s.system} via {ROUTE_LABEL[f.route]} · {s.count}"><AppIcon source={s.icon} size={14} /></button>
        {/each}
      {:else if f.status.state !== 'ok'}
        <span class="note" title={f.status.message ?? ''}>{#if f.status.state === 'not_asked'}<kbd>{chord('⌥↵', 'Alt+↵')}</kbd> {/if}{routeNote(f)}</span>
      {/if}
    </div>
  {/each}
  <span class="total">{total} {total === 1 ? 'result' : 'results'}</span>
</div>

<style>
  .bar {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 7px 14px;
  }
  .chip {
    transition: border-color 120ms;
  }
  .chip.hot,
  .chip.on {
    border-color: var(--rc);
    color: var(--tx-title);
  }
  .chip.on {
    background: color-mix(in srgb, var(--rc) 14%, transparent);
  }
  .chip.off {
    opacity: 0.45;
    text-decoration: line-through;
  }
  .route {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    color: inherit;
  }
  .sq {
    width: 8px;
    height: 8px;
    border-radius: 2px;
    background: var(--tx-label);
  }
  .n {
    color: var(--tx-label);
  }
  .div {
    width: 1px;
    height: 12px;
    background: var(--border-strong);
  }
  .src {
    display: inline-flex;
    padding: 1px;
    border-radius: 4px;
    opacity: 0.8;
  }
  .src:hover,
  .src.on {
    opacity: 1;
    box-shadow: 0 0 0 1px var(--rc);
  }
  .src.off {
    opacity: 0.35;
  }
  .note {
    color: var(--tx-label);
    font-size: 0.8846rem;
  }
  .total {
    margin-left: auto;
    color: var(--tx-label);
    font-size: 0.9231rem;
  }
</style>

<script lang="ts">
  // Carry forward: bullet-journal migration from the other end of the day.
  // > tomorrow, < schedule (a calendar hold via the outbox), × let go (nothing
  // is deleted; it just leaves the list). Decided rows stay, faded, with undo.
  import type { CarryAction, CarryRow } from './types';
  import Panel from '../../lib/ui/Panel.svelte';

  let {
    rows,
    label,
    selected = $bindable(0),
    textButtons = false,
    act,
  }: {
    rows: CarryRow[];
    label: string;
    selected?: number;
    textButtons?: boolean;
    act: (row: CarryRow, action: CarryAction) => void;
  } = $props();

  const GLYPH = {
    do: { icon: 'square', color: 'var(--blue)' },
    research: { icon: 'circle', color: 'var(--green)' },
    delegated: { icon: 'triangle', color: 'var(--red)' },
  } as const;

  const open = $derived(rows.filter((r) => r.action === null).length);

  function doneLabel(r: CarryRow): string {
    if (r.action === 'tomorrow') return 'tomorrow';
    if (r.action === 'let_go') return 'let go';
    if (r.outbox === 'confirmed') return 'scheduled';
    if (r.outbox === 'failed') return 'hold failed';
    return `scheduling · ${r.outbox ?? 'pending'}`;
  }
  const doneIcon = (r: CarryRow) => (r.action === 'tomorrow' ? 'caret-right' : r.action === 'schedule' ? 'caret-left' : 'x');
</script>

<Panel icon="arrow-bend-down-right" iconColor="var(--aqua)" {label} count={open} collapsible={false}>
  {#each rows as r, i (r.item_id + r.period)}
    {@const g = GLYPH[r.glyph] ?? GLYPH.do}
    <div class="row" class:sel={i === selected} class:done={r.action !== null}>
      <i class="ph-bold ph-{g.icon} glyph" style:color={g.color}></i>
      <span class="title">
        {#if r.owner && r.glyph === 'delegated'}<span class="who">@{r.owner}</span>{/if}
        {r.title}
      </span>
      <span class="acts">
        {#if r.action === null}
          <button class="b" title="Tomorrow (>)" aria-label="Tomorrow" onclick={() => act(r, 'tomorrow')}>
            <i class="ph ph-caret-right"></i>{#if textButtons}tomorrow{/if}
          </button>
          <button class="b" title="Schedule (<)" aria-label="Schedule" onclick={() => act(r, 'schedule')}>
            <i class="ph ph-caret-left"></i>{#if textButtons}schedule{/if}
          </button>
          <button class="b" title="Let go (x)" aria-label="Let go" onclick={() => act(r, 'let_go')}>
            <i class="ph ph-x"></i>{#if textButtons}let go{/if}
          </button>
        {:else}
          <span class="state nowrap" class:fail={r.outbox === 'failed'}><i class="ph ph-{doneIcon(r)}"></i>{doneLabel(r)}</span>
          <button class="b" title="Undo (u)" aria-label="Undo" onclick={() => act(r, 'undo')}><i class="ph ph-arrow-u-up-left"></i></button>
        {/if}
      </span>
    </div>
  {:else}
    <div class="empty">Nothing to carry.</div>
  {/each}
</Panel>

<style>
  .row {
    display: grid;
    grid-template-columns: 18px minmax(0, 1fr) auto;
    gap: 10px;
    align-items: center;
    padding: 8px 12px;
    border-top: 1px solid var(--bg-head);
  }
  .row:not(.sel):hover {
    background: var(--row-hover);
  }
  .glyph {
    font-size: 1.0769rem;
  }
  .title {
    min-width: 0;
    overflow-wrap: anywhere;
  }
  .who {
    color: var(--red);
  }
  .done .glyph,
  .done .title {
    opacity: 0.45;
  }
  .acts {
    display: flex;
    gap: 4px;
    align-items: center;
  }
  .b {
    display: flex;
    gap: 4px;
    align-items: center;
    font-size: 0.8846rem;
    padding: 2px 8px;
    border-radius: var(--r-input);
    border: 1px solid var(--border);
    color: var(--tx-muted);
    white-space: nowrap;
    transition: background 120ms;
  }
  .b:hover {
    background: var(--hover-tint);
    border-color: var(--border-strong);
  }
  .state {
    display: flex;
    gap: 4px;
    align-items: center;
    font-size: 0.8846rem;
    color: var(--tx-label);
    margin-right: 4px;
  }
  .state.fail {
    color: var(--orange);
  }
  .empty {
    padding: 10px 12px;
    color: var(--tx-label);
  }
</style>

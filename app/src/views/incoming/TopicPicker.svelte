<script lang="ts">
  // Inline "move to topic" picker (t). Type to filter, ↑/↓ to move,
  // ↵ to pick, Esc to close (lib/ui/picker.svelte.ts). Picking creates a
  // confirmed link. With `onunsorted` (the split review), a first "Unsorted"
  // row moves there. The create row isn't a cursor row: ↵ with no match creates.
  import { call } from '../../lib/api';
  import { catColor } from '../../lib/app.svelte';
  import FilterField from '../../lib/ui/FilterField.svelte';
  import { Picker } from '../../lib/ui/picker.svelte';
  import type { TopicRow } from '../../lib/types';

  let {
    exclude = null,
    excludeIds = [],
    placeholder = 'Move to topic…',
    onpick,
    onunsorted,
    onclose,
  }: {
    exclude?: number | null;
    /** More topics to leave out (e.g. ones staged to merge away). */
    excludeIds?: number[];
    placeholder?: string;
    /** The topic's name comes along for "moved to X" lines. */
    onpick: (topicId: number, name: string) => void;
    onunsorted?: () => void;
    onclose: () => void;
  } = $props();

  let all = $state<TopicRow[]>([]);

  call<TopicRow[]>('topics.list').then((t) => (all = t));

  const p = new Picker<undefined>({ rows: (): number => offset + matches.length, pick, onclose: () => onclose() });

  const matches = $derived.by(() => {
    const needle = p.q.trim().toLowerCase();
    return all.filter((t) => t.id !== exclude && !excludeIds.includes(t.id) && (!needle || t.name.toLowerCase().includes(needle))).slice(0, 8);
  });

  // The Unsorted row: offered while the query could still mean it.
  const showUnsorted = $derived(!!onunsorted && 'unsorted'.startsWith(p.q.trim().toLowerCase()));
  const offset = $derived(showUnsorted ? 1 : 0);

  const canCreate = $derived(p.q.trim().length > 0 && !all.some((t) => t.name.toLowerCase() === p.q.trim().toLowerCase()));
  let creating = $state(false);

  async function create() {
    const name = p.q.trim();
    if (!name || creating) return;
    creating = true;
    try {
      const r = await call<{ id: number }>('topics.create', { name });
      onpick(r.id, name);
    } finally {
      creating = false;
    }
  }

  function pick(i: number) {
    if (showUnsorted && i === 0) return onunsorted?.();
    const t = matches[i - offset];
    if (t) onpick(t.id, t.name);
    else if (canCreate) void create();
  }
</script>

<div class="picker popover flush" use:p.frame>
  <div class="field">
    <FilterField field={p.field} icon="arrow-bend-down-right" bind:value={p.q} {placeholder}>
      <span class="hint"><kbd>↵</kbd> pick · <kbd>Esc</kbd></span>
    </FilterField>
  </div>
  {#if showUnsorted}
    <button class="opt unsorted" class:on={p.sel === 0} {...p.row(0)}>
      <span class="bar dashed"></span>
      <span class="ellipsis"><i class="ph ph-shuffle"></i>Unsorted</span>
    </button>
  {/if}
  {#each matches as t, i}
    <button class="opt" class:on={i + offset === p.sel} {...p.row(i + offset)}>
      <span class="bar" style:background={catColor(t.category)}></span>
      <span class="ellipsis">{t.name}</span>
    </button>
  {:else}
    {#if canCreate}
      <button class="opt create" onmousedown={p.down(create)} disabled={creating}>
        <i class="ph ph-plus"></i><span class="ellipsis">Create “{p.q.trim()}”</span>
      </button>
    {:else}
      <div class="none">No topic matches.</div>
    {/if}
  {/each}
</div>

<style>
  .picker {
    width: 320px;
    border-radius: var(--r-input);
    overflow: hidden;
  }
  .field {
    padding: 6px;
    border-bottom: 1px solid var(--border);
  }
  .hint {
    font-size: 0.8462rem;
    white-space: nowrap;
  }
  .opt {
    width: 100%;
    display: grid;
    grid-template-columns: 4px minmax(0, 1fr);
    gap: 10px;
    align-items: center;
    padding: var(--row-pad-compact) 10px var(--row-pad-compact) 0;
    text-align: left;
    font-size: 0.9615rem;
    color: var(--tx-2);
  }
  .opt.on {
    background: var(--aqua-tint);
    color: var(--tx-title);
  }
  .bar {
    align-self: stretch;
    border-radius: 0 2px 2px 0;
  }
  .bar.dashed {
    background: repeating-linear-gradient(180deg, var(--tx-label) 0 3px, transparent 3px 6px);
  }
  .opt.unsorted {
    color: var(--tx-muted);
  }
  .opt.unsorted i {
    margin-right: 8px;
    color: var(--tx-label);
  }
  .opt.create {
    grid-template-columns: 16px minmax(0, 1fr);
    padding: var(--row-pad-compact) 10px;
    color: var(--tx-label);
  }
  .opt.create:hover {
    color: var(--tx-title);
  }
  .none {
    padding: 6px 10px;
    font-size: 0.9231rem;
    color: var(--tx-label);
  }
</style>

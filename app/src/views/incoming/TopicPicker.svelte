<script lang="ts">
  // Inline "move to topic" picker (t). Type to filter, ↑/↓ or ⌃j/⌃k to move,
  // ↵ to pick, Esc to close. Picking creates a confirmed link. With
  // `onunsorted` (the split review), a first "Unsorted" row moves there.
  import { call } from '../../lib/api';
  import { catColor } from '../../lib/app.svelte';
  import { pointerMoved, useKeys } from '../../lib/keys.svelte';
  import type { TopicRow } from '../../lib/types';

  let {
    exclude = null,
    placeholder = 'Move to topic…',
    onpick,
    onunsorted,
    onclose,
  }: {
    exclude?: number | null;
    placeholder?: string;
    onpick: (topicId: number) => void;
    onunsorted?: () => void;
    onclose: () => void;
  } = $props();

  let all = $state<TopicRow[]>([]);
  let q = $state('');
  let sel = $state(0);
  let input: HTMLInputElement;

  call<TopicRow[]>('topics.list').then((t) => (all = t));
  $effect(() => input?.focus());

  const matches = $derived.by(() => {
    const needle = q.trim().toLowerCase();
    return all.filter((t) => t.id !== exclude && (!needle || t.name.toLowerCase().includes(needle))).slice(0, 8);
  });

  // The Unsorted row: offered while the query could still mean it.
  const showUnsorted = $derived(!!onunsorted && 'unsorted'.startsWith(q.trim().toLowerCase()));
  const offset = $derived(showUnsorted ? 1 : 0);

  const canCreate = $derived(q.trim().length > 0 && !all.some((t) => t.name.toLowerCase() === q.trim().toLowerCase()));
  let creating = $state(false);

  async function create() {
    const name = q.trim();
    if (!name || creating) return;
    creating = true;
    try {
      const r = await call<{ id: number }>('topics.create', { name });
      onpick(r.id);
    } finally {
      creating = false;
    }
  }

  $effect(() => {
    void q;
    sel = 0;
  });

  useKeys((e) => {
    if (e.key === 'Escape') return onclose(), true;
    if (e.key === 'ArrowDown' || (e.ctrlKey && e.key === 'j')) {
      sel = Math.min(sel + 1, matches.length - 1 + offset);
      return true;
    }
    if (e.key === 'ArrowUp' || (e.ctrlKey && e.key === 'k')) {
      sel = Math.max(sel - 1, 0);
      return true;
    }
    if (e.key === 'Enter') {
      if (showUnsorted && sel === 0) return onunsorted?.(), true;
      const t = matches[sel - offset];
      if (t) onpick(t.id);
      else if (canCreate) void create();
      return true;
    }
  });
</script>

<div class="picker">
  <div class="field">
    <i class="ph ph-arrow-bend-down-right"></i>
    <input bind:this={input} bind:value={q} {placeholder} onblur={onclose} />
    <span class="hint">↵ pick · esc</span>
  </div>
  {#if showUnsorted}
    <button class="opt unsorted" class:on={sel === 0} onmousedown={(e) => (e.preventDefault(), onunsorted?.())} onmousemove={(e) => pointerMoved(e) && (sel = 0)}>
      <span class="bar dashed"></span>
      <span class="ellipsis"><i class="ph ph-shuffle"></i>Unsorted</span>
    </button>
  {/if}
  {#each matches as t, i}
    <button
      class="opt"
      class:on={i + offset === sel}
      onmousedown={(e) => (e.preventDefault(), onpick(t.id))}
      onmousemove={(e) => pointerMoved(e) && (sel = i + offset)}
    >
      <span class="bar" style:background={catColor(t.category)}></span>
      <span class="ellipsis">{t.name}</span>
    </button>
  {:else}
    {#if canCreate}
      <button class="opt create" onmousedown={(e) => (e.preventDefault(), create())} disabled={creating}>
        <i class="ph ph-plus"></i><span class="ellipsis">Create “{q.trim()}”</span>
      </button>
    {:else}
      <div class="none">No topic matches.</div>
    {/if}
  {/each}
</div>

<style>
  .picker {
    margin: 2px 0 4px;
    width: 320px;
    border-radius: var(--r-input);
    border: 1px solid var(--border-strong);
    background: var(--bg-app);
    overflow: hidden;
  }
  .field {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 10px;
    border-bottom: 1px solid var(--border);
    color: var(--tx-label);
  }
  .field input {
    flex: 1;
    min-width: 0;
    background: none;
    border: 0;
    outline: none;
    font-size: 12.5px;
    color: var(--tx-body);
  }
  .hint {
    font-size: 11px;
    white-space: nowrap;
  }
  .opt {
    width: 100%;
    display: grid;
    grid-template-columns: 4px minmax(0, 1fr);
    gap: 10px;
    align-items: center;
    padding: 6px 10px 6px 0;
    text-align: left;
    font-size: 12.5px;
    color: var(--tx-2);
  }
  .opt.on {
    background: var(--aqua-tint);
    box-shadow: inset -2px 0 0 var(--aqua);
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
    padding: 6px 10px;
    color: var(--tx-label);
  }
  .opt.create:hover {
    color: var(--tx-title);
  }
  .none {
    padding: 6px 10px;
    font-size: 12px;
    color: var(--tx-label);
  }
</style>

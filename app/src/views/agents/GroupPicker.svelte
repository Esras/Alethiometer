<script lang="ts">
  // Move a local session to another group of its project (m), like Incoming's TopicPicker (lib/ui/picker.svelte.ts).
  import FilterField from '../../lib/ui/FilterField.svelte';
  import { Picker } from '../../lib/ui/picker.svelte';
  import { act, ag, groupById, moveTo, projectKey, sessionById } from './state.svelte';

  let { session, onclose }: { session: string; onclose: () => void } = $props();

  const s = $derived(sessionById(session));
  const current = $derived(groupById(s?.group_id) ? s?.group_id : null);
  const all = $derived(
    (ag.tree?.groups ?? []).filter((g) => g.source === 'acp' && s && projectKey(g) === projectKey(s)),
  );

  let naming = $state<string | null>(null);
  let creating = $state(false);

  const p = new Picker<undefined>({
    rows: (): number => (naming === null ? offset + matches.length : 0),
    pick,
    onclose: () => onclose(),
    keys: (e) => {
      if (e.key !== 'Escape' || naming === null) return false;
      p.q = naming;
      naming = null;
      return true;
    },
  });

  const needle = $derived(p.q.trim().toLowerCase());
  const matches = $derived(all.filter((g) => g.id !== current && (!needle || g.name.toLowerCase().includes(needle))));
  const showUnsorted = $derived(!!current && 'unsorted'.startsWith(needle));
  const offset = $derived(showUnsorted ? 1 : 0);
  const canCreate = $derived(!!needle && !all.some((g) => g.name.toLowerCase() === needle));

  function done(group: string | null) {
    onclose();
    moveTo(session, group);
  }

  function pick(i: number) {
    if (naming !== null) return void create();
    if (showUnsorted && i === 0) return done(null);
    const g = matches[i - offset];
    if (g) done(g.id);
    else if (canCreate) {
      naming = p.q.trim();
      p.q = '';
    }
  }

  async function create() {
    if (naming === null || creating) return;
    creating = true;
    await act('group_create', { name: naming, goal: p.q.trim(), session }).catch(() => {});
    onclose();
  }
</script>

<div class="picker popover flush" use:p.frame>
  <div class="field">
    <FilterField
      field={p.field}
      icon={naming === null ? 'arrow-bend-down-right' : 'plus'}
      bind:value={p.q}
      placeholder={naming === null ? 'Move to group…' : `Goal for “${naming}” (optional)`}
    >
      <span class="hint"
        >{#if naming === null}<kbd>↵</kbd> pick · <kbd>Esc</kbd>{:else}<kbd>↵</kbd> create · <kbd>Esc</kbd> back{/if}</span
      >
    </FilterField>
  </div>
  {#if naming !== null}
    <div class="empty-row">New group “{naming}” in {s?.project || 'this project'}</div>
  {:else}
    {#if showUnsorted}
      <button class="opt unsorted" class:on={p.sel === 0} {...p.row(0)}>
        <span class="bar dashed"></span>
        <span class="ellipsis"><i class="ph ph-shuffle"></i>Unsorted</span>
      </button>
    {/if}
    {#each matches as g, i (g.id)}
      <button class="opt" class:on={i + offset === p.sel} {...p.row(i + offset)}>
        <span class="bar"></span>
        <span class="ellipsis"
          >{g.name}{#if g.goal}<span class="goal">{g.goal}</span>{/if}</span
        >
      </button>
    {:else}
      {#if canCreate}
        <button class="opt create" onmousedown={p.down(() => pick(offset))}>
          <i class="ph ph-plus"></i><span class="ellipsis">Create “{p.q.trim()}”</span>
        </button>
      {:else if !showUnsorted}
        <div class="empty-row">No group matches.</div>
      {/if}
    {/each}
  {/if}
</div>

<style>
  .picker {
    width: 320px;
    border-radius: var(--radius-md);
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
    min-height: var(--row-min-compact);
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
    border-radius: 0 var(--radius-sm) var(--radius-sm) 0;
    background: var(--border-strong);
  }
  .bar.dashed {
    background: repeating-linear-gradient(180deg, var(--tx-label) 0 3px, transparent 3px 6px);
  }
  .goal {
    margin-left: 8px;
    color: var(--tx-label);
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
    min-height: var(--row-min-compact);
    color: var(--tx-label);
  }
  .opt.create:hover {
    color: var(--tx-title);
  }
  .empty-row {
    padding: 6px 10px;
    line-height: inherit;
  }
</style>

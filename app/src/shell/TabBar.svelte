<script lang="ts">
  import { setTab, TABS, ui, type Tab } from '../lib/app.svelte';
  import CountdownPill from './CountdownPill.svelte';
  import SyncStatus from './SyncStatus.svelte';

  const isMac = navigator.platform.toLowerCase().includes('mac');
  // While the Search results view (18d) is open it has a tab, after Topics.
  const tabs = $derived<{ id: Tab; label: string }[]>(
    ui.tab === 'search' ? [...TABS.slice(0, 3), { id: 'search', label: 'Search' }, ...TABS.slice(3)] : TABS,
  );
</script>

<nav class="tabs">
  {#each tabs as t (t.id)}
    {@const n = TABS.findIndex((x) => x.id === t.id)}
    <button class="tab" class:on={ui.tab === t.id} onclick={() => setTab(t.id)} title={n < 0 ? '' : `${isMac ? '⌘' : 'Ctrl+'}${n + 1}`}>
      {#if t.id === 'search'}<i class="ph ph-magnifying-glass"></i>{/if}{t.label}
      {#if t.id === 'incoming' && ui.info}
        <span class="badge" class:on={ui.tab === t.id}>{ui.info.incoming_count}</span>
      {:else if t.id === 'todos' && ui.info?.todo_count}
        <span class="badge" class:on={ui.tab === t.id}>{ui.info.todo_count}</span>
      {/if}
    </button>
  {/each}
  <span class="right">
    {#if ui.tab !== 'reflect'}<SyncStatus />{/if}
    {#if ui.tab === 'reflect'}
      <CountdownPill />
    {:else}
      <button class="search" onclick={() => (ui.searchOpen = true)}>
        <i class="ph ph-magnifying-glass"></i><span class="q" class:typed={ui.tab === 'search' && !!ui.searchQuery}
          >{ui.tab === 'search' && ui.searchQuery ? ui.searchQuery : 'Search everything'}</span
        ><span class="hint">{isMac ? '⌘K' : '/'}</span>
      </button>
    {/if}
  </span>
</nav>

<style>
  .tabs {
    height: 40px;
    flex: none;
    display: flex;
    align-items: stretch;
    gap: 2px;
    padding: 0 14px;
    font-size: 12.5px;
    color: var(--tx-muted-2);
  }
  .tab {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 0 10px;
    transition: color 120ms;
  }
  .tab:hover {
    color: var(--tx-2);
  }
  .tab.on {
    color: var(--tx-body);
    box-shadow: inset 0 -2px 0 var(--aqua);
  }
  .badge {
    font-size: 10.5px;
    padding: 0 6px;
    border-radius: 8px;
    background: var(--border);
    color: var(--tx-body);
  }
  .badge.on {
    background: rgba(142, 192, 124, 0.2);
    color: var(--aqua);
  }
  .right {
    margin-left: auto;
    display: flex;
    align-items: center;
    gap: 12px;
  }
  .search {
    display: flex;
    gap: 8px;
    align-items: center;
    width: 220px;
    padding: 4px 10px;
    border-radius: var(--r-input);
    background: var(--bg-panel);
    border: 1px solid var(--border);
    color: var(--tx-label);
    font-size: 12px;
    white-space: nowrap;
  }
  .q {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .q.typed {
    color: var(--tx-body);
  }
  .hint {
    margin-left: auto;
    font-size: 11px;
  }
</style>

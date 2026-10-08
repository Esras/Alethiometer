<script lang="ts">
  import { setTab, TABS, ui, type Tab } from '../lib/app.svelte';
  import { chord } from '../lib/keys.svelte';
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
    <button
      class="tab"
      class:on={ui.tab === t.id}
      onclick={() => setTab(t.id)}
      title={n < 0 ? '' : `${isMac ? '⌘' : 'Ctrl+'}${n + 1}`}
    >
      {#if t.id === 'search'}<i class="ph ph-magnifying-glass"></i>{/if}{t.label}
      {#if t.id === 'incoming' && ui.info}
        <span class="count" class:on={ui.tab === t.id}>{ui.info.incoming_count}</span>
      {:else if t.id === 'todos' && ui.info?.todo_count}
        <span class="count" class:on={ui.tab === t.id}>{ui.info.todo_count}</span>
      {:else if t.id === 'agents' && ui.info?.agents_waiting}
        <span class="count" class:on={ui.tab === t.id}>{ui.info.agents_waiting}</span>
      {/if}
    </button>
  {/each}
  <span class="right">
    {#if ui.tab !== 'reflect'}<SyncStatus />{/if}
    <button
      class="cog"
      class:on={ui.settingsOpen}
      onclick={() => (ui.settingsOpen = !ui.settingsOpen)}
      title="Settings ({isMac ? '⌘,' : 'Ctrl+,'})"
      aria-label="Settings"><i class="ph ph-gear-six"></i></button
    >
    {#if ui.tab === 'reflect'}
      <CountdownPill />
    {:else}
      <button class="search" onclick={() => (ui.searchOpen = true)}>
        <i class="ph ph-magnifying-glass"></i><span class="q" class:typed={ui.tab === 'search' && !!ui.searchQuery}
          >{ui.tab === 'search' && ui.searchQuery ? ui.searchQuery : 'Search everything'}</span
        ><kbd>{chord('⌘K', 'Ctrl+K')}</kbd>
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
    font-size: 0.9615rem;
    color: var(--tx-muted-2);
  }
  .cog {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 26px;
    align-self: center;
    border-radius: var(--radius-md);
    color: var(--tx-muted-2);
    font-size: 1.1538rem;
  }
  .cog:hover {
    color: var(--tx-2);
    background: var(--hover-tint);
  }
  .cog.on {
    color: var(--aqua);
    background: var(--aqua-tint);
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
    /* 3px, not 4: the boxed key is 17px tall, so the button stays 25px. */
    padding: 3px 10px;
    border-radius: var(--radius-md);
    /* An .input's look (decision 7), without .input itself: this is a button that
       opens the palette, and it gets the focus back when the palette closes, so
       an aqua border on :focus would show at rest. */
    background: var(--bg-app);
    border: 1px solid var(--border-strong);
    color: var(--tx-disabled);
    font-size: 0.9231rem;
    white-space: nowrap;
  }
  .search i {
    color: var(--tx-label);
  }
  .q {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .q.typed {
    color: var(--tx-body);
  }
  .search kbd {
    margin-left: auto;
  }
</style>

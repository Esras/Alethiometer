<script lang="ts">
  import { loadInfo, setTab, TABS, ui } from './lib/app.svelte';
  import { dispatchKey, isTyping, mod } from './lib/keys.svelte';
  import AssistantPane from './shell/AssistantPane.svelte';
  import DayStrip from './shell/DayStrip.svelte';
  import SearchPalette from './shell/SearchPalette.svelte';
  import TabBar from './shell/TabBar.svelte';
  import TitleBar from './shell/TitleBar.svelte';
  import Incoming from './views/Incoming.svelte';
  import Notes from './views/Notes.svelte';
  import Priorities from './views/Priorities.svelte';
  import Reflect from './views/Reflect.svelte';
  import Topics from './views/Topics.svelte';
  import Nudge from './views/reflect/Nudge.svelte';

  const inTauri = '__TAURI_INTERNALS__' in window;
  let ready = $state(false);
  loadInfo()
    .then(() => (ready = true))
    .catch((e) => (ui.error = String(e.message ?? e)));

  function onKey(e: KeyboardEvent) {
    if (mod(e) && e.key === 'k' && ui.tab !== 'reflect') {
      ui.searchOpen = true;
      e.preventDefault();
      return;
    }
    if (e.key === '/' && !isTyping(e) && !ui.searchOpen && ui.tab !== 'reflect') {
      ui.searchOpen = true;
      e.preventDefault();
      return;
    }
    if (mod(e) && e.key === 'i') {
      ui.assistantOpen = !ui.assistantOpen;
      e.preventDefault();
      return;
    }
    if (mod(e) && /^[1-5]$/.test(e.key)) {
      setTab(TABS[Number(e.key) - 1].id);
      e.preventDefault();
      return;
    }
    dispatchKey(e);
  }
</script>

<svelte:window onkeydown={onKey} />

<div class="window">
  {#if !(inTauri && ui.info?.platform === 'linux')}<TitleBar />{/if}
  <TabBar />
  {#if ui.error}
    <div class="fatal">Couldn't reach the Alethiometer core: {ui.error}</div>
  {:else if ready}
    {#if ui.tab !== 'reflect'}<DayStrip />{/if}
    <main>
      {#if ui.tab === 'incoming'}<Incoming />
      {:else if ui.tab === 'priorities'}<Priorities />
      {:else if ui.tab === 'topics'}<Topics />
      {:else if ui.tab === 'notes'}<Notes />
      {:else}<Reflect />{/if}
    </main>
  {/if}
  {#if ui.searchOpen}<SearchPalette />{/if}
  {#if ready}<Nudge />{/if}
</div>

<style>
  .window {
    height: 100%;
    display: flex;
    flex-direction: column;
  }
  main {
    flex: 1;
    min-height: 0;
    display: flex;
  }
  .fatal {
    padding: 24px;
    color: var(--orange);
  }
</style>

<script lang="ts">
  import { call } from './lib/api';
  import { loadInfo, setTab, TABS, ui } from './lib/app.svelte';
  import { dispatchKey, isTyping, mod } from './lib/keys.svelte';
  import AssistantPane from './shell/AssistantPane.svelte';
  import DayStrip from './shell/DayStrip.svelte';
  import SearchPalette from './shell/SearchPalette.svelte';
  import Settings from './shell/settings/Settings.svelte';
  import TabBar from './shell/TabBar.svelte';
  import TitleBar from './shell/TitleBar.svelte';
  import Incoming from './views/Incoming.svelte';
  import Notes from './views/Notes.svelte';
  import Priorities from './views/Priorities.svelte';
  import Search from './views/Search.svelte';
  import Reflect from './views/Reflect.svelte';
  import Todos from './views/Todos.svelte';
  import Agents from './views/Agents.svelte';
  import Topics from './views/Topics.svelte';
  import Nudge from './views/reflect/Nudge.svelte';
  import StandinNotes from './shell/StandinNotes.svelte';

  const inTauri = '__TAURI_INTERNALS__' in window;
  let ready = $state(false);
  // Notes left in the stand-in vault after a real one was set (asked once per start).
  let standin = $state<{ standin: string; vault: string; files: { path: string; state: 'missing' | 'differs'; modified: string | null; preview: string }[] } | null>(null);
  loadInfo()
    .then(() => {
      ready = true;
      call<NonNullable<typeof standin>>('standin.list')
        .then((l) => (standin = l.files.length ? l : null))
        .catch((e) => console.warn('standin.list', e));
    })
    .catch((e) => (ui.error = String(e.message ?? e)));

  function onKey(e: KeyboardEvent) {
    // ⌘, opens Settings (a modal, not a tab); ⌘⇧, opens settings.json itself.
    if (mod(e) && e.code === 'Comma' && !ui.settingsOpen) {
      if (e.shiftKey) call('settings.open_file').catch((x) => (ui.error = String(x.message ?? x)));
      else ui.settingsOpen = true;
      e.preventDefault();
      return;
    }
    // While Settings is open it owns the keyboard.
    if (ui.settingsOpen) {
      dispatchKey(e);
      return;
    }
    if (mod(e) && e.key === 'k' && ui.tab !== 'reflect') {
      ui.searchOpen = true;
      e.preventDefault();
      return;
    }
    if (
      e.key === '/' &&
      !isTyping(e) &&
      !ui.searchOpen &&
      ui.tab !== 'reflect' &&
      ui.tab !== 'todos' &&
      ui.tab !== 'priorities' &&
      ui.tab !== 'agents' &&
      !(ui.tab === 'topics' && ui.openTopicId === null)
    ) {
      ui.searchOpen = true;
      e.preventDefault();
      return;
    }
    if (mod(e) && e.key === 'i') {
      ui.assistantOpen = !ui.assistantOpen;
      e.preventDefault();
      return;
    }
    if (mod(e) && /^[1-7]$/.test(e.key)) {
      setTab(TABS[Number(e.key) - 1].id);
      e.preventDefault();
      return;
    }
    // g i / g a: go to Incoming / Agents (the g itself still reaches the view).
    if (!isTyping(e) && !mod(e) && !e.altKey && !ui.searchOpen) {
      if (Date.now() - gAt < 800 && (e.key === 'i' || e.key === 'a')) {
        gAt = 0;
        setTab(e.key === 'i' ? 'incoming' : 'agents');
        e.preventDefault();
        return;
      }
      gAt = e.key === 'g' ? Date.now() : 0;
    }
    dispatchKey(e);
  }
  let gAt = 0;
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
      {:else if ui.tab === 'todos'}<Todos />
      {:else if ui.tab === 'topics'}<Topics />
      {:else if ui.tab === 'notes'}<Notes />
      {:else if ui.tab === 'search'}<Search />
      {:else if ui.tab === 'agents'}<Agents />
      {:else}<Reflect />{/if}
    </main>
  {/if}
  {#if ui.searchOpen}<SearchPalette />{/if}
  {#if ui.settingsOpen && ready}<Settings />{/if}
  {#if ready}<Nudge />{/if}
  {#if ready && standin && !ui.settingsOpen}<StandinNotes listing={standin} onclose={() => (standin = null)} />{/if}
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

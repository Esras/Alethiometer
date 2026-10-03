<script lang="ts">
  import './form.css';
  // New local session: the same form language as Scion's New agent (05), for
  // an ACP harness Alethiometer launches itself. The harness signs in its own way.
  import { untrack } from 'svelte';
  import { call } from '../../lib/api';
  import { mod } from '../../lib/keys.svelte';
  import { act, ag, openThread } from './state.svelte';

  let { group = null, onClose }: { group?: string | null; onClose: () => void } = $props();

  const harnesses = $derived(ag.tree?.harnesses ?? []);
  const groups = $derived((ag.tree?.groups ?? []).filter((g) => g.source === 'acp'));
  let harness = $state('');
  let name = $state('');
  let cwd = $state('');
  let task = $state('');
  let groupId = $state<string>('');
  let newName = $state('');
  let newGoal = $state('');
  let dirs = $state<string[]>([]);
  let model = $state('');
  let models = $state<{ id: string; name: string }[]>([]);
  let modelsNote = $state('');
  let busy = $state(false);
  let err = $state<string | null>(null);
  let taskEl = $state<HTMLTextAreaElement>();

  $effect.pre(() => {
    if (!harness && harnesses.length) harness = harnesses[0].key;
  });
  groupId = untrack(() => (group && group.startsWith('local:') ? group : ''));
  call<{ dirs: string[] }>('agents.dirs').then((d) => {
    dirs = d.dirs;
    if (!cwd && d.dirs.length) cwd = d.dirs[0];
  });
  $effect(() => {
    taskEl?.focus();
  });

  $effect(() => {
    const k = harness;
    if (!k) return;
    model = '';
    models = [];
    modelsNote = 'loading…';
    call<{ models: { id: string; name: string }[]; current: string }>('agents.models', { harness: k, cwd: untrack(() => cwd) })
      .then((r) => {
        if (k !== harness) return;
        models = r.models;
        modelsNote = r.current ? `default ${r.current}` : 'optional';
      })
      .catch(() => {
        if (k === harness) modelsNote = 'optional';
      });
  });

  const h = $derived(harnesses.find((x) => x.key === harness));

  async function start() {
    if (busy) return;
    if (!cwd.trim()) {
      err = 'Pick a directory';
      return;
    }
    busy = true;
    try {
      let g: string | null = groupId || null;
      if (groupId === 'new') {
        if (!newName.trim()) throw new Error('Name the new group');
        const r = await call<{ id: string }>('agents.group_create', { name: newName, goal: newGoal, cwd });
        g = r.id;
      }
      const r = (await act('start_local', { harness, cwd, name, group: g ?? '', task, model })) as { id: string };
      ag.sel = `s:${r.id}`;
      openThread(`s:${r.id}`);
      onClose();
    } catch (e) {
      err = String((e as Error).message ?? e);
    } finally {
      busy = false;
    }
  }

  function onKey(e: KeyboardEvent) {
    if (mod(e) && e.key === 'Enter') {
      start();
      e.preventDefault();
    } else if (e.key === 'Escape') {
      onClose();
      e.preventDefault();
    }
    e.stopPropagation();
  }

  function chipKey(e: KeyboardEvent) {
    const i = harnesses.findIndex((x) => x.key === harness);
    if (e.key === 'ArrowRight' && i < harnesses.length - 1) harness = harnesses[i + 1].key;
    else if (e.key === 'ArrowLeft' && i > 0) harness = harnesses[i - 1].key;
    else return;
    e.preventDefault();
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="form" onkeydown={onKey}>
  <div class="top">
    <i class="ph ph-user-plus"></i><span class="t">New session</span><span class="sub">local · ACP</span><span class="esc">Esc</span>
  </div>
  <div class="cols">
    <div class="fields">
      <span class="lab">Harness</span>
      <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
      <span class="chips" tabindex="0" onkeydown={chipKey}>
        {#each harnesses as x (x.key)}
          <button class="chip" class:on={x.key === harness} tabindex="-1" onclick={() => (harness = x.key)}>{x.label}</button>
        {/each}
      </span>
      <span class="lab">Model</span>
      <span class="input"><input bind:value={model} list="agent-models" placeholder="default" /><span class="note">{modelsNote}</span></span>
      <datalist id="agent-models">{#each models as m (m.id)}<option value={m.id}>{m.name}</option>{/each}</datalist>
      <span class="lab">Name</span>
      <span class="input"><input bind:value={name} placeholder="{harness || 'agent'}-…" /><span class="note">optional</span></span>
      <span class="lab">Directory</span>
      <span class="input"><input bind:value={cwd} list="agent-dirs" placeholder="~/src/project" /><span class="note">its working directory</span></span>
      <datalist id="agent-dirs">{#each dirs as d (d)}<option value={d}></option>{/each}</datalist>
      <span class="lab">Group</span>
      <span class="input">
        <select bind:value={groupId}>
          <option value="">Unsorted</option>
          {#each groups as g (g.id)}<option value={g.id}>{g.project ? `${g.project} › ` : ''}{g.name}</option>{/each}
          <option value="new">New group…</option>
        </select>
      </span>
      {#if groupId === 'new'}
        <span class="lab">Group name</span>
        <span class="input"><input bind:value={newName} placeholder="Settings polish" /></span>
        <span class="lab">Goal</span>
        <span class="input"><input bind:value={newGoal} placeholder="Ship Settings; no overlaps" /></span>
      {/if}
    </div>
    <div class="right">
      <span class="row-lab"><span class="lab">Task</span><span class="note">first prompt, sent on start</span></span>
      <textarea bind:this={taskEl} bind:value={task} rows="5" placeholder="What should it do?"></textarea>
      {#if err}<div class="err">{err}</div>{/if}
    </div>
  </div>
  <div class="foot">
    <button class="b primary" disabled={busy} onclick={start}>Start session<span class="k">⌘⏎</span></button>
    <button class="b" onclick={onClose}>Cancel<span class="k">Esc</span></button>
    <span class="cmd"><i class="ph ph-terminal"></i><span>cd {cwd || '…'} && {h?.command ?? ''}</span></span>
  </div>
</div>


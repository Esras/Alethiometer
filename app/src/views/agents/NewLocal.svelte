<script lang="ts">
  import './form.css';
  // New local agent in the new-agent modal (05), for an ACP harness
  // Alethiometer launches itself; the harness signs in its own way. Joins shows
  // the group it lands in. The model list comes from the harness itself.
  import { untrack } from 'svelte';
  import { call } from '../../lib/api';
  import { chord } from '../../lib/keys.svelte';
  import Joins from './Joins.svelte';
  import type { ConfigChoice } from './types';
  import NewModal from './NewModal.svelte';
  import Where from './Where.svelte';
  import { act, ag, groupById, openThread } from './state.svelte';

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
  let current = $state('');
  let thinking = $state('');
  let levels = $state<ConfigChoice | null>(null);
  let loading = $state(false);
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
    current = '';
    thinking = '';
    levels = null;
    loading = true;
    call<{ models: { id: string; name: string }[]; current: string; thinking: ConfigChoice | null }>('agents.models', { harness: k, cwd: untrack(() => cwd) })
      .then((r) => {
        if (k !== harness) return;
        models = r.models;
        current = r.current;
        levels = r.thinking;
      })
      .catch(() => {})
      .finally(() => {
        if (k === harness) loading = false;
      });
  });

  const h = $derived(harnesses.find((x) => x.key === harness));
  const g = $derived(groupId && groupId !== 'new' ? groupById(groupId) : undefined);
  const members = $derived(g ? (ag.tree?.sessions ?? []).filter((s) => s.group_id === g.id && !s.parent_id) : []);
  const modelName = (id: string) => models.find((m) => m.id === id)?.name || id;

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
      const r = (await act('start_local', { harness, cwd, name, group: g ?? '', task, model, thinking })) as { id: string };
      ag.sel = `s:${r.id}`;
      openThread(`s:${r.id}`);
      onClose();
    } catch (e) {
      err = String((e as Error).message ?? e);
    } finally {
      busy = false;
    }
  }

  function chipKey(e: KeyboardEvent) {
    const i = harnesses.findIndex((x) => x.key === harness);
    if (e.key === 'ArrowRight' && i < harnesses.length - 1) harness = harnesses[i + 1].key;
    else if (e.key === 'ArrowLeft' && i > 0) harness = harnesses[i - 1].key;
    else return;
    e.preventDefault();
  }
</script>

<NewModal sub="local · ACP" onStart={start} {onClose}>
  <Joins
    project={g?.project ?? ''}
    name={groupId === 'new' ? newName || 'New group' : (g?.name ?? 'Unsorted')}
    goal={groupId === 'new' ? newGoal : (g?.goal ?? '')}
    host={g?.host ?? ''}
    {members}
    newName={name.trim() || `${harness || 'agent'}-…`}
  />
  <Where />
  <div class="fields">
    <span class="lab label">Harness</span>
    <span class="chips" role="radiogroup" aria-label="Choose with ← →" tabindex="0" onkeydown={chipKey}>
      {#each harnesses as x (x.key)}
        <button class="chip" class:on={x.key === harness} tabindex="-1" onclick={() => (harness = x.key)}>{x.label}</button>
      {/each}
    </span>
    <span class="lab label">Model</span>
    <span class="input">
      {#if models.length}
        <select bind:value={model}>
          <option value="">default{current ? ` · ${modelName(current)}` : ''}</option>
          {#each models.filter((m) => m.id !== current) as m (m.id)}<option value={m.id}>{m.name || m.id}{m.name && m.name !== m.id ? ` · ${m.id}` : ''}</option>{/each}
        </select>
      {:else}
        <input bind:value={model} placeholder={loading ? 'asking the harness…' : 'default'} />
      {/if}
      <span class="note">{loading ? 'from the harness' : models.length ? `${models.length} from ${h?.label ?? 'the harness'}` : 'none listed; type one or leave the default'}</span>
    </span>
    {#if levels}
      <span class="lab label">Thinking</span>
      <span class="input">
        <select bind:value={thinking}>
          <option value="">default · {(levels.options.find((o) => o.value === levels?.current)?.name ?? levels.current).replace(/^thinking:?\s*/i, '')}</option>
          {#each levels.options.filter((o) => o.value !== levels?.current) as o (o.value)}<option value={o.value}>{o.name.replace(/^thinking:?\s*/i, '')}</option>{/each}
        </select>
        <span class="note">{model ? 'levels of the default model' : 'from the harness'}</span>
      </span>
    {/if}
    <span class="lab label">Name</span>
    <span class="input"><input bind:value={name} placeholder="{harness || 'agent'}-…" /><span class="note">optional</span></span>
    <span class="lab label">Directory</span>
    <span class="input"><input bind:value={cwd} list="agent-dirs" placeholder="~/src/project" /><span class="note">its working directory</span></span>
    <datalist id="agent-dirs">{#each dirs as d (d)}<option value={d}></option>{/each}</datalist>
    <span class="lab label">Group</span>
    <span class="input">
      <select bind:value={groupId}>
        <option value="">Unsorted</option>
        {#each groups as x (x.id)}<option value={x.id}>{x.project ? `${x.project} › ` : ''}{x.name}</option>{/each}
        <option value="new">New group…</option>
      </select>
    </span>
    {#if groupId === 'new'}
      <span class="lab label">Group name</span>
      <span class="input"><input bind:value={newName} placeholder="Settings polish" /></span>
      <span class="lab label">Goal</span>
      <span class="input"><input bind:value={newGoal} placeholder="Ship Settings; no overlaps" /></span>
    {/if}
  </div>
  <div class="right">
    <span class="row-lab"><span class="lab label">Task</span><span class="note">first prompt, sent on start</span></span>
    <textarea bind:this={taskEl} bind:value={task} rows="4" placeholder="What should it do?"></textarea>
    {#if err}<div class="err">{err}</div>{/if}
  </div>
  {#snippet foot()}
    <button class="btn md primary" disabled={busy} onclick={start}>Start agent<kbd>{chord('⌘⏎', 'Ctrl+⏎')}</kbd></button>
    <button class="btn md" onclick={onClose}>Cancel<kbd>Esc</kbd></button>
    <span class="cmd"><i class="ph ph-terminal"></i><span>cd {cwd || '…'} && {h?.command ?? ''}</span></span>
  {/snippet}
</NewModal>

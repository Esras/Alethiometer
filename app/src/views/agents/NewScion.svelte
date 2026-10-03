<script lang="ts">
  import './form.css';
  // New agent on a Scion Hub (05). Fields follow Scion: name (unique in the
  // project), template (a role: instructions + skills), harness config, broker,
  // branch, then "add to #room" as a step Alethiometer takes after start, and
  // the first task. The footer shows the matching `scion start` command.
  import { untrack } from 'svelte';
  import { call } from '../../lib/api';
  import { mod } from '../../lib/keys.svelte';
  import { act, ag, groupById, openThread } from './state.svelte';

  type Catalog = {
    templates: { name: string; versions: string[]; instructions: string; skills: string[] }[];
    harness_configs: { name: string; harness: string; model: string; auth: string }[];
    brokers: { name: string; kind: string; note: string }[];
    branch: string;
    project: string;
    command_template?: string;
  };

  let { group, room = null, onClose }: { group: string; room?: string | null; onClose: () => void } = $props();

  const g = $derived(groupById(group));
  let cat = $state<Catalog | null>(null);
  let err = $state<string | null>(null);
  let busy = $state(false);
  let name = $state('');
  let template = $state('');
  let version = $state('');
  let harness = $state('');
  let broker = $state('');
  let branch = $state('');
  let addToRoom = $state(true);
  let task = $state('');
  let nameEl = $state<HTMLInputElement>();

  call<Catalog>('agents.catalog', { group: untrack(() => group) })
    .then((c) => {
      cat = c;
      template = c.templates[0]?.name ?? '';
      version = c.templates[0]?.versions[0] ?? '';
      harness = c.harness_configs[0]?.name ?? '';
      broker = c.brokers[0]?.name ?? '';
      branch = c.branch ?? '';
    })
    .catch((e) => (err = String(e.message ?? e)));
  $effect(() => {
    nameEl?.focus();
  });

  const t = $derived(cat?.templates.find((x) => x.name === template));
  const taken = $derived(!!name && (ag.tree?.sessions ?? []).some((s) => s.group_id && groupById(s.group_id)?.project === g?.project && s.name === name));
  const short = (s: string) => (s.length > 28 ? `${s.slice(0, 26)}…` : s);
  const command = $derived(`scion start ${name || '<name>'} -t ${template || '<template>'}${branch ? ` -b ${branch}` : ''} "${short(task || '<task>')}"`);

  async function start() {
    if (busy || !name.trim() || taken) {
      if (!name.trim()) err = 'Name it first';
      return;
    }
    busy = true;
    try {
      const r = (await act('start_scion', {
        group,
        name: name.trim(),
        template,
        template_version: version,
        harness_config: harness,
        broker,
        branch,
        task,
        room: room && addToRoom ? room : '',
      })) as { id: string };
      if (!room) {
        ag.sel = `s:${r.id}`;
        openThread(`s:${r.id}`);
      }
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
    const list = cat?.templates ?? [];
    const i = list.findIndex((x) => x.name === template);
    const next = e.key === 'ArrowRight' ? list[i + 1] : e.key === 'ArrowLeft' ? list[i - 1] : undefined;
    if (!next) return;
    template = next.name;
    version = next.versions[0] ?? '';
    e.preventDefault();
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="form" onkeydown={onKey}>
  <div class="top">
    <i class="ph ph-user-plus"></i><span class="t">New agent</span><span class="sub">project {g?.project} · Scion Hub</span><span class="esc">Esc</span>
  </div>
  {#if !cat && !err}
    <div class="cols"><span class="sub">Reading templates from the Hub…</span></div>
  {/if}
  <div class="cols">
    <div class="fields">
      <span class="lab">Name</span>
      <span class="input"><input bind:this={nameEl} bind:value={name} placeholder="reviewer" /><span class="note" class:taken>{taken ? 'taken' : 'unique in project'}</span></span>
      <span class="lab">Template</span>
      <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
      <span class="chips" tabindex="0" onkeydown={chipKey}>
        {#each cat?.templates ?? [] as x (x.name)}
          <button
            class="chip"
            class:on={x.name === template}
            tabindex="-1"
            onclick={() => {
              template = x.name;
              version = x.versions[0] ?? '';
            }}>{x.name}<span class="v">{x.name === template ? version || 'default' : (x.versions[0] ?? 'default')}</span></button
          >
        {/each}
      </span>
      <span class="lab">Harness</span>
      <span class="input">
        <select bind:value={harness}>
          {#each cat?.harness_configs ?? [] as h (h.name)}<option value={h.name}>{h.name} · {h.harness} · {h.model} · {h.auth}</option>{/each}
        </select>
      </span>
      <span class="lab">Broker</span>
      <span class="input">
        <select bind:value={broker}>
          {#each cat?.brokers ?? [] as b (b.name)}<option value={b.name}>{b.name}{b.kind ? ` · ${b.kind}` : ''}{b.note ? ` · ${b.note}` : ''}</option>{/each}
        </select>
      </span>
      <span class="lab">Branch</span>
      <span class="input"><input bind:value={branch} placeholder="feat/…" /><span class="note">own clone</span></span>
      {#if room}
        <span class="lab">Then</span>
        <button class="check" onclick={() => (addToRoom = !addToRoom)}>
          <span class="box" class:on={addToRoom}>{#if addToRoom}<i class="ph ph-check"></i>{/if}</span>Add to #{g?.room?.name}<span class="note">Alethiometer does this after start</span>
        </button>
      {/if}
    </div>
    <div class="right">
      <span class="row-lab"><span class="lab">Instructions</span><span class="note">agent_instructions from {template} {version} · edit the template on Hub</span></span>
      <div class="ro">{t?.instructions || '—'}</div>
      {#if t?.skills.length}
        <span class="skills"><span class="lab">Skills</span>{#each t.skills as s (s)}<span class="skill">{s}</span>{/each}</span>
      {/if}
      <span class="row-lab"><span class="lab">Task</span><span class="note">first instruction, sent on start</span></span>
      <textarea bind:value={task} rows="2" placeholder="What should it do first?"></textarea>
      {#if err}<div class="err">{err}</div>{/if}
    </div>
  </div>
  <div class="foot">
    <button class="b primary" disabled={busy || !cat} onclick={start}>Start agent<span class="k">⌘⏎</span></button>
    <button class="b" onclick={onClose}>Cancel<span class="k">Esc</span></button>
    <span class="cmd"><i class="ph ph-terminal"></i><span>{command}</span><button title="Copy" onclick={() => navigator.clipboard?.writeText(command)}><i class="ph ph-copy"></i></button></span>
  </div>
</div>

<style>
  :global(.form .note.taken) {
    color: var(--orange);
  }
</style>

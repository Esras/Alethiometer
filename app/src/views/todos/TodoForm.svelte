<script lang="ts">
  import { untrack } from 'svelte';
  import { call } from '../../lib/api';
  import { catColor } from '../../lib/app.svelte';
  import { mod } from '../../lib/keys.svelte';
  import TopicPicker from '../incoming/TopicPicker.svelte';
  import type { TopicRef, TopicRow } from '../../lib/types';
  import type { Draft, JiraTarget } from './types';

  let {
    jira,
    topic = null,
    choose = false,
    held = false,
    gmail = false,
    initial = null,
    onsave,
    oncancel,
  }: {
    jira: JiraTarget | null;
    topic?: TopicRef | null;
    choose?: boolean;
    held?: boolean;
    gmail?: boolean;
    initial?: { target: 'local' | 'jira'; text: string; due: string | null } | null;
    onsave: (d: Draft) => void;
    oncancel: () => void;
  } = $props();

  const init = untrack(() => ({ ...initial, topic }));
  let target = $state<'local' | 'jira'>(init.target ?? 'local');
  let text = $state(init.text ?? '');
  let due = $state(init.due ?? '');
  let picked = $state<TopicRef | null>(init.topic);
  let picking = $state(false);
  let input: HTMLInputElement;
  $effect(() => input?.focus());

  async function pickTopic(id: number) {
    picking = false;
    const all = await call<TopicRow[]>('topics.list');
    picked = all.find((t) => t.id === id) ?? null;
  }

  const effects = $derived.by(() => {
    const out = [target === 'jira' && jira ? `create the issue in ${jira.project} (${jira.issue_type}, assigned to you)` : 'add a todo in Alethiometer'];
    if (gmail) out.push('archive the thread');
    return out;
  });

  function save() {
    if (!text.trim()) return;
    onsave({ target, text: text.trim(), due: due || null, topic_id: choose ? (picked?.id ?? null) : undefined });
  }

  function onkeydown(e: KeyboardEvent) {
    if (picking) return;
    if (mod(e) && e.key === '1') target = 'local';
    else if (mod(e) && e.key === '2' && jira) target = 'jira';
    else if (mod(e) && e.key === 'Enter') save();
    else if (e.key === 'Escape') oncancel();
    else return;
    e.preventDefault();
    e.stopPropagation();
  }
</script>

<div class="form" {onkeydown} onclick={(e) => e.stopPropagation()} role="presentation">
  <div class="switch">
    <button class:on={target === 'local'} onclick={() => (target = 'local')}>Todo <span class="k">⌘1</span></button>
    <button class:on={target === 'jira'} disabled={!jira} title={jira ? '' : 'Set create_project on the Jira source to create issues'} onclick={() => (target = 'jira')}>
      Jira issue <span class="k">⌘2</span>
    </button>
  </div>
  <div class="grid">
    <span class="lbl">{target === 'jira' ? 'Summary' : 'Todo'}</span>
    <input bind:this={input} bind:value={text} placeholder={target === 'jira' ? 'Issue summary' : 'What to do'} />
    {#if target === 'jira' && jira}
      <span class="lbl">Issue</span>
      <span class="val">{jira.project} · {jira.issue_type} · you</span>
    {/if}
    <span class="lbl">Due</span>
    <span class="due">
      <input class="date" class:empty={!due} type="date" bind:value={due} />
      {#if due}<button class="ghost" onclick={() => (due = '')} title="No due date">Clear</button>{:else}<span class="muted">none</span>{/if}
    </span>
    <span class="lbl">Topic</span>
    {#if choose}
      {#if picking}
        <TopicPicker placeholder="Topic…" onpick={pickTopic} onclose={() => (picking = false)} />
      {:else}
        <button class="val topic" onclick={() => (picking = true)}>
          {#if picked}<span class="bar" style:background={catColor(picked.category)}></span>{picked.name}{:else}No topic{/if}
        </button>
      {/if}
    {:else}
      <span class="val muted">
        {#if topic}<span class="bar" style:background={catColor(topic.category)}></span>{topic.name} · from the signal{:else}the one you file the signal under{/if}
      </span>
    {/if}
  </div>
  <div class="foot">
    <span class="fx">{held ? 'Once you confirm the signal, this will ' : 'This will '}{effects.join(' · ')}</span>
    <button class="ghost" onclick={oncancel}>Cancel <span class="k">esc</span></button>
    <button class="btn primary" disabled={!text.trim()} onclick={save}>Save <span class="k">⌘↵</span></button>
  </div>
</div>

<style>
  .form {
    display: flex;
    flex-direction: column;
    margin: 6px 0 8px;
    padding: 10px 12px;
    border: 1px solid var(--border-strong);
    border-radius: var(--r-input);
    background: var(--bg-app);
    font-size: 0.9615rem;
  }
  .switch {
    display: flex;
    gap: 4px;
    margin-bottom: 10px;
  }
  .switch button {
    padding: 3px 10px;
    border-radius: var(--r-input);
    color: var(--tx-muted-2);
  }
  .switch button.on {
    background: var(--aqua-tint);
    color: var(--aqua);
  }
  .switch button:disabled {
    color: var(--tx-disabled);
  }
  .grid {
    display: grid;
    grid-template-columns: 64px minmax(0, 1fr);
    gap: 6px 10px;
    align-items: center;
  }
  .lbl {
    color: var(--tx-label);
  }
  input {
    background: var(--bg-panel);
    border: 1px solid var(--border);
    border-radius: var(--r-input);
    padding: 4px 8px;
    color: var(--tx-body);
    outline: none;
  }
  input:focus {
    border-color: var(--aqua);
  }
  .date {
    color-scheme: dark;
  }
  .date.empty {
    color: var(--tx-label);
  }
  .due {
    justify-self: start;
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .val {
    display: flex;
    align-items: center;
    gap: 6px;
    color: var(--tx-2);
    text-align: left;
  }
  .muted {
    color: var(--tx-muted-2);
  }
  .bar {
    width: 3px;
    height: 12px;
    border-radius: 2px;
  }
  .foot {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 10px;
    padding: 10px 12px;
    min-width: 0;
  }
  .fx {
    flex: 1;
    font-size: 0.9231rem;
    color: var(--tx-muted-2);
  }
  .ghost {
    color: var(--tx-muted-2);
  }
  .k {
    font: 0.808rem var(--font-mono);
    color: var(--tx-label);
  }
  .primary .k {
    color: var(--aqua);
    opacity: 0.75;
  }
  .btn:disabled .k {
    color: var(--tx-disabled);
    opacity: 1;
  }
</style>

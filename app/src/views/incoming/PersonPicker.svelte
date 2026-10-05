<script lang="ts">
  // Link to person (17b). Built like the topic picker: type to match People
  // notes by name or alias, ↑/↓ or ⌃j/⌃k to move, ↵ adds this account to the
  // chosen note's frontmatter (an Obsidian write, through the outbox), and the
  // last row creates a new person's note. Never automatic: nothing is linked
  // or merged unless picked here.
  import { untrack } from 'svelte';
  import { call } from '../../lib/api';
  import { accountLabel, initial, personColor, type Account, type PersonHit } from '../../lib/people';
  import { pointerMoved, useKeys } from '../../lib/keys.svelte';

  let {
    account,
    name,
    source,
    ondone,
    onclose,
  }: {
    account: Account;
    /** The display name the source gave them: the starting query and the new note's alias. */
    name: string;
    /** "Slack", "Google Cal"… */
    source: string;
    ondone: () => void;
    onclose: () => void;
  } = $props();

  // Starts from their first name; the field is the user's after that.
  let q = $state(untrack(() => name.split(/\s+/)[0] ?? ''));
  let hits = $state<PersonHit[]>([]);
  let sel = $state(0);
  let busy = $state(false);
  let err = $state<string | null>(null);
  let input: HTMLInputElement;

  $effect(() => input?.focus());
  $effect(() => {
    const query = q;
    call<PersonHit[]>('people.search', { q: query }).then((h) => {
      if (query === q) {
        hits = h;
        sel = 0;
      }
    });
  });

  const newName = $derived(q.trim());
  // Rows: the hits, then "Create person '…'".
  const rows = $derived(hits.length + (newName ? 1 : 0));

  const what = $derived(
    account.kind === 'email' ? 'email' : account.kind === 'slack' ? 'Slack ID' : account.kind === 'jira' ? 'Jira account' : 'GitHub login',
  );

  async function run(cmd: string, args: Record<string, unknown>) {
    if (busy) return;
    busy = true;
    try {
      await call(cmd, args);
      ondone();
    } catch (e) {
      err = String((e as Error).message ?? e);
    } finally {
      busy = false;
    }
  }

  function pick(i: number) {
    const h = hits[i];
    if (h) run('people.link', { person_id: h.id, kind: account.kind, account: account.id });
    else if (newName) run('people.create', { name: newName, alias: name, kind: account.kind, account: account.id });
  }

  useKeys((e) => {
    if (e.key === 'Escape') return onclose(), true;
    if (e.key === 'ArrowDown' || (e.ctrlKey && e.key === 'j')) return (sel = Math.min(sel + 1, rows - 1)), true;
    if (e.key === 'ArrowUp' || (e.ctrlKey && e.key === 'k')) return (sel = Math.max(sel - 1, 0)), true;
    if (e.key === 'Enter') return pick(sel), true;
  });
</script>

<div class="card" role="dialog" aria-label="Link to person">
  <div class="head">
    <span class="label">Link to person</span>
    <span class="acct ellipsis">{account.kind === 'email' || account.kind === 'github' ? account.id : name} · {source}</span>
  </div>
  <div class="field">
    <i class="ph ph-magnifying-glass"></i>
    <input bind:this={input} bind:value={q} placeholder="Name or alias" onblur={onclose} />
  </div>
  <div class="rows">
    {#each hits as h, i (h.id)}
      <button class="row" class:on={i === sel} onmousedown={(e) => (e.preventDefault(), pick(i))} onmousemove={(e) => pointerMoved(e) && (sel = i)}>
        <span class="dot" style:background={personColor(h.color)}>{initial(h.name)}</span>
        <span class="who">
          <span class="name ellipsis">{h.name}</span>
          <span class="path ellipsis">{h.note_path}</span>
        </span>
        <span class="kinds">{h.kinds.map(accountLabel).join(' · ')}</span>
      </button>
    {/each}
    {#if newName}
      <button
        class="row create"
        class:on={sel === hits.length}
        onmousedown={(e) => (e.preventDefault(), pick(hits.length))}
        onmousemove={(e) => pointerMoved(e) && (sel = hits.length)}
      >
        <i class="ph ph-plus"></i><span class="ellipsis">Create person “{newName}”</span>
      </button>
    {/if}
  </div>
  <div class="foot">
    {#if err}
      <span class="err ellipsis" title={err}>{err}</span>
    {:else}
      <span>↵ link, adds {what} to the note</span><span>esc</span><span class="right">colour follows the note</span>
    {/if}
  </div>
</div>

<style>
  .card {
    width: 400px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 10px;
    border-radius: var(--r-panel);
    background: var(--bg-panel);
    border: 1px solid var(--border-strong);
    box-shadow: var(--popover);
    font-size: 0.9615rem;
    color: var(--tx-body);
  }
  .head {
    display: flex;
    gap: 8px;
    align-items: baseline;
    padding: 0 2px;
    min-width: 0;
  }
  .label {
    flex: none;
  }
  .acct {
    font-size: 0.8846rem;
    color: var(--tx-muted);
  }
  .field {
    display: flex;
    gap: 8px;
    align-items: center;
    padding: 6px 10px;
    border-radius: var(--r-input);
    background: var(--bg-app);
    border: 1px solid var(--aqua);
    color: var(--tx-label);
  }
  .field input {
    flex: 1;
    min-width: 0;
    background: none;
    border: 0;
    outline: none;
    font: inherit;
    color: var(--tx-body);
  }
  .rows {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .row {
    display: grid;
    grid-template-columns: 22px minmax(0, 1fr) auto;
    gap: 10px;
    align-items: center;
    padding: 6px 10px;
    border-radius: 5px;
    text-align: left;
  }
  .row.on {
    background: rgba(142, 192, 124, 0.12);
    box-shadow: inset 2px 0 0 var(--aqua);
  }
  .dot {
    width: 22px;
    height: 22px;
    border-radius: 50%;
    color: var(--bg-app);
    font-size: 0.8077rem;
    font-weight: 600;
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .who {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 1px;
  }
  .name {
    color: var(--tx-body);
  }
  .path {
    font-size: 0.8462rem;
    color: var(--tx-label);
  }
  .kinds {
    font-size: 0.8462rem;
    color: var(--tx-muted-2);
    white-space: nowrap;
  }
  .row.create {
    display: flex;
    gap: 10px;
    color: var(--tx-muted);
  }
  .row.create i {
    width: 22px;
    text-align: center;
  }
  .foot {
    display: flex;
    gap: 14px;
    padding: 6px 2px 0;
    border-top: 1px solid var(--border);
    font-size: 0.8846rem;
    color: var(--tx-label);
    min-width: 0;
  }
  .right {
    margin-left: auto;
  }
  .err {
    color: var(--orange);
  }
</style>

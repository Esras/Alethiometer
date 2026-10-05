<script lang="ts">
  // Link to person (17b). Built like the topic picker: type to match People
  // notes by name or alias, ↑/↓ to move, ↵ adds this account to the
  // chosen note's frontmatter (an Obsidian write, through the outbox), and the
  // last row creates a new person's note. Never automatic: nothing is linked
  // or merged unless picked here. The search is lib/ui/picker.svelte.ts's
  // async source: debounced, and only the latest reply counts.
  import { untrack } from 'svelte';
  import { call } from '../../lib/api';
  import { accountLabel, initial, personColor, type Account, type PersonHit } from '../../lib/people';
  import { Picker } from '../../lib/ui/picker.svelte';
  import Avatar from '../../lib/ui/Avatar.svelte';
  import FilterField from '../../lib/ui/FilterField.svelte';

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

  let busy = $state(false);
  let err = $state<string | null>(null);

  // Starts from their first name; the field is the user's after that.
  const p = new Picker<PersonHit[]>({
    q: untrack(() => name.split(/\s+/)[0] ?? ''),
    fetch: (q) => call<PersonHit[]>('people.search', { q }),
    initial: [],
    rows: (): number => rows,
    pick,
    onclose: () => onclose(),
  });
  const hits = $derived(p.result);

  const newName = $derived(p.q.trim());
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
</script>

<div class="card popover" role="dialog" aria-label="Link to person" use:p.frame>
  <div class="head">
    <span class="label">Link to person</span>
    <span class="acct ellipsis">{account.kind === 'email' || account.kind === 'github' ? account.id : name} · {source}</span>
  </div>
  <FilterField field={p.field} bind:value={p.q} placeholder="Name or alias" />
  <div class="rows">
    {#each hits as h, i (h.id)}
      <button class="row" class:on={i === p.sel} {...p.row(i)}>
        <Avatar initials={initial(h.name)} color={personColor(h.color)} size={22} font="0.8077rem" />
        <span class="who">
          <span class="name ellipsis">{h.name}</span>
          <span class="path ellipsis">{h.note_path}</span>
        </span>
        <span class="kinds">{h.kinds.map(accountLabel).join(' · ')}</span>
      </button>
    {/each}
    {#if newName}
      <button class="row create" class:on={p.sel === hits.length} {...p.row(hits.length)}>
        <i class="ph ph-plus"></i><span class="ellipsis">Create person “{newName}”</span>
      </button>
    {/if}
  </div>
  <div class="foot">
    {#if err}
      <span class="err ellipsis" title={err}>{err}</span>
    {:else}
      <span><kbd>↵</kbd> link, adds {what} to the note</span><span><kbd>Esc</kbd></span><span class="right">colour follows the note</span>
    {/if}
  </div>
</div>

<style>
  .card {
    width: 400px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    border-radius: var(--radius-md);
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
    padding: var(--row-pad-compact) 10px;
    min-height: var(--row-min-compact);
    border-radius: var(--radius-md);
    text-align: left;
  }
  .row.on {
    background: var(--aqua-tint);
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
</style>

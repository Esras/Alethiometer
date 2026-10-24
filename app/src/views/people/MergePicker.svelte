<script lang="ts">
  // Merge into… (m on the People index; design canvas `MergeFromPage`): pick
  // the person to merge `from` into. Matches names, aliases and accounts, best
  // first, then anyone sharing an email domain with `from`, then by signals.
  // ↵ opens the two notes side by side (MergeModal); nothing changes here.
  import { untrack } from 'svelte';
  import { initial, personColor, type PeopleRow } from '../../lib/people';
  import Avatar from '../../lib/ui/Avatar.svelte';
  import FilterField from '../../lib/ui/FilterField.svelte';
  import { Picker } from '../../lib/ui/picker.svelte';

  let {
    from,
    people,
    exclude = [],
    onpick,
    onclose,
  }: {
    from: PeopleRow;
    /** Everyone on the index; only People notes can be picked. */
    people: PeopleRow[];
    /** People staged to merge away already. */
    exclude?: number[];
    onpick: (into: PeopleRow) => void;
    onclose: () => void;
  } = $props();

  const domains = (r: PeopleRow) => new Set(r.terms.filter((t) => t.includes('@')).map((t) => t.split('@')[1]));
  const mine = $derived(domains(from));
  const sameDomain = (r: PeopleRow) => [...domains(r)].some((d) => mine.has(d));

  // Starts from their first name; the field is the user's after that.
  const p = new Picker({
    q: untrack(() => from.name.split(/\s+/)[0] ?? ''),
    rows: (): number => matches.length,
    pick: (i) => matches[i] && onpick(matches[i]),
    onclose: () => onclose(),
  });

  function rank(r: PeopleRow, needle: string): number {
    if (!needle) return 2;
    return Math.min(
      ...[r.name.toLowerCase(), ...r.terms].map((n) =>
        n.startsWith(needle) ? 0 : n.split(/\s+/).some((w) => w.startsWith(needle)) ? 1 : n.includes(needle) ? 2 : 9,
      ),
    );
  }

  const matches = $derived.by(() => {
    const needle = p.q.trim().toLowerCase();
    return people
      .filter((r) => r.known && r.id !== null && r.id !== from.id && !exclude.includes(r.id))
      .map((r) => ({ r, rank: rank(r, needle), same: sameDomain(r) }))
      .filter((x) => x.rank < 9)
      .sort(
        (a, b) =>
          a.rank - b.rank ||
          Number(b.same) - Number(a.same) ||
          b.r.total - a.r.total ||
          a.r.name.localeCompare(b.r.name),
      )
      .slice(0, 8)
      .map((x) => x.r);
  });
</script>

<div class="picker popover flush" role="dialog" aria-label="Merge {from.name} into" use:p.frame>
  <div class="field">
    <FilterField field={p.field} icon="git-merge" bind:value={p.q} placeholder={`Merge “${from.name}” into…`} />
  </div>
  {#each matches as r, i (r.key)}
    <button class="opt" class:on={i === p.sel} {...p.row(i)}>
      <Avatar initials={initial(r.name)} color={personColor(r.color)} size="sm" />
      <span class="name ellipsis" style:color={personColor(r.color)}>{r.name}</span>
      <span class="role ellipsis">{r.role ?? ''}</span>
      <span class="why tnum"
        >{r.total ? `${r.total} signal${r.total === 1 ? '' : 's'}` : ''}{r.total && sameDomain(r)
          ? ' · '
          : ''}{sameDomain(r) ? 'same email domain' : ''}</span
      >
    </button>
  {:else}
    <div class="empty-row">No one matches.</div>
  {/each}
  <div class="foot"><kbd>↵</kbd> opens the two side by side. Nothing changes until you confirm there.</div>
</div>

<style>
  .picker {
    width: 460px;
    border-radius: var(--radius-md);
    overflow: hidden;
  }
  .field {
    padding: 6px;
    border-bottom: 1px solid var(--border);
  }
  .opt {
    width: 100%;
    display: grid;
    grid-template-columns: 18px auto minmax(0, 1fr) auto;
    gap: 10px;
    align-items: center;
    padding: var(--row-pad-cozy) 12px;
    min-height: var(--row-min-cozy);
    text-align: left;
    font-size: 0.9615rem;
  }
  .opt.on {
    background: var(--aqua-tint);
  }
  .name {
    font-weight: 500;
  }
  .role {
    color: var(--tx-label);
  }
  .why {
    font-size: 0.8846rem;
    color: var(--tx-label);
    white-space: nowrap;
  }
  .empty-row {
    padding: 6px 12px;
  }
  .foot {
    padding: 8px 12px;
    border-top: 1px solid var(--border);
    font-size: 0.9231rem;
    color: var(--tx-muted-2);
  }
</style>

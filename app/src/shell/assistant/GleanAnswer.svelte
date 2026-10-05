<script lang="ts">
  // One answer (hi-fi 3a, 3d). Citations are small number tags on the
  // baseline; hovering a query in Sources lights the tags of what it found
  // and underlines the sentences they support. People questions add the
  // owner list: order is the ranking (ownership › code › review ›
  // management), each row a role, one sentence of why, then evidence.
  import { personColor } from '../../lib/people';
  import AppIcon from '../../lib/ui/AppIcon.svelte';
  import Avatar from '../../lib/ui/Avatar.svelte';
  import { claims, sentences, type Reply } from './types';

  let {
    reply,
    hoverQuery = null,
    hoverSource = null,
    hoverPerson = $bindable(null),
  }: { reply: Reply; hoverQuery?: number | null; hoverSource?: string | null; hoverPerson?: string | null } = $props();

  const parts = $derived(sentences(reply.answer));
  /** Citation numbers lit by what's hovered in Sources. */
  const lit = $derived.by(() => {
    const out = new Set<number>();
    for (const s of reply.sources) {
      if (s.n === null) continue;
      if ((hoverQuery !== null && s.queries.includes(hoverQuery)) || hoverSource === s.key) out.add(s.n);
    }
    return out;
  });
  const initial = (n: string) => n.trim().charAt(0).toUpperCase();
  const colorOf = (name: string) => {
    const p = reply.people.find((x) => x.name.toLowerCase() === name.toLowerCase());
    return personColor(p?.color ?? 0);
  };
  let expanded = $state<string | null>(null);
</script>

<div class="answer">
  {#each parts as s}
    {#each claims(s) as c}
    <span class="sentence" class:under={c.cites.some((n) => lit.has(n))}>
      {#each c.pieces as p}
        {#if p.t === 'text'}{p.s}{:else if p.t === 'cite'}<span class="cite" class:lit={lit.has(p.n)}>{p.n}</span>{:else}<span
            class="person"
            class:hot={hoverPerson === p.name}
            style:--pc={colorOf(p.name)}
            onmouseenter={() => (hoverPerson = p.name)}
            onmouseleave={() => (hoverPerson = null)}
            role="presentation"><Avatar initials={initial(p.name)} color={colorOf(p.name)} size="sm" px={15} />{p.name}</span
          >{/if}
      {/each}
    </span>
    {/each}
  {/each}
</div>

{#if reply.people.length}
  <div class="people">
    <div class="phead"><span class="label">People · {reply.people.length}</span><span class="order">ownership › code › review › management</span></div>
    {#each reply.people as p, i}
      {@const more = expanded === p.name ? p.evidence.length : i === 0 ? 2 : 1}
      <div class="prow" class:hot={hoverPerson === p.name} onmouseenter={() => (hoverPerson = p.name)} onmouseleave={() => (hoverPerson = null)} role="presentation">
        <div class="pline">
          <span class="person" style:--pc={personColor(p.color)}><Avatar initials={initial(p.name)} color={personColor(p.color)} size="sm" px={15} />{p.name}</span>
          <span class="role">{p.role}</span>
        </div>
        {#if p.why}<div class="why">{p.why}</div>{/if}
        <div class="ev">
          {#each p.evidence.slice(0, more) as e}
            <span class="evi"><AppIcon source={e.icon} size={12} />{e.text}</span>
          {/each}
          {#if p.evidence.length > more}<button class="moreb" onclick={() => (expanded = p.name)}>+ {p.evidence.length - more} more</button>{/if}
        </div>
      </div>
    {/each}
  </div>
{/if}

<style>
  .answer {
    color: var(--tx-body);
    line-height: 1.6;
  }
  .sentence.under {
    text-decoration: underline;
    text-decoration-color: var(--orange);
    text-underline-offset: 3px;
  }
  .cite {
    display: inline-block;
    min-width: 14px;
    height: 14px;
    margin: 0 1px;
    padding: 0 3px;
    border-radius: var(--radius-sm);
    background: var(--bg-head);
    color: var(--tx-muted-2);
    font-size: 0.7308rem;
    line-height: 14px;
    text-align: center;
    vertical-align: 0.15em;
    text-decoration: none;
  }
  .cite.lit {
    background: var(--orange);
    color: var(--bg-app);
  }
  .person {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 0 6px 0 2px;
    border-radius: var(--radius-full);
    color: var(--pc);
    background: color-mix(in srgb, var(--pc) 12%, transparent);
    text-decoration: none;
  }
  .person.hot {
    box-shadow: 0 0 0 1px var(--pc);
  }
  .people {
    margin-top: 12px;
    border-top: 1px solid var(--border);
  }
  .phead {
    display: flex;
    justify-content: space-between;
    padding: 8px 2px 4px;
  }
  .order {
    color: var(--tx-label);
    font-size: 0.8462rem;
  }
  .prow {
    padding: 8px 10px;
    border-top: 1px solid var(--border);
  }
  .prow.hot {
    background: var(--row-hover);
  }
  .pline {
    display: flex;
    justify-content: space-between;
    align-items: center;
  }
  .role {
    color: var(--tx-muted-2);
    font-size: 0.9231rem;
  }
  .why {
    margin-top: 3px;
    color: var(--tx-2);
  }
  .ev {
    margin-top: 3px;
    display: flex;
    flex-wrap: wrap;
    gap: 2px 10px;
    color: var(--tx-label);
    font-size: 0.8846rem;
  }
  .evi {
    display: inline-flex;
    align-items: center;
    gap: 4px;
  }
  .moreb {
    color: var(--tx-muted-2);
  }
</style>

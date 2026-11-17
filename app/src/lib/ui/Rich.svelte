<script lang="ts">
  // Plain text with `@Person` as a PersonPill (an @word matching no one stays as typed) and `[item 12]` as a button that opens the item.
  import { parts, viewing } from '../mentions.svelte';
  import PersonPill from './PersonPill.svelte';

  let { text, cite = true }: { text: string; cite?: boolean } = $props();
</script>

{#each parts(text, cite) as p}
  {#if 'mention' in p}{#if p.color < 0}<span class="mention unknown">{p.mention}</span>{:else}<PersonPill
        who={{ color: p.color, known: true }}
        name={p.mention.replace(/^@/, '')}
        title={p.mention}
      />{/if}
  {:else if 'items' in p}{#each p.items as id}<button class="cite" onclick={() => (viewing.id = id)}>item {id}</button
      >{/each}
  {:else}{p.text}{/if}
{/each}

<style>
  .mention.unknown {
    font: 0.9615rem var(--font-mono);
    font-weight: 400;
    color: var(--msg, #9a9fd6);
  }
  .cite {
    font: 0.92em var(--font-mono);
    padding: 0 4px;
    margin: 0 1px;
    border-radius: var(--radius-sm);
    color: var(--tx-title);
    background: var(--hover-tint);
    border: 1px solid var(--border-strong);
    cursor: pointer;
  }
  .cite:hover {
    text-decoration: underline;
  }
</style>

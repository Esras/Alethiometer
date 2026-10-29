<script lang="ts">
  // Plain text with `@Person` as a pill in the person's colour and `[item 12]` as a button that opens the item.
  import { parts, viewing } from '../mentions.svelte';
  import { personColor, personTint } from '../people';

  let { text, cite = true }: { text: string; cite?: boolean } = $props();
</script>

{#each parts(text, cite) as p}
  {#if 'mention' in p}<span
      class="mention"
      class:unknown={p.color < 0}
      style:color={p.color < 0 ? null : personColor(p.color)}
      style:background={p.color < 0 ? null : personTint(p.color, 0.14)}>{p.mention}</span
    >
  {:else if 'items' in p}{#each p.items as id}<button class="cite" onclick={() => (viewing.id = id)}>item {id}</button
      >{/each}
  {:else}{p.text}{/if}
{/each}

<style>
  .mention {
    padding: 0 4px;
    border-radius: var(--radius-sm);
    font-weight: 500;
  }
  .mention.unknown {
    padding: 0;
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

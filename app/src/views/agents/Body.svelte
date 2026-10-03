<script lang="ts">
  // A row's text: Markdown when it has blocks (agents write Markdown), else one
  // paragraph with `code` and @mentions picked out.
  import Markdown from '../../lib/ui/Markdown.svelte';

  let { text, tone = 'agent' }: { text: string; tone?: 'you' | 'agent' | 'boxed' } = $props();

  const blocky = $derived(/\n\s*\n|^\s*([-*+]|\d+[.)])\s|^\s*```|^#{1,6}\s/m.test(text));
  const parts = $derived(text.split(/(`[^`\n]+`|@[\w.-]+)/g).filter((p) => p !== ''));
</script>

<div class="body {tone}">
  {#if blocky}
    <Markdown {text} />
  {:else}
    {#each parts as p, i (i)}
      {#if p.startsWith('`') && p.endsWith('`') && p.length > 1}<code>{p.slice(1, -1)}</code>
      {:else if p.startsWith('@')}<span class="mention">{p}</span>
      {:else}{p}{/if}
    {/each}
  {/if}
</div>

<style>
  .body {
    font-size: calc(var(--fs-read) - 0.5px);
    line-height: 1.55;
    text-wrap: pretty;
    margin-top: 1px;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .body.you {
    color: var(--tx-title);
  }
  .body.agent {
    color: var(--tx-body);
  }
  .body.boxed {
    color: var(--tx-2);
  }
  .body :global(p) {
    margin: 0 0 6px;
  }
  .body :global(p:last-child) {
    margin-bottom: 0;
  }
  code {
    font: 0.9615rem var(--font-mono);
  }
  .mention {
    font: 0.9615rem var(--font-mono);
    color: var(--msg, #9a9fd6);
  }
</style>

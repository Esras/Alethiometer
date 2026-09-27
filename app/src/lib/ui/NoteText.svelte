<script lang="ts">
  // Minimal Obsidian-flavored markdown display: ## headings, - lists, [ ] boxes,
  // [[wikilinks]]. Read-only; editing happens in Obsidian (never a whole-file rewrite).
  let { text }: { text: string } = $props();

  type Part = { t: string; kind: 'text' | 'box' | 'link' };
  function parts(line: string): Part[] {
    const out: Part[] = [];
    const re = /(\[[ xX]\])|(\[\[[^\]]+\]\])/g;
    let last = 0;
    for (const m of line.matchAll(re)) {
      if (m.index! > last) out.push({ t: line.slice(last, m.index), kind: 'text' });
      out.push({ t: m[0], kind: m[1] ? 'box' : 'link' });
      last = m.index! + m[0].length;
    }
    if (last < line.length) out.push({ t: line.slice(last), kind: 'text' });
    return out;
  }

  const lines = $derived(text.replace(/\n+$/, '').split('\n'));
</script>

{#each lines as line, i}
  {#if /^#{1,6} /.test(line)}
    <div class="h" class:first={i === 0}>{line}</div>
  {:else if line.trim() === ''}
    <!-- blank lines collapse; headings carry their own spacing -->
  {:else}
    <div>
      {#each parts(line) as p}
        {#if p.kind === 'box'}<span class="box">{p.t}</span>{:else if p.kind === 'link'}<span class="link">{p.t}</span>{:else}{p.t}{/if}
      {/each}
    </div>
  {/if}
{/each}

<style>
  .h {
    color: var(--orange);
    font-weight: 500;
    margin-top: 10px;
  }
  .h.first {
    margin-top: 0;
  }
  .box {
    color: var(--tx-label);
  }
  .link {
    color: var(--blue);
  }
</style>

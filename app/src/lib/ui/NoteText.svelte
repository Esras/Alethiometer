<script lang="ts">
  // Obsidian-flavored markdown display, colored like the gruvbox Obsidian theme
  // (headings, bold/italic, code, strike, ==highlight==, [[wikilinks]], [links](url),
  // checkboxes, quotes, lists, ``` fences, --- rules). Read-only; editing happens
  // in Obsidian or the textarea (never a whole-file rewrite).
  import { openExternal } from '../api';

  let { text }: { text: string } = $props();

  type Part = { t: string; kind: 'text' | 'box' | 'wikilink' | 'link' | 'code' | 'bold' | 'italic' | 'strike' | 'mark'; url?: string };

  const inlineRe =
    /(`[^`]+`)|(\*\*[^*]+\*\*|__[^_]+__)|(~~[^~]+~~)|(==[^=]+==)|(\[\[[^\]]+\]\])|(\[[^\]]+\]\([^)]+\))|(\[[ xX]\])|(\*[^*\s][^*]*\*|_[^_\s][^_]*_)/g;

  function parts(line: string): Part[] {
    const out: Part[] = [];
    let last = 0;
    for (const m of line.matchAll(inlineRe)) {
      if (m.index! > last) out.push({ t: line.slice(last, m.index), kind: 'text' });
      const [whole, code, bold, strike, mark, wikilink, link, box] = m;
      if (code) out.push({ t: code.slice(1, -1), kind: 'code' });
      else if (bold) out.push({ t: bold.slice(2, -2), kind: 'bold' });
      else if (strike) out.push({ t: strike.slice(2, -2), kind: 'strike' });
      else if (mark) out.push({ t: mark.slice(2, -2), kind: 'mark' });
      else if (wikilink) out.push({ t: whole, kind: 'wikilink' });
      else if (link) {
        const lm = link.match(/^\[([^\]]+)\]\(([^)]+)\)$/);
        out.push({ t: lm?.[1] ?? link, kind: 'link', url: lm?.[2] });
      } else if (box) out.push({ t: whole, kind: 'box' });
      else out.push({ t: whole.slice(1, -1), kind: 'italic' });
      last = m.index! + whole.length;
    }
    if (last < line.length) out.push({ t: line.slice(last), kind: 'text' });
    return out;
  }

  type Block =
    | { kind: 'heading'; level: number; line: string }
    | { kind: 'hr' }
    | { kind: 'quote'; text: string }
    | { kind: 'list'; ordered: boolean; indent: number; marker: string; text: string }
    | { kind: 'code'; lines: string[] }
    | { kind: 'para'; text: string };

  function blocks(raw: string): Block[] {
    const lines = raw.replace(/\n+$/, '').split('\n');
    const out: Block[] = [];
    let i = 0;
    while (i < lines.length) {
      const line = lines[i];
      const fence = line.match(/^```/);
      if (fence) {
        const code: string[] = [];
        i++;
        while (i < lines.length && !/^```/.test(lines[i])) code.push(lines[i++]);
        i++; // closing fence
        out.push({ kind: 'code', lines: code });
        continue;
      }
      if (/^(-{3,}|\*{3,}|_{3,})\s*$/.test(line)) {
        out.push({ kind: 'hr' });
      } else if (/^#{1,6} /.test(line)) {
        out.push({ kind: 'heading', level: line.match(/^#+/)![0].length, line });
      } else if (/^>\s?/.test(line)) {
        out.push({ kind: 'quote', text: line.replace(/^>\s?/, '') });
      } else {
        const li = line.match(/^(\s*)([-*+]|\d+[.)])\s+(.*)/);
        if (li) out.push({ kind: 'list', ordered: /\d/.test(li[2]), indent: li[1].length, marker: li[2], text: li[3] });
        else if (line.trim() !== '') out.push({ kind: 'para', text: line });
      }
      i++;
    }
    return out;
  }

  const rendered = $derived(blocks(text));
</script>

{#each rendered as b, i}
  {#if b.kind === 'heading'}
    <div class="h h{b.level}" class:first={i === 0}>{b.line}</div>
  {:else if b.kind === 'hr'}
    <hr />
  {:else if b.kind === 'quote'}
    <div class="quote">{#each parts(b.text) as p}{@render inline(p)}{/each}</div>
  {:else if b.kind === 'list'}
    <div class="li" style:padding-left="{14 + b.indent * 14}px">
      <span class="marker">{b.ordered ? b.marker : '•'}</span>
      <span>{#each parts(b.text) as p}{@render inline(p)}{/each}</span>
    </div>
  {:else if b.kind === 'code'}
    <pre class="code"><code>{b.lines.join('\n')}</code></pre>
  {:else}
    <div>{#each parts(b.text) as p}{@render inline(p)}{/each}</div>
  {/if}
{/each}

{#snippet inline(p: Part)}
  {#if p.kind === 'box'}<span class="box">{p.t}</span>
  {:else if p.kind === 'wikilink'}<span class="wikilink">{p.t}</span>
  {:else if p.kind === 'link'}<button class="link" onclick={() => p.url && openExternal(p.url)}>{p.t}</button>
  {:else if p.kind === 'code'}<code class="inline-code">{p.t}</code>
  {:else if p.kind === 'bold'}<b class="bold">{p.t}</b>
  {:else if p.kind === 'italic'}<i>{p.t}</i>
  {:else if p.kind === 'strike'}<s>{p.t}</s>
  {:else if p.kind === 'mark'}<mark>{p.t}</mark>
  {:else}{p.t}{/if}
{/snippet}

<style>
  .h {
    font-weight: 500;
    margin-top: 10px;
  }
  .h.first {
    margin-top: 0;
  }
  .h1 {
    color: var(--red);
  }
  .h2 {
    color: var(--yellow);
  }
  .h3 {
    color: var(--green);
  }
  .h4 {
    color: var(--aqua);
  }
  .h5 {
    color: var(--blue);
  }
  .h6 {
    color: var(--purple);
  }
  .box {
    color: var(--tx-label);
  }
  .wikilink {
    color: var(--orange);
  }
  .link {
    color: var(--green);
    text-decoration: underline;
    text-decoration-color: rgba(184, 187, 38, 0.4);
  }
  .link:hover {
    color: var(--tx-title);
  }
  .bold {
    color: var(--yellow);
    font-weight: 600;
  }
  :global(i) {
    color: var(--yellow);
    font-style: italic;
  }
  :global(mark) {
    background: var(--yellow);
    color: var(--bg-app);
    padding: 0 2px;
  }
  :global(s) {
    color: var(--tx-label);
  }
  .inline-code {
    background: var(--border);
    color: var(--blue);
    border-radius: 3px;
    padding: 0 4px;
    font-size: 0.95em;
  }
  .quote {
    padding-left: 10px;
    border-left: 2px solid var(--border-strong);
    color: var(--tx-muted);
  }
  .li {
    display: flex;
    gap: 8px;
  }
  .marker {
    flex: none;
    color: var(--tx-label);
  }
  .code {
    margin: 6px 0;
    padding: 8px 10px;
    background: var(--border);
    border-radius: var(--r-block);
    color: var(--blue);
    font-size: 0.95em;
    overflow-x: auto;
  }
  hr {
    border: none;
    border-top: 1px solid var(--border-strong);
    margin: 10px 0;
  }
</style>

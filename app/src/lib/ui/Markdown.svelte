<script lang="ts">
  // Markdown for untrusted text (AI answers, GitHub/Jira bodies, Slack). Builds elements, never HTML strings.
  import type { Token } from 'marked';
  import { openExternal } from '../api';
  import { mdTokens, safeHref } from '../markdown';
  import Rich from './Rich.svelte';

  /** `tags`: text after an opening fence may be a language tag. Slack has none, so it's content there. */
  let { text, tags = true, rich = false }: { text: string; tags?: boolean; rich?: boolean } = $props();

  const parsed = $derived(mdTokens(text, tags));
  const code = (t: string) => (tags ? t : t.replace(/^\n+|\n+$/g, ''));
</script>

{#snippet link(href: string | null, label: string, kids: Token[] | undefined)}
  {#if href}<a class="link-out" {href} onclick={(e) => (e.preventDefault(), openExternal(href))}
      >{#if kids}{@render inline(kids)}{:else}{label}{/if}</a
    >{:else if kids}{@render inline(kids)}{:else}{label}{/if}
{/snippet}

{#snippet inline(tokens: Token[] | undefined)}
  {#each tokens ?? [] as t}
    {#if t.type === 'text' && t.tokens}{@render inline(t.tokens)}
    {:else if t.type === 'text'}{#if rich}<Rich text={t.text} />{:else}{t.text}{/if}
    {:else if t.type === 'codespan'}<code>{t.text}</code>
    {:else if t.type === 'strong'}<strong>{@render inline(t.tokens)}</strong>
    {:else if t.type === 'em'}<em>{@render inline(t.tokens)}</em>
    {:else if t.type === 'del'}<s>{@render inline(t.tokens)}</s>
    {:else if t.type === 'br'}<br />
    {:else if t.type === 'link'}{@render link(safeHref(t.href), t.text, t.tokens)}
    {:else if t.type === 'image'}{@render link(safeHref(t.href), t.text || t.href, undefined)}
    {:else if t.type === 'checkbox'}{t.checked ? '\u2611 ' : '\u2610 '}
    {:else if t.type === 'html'}{t.raw}
    {:else if 'tokens' in t && t.tokens}{@render inline(t.tokens)}
    {:else if 'text' in t}{t.text}{/if}
  {/each}
{/snippet}

{#snippet blocks(tokens: Token[] | undefined)}
  {#each tokens ?? [] as b}
    {#if b.type === 'space'}{:else if b.type === 'code'}<pre><code
          >{code(tags || !b.lang ? b.text : `${b.lang}\n${b.text}`)}</code
        ></pre>
    {:else if b.type === 'heading'}<div class="h h{Math.min(b.depth, 3)}">{@render inline(b.tokens)}</div>
    {:else if b.type === 'paragraph'}<p>{@render inline(b.tokens)}</p>
    {:else if b.type === 'blockquote'}<blockquote>{@render blocks(b.tokens)}</blockquote>
    {:else if b.type === 'hr'}<hr />
    {:else if b.type === 'list'}
      {#if b.ordered}<ol start={b.start === '' ? undefined : b.start}>
          {#each b.items as it}<li>{@render blocks(it.tokens)}</li>{/each}
        </ol>
      {:else}<ul>
          {#each b.items as it}<li>{@render blocks(it.tokens)}</li>{/each}
        </ul>{/if}
    {:else if b.type === 'table'}<div class="table">
        <table>
          <thead
            ><tr
              >{#each b.header as c, i}<th style:text-align={b.align[i]}>{@render inline(c.tokens)}</th>{/each}</tr
            ></thead
          >
          <tbody>
            {#each b.rows as row}<tr
                >{#each row as c, i}<td style:text-align={b.align[i]}>{@render inline(c.tokens)}</td>{/each}</tr
              >{/each}
          </tbody>
        </table>
      </div>
    {:else if b.type === 'html'}<p>{b.raw}</p>
    {:else if 'tokens' in b && b.tokens}{@render inline(b.tokens)}
    {:else if 'text' in b}{b.text}{/if}
  {/each}
{/snippet}

<div class="md">{@render blocks(parsed)}</div>

<style>
  .md {
    min-width: 0;
    overflow-wrap: anywhere;
  }
  .md > :first-child {
    margin-top: 0;
  }
  .md > :last-child {
    margin-bottom: 0;
  }
  p,
  ul,
  ol,
  pre,
  blockquote,
  .table {
    margin: 0 0 6px;
  }
  li > p {
    margin: 0;
  }
  ul,
  ol {
    padding-left: 18px;
  }
  .h {
    font-weight: 600;
    color: var(--tx-title);
    margin: 8px 0 4px;
  }
  .h1 {
    font-size: 1.1em;
  }
  code {
    font-family: var(--font-mono);
    font-size: 0.92em;
    padding: 0 3px;
    border-radius: var(--radius-sm);
    background: rgba(235, 219, 178, 0.07);
  }
  pre {
    padding: 6px 8px;
    border-radius: var(--radius-md);
    background: var(--hover-tint);
    overflow-x: auto;
  }
  pre code {
    padding: 0;
    background: none;
    white-space: pre;
  }
  blockquote {
    padding-left: 8px;
    border-left: 2px solid var(--border-strong);
    color: var(--tx-muted);
  }
  blockquote > :last-child {
    margin-bottom: 0;
  }
  hr {
    border: none;
    border-top: 1px solid var(--border-strong);
    margin: 8px 0;
  }
  .table {
    overflow-x: auto;
  }
  table {
    border-collapse: collapse;
    overflow-wrap: normal;
  }
  th,
  td {
    padding: 2px 8px;
    border: 1px solid var(--border-strong);
    text-align: left;
    vertical-align: top;
  }
  th {
    font-weight: 600;
    color: var(--tx-title);
  }
  a:hover {
    text-underline-offset: auto;
  }
</style>

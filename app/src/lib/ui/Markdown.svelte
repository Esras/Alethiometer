<script lang="ts">
  // Small markdown renderer for untrusted text (AI answers, GitHub/Jira bodies). Builds elements, never HTML strings.
  import { openExternal } from '../api';

  /** `tags`: text after an opening fence may be a language tag. Slack has none, so it's content there. */
  let { text, tags = true }: { text: string; tags?: boolean } = $props();

  type Block =
    | { kind: 'code'; text: string }
    | { kind: 'h'; level: number; text: string }
    | { kind: 'ul' | 'ol'; items: string[]; start?: number }
    | { kind: 'quote' | 'p'; lines: string[] };

  function blocks(src: string): Block[] {
    const out: Block[] = [];
    const lines = src.replace(/\r\n?/g, '\n').split('\n');
    let gap = false;
    for (let i = 0; i < lines.length; i++) {
      const line = lines[i];
      if (line.trim() === '') {
        gap = true;
        continue;
      }
      const afterGap = gap;
      gap = false;
      if (/^\s*```/.test(line)) {
        const body: string[] = [];
        const rest = line.replace(/^\s*```/, '');
        const close = rest.lastIndexOf('```');
        if (close >= 0) {
          out.push({ kind: 'code', text: rest.slice(0, close).trim() });
          continue;
        }
        if (rest.trim() && !(tags && /^[\w+#.-]+$/.test(rest.trim()))) body.push(rest);
        while (++i < lines.length && !/^\s*```/.test(lines[i])) body.push(lines[i]);
        out.push({ kind: 'code', text: body.join('\n') });
        continue;
      }
      const h = /^(#{1,6})\s+(.*)$/.exec(line);
      if (h) {
        out.push({ kind: 'h', level: h[1].length, text: h[2] });
        continue;
      }
      const last = out.at(-1);
      const li = /^\s*(?:([-*+])|(\d+)[.)])\s+(.*)$/.exec(line);
      if (li) {
        const kind = li[1] ? 'ul' : 'ol';
        const t = li[3].replace(/^\[([ xX])\]\s*/, (_, c) => (c === ' ' ? '\u2610 ' : '\u2611 '));
        if (last?.kind === kind) last.items.push(t);
        else out.push({ kind, items: [t], start: li[2] ? Number(li[2]) : undefined });
        continue;
      }
      const q = /^\s*>\s?(.*)$/.exec(line);
      const kind = q ? 'quote' : 'p';
      const t = q ? q[1] : line;
      if (last?.kind === kind && !afterGap) last.lines.push(t);
      else out.push({ kind, lines: [t] });
    }
    return out;
  }

  type Span = { kind: 'text' | 'code' | 'b' | 'i' | 's' | 'link'; text: string; href?: string };
  const INLINE = /(`[^`]+`)|\[([^\]]+)\]\((https?:\/\/[^)\s]+)\)|\*\*([^*]+)\*\*|__([^_]+)__|\*([^*\s][^*]*)\*|(https?:\/\/[^\s<>()]+[^\s<>().,;:!?'"])|~~([^~]+)~~|(?<!\w)_([^_\s](?:[^_]*[^_\s])?)_(?!\w)/g;

  function spans(line: string): Span[] {
    const out: Span[] = [];
    let at = 0;
    for (const m of line.matchAll(INLINE)) {
      if (m.index! > at) out.push({ kind: 'text', text: line.slice(at, m.index) });
      if (m[1]) out.push({ kind: 'code', text: m[1].slice(1, -1) });
      else if (m[2]) out.push({ kind: 'link', text: m[2], href: m[3] });
      else if (m[4] ?? m[5]) out.push({ kind: 'b', text: (m[4] ?? m[5])! });
      else if (m[6] ?? m[9]) out.push({ kind: 'i', text: (m[6] ?? m[9])! });
      else if (m[8]) out.push({ kind: 's', text: m[8] });
      else out.push({ kind: 'link', text: m[7], href: m[7] });
      at = m.index! + m[0].length;
    }
    if (at < line.length) out.push({ kind: 'text', text: line.slice(at) });
    return out;
  }

  const parsed = $derived(blocks(text));
</script>

{#snippet inline(line: string)}
  {#each spans(line) as s}
    {#if s.kind === 'code'}<code>{s.text}</code>
    {:else if s.kind === 'b'}<strong>{s.text}</strong>
    {:else if s.kind === 'i'}<em>{s.text}</em>
    {:else if s.kind === 's'}<s>{s.text}</s>
    {:else if s.kind === 'link'}<a class="link-out" href={s.href} onclick={(e) => (e.preventDefault(), openExternal(s.href!))}>{s.text}</a>
    {:else}{s.text}{/if}
  {/each}
{/snippet}

<div class="md">
  {#each parsed as b}
    {#if b.kind === 'code'}<pre><code>{b.text}</code></pre>
    {:else if b.kind === 'h'}<div class="h h{Math.min(b.level, 3)}">{@render inline(b.text)}</div>
    {:else if b.kind === 'ul'}<ul>{#each b.items as it}<li>{@render inline(it)}</li>{/each}</ul>
    {:else if b.kind === 'ol'}<ol start={b.start}>{#each b.items as it}<li>{@render inline(it)}</li>{/each}</ol>
    {:else if b.kind === 'quote'}<blockquote>{#each b.lines as l, i}{#if i}<br />{/if}{@render inline(l)}{/each}</blockquote>
    {:else if b.kind === 'p'}<p>{#each b.lines as l, i}{#if i}<br />{/if}{@render inline(l)}{/each}</p>{/if}
  {/each}
</div>

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
  blockquote {
    margin: 0 0 6px;
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
  a:hover {
    text-underline-offset: auto;
  }
</style>

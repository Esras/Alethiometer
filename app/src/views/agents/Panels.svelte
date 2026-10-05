<script lang="ts">
  // An approval's Parsed and Explained panels, side by side: the pinned card
  // in Agents and an open approval in Incoming show the same two.
  import Markdown from '../../lib/ui/Markdown.svelte';
  import type { Request } from './types';

  let { req }: { req: Request } = $props();

  const verdictClass = (v: string) => (v === 'read-only' ? 'green' : v === 'unknown' ? 'yellow' : 'orange');
  const tagClass = (t: string) =>
    ['reads files', 'reads git'].includes(t) ? 'blue' : ['local bin'].includes(t) ? 'green' : ['shell per file', 'runs shell', 'env', 'unknown command'].includes(t) ? 'yellow' : 'orange';
</script>

<div class="panels" class:solo={!req.parsed || !req.explained}>
  {#if req.parsed}
  <div class="facts-box">
    <div class="ph-head">
      <i class="ph ph-tree-structure" style:color="var(--blue)"></i>Parsed<span class="sub">shell parser · same answer every time</span>
      <span class="badge-wrap"><span class="mono-tag {verdictClass(req.parsed.verdict)}">{req.parsed.verdict}</span></span>
    </div>
    {#each req.parsed.steps as s (s.n)}
      <div class="step" class:deep={s.depth > 0}>
        <span class="n">{s.n}</span>
        <span class="st">{s.text}</span>
        <span class="tags">{#each s.tags as t (t)}<span class="mono-tag {tagClass(t)}">{t}</span>{/each}</span>
      </div>
    {/each}
    {#if req.parsed.checks.length}
      <div class="checks">
        {#each req.parsed.checks as c (c.text)}
          <span class:warn={!c.ok}><i class="ph {c.ok ? 'ph-check' : 'ph-warning'}"></i>{c.text}</span>
        {/each}
      </div>
    {/if}
  </div>
  {/if}
  {#if req.explained}
    {@const x = req.explained}
    <div class="facts-box">
      <div class="ph-head"><i class="ph ph-sparkle" style:color="var(--purple)"></i>Explained<span class="sub" title="Sees the command and your request, never the agent’s own words about it">{x.model}</span></div>
      {#if x.state === 'pending'}<div class="ex muted">Asking{x.read.length ? `, with ${x.read.join(', ')}` : ''}…</div>
      {:else if x.state === 'ok'}
        <div class="ex"><Markdown text={x.text} /></div>
        {#if x.fits === false && x.fit_note}<div class="fit"><i class="ph ph-warning"></i><span><b>Beyond your request:</b> {x.fit_note}</span></div>{/if}
        {#if x.misuse.length}
          <div class="misuse">
            <span class="mh">If planted</span>
            <div class="ex"><Markdown text={x.misuse.map((m) => `- ${m}`).join('\n')} /></div>
            {#if x.worst}<div class="worst"><Markdown text={`**Worst case:** ${x.worst}`} /></div>{/if}
          </div>
        {:else if x.fits !== null}
          <div class="benign"><i class="ph ph-check"></i>Nothing it could be misused for beyond what it says.</div>
        {/if}
        {#if x.read.length}<div class="readf">Read {x.read.join(', ')}</div>{/if}
      {:else}<div class="ex muted">{req.explained.state === 'off' ? 'Off: ' : 'Couldn’t explain: '}{req.explained.text}</div>{/if}
    </div>
  {/if}
</div>

<style>
  .panels {
    display: grid;
    align-items: start;
    grid-template-columns: minmax(0, 1.35fr) minmax(0, 1fr);
    gap: 10px;
  }
  .panels.solo {
    grid-template-columns: minmax(0, 1fr);
  }
  .facts-box {
    padding: 7px 10px;
    border-radius: var(--r-input);
    background: var(--bg-panel);
    border: 1px solid var(--border);
    display: flex;
    flex-direction: column;
    gap: 3px;
    min-width: 0;
  }
  .ph-head {
    display: flex;
    gap: 7px;
    align-items: center;
    font-size: 0.846rem;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--tx-label);
    margin-bottom: 2px;
  }
  .ph-head i {
    font-size: 0.923rem;
  }
  .sub {
    letter-spacing: 0;
    text-transform: none;
    font: 0.846rem var(--font-mono);
    color: var(--tx-disabled);
  }
  .badge-wrap {
    margin-left: auto;
    letter-spacing: 0;
    text-transform: none;
  }
  .step {
    display: grid;
    grid-template-columns: 20px minmax(0, 1fr) auto;
    gap: 8px;
    align-items: center;
    padding: 2px 0;
  }
  .step.deep {
    grid-template-columns: 34px minmax(0, 1fr) auto;
  }
  .n {
    font: 0.846rem var(--font-mono);
    color: var(--tx-disabled);
    text-align: right;
  }
  .st {
    font: 0.923rem var(--font-mono);
    color: var(--tx-2);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .tags {
    display: flex;
    gap: 4px;
  }
  .checks {
    display: flex;
    gap: 4px 14px;
    flex-wrap: wrap;
    border-top: 1px solid var(--bg-head);
    padding-top: 5px;
    margin-top: 2px;
  }
  .checks span {
    display: flex;
    gap: 5px;
    align-items: center;
    font-size: 0.923rem;
    color: var(--tx-muted);
  }
  .checks i {
    font-size: 0.923rem;
    color: var(--green);
  }
  .checks .warn,
  .checks .warn i {
    color: var(--yellow);
  }
  .ex {
    font-size: 1rem;
    line-height: 1.5;
    color: var(--tx-body);
    text-wrap: pretty;
  }
  .ex.muted {
    color: var(--tx-label);
  }
  .fit {
    display: flex;
    gap: 6px;
    align-items: baseline;
    font-size: 0.9615rem;
    line-height: 1.45;
    color: var(--yellow);
  }
  .fit b {
    font-weight: 500;
  }
  .misuse {
    display: flex;
    flex-direction: column;
    gap: 3px;
    border-top: 1px solid var(--bg-head);
    padding-top: 5px;
    font-size: 0.9615rem;
    line-height: 1.45;
    color: var(--tx-muted);
    text-wrap: pretty;
  }
  .mh {
    font-size: 0.808rem;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--orange);
  }
  .worst {
    color: var(--tx-2);
  }
  .benign {
    display: flex;
    gap: 6px;
    align-items: center;
    border-top: 1px solid var(--bg-head);
    padding-top: 5px;
    font-size: 0.9615rem;
    color: var(--tx-muted);
  }
  .benign i {
    color: var(--green);
  }
  .readf {
    font: 0.846rem var(--font-mono);
    color: var(--tx-disabled);
  }
</style>

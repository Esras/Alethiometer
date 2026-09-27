<script lang="ts">
  // Tab on a line: a small picker in its margin cell. The next likely
  // destinations, type to search topics / people / Jira, or keep it as a note.
  // ↑↓ (or Tab) move, ↵ picks, Esc closes.
  import { call } from '../../lib/api';
  import { catColor } from '../../lib/app.svelte';
  import AppIcon from '../../lib/ui/AppIcon.svelte';
  import { KEEP, type EditLine, type Options, type Target } from './types';

  let {
    line,
    onpick,
    onclose,
  }: { line: EditLine; onpick: (t: Target | null) => void; onclose: () => void } = $props();

  let q = $state('');
  let res = $state<Options>({ options: [], create: null });
  let sel = $state(0);
  let input: HTMLInputElement;

  $effect(() => {
    const query = q;
    const timer = setTimeout(() => {
      call<Options>('notes.options', { text: line.text, kind: line.kind, q: query }).then((r) => {
        if (query === q) {
          res = r;
          sel = 0;
        }
      });
    }, query ? 80 : 0);
    return () => clearTimeout(timer);
  });

  $effect(() => input.focus());

  type Opt = { t: Target | null; label: string; hint?: string };
  const opts = $derived.by(() => {
    const out: Opt[] = res.options.map((t) => ({ t, label: t.label }));
    if (res.create && !q) out.push({ t: res.create, label: res.create.label });
    out.push({ t: KEEP, label: KEEP.label });
    if (line.target && !line.target.auto) out.push({ t: null, label: 'Back to the suggestion' });
    return out;
  });

  function onkey(e: KeyboardEvent) {
    const n = opts.length;
    if (e.key === 'ArrowDown' || (e.key === 'Tab' && !e.shiftKey)) sel = (sel + 1) % n;
    else if (e.key === 'ArrowUp' || (e.key === 'Tab' && e.shiftKey)) sel = (sel - 1 + n) % n;
    else if (e.key === 'Enter' && !e.metaKey && !e.ctrlKey) onpick(opts[sel].t);
    else if (e.key === 'Escape') onclose();
    else return;
    e.preventDefault();
    e.stopPropagation();
  }
</script>

<div class="picker" role="dialog" aria-label="Where it goes">
  <div class="search">
    <i class="ph ph-magnifying-glass"></i>
    <input bind:this={input} bind:value={q} onkeydown={onkey} onblur={onclose} placeholder="Topic, person or PLAT-…" spellcheck="false" />
  </div>
  <div class="list">
    {#each opts as o, i}
      <button class="opt" class:on={i === sel} onmousedown={(e) => e.preventDefault()} onclick={() => onpick(o.t)} onmouseenter={() => (sel = i)}>
        <span class="ic">
          {#if !o.t}<i class="ph ph-arrow-counter-clockwise"></i>
          {:else if o.t.type === 'topic'}<span class="sq" style:background={catColor(o.t.category ?? 'work')}></span>
          {:else if o.t.type === 'person'}<i class="ph ph-user"></i>
          {:else if o.t.type === 'keep'}<i class="ph ph-note-blank"></i>
          {:else if o.t.app}<AppIcon source={o.t.app} size={14} />{/if}
        </span>
        <span class="ellipsis">{o.label}</span>
        {#if o.hint}<span class="hint">{o.hint}</span>{/if}
      </button>
    {/each}
  </div>
  <div class="foot"><span><kbd>↵</kbd> pick</span><span><kbd>Esc</kbd> close</span></div>
</div>

<style>
  .picker {
    position: absolute;
    z-index: 20;
    top: -6px;
    left: -1px;
    width: 270px;
    background: var(--bg-panel);
    border: 1px solid var(--border-strong);
    border-radius: var(--r-panel);
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.45);
    overflow: hidden;
    font-size: 12.5px;
  }
  .search {
    display: flex;
    gap: 7px;
    align-items: center;
    padding: 7px 10px;
    border-bottom: 1px solid var(--border);
    color: var(--tx-label);
  }
  input {
    flex: 1;
    min-width: 0;
    background: none;
    border: 0;
    outline: none;
    font-size: 12.5px;
    color: var(--tx-body);
    caret-color: var(--aqua);
  }
  input::placeholder {
    color: var(--tx-disabled);
  }
  .list {
    padding: 4px;
    display: flex;
    flex-direction: column;
  }
  .opt {
    display: flex;
    gap: 8px;
    align-items: center;
    padding: 5px 8px;
    border-radius: var(--r-block);
    color: var(--tx-2);
    text-align: left;
    min-width: 0;
  }
  .opt.on {
    background: var(--aqua-tint);
    box-shadow: inset 2px 0 0 var(--aqua);
    color: var(--tx-body);
  }
  .ic {
    width: 14px;
    flex: none;
    display: flex;
    justify-content: center;
    color: var(--tx-muted-2);
    font-size: 13px;
  }
  .sq {
    width: 7px;
    height: 7px;
    border-radius: 2px;
  }
  .hint {
    margin-left: auto;
    font-size: 11px;
    color: var(--tx-label);
    white-space: nowrap;
  }
  .foot {
    display: flex;
    gap: 12px;
    padding: 5px 10px;
    border-top: 1px solid var(--border);
    font-size: 11px;
    color: var(--tx-label);
  }
</style>

<script lang="ts">
  // Tab on a line: a small picker in its margin cell. The next likely
  // destinations, type to search topics / people / Jira, or keep it as a note.
  // ↑↓ (or Tab) move, ↵ picks, Esc closes (lib/ui/picker.svelte.ts).
  import { call } from '../../lib/api';
  import { catColor } from '../../lib/app.svelte';
  import { Picker } from '../../lib/ui/picker.svelte';
  import AppIcon from '../../lib/ui/AppIcon.svelte';
  import FilterField from '../../lib/ui/FilterField.svelte';
  import { KEEP, type EditLine, type Options, type Target } from './types';

  let { line, onpick, onclose }: { line: EditLine; onpick: (t: Target | null) => void; onclose: () => void } = $props();

  const p = new Picker<Options>({
    fetch: (q) => call<Options>('notes.options', { text: line.text, kind: line.kind, q }),
    initial: { options: [], create: null },
    rows: (): number => opts.length,
    pick: (i) => onpick(opts[i].t),
    onclose: () => onclose(),
    // Tab steps the cursor too.
    keys: (e): boolean => e.key === 'Tab' && (p.move(e.shiftKey ? -1 : 1), true),
  });

  type Opt = { t: Target | null; label: string; hint?: string };
  const opts = $derived.by(() => {
    const out: Opt[] = p.result.options.map((t) => ({ t, label: t.label }));
    if (p.result.create && !p.q) out.push({ t: p.result.create, label: p.result.create.label });
    out.push({ t: KEEP, label: KEEP.label });
    if (line.target && !line.target.auto) out.push({ t: null, label: 'Back to the suggestion' });
    return out;
  });
</script>

<div class="picker popover flush" role="dialog" aria-label="Where it goes" use:p.frame>
  <div class="search">
    <FilterField field={p.field} bind:value={p.q} placeholder="Topic, person or PLAT-…" spellcheck="false" />
  </div>
  <div class="list">
    {#each opts as o, i}
      <button class="opt" class:on={i === p.sel} {...p.row(i)}>
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
    border-radius: var(--radius-md);
    overflow: hidden;
    font-size: 0.9615rem;
  }
  .search {
    padding: 6px;
    border-bottom: 1px solid var(--border);
  }
  .search {
    caret-color: var(--aqua); /* inherited by FilterField's input */
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
    padding: var(--row-pad-compact) 8px;
    min-height: var(--row-min-compact);
    border-radius: var(--radius-sm);
    color: var(--tx-2);
    text-align: left;
    min-width: 0;
  }
  .opt.on {
    background: var(--aqua-tint);
    color: var(--tx-body);
  }
  .ic {
    width: 14px;
    flex: none;
    display: flex;
    justify-content: center;
    color: var(--tx-muted-2);
    font-size: 1rem;
  }
  .sq {
    width: 7px;
    height: 7px;
    border-radius: 2px;
  }
  .hint {
    margin-left: auto;
    font-size: 0.8462rem;
    color: var(--tx-label);
    white-space: nowrap;
  }
  .foot {
    display: flex;
    gap: 12px;
    padding: 5px 10px;
    border-top: 1px solid var(--border);
    font-size: 0.8462rem;
    color: var(--tx-label);
  }
</style>

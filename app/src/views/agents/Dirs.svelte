<script lang="ts">
  // The Directories field (project/new-agent/Dir-chips.dc.html): one chip per directory, the first being the
  // session's working directory. Typing offers recent directories (the typed part brighter); ⏎ or Tab adds
  // the typed path, and the field stays open for the next; ⌫ on an empty field takes the last chip off. A
  // path that isn't there stays a chip, marked. The field wraps rather than scrolling.
  import { age } from './state.svelte';
  import { chipLabels } from './newAgent';

  let {
    dirs = $bindable(),
    recent = [],
    bad = [],
    placeholder = 'add a path…',
    now,
    onfocus,
  }: {
    dirs: string[];
    recent?: { dir: string; at: string }[];
    /** Chips to mark (missing, or colliding in a sandbox). */
    bad?: string[];
    placeholder?: string;
    now: number;
    onfocus?: () => void;
  } = $props();

  let text = $state('');
  let focused = $state(false);
  let hi = $state(0);
  let el = $state<HTMLInputElement>();

  const labels = $derived(chipLabels(dirs));
  const matches = $derived.by(() => {
    const q = text.trim().toLowerCase();
    if (!focused || !q) return [];
    return recent.filter((r) => !dirs.includes(r.dir) && r.dir.toLowerCase().includes(q)).slice(0, 6);
  });
  $effect(() => {
    if (hi >= matches.length) hi = 0;
  });

  function add(d: string) {
    const v = d.trim().replace(/(.)\/+$/, '$1');
    if (v && !dirs.includes(v)) dirs = [...dirs, v];
    text = '';
    hi = 0;
  }
  function remove(i: number) {
    dirs = dirs.filter((_, j) => j !== i);
    el?.focus();
  }

  function keydown(e: KeyboardEvent) {
    if (e.key === 'ArrowDown' && matches.length) hi = (hi + 1) % matches.length;
    else if (e.key === 'ArrowUp' && matches.length) hi = (hi - 1 + matches.length) % matches.length;
    else if ((e.key === 'Enter' || e.key === 'Tab') && text.trim() && !e.metaKey && !e.ctrlKey)
      add(matches[hi]?.dir ?? text);
    else if (e.key === 'Backspace' && !text && dirs.length) dirs = dirs.slice(0, -1);
    else if (e.key === 'Escape' && matches.length) text = '';
    else return;
    e.preventDefault();
    e.stopPropagation();
  }

  /** The match with the typed part brighter. */
  function parts(d: string): [string, string, string] {
    const i = d.toLowerCase().indexOf(text.trim().toLowerCase());
    const n = text.trim().length;
    return i < 0 ? [d, '', ''] : [d.slice(0, i), d.slice(i, i + n), d.slice(i + n)];
  }
</script>

<div class="wrap">
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions (a click on the field's empty part types in it) -->
  <div
    class="dirs"
    class:f={focused}
    role="group"
    aria-label="Directories"
    onmousedown={(e) => {
      if (e.target === e.currentTarget) {
        e.preventDefault();
        el?.focus();
      }
    }}
  >
    {#each dirs as d, i (d)}
      <span class="dir" class:bad={bad.includes(d)} title={d}
        >{#if bad.includes(d)}<i class="ph ph-warning"></i>{/if}{labels[i]}<button
          class="x"
          tabindex="-1"
          aria-label="Remove {labels[i]}"
          onclick={() => remove(i)}><i class="ph ph-x"></i></button
        ></span
      >
    {/each}
    <input
      bind:this={el}
      bind:value={text}
      aria-label="Add a directory"
      placeholder={dirs.length ? 'add…' : placeholder}
      spellcheck="false"
      autocomplete="off"
      onkeydown={keydown}
      onfocus={() => ((focused = true), onfocus?.())}
      onblur={() => {
        focused = false;
        if (text.trim()) add(text);
      }}
    />
  </div>
  {#if matches.length}
    <div class="pop" role="listbox">
      {#each matches as m, i (m.dir)}
        {@const [a, b, c] = parts(m.dir)}
        <button
          class="o"
          class:cur={i === hi}
          role="option"
          aria-selected={i === hi}
          tabindex="-1"
          onmousedown={(e) => {
            e.preventDefault();
            add(m.dir);
          }}><i class="ph ph-folder"></i><span>{a}<b>{b}</b>{c}</span><span class="r">{age(m.at, now)}</span></button
        >
      {/each}
    </div>
  {/if}
</div>

<style>
  .wrap {
    position: relative;
    min-width: 0;
  }
  .dirs {
    display: flex;
    flex-wrap: wrap;
    gap: 5px;
    align-items: center;
    min-width: 0;
    min-height: 30px;
    box-sizing: border-box;
    padding: 4px;
    border-radius: var(--radius-md);
    background: var(--bg-app);
    border: 1px solid var(--border-strong);
    font: 0.923rem var(--font-mono);
    cursor: text;
  }
  .dirs.f {
    border-color: var(--aqua);
  }
  .dirs input {
    flex: 1;
    min-width: 80px;
    background: none;
    border: 0;
    outline: 0;
    padding: 0 6px;
    font: 0.9615rem var(--font-mono);
    color: var(--tx-body);
  }
  .dirs input::placeholder {
    color: var(--tx-disabled);
  }
  /* The × sits 4px after the name and as far from the edge as the name is from the other edge's padding. */
  .dir {
    display: inline-flex;
    gap: 2px;
    align-items: center;
    height: 20px;
    box-sizing: border-box;
    padding: 0 4px 0 9px;
    border-radius: 999px;
    border: 1px solid var(--border-strong);
    color: var(--tx-2);
    font: 0.923rem/1 var(--font-mono);
    white-space: nowrap;
    cursor: default;
  }
  .dir.bad {
    border-color: var(--need-border, var(--orange));
  }
  .dir.bad > i {
    font-size: 0.846rem;
    color: var(--orange);
    margin-right: 3px;
  }
  .x {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 14px;
    height: 14px;
    border-radius: 50%;
    font-size: 0.769rem;
    color: var(--tx-label);
  }
  .x:hover {
    background: var(--hover-tint);
    color: var(--tx-body);
  }
  .pop {
    position: absolute;
    z-index: 2;
    left: 0;
    right: 0;
    top: calc(100% + 4px);
    background: var(--bg-panel);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-md);
    box-shadow: var(--popover);
    padding: 4px 0;
    font: 0.923rem var(--font-mono);
    display: flex;
    flex-direction: column;
  }
  .o {
    display: flex;
    gap: 8px;
    align-items: center;
    min-height: 28px;
    padding: 0 12px;
    color: var(--tx-2);
    text-align: left;
    white-space: nowrap;
  }
  .o span:first-of-type {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .o.cur {
    background: var(--aqua-tint);
    color: var(--tx-title);
  }
  .o b {
    font-weight: 400;
    color: var(--tx-title);
  }
  .o i {
    font-size: 0.923rem;
    color: var(--yellow);
  }
  .o .r {
    margin-left: auto;
    color: var(--tx-muted-2);
    font-size: 0.846rem;
  }
</style>

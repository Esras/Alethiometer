<script lang="ts">
  // Installed fonts, each shown in its own face. Type to filter, ↵ picks.
  let {
    value,
    fonts,
    mono = false,
    open = false,
    onclose,
    onchange,
  }: {
    value: string;
    fonts: { name: string; mono: boolean }[];
    mono?: boolean;
    open?: boolean;
    onclose: () => void;
    onchange: (v: string) => void;
  } = $props();

  let shown = $state(false);
  let q = $state('');
  let sel = $state(0);
  let input = $state<HTMLInputElement | null>(null);

  $effect(() => {
    if (open) show();
  });

  // Monospace first for the mono picker; everything stays pickable.
  const list = $derived.by(() => {
    const all = [{ name: '', mono: true }, ...fonts];
    const ordered = mono ? [...all.filter((f) => f.mono), ...all.filter((f) => !f.mono)] : all;
    const n = q.trim().toLowerCase();
    return ordered.filter((f) => !n || (f.name || 'system').toLowerCase().includes(n)).slice(0, 200);
  });

  function show() {
    shown = true;
    q = '';
    sel = Math.max(0, list.findIndex((f) => f.name === value));
    queueMicrotask(() => input?.focus());
  }

  function close() {
    shown = false;
    onclose();
  }

  function pick(name: string) {
    close();
    if (name !== value) onchange(name);
  }

  const label = (n: string) => n || (mono ? 'System monospace' : 'System');
  const face = (n: string) => (n ? `"${n}", ${mono ? 'monospace' : 'sans-serif'}` : mono ? 'ui-monospace, monospace' : 'system-ui, sans-serif');
</script>

<span class="pick">
  <button class="current" style:font-family={face(value)} onclick={show}>{label(value)}<i class="ph ph-caret-down"></i></button>
  {#if shown}
    <div class="pop" role="listbox">
      <input
        bind:this={input}
        bind:value={q}
        placeholder="Filter fonts…"
        onblur={() => setTimeout(close, 120)}
        oninput={() => (sel = 0)}
        onkeydown={(e) => {
          e.stopPropagation();
          if (e.key === 'ArrowDown' || (e.ctrlKey && e.key === 'j')) sel = Math.min(sel + 1, list.length - 1);
          else if (e.key === 'ArrowUp' || (e.ctrlKey && e.key === 'k')) sel = Math.max(sel - 1, 0);
          else if (e.key === 'Enter' && list[sel]) pick(list[sel].name);
          else if (e.key === 'Escape') close();
          else return;
          e.preventDefault();
        }} />
      <div class="opts scroll">
        {#each list as f, i (f.name)}
          <button class="opt" class:sel={i === sel} style:font-family={face(f.name)} onmousedown={() => pick(f.name)}>{label(f.name)}</button>
        {/each}
        {#if !list.length}<div class="none">No installed font matches</div>{/if}
      </div>
    </div>
  {/if}
</span>

<style>
  .pick {
    position: relative;
  }
  .current {
    width: 360px;
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 5px 10px;
    border-radius: var(--r-input);
    border: 1px solid var(--border-strong);
    background: var(--bg-app);
    color: var(--tx-title);
  }
  .current i {
    color: var(--tx-label);
  }
  .pop {
    position: absolute;
    right: 0;
    top: calc(100% + 4px);
    width: 360px;
    z-index: 5;
    border-radius: var(--r-panel);
    border: 1px solid var(--border-strong);
    background: var(--bg-panel);
    box-shadow: 0 10px 28px rgba(0, 0, 0, 0.5);
    overflow: hidden;
  }
  input {
    width: 100%;
    padding: 7px 10px;
    border: 0;
    border-bottom: 1px solid var(--border);
    background: none;
    outline: none;
  }
  .opts {
    max-height: 260px;
  }
  .opt {
    display: block;
    width: 100%;
    text-align: left;
    padding: 5px 10px;
    font-size: 1.0769rem;
  }
  .opt.sel {
    background: var(--aqua-tint);
  }
  .none {
    padding: 8px 10px;
    color: var(--tx-label);
  }
</style>

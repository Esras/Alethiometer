<script lang="ts">
  // Installed fonts, each shown in its own face. Type to filter, ↵ picks
  // (lib/ui/picker.svelte.ts).
  import { Picker } from '../../lib/ui/picker.svelte';

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

  // Stays mounted while closed, so it takes keys only while the list is open.
  const p = new Picker<undefined>({ rows: (): number => list.length, pick: (i) => list[i] && pick(list[i].name), onclose: () => close(), active: () => shown });

  $effect(() => {
    if (open) show();
  });

  // Monospace first for the mono picker; everything stays pickable.
  const list = $derived.by(() => {
    const all = [{ name: '', mono: true }, ...fonts];
    const ordered = mono ? [...all.filter((f) => f.mono), ...all.filter((f) => !f.mono)] : all;
    const n = p.q.trim().toLowerCase();
    return ordered.filter((f) => !n || (f.name || 'system').toLowerCase().includes(n)).slice(0, 200);
  });

  function show() {
    shown = true;
    p.q = '';
    p.sel = Math.max(0, list.findIndex((f) => f.name === value));
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
  <!-- Open, a press here keeps the focus in the field, so the click closes (not blur, then reopen). -->
  <button class="current" style:font-family={face(value)} onmousedown={(e) => shown && e.preventDefault()} onclick={() => (shown ? close() : show())}>{label(value)}<i class="ph ph-caret-down"></i></button>
  {#if shown}
    <div class="pop popover flush" role="listbox" use:p.frame>
      <div class="search"><input class="input" use:p.field bind:value={p.q} placeholder="Filter fonts…" /></div>
      <div class="opts scroll">
        {#each list as f, i (f.name)}
          <button class="opt" class:sel={i === p.sel} style:font-family={face(f.name)} {...p.row(i)}>{label(f.name)}</button>
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
    overflow: hidden;
  }
  .search {
    padding: 6px;
    border-bottom: 1px solid var(--border);
  }
  .opts {
    max-height: 260px;
  }
  .opt {
    display: block;
    width: 100%;
    text-align: left;
    padding: var(--row-pad-compact) 10px;
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

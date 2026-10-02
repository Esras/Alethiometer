<script lang="ts">
  // key → value pairs (Gmail category labels, calendar colour → category).
  import { ui } from '../../lib/app.svelte';

  let {
    map,
    categories = false,
    editing = false,
    onedit = () => {},
    onchange,
  }: {
    map: Record<string, string>;
    /** Keys (Gmail) or values (calendar colours) are category keys. */
    categories?: boolean;
    editing?: boolean;
    onedit?: (on: boolean) => void;
    onchange: (v: Record<string, string>) => void;
  } = $props();

  let k = $state('');
  let v = $state('');
  let first = $state<HTMLInputElement | null>(null);
  $effect(() => {
    if (editing && first) first.focus();
  });

  const cat = (key: string) => ui.info?.categories[key]?.label;

  function add() {
    onedit(false);
    if (!k.trim() || !v.trim()) return;
    onchange({ ...map, [k.trim()]: v.trim() });
    k = v = '';
  }

  function remove(key: string) {
    const next = { ...map };
    delete next[key];
    onchange(next);
  }
</script>

<span class="map">
  {#each Object.entries(map) as [key, val]}
    <span class="pair">
      <span class="mono">{key}</span>{#if categories && cat(key)}<span class="cat">{cat(key)}</span>{/if}
      <i class="ph ph-arrow-right"></i>
      <span>{val}</span>{#if categories && cat(val)}<span class="cat">{cat(val)}</span>{/if}
      <button onclick={() => remove(key)} aria-label="Remove {key}"><i class="ph ph-x"></i></button>
    </span>
  {/each}
  {#if editing}
    <span class="pair new">
      <input bind:this={first} bind:value={k} placeholder="key" onkeydown={(e) => { e.stopPropagation(); if (e.key === 'Escape') onedit(false); }} />
      <i class="ph ph-arrow-right"></i>
      <input bind:value={v} placeholder="value" onkeydown={(e) => { e.stopPropagation(); if (e.key === 'Enter') add(); if (e.key === 'Escape') onedit(false); }} />
      <button onclick={add} aria-label="Add"><i class="ph ph-check"></i></button>
    </span>
  {:else}
    <button class="add" onclick={() => onedit(true)}><i class="ph ph-plus"></i>add</button>
  {/if}
</span>

<style>
  .map {
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    gap: 4px;
  }
  .pair {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 2px 4px 2px 8px;
    border-radius: var(--r-input);
    background: var(--bg-head);
    color: var(--tx-2);
  }
  .pair i {
    color: var(--tx-label);
  }
  .cat {
    color: var(--tx-label);
    font-size: 0.8462rem;
  }
  .pair button {
    color: var(--tx-label);
    display: flex;
  }
  .add {
    display: inline-flex;
    gap: 4px;
    align-items: center;
    color: var(--tx-label);
  }
  input {
    width: 110px;
    padding: 1px 6px;
    border-radius: 4px;
    border: 1px solid var(--border-strong);
    background: var(--bg-app);
    outline: none;
  }
  input:focus {
    border-color: var(--aqua);
  }
</style>

<script lang="ts">
  // Add, rename, recolor and remove categories. Changes save as you make them.
  import { call } from '../../lib/api';
  import { refresh } from '../../lib/app.svelte';
  import { live } from '../../lib/live';
  import type { CategoryRow } from '../../lib/types';

  let cats = $state<CategoryRow[]>([]);
  let err = $state<string | null>(null);
  let name = $state('');
  let color = $state('#8ec07c');

  $effect(() => live<CategoryRow[]>('categories.list', {}, (c) => (cats = c)));

  async function run(cmd: string, args: Record<string, unknown>) {
    try {
      await call(cmd, args);
      err = null;
    } catch (e) {
      err = String((e as Error).message ?? e);
    }
    refresh();
  }

  async function add(e: SubmitEvent) {
    e.preventDefault();
    if (!name.trim()) return;
    await run('categories.create', { label: name, color });
    if (!err) name = '';
  }
</script>

<div class="mgr">
  {#each cats as c (c.key)}
    <div class="cat">
      <input type="color" value={c.color} onchange={(e) => run('categories.update', { key: c.key, color: e.currentTarget.value })} title="Color" />
      <input
        class="input sm"
        value={c.label}
        onchange={(e) => run('categories.update', { key: c.key, label: e.currentTarget.value })}
        onkeydown={(e) => e.key === 'Enter' && e.currentTarget.blur()}
      />
      <span class="key">{c.key}</span>
      <button class="btn" onclick={() => run('categories.delete', { key: c.key })} title="Remove (only when nothing uses it)"><i class="ph ph-trash"></i></button>
    </div>
  {/each}
  <form class="cat" onsubmit={add}>
    <input type="color" bind:value={color} title="Color" />
    <input class="input sm" bind:value={name} placeholder="New category…" />
    <span class="key"></span>
    <button class="btn primary" type="submit" disabled={!name.trim()} title="Add category"><i class="ph ph-plus"></i></button>
  </form>
  {#if err}<div class="err"><i class="ph ph-warning"></i>{err}</div>{/if}
</div>

<style>
  .mgr {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 12px 18px;
    border-bottom: 1px solid var(--border);
    background: var(--bg-head);
  }
  .cat {
    display: grid;
    grid-template-columns: 28px minmax(0, 280px) 120px auto;
    gap: 10px;
    align-items: center;
  }
  input[type='color'] {
    width: 28px;
    height: 22px;
    padding: 0;
    border: 1px solid var(--border-strong);
    border-radius: 4px;
    background: none;
  }
  .key {
    font-size: 0.8846rem;
    color: var(--tx-label);
  }
  .err {
    color: var(--orange);
    display: flex;
    gap: 6px;
    align-items: center;
  }
</style>

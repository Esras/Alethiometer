<script lang="ts">
  // A list of short strings as chips, with + add. ↵ on the row adds.
  let {
    items,
    editing = false,
    masked = false,
    onedit = () => {},
    onchange,
  }: {
    items: string[];
    editing?: boolean;
    /** Show each item shortened (secret-ish links). */
    masked?: boolean;
    onedit?: (on: boolean) => void;
    onchange: (v: string[]) => void;
  } = $props();

  let input = $state<HTMLInputElement | null>(null);
  $effect(() => {
    if (editing && input) input.focus();
  });

  function shorten(s: string): string {
    try {
      const u = new URL(s);
      return `${u.host}/…`;
    } catch {
      return s.length > 12 ? `${s.slice(0, 8)}…` : s;
    }
  }

  function add(v: string) {
    const t = v.trim();
    onedit(false);
    if (t && !items.includes(t)) onchange([...items, t]);
  }
</script>

<span class="list">
  {#each items as it, i}
    <span class="item" class:mono={masked}>
      {masked ? shorten(it) : it}
      <button onclick={() => onchange(items.filter((_, j) => j !== i))} aria-label="Remove {it}"><i class="ph ph-x"></i></button>
    </span>
  {/each}
  {#if editing}
    <input
      bind:this={input}
      placeholder="add…"
      onblur={(e) => add(e.currentTarget.value)}
      onkeydown={(e) => {
        if (e.key === 'Enter') {
          add(e.currentTarget.value);
          e.currentTarget.value = '';
        }
        if (e.key === 'Escape') {
          e.currentTarget.value = '';
          e.currentTarget.blur();
        }
        e.stopPropagation();
      }} />
  {:else}
    <button class="add" onclick={() => onedit(true)}><i class="ph ph-plus"></i>add</button>
  {/if}
</span>

<style>
  .list {
    display: flex;
    flex-wrap: wrap;
    justify-content: flex-end;
    gap: 6px;
    max-width: 520px;
  }
  .item {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 2px 4px 2px 8px;
    border-radius: var(--r-input);
    background: var(--bg-head);
    color: var(--tx-2);
  }
  .item button {
    color: var(--tx-label);
    font-size: 0.8462rem;
    display: flex;
  }
  .item button:hover {
    color: var(--tx-title);
  }
  .add {
    display: inline-flex;
    gap: 4px;
    align-items: center;
    color: var(--tx-label);
    padding: 2px 6px;
  }
  .add:hover {
    color: var(--tx-2);
  }
  input {
    width: 160px;
    padding: 2px 8px;
    border-radius: var(--r-input);
    border: 1px solid var(--aqua);
    background: var(--bg-app);
    outline: none;
  }
</style>

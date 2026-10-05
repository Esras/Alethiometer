<script lang="ts">
  // − value + with a unit. ↵ on the row types a value; −/+ step it.
  let {
    value,
    step = 1,
    min,
    max,
    unit,
    digits,
    format,
    editing = false,
    onedit = () => {},
    onchange,
  }: {
    value: number;
    step?: number;
    min?: number;
    max?: number;
    unit?: string;
    digits?: number;
    format?: (v: number) => string;
    editing?: boolean;
    onedit?: (on: boolean) => void;
    onchange: (v: number) => void;
  } = $props();

  let input = $state<HTMLInputElement | null>(null);
  $effect(() => {
    if (editing && input) {
      input.focus();
      input.select();
    }
  });

  const decimals = $derived(digits ?? (String(step).split('.')[1]?.length ?? 0));
  const clamp = (v: number) => Math.min(max ?? Infinity, Math.max(min ?? -Infinity, v));
  const round = (v: number) => Number(v.toFixed(decimals));
  const shown = $derived(format ? format(value) : digits === undefined && Number.isInteger(value) ? String(value) : value.toFixed(decimals));

  function bump(dir: number) {
    const next = clamp(round(value + dir * step));
    if (next !== value) onchange(next);
  }

  function commit(text: string) {
    onedit(false);
    const n = Number(text.replace(/:00$/, ''));
    if (text.trim() !== '' && Number.isFinite(n)) {
      const next = clamp(round(n));
      if (next !== value) onchange(next);
    }
  }

  export function stepBy(dir: number) {
    bump(dir);
  }
</script>

<span class="input stepper">
  <button onclick={() => bump(-1)} disabled={min !== undefined && value <= min} aria-label="Less">−</button>
  {#if editing}
    <input
      bind:this={input}
      value={format ? String(value) : shown}
      onblur={(e) => commit(e.currentTarget.value)}
      onkeydown={(e) => {
        if (e.key === 'Enter') e.currentTarget.blur();
        if (e.key === 'Escape') {
          e.currentTarget.value = String(value);
          e.currentTarget.blur();
        }
        e.stopPropagation();
      }} />
  {:else}
    <span class="v tnum" ondblclick={() => onedit(true)} role="presentation">{shown}</span>
  {/if}
  {#if unit}<span class="unit">{unit}</span>{/if}
  <button onclick={() => bump(1)} disabled={max !== undefined && value >= max} aria-label="More">+</button>
</span>

<style>
  .stepper {
    display: inline-flex;
    align-items: center;
    width: auto;
    padding: 0;
  }
  /* Aqua while you type a value, not while a − or + holds the focus after a click. */
  .stepper:focus-within:not(:has(input:focus)) {
    border-color: var(--border-strong);
  }
  button {
    width: 26px;
    height: 26px;
    color: var(--tx-muted-2);
  }
  button:hover:not(:disabled) {
    color: var(--tx-title);
  }
  button:disabled {
    color: var(--tx-disabled);
    cursor: default;
  }
  .v,
  input {
    min-width: 44px;
    text-align: center;
    color: var(--tx-title);
  }
  input {
    width: 56px;
    background: none;
    border: 0;
    outline: none;
  }
  .unit {
    color: var(--tx-label);
    font-size: 0.9231rem;
    margin-right: 4px;
  }
</style>

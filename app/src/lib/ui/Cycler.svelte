<script lang="ts" generics="T">
  // A cycler (ui-plan decision 10): one button shows the current value, and a click
  // or its key steps to the next option, wrapping around. Shift-click or Shift+key
  // steps back. For two to four options changed rarely.
  //
  // `key` registers the key here, on the key stack, and shows it. Leave it out where
  // the control sits under a modal key floor (Settings), or where the view already
  // binds a key to each option or needs its own handler to come first (Reflect,
  // SplitReview, TodoForm): pass `hint` there to show the keys without registering
  // anything, so the same key never has two handlers.
  import { plain, useKeys } from '../keys.svelte';

  let {
    options,
    value = $bindable(),
    key,
    hint,
    label,
    title,
    disabled = false,
  }: {
    options: { value: T; label: string }[];
    value: T;
    key?: string;
    hint?: string;
    /** What it sets, for the accessible name and the tooltip. */
    label: string;
    /** The tooltip, when it should say more than `label` (a disabled one: why). */
    title?: string;
    disabled?: boolean;
  } = $props();

  const at = $derived(options.findIndex((o) => o.value === value));
  const shown = $derived(at >= 0 ? options[at].label : String(value ?? ''));
  const cap = $derived(hint ?? (key === 'Tab' ? '⇥' : key));

  function step(dir: number) {
    if (disabled || !options.length) return;
    // A value that isn't an option yet: forward lands on the first, back on the last.
    const next = at < 0 ? (dir > 0 ? 0 : options.length - 1) : (at + dir + options.length) % options.length;
    value = options[next].value;
  }

  useKeys((e) => {
    if (!key || disabled) return false;
    const up = key.length === 1 ? key.toUpperCase() : key;
    if (!plain(e, key) && !(up !== key && plain(e, up))) return false;
    step(e.shiftKey ? -1 : 1);
    return true;
  });
</script>

<button class="cycler" {disabled} title={title ?? label} aria-label="{label}: {shown}" onclick={(e) => step(e.shiftKey ? -1 : 1)}>
  <span class="val">{shown}</span><i class="ph ph-arrows-clockwise"></i>{#if cap}<kbd>{cap}</kbd>{/if}
</button>

<style>
  .cycler {
    display: inline-flex;
    gap: 8px;
    align-items: center;
    /* In a grid cell it keeps its own width instead of filling the column. */
    justify-self: start;
    padding: 3px 6px 3px 10px;
    border: 1px solid var(--border-strong);
    border-radius: var(--r-input);
    font-size: 0.9231rem;
    white-space: nowrap;
    transition: background 120ms;
  }
  .cycler:hover:not(:disabled) {
    background: var(--hover-tint);
  }
  .val {
    color: var(--tx-title);
  }
  .ph {
    color: var(--tx-label);
  }
  .cycler:disabled {
    border-color: var(--border);
    cursor: default;
  }
  .cycler:disabled .val,
  .cycler:disabled .ph,
  .cycler:disabled kbd {
    color: var(--tx-disabled);
  }
</style>

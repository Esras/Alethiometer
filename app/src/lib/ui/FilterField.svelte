<script lang="ts">
  // A search or filter field: an icon and a bare input in an .input box (the
  // box takes the focus border through :focus-within). Behaviour stays with
  // the site: keys go through `onkeydown` (or the input's own handlers via the
  // rest props), focus through the bound `el`. `children` is what trails the
  // input (a count, a key hint). `field` is an action for the input (the
  // pickers' p.field). Layout (width, flex, margins) comes from the site
  // through `class`.
  import type { Action } from 'svelte/action';
  import type { HTMLInputAttributes } from 'svelte/elements';
  import type { Snippet } from 'svelte';

  const none: Action<HTMLInputElement> = () => {};

  let {
    value = $bindable(''),
    el = $bindable(),
    placeholder,
    icon = 'magnifying-glass',
    sm = false,
    field = none,
    class: cls = '',
    children,
    ...rest
  }: {
    value?: string;
    el?: HTMLInputElement | null;
    placeholder?: string;
    /** A Phosphor icon name without the prefix. */
    icon?: string;
    /** The small field, for a row or toolbar. */
    sm?: boolean;
    field?: Action<HTMLInputElement>;
    class?: string;
    children?: Snippet;
  } & Omit<HTMLInputAttributes, 'value' | 'class' | 'children'> = $props();
</script>

<label class="input filter {cls}" class:sm>
  <i class="ph ph-{icon}"></i>
  <input bind:this={el} bind:value {placeholder} use:field {...rest} />
  {@render children?.()}
</label>

<style>
  .filter {
    display: flex;
    gap: 8px;
    align-items: center;
    color: var(--tx-label);
  }
  .filter input {
    flex: 1;
    min-width: 0;
    background: none;
    border: 0;
    padding: 0;
    outline: none;
    font: inherit;
    color: var(--tx-body);
  }
</style>

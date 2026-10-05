<script lang="ts">
  // A modal's behaviour; the look stays with each site (its box is the
  // children, with its own CSS). Renders the scrim and an unstyled dialog
  // wrapper.
  //   Keys:   one modal entry on the key stack, so keys never fall through to
  //           the view underneath. `onkeys` runs first (the site's own keys);
  //           what it leaves, Escape closes and Tab stays inside the dialog.
  //   Mouse:  a mousedown on the scrim itself closes.
  //   Focus:  returns to what had it before the modal opened.
  //
  // Props that exist only because the sites look different today. To settle
  // in the visible pass, when the design system's one scrim replaces them:
  //   scrim     Settings .45, SlackReview .3, palette .35, NewModal var(--scrim),
  //             Keys rgba(12,13,14,.66)
  //   z         50 (palette, NewModal), 60 (Settings), 70 (SlackReview), 20 (Keys)
  //   position  Keys is absolute inside the Agents pane; the rest are fixed
  //   inset     NewModal starts below the 32px title bar; the rest cover the window
  //   isolate   NewModal never let a key reach the window handler (⌘K, ⌘1–7,
  //             g i); the rest do
  import type { Snippet } from 'svelte';
  import { dispatchKey, useKeys, type KeyHandler } from '../keys.svelte';

  let {
    onclose,
    label,
    onkeys,
    scrim = 'rgba(0, 0, 0, 0.45)',
    z = 50,
    position = 'fixed',
    inset = '0',
    isolate = false,
    children,
  }: {
    onclose: () => void;
    label: string;
    onkeys?: KeyHandler;
    scrim?: string;
    z?: number;
    position?: 'fixed' | 'absolute';
    inset?: string;
    isolate?: boolean;
    children: Snippet;
  } = $props();

  let backdrop = $state<HTMLDivElement>();
  let dialog = $state<HTMLDivElement>();

  const TABBABLE = 'input, select, textarea, button, a[href], [tabindex]:not([tabindex="-1"])';

  function tab(e: KeyboardEvent): boolean {
    const all = [...(dialog?.querySelectorAll<HTMLElement>(TABBABLE) ?? [])].filter((el) => !el.hasAttribute('disabled') && el.tabIndex >= 0 && el.getClientRects().length);
    if (!all.length) {
      // Nothing to tab to: park focus on the scrim, so it is out of the page behind.
      if (backdrop) {
        backdrop.tabIndex = -1;
        backdrop.focus();
      }
      return true;
    }
    const i = all.indexOf(document.activeElement as HTMLElement);
    const next = e.shiftKey ? (i <= 0 ? all.at(-1) : all[i - 1]) : i === all.length - 1 ? all[0] : all[i + 1];
    next?.focus();
    return true;
  }

  useKeys(
    (e) => {
      if (onkeys?.(e)) return true;
      if (e.key === 'Escape') {
        onclose();
        return true;
      }
      if (e.key === 'Tab') return tab(e);
      return false;
    },
    { modal: true },
  );

  // Read before the children mount, so a site that focuses its own field
  // doesn't hide where focus was.
  const before = document.activeElement as HTMLElement | null;
  $effect(() => {
    const d = dialog;
    if (!d) return;
    return () => {
      // Not if focus has already gone somewhere on purpose.
      const now = document.activeElement;
      if (before?.isConnected && before !== document.body && (!now || now === document.body || d.contains(now))) before.focus();
    };
  });

  function onkeydown(e: KeyboardEvent) {
    if (!isolate) return;
    dispatchKey(e);
    e.stopPropagation();
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="scrim"
  bind:this={backdrop}
  style:position
  style:inset
  style:z-index={z}
  style:background={scrim}
  {onkeydown}
  onmousedown={(e) => e.target === e.currentTarget && onclose()}>
  <div class="dialog" bind:this={dialog} role="dialog" aria-modal="true" aria-label={label}>
    {@render children()}
  </div>
</div>

<style>
  .scrim {
    display: flex;
    align-items: center;
    justify-content: center;
    outline: none;
  }
  /* No box of its own: the site's box is laid out as if it were the scrim's child. */
  .dialog {
    display: contents;
  }
</style>

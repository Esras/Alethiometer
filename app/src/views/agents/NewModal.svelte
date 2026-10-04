<script lang="ts">
  // The new-agent modal (05): over the whole tab, below the title bar. It takes
  // the keyboard: Tab moves through its fields (and stays inside), letters
  // type, ⌘⏎ starts, Esc closes.
  import type { Snippet } from 'svelte';
  import { mod } from '../../lib/keys.svelte';

  let { sub, onStart, onClose, children, foot }: { sub: string; onStart: () => void; onClose: () => void; children: Snippet; foot: Snippet } = $props();

  let box = $state<HTMLDivElement>();

  function onKey(e: KeyboardEvent) {
    if (mod(e) && e.key === 'Enter') {
      onStart();
      e.preventDefault();
    } else if (e.key === 'Escape') {
      onClose();
      e.preventDefault();
    } else if (e.key === 'Tab' && box) {
      const all = [...box.querySelectorAll<HTMLElement>('input, select, textarea, button, [tabindex]:not([tabindex="-1"])')].filter((el) => !el.hasAttribute('disabled') && el.tabIndex >= 0);
      const i = all.indexOf(document.activeElement as HTMLElement);
      const next = e.shiftKey ? (i <= 0 ? all.at(-1) : all[i - 1]) : i === all.length - 1 ? all[0] : all[i + 1];
      next?.focus();
      e.preventDefault();
    }
    e.stopPropagation();
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="scrim" onkeydown={onKey} onmousedown={(e) => e.target === e.currentTarget && onClose()}>
  <div class="modal agent-form" bind:this={box} role="dialog" aria-modal="true" aria-label="New agent">
    <div class="top">
      <i class="ph ph-user-plus"></i><span class="t">New agent</span><span class="sub">{sub}</span><span class="esc">Esc</span>
    </div>
    <div class="body">
      {@render children()}
    </div>
    <div class="foot">
      {@render foot()}
    </div>
  </div>
</div>

<style>
  .scrim {
    position: fixed;
    inset: 32px 0 0 0;
    z-index: 50;
    background: rgba(15, 16, 18, 0.68);
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .modal {
    width: min(740px, calc(100vw - 48px));
    max-height: calc(100vh - 80px);
    border-radius: 10px;
    background: var(--bg-panel);
    border: 1px solid var(--border-strong);
    box-shadow: 0 24px 64px rgba(0, 0, 0, 0.55);
  }
  .modal .top {
    padding: 12px 16px;
  }
  .body {
    padding: 14px 16px;
    display: flex;
    flex-direction: column;
    gap: 16px;
    overflow-y: auto;
    min-height: 0;
  }
  .modal .foot {
    padding: 12px 16px;
    border-top: 1px solid var(--border);
  }
</style>

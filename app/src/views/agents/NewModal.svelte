<script lang="ts">
  // The new-agent modal (05): over the whole tab, below the title bar. It takes
  // the keyboard: Tab moves through its fields (and stays inside), letters
  // type, ⌘⏎ starts, Esc closes.
  import type { Snippet } from 'svelte';
  import { mod } from '../../lib/keys.svelte';
  import Modal from '../../lib/ui/Modal.svelte';

  let { sub, onStart, onClose, children, foot }: { sub: string; onStart: () => void; onClose: () => void; children: Snippet; foot: Snippet } = $props();

  // Modal does Esc and Tab.
  function onkeys(e: KeyboardEvent): boolean {
    if (!(mod(e) && e.key === 'Enter')) return false;
    onStart();
    return true;
  }
</script>

<Modal label="New agent" onclose={onClose} {onkeys} z={50} scrim="var(--scrim)" inset="32px 0 0 0" isolate>
  <div class="modal agent-form">
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
</Modal>

<style>
  .modal {
    width: min(740px, calc(100vw - 48px));
    max-height: calc(100vh - 80px);
    border-radius: 10px;
    background: var(--bg-panel);
    border: 1px solid var(--border-strong);
    box-shadow: var(--modal);
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

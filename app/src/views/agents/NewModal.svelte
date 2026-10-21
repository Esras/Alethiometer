<script lang="ts">
  // The new-agent modal (05): over the whole tab, below the title bar. It takes
  // the keyboard: Tab moves through its fields (and stays inside), letters
  // type, ⌘⏎ starts, Esc closes.
  import type { Snippet } from 'svelte';
  import { mod } from '../../lib/keys.svelte';
  import Modal from '../../lib/ui/Modal.svelte';

  let {
    sub,
    onStart,
    onClose,
    children,
    foot,
  }: { sub: string; onStart: () => void; onClose: () => void; children: Snippet; foot: Snippet } = $props();

  // Modal does Esc and Tab.
  function onkeys(e: KeyboardEvent): boolean {
    if (!(mod(e) && e.key === 'Enter')) return false;
    onStart();
    return true;
  }
</script>

<Modal label="New agent" onclose={onClose} {onkeys} inset="32px 0 0 0" isolate sticky>
  <div class="new modal agent-form">
    <div class="top">
      <i class="ph ph-user-plus"></i><span class="t">New agent</span><span class="sub">{sub}</span><kbd>Esc</kbd>
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
  .new {
    width: min(740px, calc(100vw - 48px));
    max-height: calc(100vh - 80px);
    border-radius: var(--radius-md);
    /* Its header, body and footer run edge to edge and carry their own padding. */
    padding: 0;
  }
  .new .top {
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
  .new .foot {
    padding: 12px 16px;
    border-top: 1px solid var(--border);
  }
</style>

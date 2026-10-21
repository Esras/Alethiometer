<script lang="ts" module>
  export type KeySection = { title: string; keys: [string[], string][] };
</script>

<script lang="ts">
  import type { Snippet } from 'svelte';
  import Modal from './Modal.svelte';

  let {
    sections,
    onclose,
    position = 'fixed',
    lede,
    foot,
  }: {
    sections: KeySection[];
    onclose: () => void;
    position?: 'fixed' | 'absolute';
    lede?: Snippet;
    foot?: string;
  } = $props();

  function onkeys(e: KeyboardEvent): boolean {
    if (e.key !== '?') return false;
    onclose();
    return true;
  }
</script>

<Modal label="Keys" {onclose} {onkeys} {position}>
  <div class="sheet modal">
    <div class="head">
      <span class="pane-title">Keys</span>
      {#if lede}<span class="lede">{@render lede()}</span>{/if}
      <span class="close"><kbd>?</kbd> or <kbd>Esc</kbd> to close</span>
    </div>
    <div class="grid">
      {#each sections as s (s.title)}
        <div class="sec">
          <div class="st label">{s.title}</div>
          <div class="keys">
            {#each s.keys as [ks, what] (what)}
              <span class="ks"
                >{#each ks as k (k)}<kbd>{k}</kbd>{/each}</span
              >
              <span class="what">{what}</span>
            {/each}
          </div>
        </div>
      {/each}
    </div>
    {#if foot}<div class="foot">{foot}</div>{/if}
  </div>
</Modal>

<style>
  .sheet {
    width: min(1080px, calc(100% - 48px));
    border-radius: var(--radius-md);
    display: flex;
    flex-direction: column;
    gap: 16px;
  }
  .head {
    display: flex;
    gap: 14px;
    align-items: baseline;
  }
  .lede {
    font-size: 1rem;
    color: var(--tx-muted);
  }
  .close {
    margin-left: auto;
    font-size: 0.885rem;
    color: var(--tx-label);
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: 18px 28px;
  }
  .sec {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .st {
    padding-bottom: 2px;
    border-bottom: 1px solid var(--border);
  }
  .keys {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr);
    gap: 6px 12px;
    align-items: center;
  }
  .ks {
    display: flex;
    gap: 3px;
    justify-content: flex-end;
  }
  .what {
    font-size: 1rem;
    color: var(--tx-2);
  }
  .foot {
    font-size: 0.923rem;
    color: var(--tx-label);
  }
</style>

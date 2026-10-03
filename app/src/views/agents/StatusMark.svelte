<script lang="ts">
  import type { Status } from './types';

  // running: dot · waiting: half-filled ring · idle: ring · done: check · failed: x
  let { status, size = 8 }: { status: Status; size?: number } = $props();
</script>

{#if status === 'running'}
  <span class="dot" style:width="{size - 1}px" style:height="{size - 1}px"></span>
{:else if status === 'waiting'}
  <span class="ring waiting" style:width="{size}px" style:height="{size}px"></span>
{:else if status === 'idle'}
  <span class="ring" style:width="{size}px" style:height="{size}px"></span>
{:else if status === 'done'}
  <i class="ph ph-check done"></i>
{:else}
  <i class="ph ph-x failed"></i>
{/if}

<style>
  .dot {
    display: inline-block;
    flex: none;
    border-radius: 50%;
    background: var(--aqua);
  }
  .ring {
    display: inline-block;
    flex: none;
    box-sizing: border-box;
    border-radius: 50%;
    border: 1.5px solid var(--tx-label);
  }
  .ring.waiting {
    border-color: var(--orange);
    background: linear-gradient(90deg, var(--orange) 50%, transparent 50%);
  }
  i {
    font-size: 0.846rem;
    width: 8px;
    display: inline-flex;
    justify-content: center;
  }
  .done {
    color: var(--green);
  }
  .failed {
    color: var(--red);
  }
</style>

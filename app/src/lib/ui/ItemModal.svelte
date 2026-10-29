<script lang="ts">
  // The item an `[item N]` citation names, over whatever view cited it.
  import SignalDetail from '../../views/shared/SignalDetail.svelte';
  import { viewing } from '../mentions.svelte';
  import Modal from './Modal.svelte';
</script>

{#if viewing.id !== null}
  <Modal label="Item {viewing.id}" onclose={() => (viewing.id = null)}>
    <div class="modal box" role="document">
      <SignalDetail itemId={viewing.id}>
        {#snippet hints()}<kbd>Esc</kbd> close{/snippet}
      </SignalDetail>
    </div>
  </Modal>
{/if}

<style>
  .box {
    position: fixed;
    left: 50%;
    top: 12vh;
    transform: translateX(-50%);
    width: min(720px, calc(100vw - 80px));
    max-height: 76vh;
    overflow: auto;
    padding: 12px 14px;
  }
</style>

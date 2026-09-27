<script lang="ts">
  // macOS title bar (11a). In the Tauri window on macOS the real traffic lights
  // overlay the left edge (titleBarStyle: Overlay); in a browser we draw them.
  const inTauri = '__TAURI_INTERNALS__' in window;
</script>

<div class="titlebar" data-tauri-drag-region>
  <span class="lights" data-tauri-drag-region>
    {#if !inTauri}<span style:background="var(--red)"></span><span style:background="var(--yellow)"></span><span style:background="var(--green)"></span>{/if}
  </span>
  <span class="title" data-tauri-drag-region><i class="ph ph-compass"></i>Helm</span>
  <span class="spacer"></span>
</div>

<style>
  .titlebar {
    height: 32px;
    flex: none;
    display: flex;
    align-items: center;
    padding: 0 12px;
    font-size: 12px;
    color: var(--tx-muted-2);
    border-bottom: 1px solid var(--bg-head);
    user-select: none;
    -webkit-user-select: none;
  }
  .lights {
    display: flex;
    gap: 8px;
    width: 52px;
  }
  .lights span {
    width: 12px;
    height: 12px;
    border-radius: 50%;
    opacity: 0.85;
  }
  .title {
    flex: 1;
    text-align: center;
    color: var(--tx-2);
    display: flex;
    gap: 7px;
    justify-content: center;
    align-items: center;
  }
  .title i {
    font-size: 14px;
    color: var(--aqua);
  }
  .spacer {
    width: 52px;
  }
</style>

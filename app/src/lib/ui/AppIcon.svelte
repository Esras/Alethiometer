<script lang="ts">
  // Desaturated app icons supplied by the user, in public/icons/<source>.png.
  let { source, size = 16 }: { source: string; size?: number } = $props();
</script>

{#if source === 'agents'}
  <!-- Built in, so no supplied icon: Phosphor's robot on the same tile. -->
  <span class="ph-tile" style:width="{size}px" style:height="{size}px" style:font-size="{Math.round(size * 0.78)}px"><i class="ph ph-robot"></i></span>
{:else}
  <img src="icons/{source}.png" alt="" width={size} height={size} style:width="{size}px" style:height="{size}px" />
{/if}

<style>
  .ph-tile {
    display: flex;
    flex: none;
    align-items: center;
    justify-content: center;
    border-radius: 4px;
    background: var(--bg-head);
    color: var(--tx-2);
  }
  img {
    display: block;
    flex: none;
    border-radius: 4px;
  }
</style>

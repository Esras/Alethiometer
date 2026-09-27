<script lang="ts">
  // Desaturated app icons supplied by the user, in public/icons/<source>.png.
  let { source, size = 16 }: { source: string; size?: number } = $props();
</script>

<img src="icons/{source}.png" alt="" width={size} height={size} style:width="{size}px" style:height="{size}px" />

<style>
  img {
    display: block;
    flex: none;
    border-radius: 4px;
  }
</style>

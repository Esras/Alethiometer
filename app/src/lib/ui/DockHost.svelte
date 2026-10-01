<script lang="ts">
  // Where `<Dock>` content floats: bottom-left of the panel it sits in (which
  // must be `position: relative`), `bottom` px above its bottom edge (clear
  // of the key hints).
  let { bottom = 44, left = 38 }: { bottom?: number; left?: number } = $props();
</script>

<div class="host" data-dock-host style:bottom="{bottom}px" style:left="{left}px"></div>

<style>
  .host {
    position: absolute;
    width: 0;
    height: 0;
    pointer-events: none;
  }
</style>

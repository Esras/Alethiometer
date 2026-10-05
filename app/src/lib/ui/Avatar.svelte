<script lang="ts">
  // The round initials avatar that leads a row about a person (Topics' People
  // panel, the person picker, Glean's people). Never used in threads.
  // `size` is px; `font` is the initials' size, set per size until the design
  // system's two sizes (26 and 18) replace the three in use.
  let { initials, color, size, font }: { initials: string; color: string; size: number; font: string } = $props();
</script>

<span class="avatar" style:background={color} style:width="{size}px" style:height="{size}px" style:font-size={font}>{initials}</span>

<style>
  .avatar {
    flex: none;
    border-radius: 50%;
    display: flex;
    align-items: center;
    justify-content: center;
    font-weight: 600;
    color: var(--bg-app);
  }
</style>

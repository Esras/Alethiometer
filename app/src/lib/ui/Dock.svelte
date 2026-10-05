<script lang="ts">
  // Pickers (move to topic, link to person) always open in one fixed spot: the
  // view's dock, at the bottom of its panel just above the key hints, floating
  // over the list. Nothing shifts and there's nothing to hunt for. A view puts
  // one `<DockHost />` in its panel; `<Dock>` floats its content there. (It's
  // positioned onto the host rather than moved into it: moving the node would
  // blur the picker's field, which closes it.)
  import type { Snippet } from 'svelte';

  let { children }: { children: Snippet } = $props();

  // Placed before the first paint (the host is already on screen), so the
  // picker's field can take focus as it mounts.
  function spot() {
    const r = document.querySelector('[data-dock-host]')?.getBoundingClientRect();
    return { left: r?.left ?? 16, bottom: r ? window.innerHeight - r.bottom : 16 };
  }
  let pos = $state(spot());

  $effect(() => {
    const place = () => (pos = spot());
    window.addEventListener('resize', place);
    return () => window.removeEventListener('resize', place);
  });
</script>

<div class="dock" style:left="{pos.left}px" style:bottom="{pos.bottom}px">{@render children()}</div>

<style>
  .dock {
    position: fixed;
    z-index: 30;
  }
</style>

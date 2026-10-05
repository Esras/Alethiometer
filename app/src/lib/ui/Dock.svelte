<script lang="ts">
  // Where a picker (move to topic, link to person, merge into) floats.
  //   `under` (a selector for the row it acts on): just below that row, lined up
  //   with its left edge, so your eyes stay on the thing being moved (user,
  //   2026-10-05). Above the row instead when there's no room below. It follows
  //   the row as the list scrolls.
  //   No `under`, or the row isn't on screen: the view's dock, at the bottom of
  //   its panel just above the key hints (one `<DockHost />` per view).
  // It's positioned onto the spot rather than moved into it: moving the node
  // would blur the picker's field, which closes it.
  import type { Snippet } from 'svelte';

  let { under, children }: { under?: string; children: Snippet } = $props();

  const GAP = 4;
  let box = $state<HTMLDivElement>();

  type Pos = { left: number; top?: number; bottom?: number };
  function spot(): Pos {
    const row = under ? document.querySelector(under)?.getBoundingClientRect() : undefined;
    if (row && row.bottom > 0 && row.top < window.innerHeight) {
      const h = box?.offsetHeight ?? 0;
      const left = Math.max(8, Math.min(row.left, window.innerWidth - (box?.offsetWidth ?? 0) - 8));
      if (row.bottom + GAP + h <= window.innerHeight - 8) return { left, top: row.bottom + GAP };
      return { left, bottom: window.innerHeight - row.top + GAP };
    }
    const r = document.querySelector('[data-dock-host]')?.getBoundingClientRect();
    return { left: r?.left ?? 16, bottom: r ? window.innerHeight - r.bottom : 16 };
  }
  // Placed before the first paint (the row or host is already on screen), so the
  // picker's field can take focus as it mounts.
  let pos = $state(spot());

  $effect(() => {
    const place = () => (pos = spot());
    // Once mounted its size is known: flip above the row if it doesn't fit below.
    place();
    window.addEventListener('resize', place);
    window.addEventListener('scroll', place, true);
    return () => {
      window.removeEventListener('resize', place);
      window.removeEventListener('scroll', place, true);
    };
  });
</script>

<div
  class="dock"
  bind:this={box}
  style:left="{pos.left}px"
  style:top={pos.top === undefined ? null : `${pos.top}px`}
  style:bottom={pos.bottom === undefined ? null : `${pos.bottom}px`}>
  {@render children()}
</div>

<style>
  .dock {
    position: fixed;
    z-index: 30;
  }
</style>

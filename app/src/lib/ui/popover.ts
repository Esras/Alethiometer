// Popover behaviour, as a Svelte action on the popover's own element:
//   <div class="pop" use:popover={{ onclose, trigger }}>
// While the element exists, Escape closes it (through the key stack, so the
// newest popover closes first and every other key still reaches the view) and
// so does a pointerdown outside it and its trigger. The trigger is left out so
// its own click can toggle: a pointerdown on it would close the popover and
// the click would open it again.
//
// An action rather than a component: it adds no wrapper element, so it can't
// change layout, and each site keeps its own positioning and look. The design
// system's one popover style (ui-plan decision 5) comes later.
import { pushKeys } from '../keys.svelte';

type Trigger = Element | null | undefined;

export type PopoverOpts = {
  onclose: () => void;
  /** What opened it: the element, or a function when it's one of several. */
  trigger?: Trigger | (() => Trigger);
};

export function popover(node: HTMLElement, opts: PopoverOpts) {
  let cur = opts;
  const pop = pushKeys((e) => {
    if (e.key !== 'Escape') return;
    cur.onclose();
    return true;
  });
  // Capture, so an element that stops the event's propagation can't hide it.
  const down = (e: PointerEvent) => {
    const t = e.target;
    if (!(t instanceof Node) || node.contains(t)) return;
    const trigger = typeof cur.trigger === 'function' ? cur.trigger() : cur.trigger;
    if (trigger?.contains(t)) return;
    cur.onclose();
  };
  document.addEventListener('pointerdown', down, true);
  return {
    update(next: PopoverOpts) {
      cur = next;
    },
    destroy() {
      pop();
      document.removeEventListener('pointerdown', down, true);
    },
  };
}

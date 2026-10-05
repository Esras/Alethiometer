// The keys that move a cursor along a list, written once. Each site keeps its
// own cursor and what it does on arrival; this only reads the key.
import { plain } from '../keys.svelte';

/**
 * +1 for ↓, -1 for ↑, else 0. For a field that has the focus (j and k type).
 * `bare`: also j and k, with no modifier and not typing, for a list that has
 * the focus itself. The list a level up moves with [ and ] (Incoming, Reflect).
 */
export function listNav(e: KeyboardEvent, opts: { bare?: boolean } = {}): -1 | 0 | 1 {
  if (opts.bare) return plain(e, 'j') || plain(e, 'ArrowDown') ? 1 : plain(e, 'k') || plain(e, 'ArrowUp') ? -1 : 0;
  if (e.key === 'ArrowDown') return 1;
  if (e.key === 'ArrowUp') return -1;
  return 0;
}

/** The cursor after a step of `d` in a list of `n` rows. It stops at both ends, it doesn't wrap. */
export function moveSel(sel: number, d: number, n: number): number {
  return Math.max(0, Math.min(sel + d, n - 1));
}

// Shift+j / shift+k in message lists (the split review, Unsorted's message
// mode): a range selection like text. The range runs from the anchor (where
// the shift run started) to the cursor; going back shrinks it, and past the
// anchor it grows the other way. Rows selected before the run (space) stay.

export interface Range {
  anchor: number;
  /** The selection when the run started. */
  base: Set<number>;
}

/** Start a run at `focus` (if none) and return the selection for the cursor now at `to`. */
export function extend(order: number[], run: Range | null, focus: number, to: number, chosen: Set<number>): { run: Range; chosen: Set<number> } {
  const r = run ?? { anchor: focus, base: new Set(chosen) };
  const a = order.indexOf(r.anchor);
  const b = order.indexOf(to);
  const next = new Set(r.base);
  if (a >= 0 && b >= 0) for (let i = Math.min(a, b); i <= Math.max(a, b); i++) next.add(order[i]);
  return { run: r, chosen: next };
}

/** Shift+j / shift+k (or shift+arrow): +1 / -1, else 0. */
export function shiftStep(e: KeyboardEvent): number {
  if (!e.shiftKey || e.metaKey || e.ctrlKey || e.altKey) return 0;
  const t = e.target as HTMLElement | null;
  if (t && (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA' || t.isContentEditable)) return 0;
  if (e.key === 'J' || e.key === 'ArrowDown') return 1;
  if (e.key === 'K' || e.key === 'ArrowUp') return -1;
  return 0;
}

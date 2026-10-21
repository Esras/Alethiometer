/** The list to select once the selected one (`sel`, at its place in `before`)
 *  has gone: the next one that's left, a new one included, else the last. */
export function nextKey<K>(before: K[], sel: K | null, now: K[]): K | undefined {
  const at = sel === null ? -1 : before.indexOf(sel);
  return now.find((k) => before.indexOf(k) > at || !before.includes(k)) ?? now[now.length - 1];
}

/** Dismissed rows still to show as dismissed: listed, and not back with new activity since (a higher rev). */
export function stillDismissed(dismissed: Set<number>, revAt: Map<number, number>, listed: Map<number, number>) {
  return new Set([...dismissed].filter((id) => listed.has(id) && !(revAt.has(id) && listed.get(id)! > revAt.get(id)!)));
}

/** Put `id` back before the first row that followed it in `from` and is still in `list`. */
export function putBack<T extends { item_id: number }>(list: T[], from: T[], id: number) {
  const i = from.findIndex((s) => s.item_id === id);
  const listed = new Set(list.map((s) => s.item_id));
  const after = from.slice(i + 1).find((s) => listed.has(s.item_id));
  const at = after ? list.findIndex((s) => s.item_id === after.item_id) : list.length;
  list.splice(at, 0, from[i]);
}

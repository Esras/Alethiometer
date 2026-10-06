/** The list to select once the selected one (`sel`, at its place in `before`)
 *  has gone: the next one that's left, a new one included, else the last. */
export function nextKey<K>(before: K[], sel: K | null, now: K[]): K | undefined {
  const at = sel === null ? -1 : before.indexOf(sel);
  return now.find((k) => before.indexOf(k) > at || !before.includes(k)) ?? now[now.length - 1];
}

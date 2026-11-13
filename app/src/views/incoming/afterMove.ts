/** Where the split review's cursor goes once `moved` went to another group: the next message in time
 *  order that didn't move (and isn't dismissed), else the nearest one before them; else it stays. */
export function afterMove(order: number[], moved: Set<number>, skip: (id: number) => boolean, focus: number | null) {
  const free = (id: number) => !moved.has(id) && !skip(id);
  const last = order.reduce((at, id, i) => (moved.has(id) ? i : at), -1);
  if (last < 0) return focus;
  const next = order.slice(last + 1).find(free);
  if (next !== undefined) return next;
  const first = order.findIndex((id) => moved.has(id));
  return order.slice(0, first).reverse().find(free) ?? focus;
}

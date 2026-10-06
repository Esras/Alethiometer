export type Pos = { left: number; top?: number; bottom?: number };
type Rect = { left: number; top: number; bottom: number };

const GAP = 4;
const MARGIN = 8;

/** Where a picker of size `w`×`h` goes for a row: below it, else above it,
 *  else (a tall row or a short window) as low as it fits, over the row. */
export function placeByRow(row: Rect, w: number, h: number, vw: number, vh: number): Pos {
  const left = Math.max(MARGIN, Math.min(row.left, vw - w - MARGIN));
  if (row.bottom + GAP + h <= vh - MARGIN) return { left, top: row.bottom + GAP };
  if (row.top - GAP - h >= MARGIN) return { left, bottom: vh - row.top + GAP };
  return { left, top: Math.max(MARGIN, vh - MARGIN - h) };
}

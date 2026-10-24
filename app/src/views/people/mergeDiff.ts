// The rows of the merge dialog (design canvas `MergeModal`): the merged-away
// note on the left, the kept note after the merge on the right, each left line
// beside the line it ends up in. Right lines are marked `+` (new), `~` (a field
// the kept note had, now changed) or `≠` (a field both notes set differently:
// the user picks a side). The merge itself is computed by the core
// (`people.merge_preview`); this only lines the two texts up.

export type Mark = '' | '+' | '~' | '≠';

export interface Line {
  n: number;
  text: string;
}

export interface DiffRow {
  left: Line | null;
  right: (Line & { mark: Mark; key: string | null }) | null;
}

const lines = (t: string) => t.replace(/\r\n/g, '\n').replace(/\n$/, '').split('\n');

/** The closing `---` of a leading frontmatter block, or -1. */
function fenceEnd(ls: string[]): number {
  if (ls[0]?.replace(/^﻿/, '').trimEnd() !== '---') return -1;
  return ls.findIndex((l, i) => i > 0 && (l.trimEnd() === '---' || l.trimEnd() === '...'));
}

/** A top-level `key:` line's key. */
function keyOf(l: string): string | null {
  if (/^\s/.test(l) || l.startsWith('#') || l.startsWith('- ')) return null;
  const i = l.indexOf(':');
  return i > 0 ? l.slice(0, i).trim() : null;
}

export function diffRows(keep: string, gone: string, merged: string, conflicts: string[]): DiffRow[] {
  const k = lines(keep);
  const g = lines(gone);
  const m = lines(merged);
  const kEnd = fenceEnd(k);
  const gEnd = fenceEnd(g);
  const mEnd = fenceEnd(m);
  const kept = new Set(k.slice(0, kEnd + 1));
  const keptKeys = new Set(
    k
      .slice(1, Math.max(kEnd, 0))
      .map(keyOf)
      .filter((x) => x !== null),
  );
  // The merged body is the kept body, then what was appended.
  const appendedFrom = mEnd + 1 + (k.length - (kEnd + 1));

  // Each right line: its mark, and its key when it's a field.
  let owner: string | null = null;
  const right = m.map((text, i) => {
    let mark: Mark = '';
    let key: string | null = null;
    if (i > 0 && i < mEnd) {
      key = keyOf(text);
      if (key !== null) owner = key;
      else key = owner;
      if (key !== null && conflicts.includes(key)) mark = '≠';
      else if (!kept.has(text)) mark = keyOf(text) !== null && keptKeys.has(keyOf(text)!) ? '~' : '+';
    } else if (mEnd >= 0 ? i >= appendedFrom : i >= k.length) {
      mark = '+';
    }
    return { n: i + 1, text, mark, key };
  });

  // Where each left line went.
  const at = new Map<number, number[]>();
  const put = (r: number, l: number) => at.set(r, [...(at.get(r) ?? []), l]);
  const fieldLine = (key: string) => right.findIndex((r, i) => i > 0 && i < mEnd && keyOf(r.text) === key);
  let gOwner: string | null = null;
  let body = right.findIndex((r) => r.mark === '+' && r.text.startsWith('## Merged from ')) + 1;
  g.forEach((text, i) => {
    if (gEnd >= 0 && (i === 0 || i === gEnd)) {
      if (mEnd >= 0) put(i === 0 ? 0 : mEnd, i);
      return;
    }
    if (gEnd >= 0 && i < gEnd) {
      const key = keyOf(text);
      if (key !== null) gOwner = key;
      const r = gOwner === null ? -1 : fieldLine(gOwner);
      if (r >= 0) put(r, i);
      return;
    }
    if (!text.trim() || body <= 0) return;
    while (body < right.length && right[body].text !== text) body++;
    if (body < right.length) put(body++, i);
  });

  const rows: DiffRow[] = [];
  const placed = new Set<number>();
  right.forEach((r, i) => {
    const ls = at.get(i) ?? [];
    rows.push({ left: ls.length ? { n: ls[0] + 1, text: g[ls[0]] } : null, right: r });
    for (const l of ls.slice(1)) rows.push({ left: { n: l + 1, text: g[l] }, right: null });
    ls.forEach((l) => placed.add(l));
  });
  g.forEach((text, i) => {
    if (!placed.has(i) && text.trim()) rows.push({ left: { n: i + 1, text }, right: null });
  });
  return rows;
}

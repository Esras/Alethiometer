// What the thread draws: rows, with runs of messages between the same two
// agents folded to their last message ("N earlier · E"). The latest run stays
// open, in one box.

import type { Kind, Row } from './types';

export type Item = { key: string; row: Row; run?: Row[]; folded?: boolean };

/** Legend chips per thread kind: label, the kinds it filters, icon, box class. */
export const LEGEND: Record<'local' | 'scion' | 'room', { label: string; kinds: string[]; icon: string; box: string; color: string }[]> = {
  local: [
    { label: 'You', kinds: ['you'], icon: 'ph-user', box: 'you', color: '#ebdbb2' },
    { label: 'Output', kinds: ['output'], icon: 'ph-text-align-left', box: '', color: 'var(--tx-2)' },
    { label: 'Thinking', kinds: ['thinking'], icon: 'ph-brain', box: '', color: '#7c6f64' },
    { label: 'Tool calls', kinds: ['tool'], icon: 'ph-wrench', box: 'tool', color: 'var(--tool)' },
    { label: 'Results', kinds: ['result'], icon: 'ph-arrow-elbow-down-right', box: '', color: 'var(--tx-label)' },
    { label: 'Messages', kinds: ['message'], icon: 'ph-arrows-left-right', box: 'msg', color: 'var(--msg)' },
    { label: 'State', kinds: ['state'], icon: 'ph-swap', box: '', color: 'var(--tx-label)' },
    { label: 'Approvals', kinds: ['approval'], icon: 'ph-shield-warning', box: 'need', color: 'var(--need)' },
  ],
  scion: [
    { label: 'You', kinds: ['you'], icon: 'ph-user', box: 'you', color: '#ebdbb2' },
    { label: 'Output', kinds: ['output'], icon: 'ph-text-align-left', box: '', color: 'var(--tx-2)' },
    { label: 'Thinking', kinds: ['thinking'], icon: 'ph-brain', box: '', color: '#7c6f64' },
    { label: 'Tool calls', kinds: ['tool'], icon: 'ph-wrench', box: 'tool', color: 'var(--tool)' },
    { label: 'Results', kinds: ['result'], icon: 'ph-arrow-elbow-down-right', box: '', color: 'var(--tx-label)' },
    { label: 'Messages', kinds: ['message'], icon: 'ph-arrows-left-right', box: 'msg', color: 'var(--msg)' },
    { label: 'State', kinds: ['state'], icon: 'ph-swap', box: '', color: 'var(--tx-label)' },
    { label: 'Hub', kinds: ['hub'], icon: 'ph-info', box: 'hub', color: 'var(--hub)' },
    { label: 'Asks', kinds: ['ask', 'approval'], icon: 'ph-question', box: 'need', color: 'var(--need)' },
  ],
  room: [
    { label: 'You', kinds: ['you'], icon: 'ph-user', box: 'you', color: '#ebdbb2' },
    { label: 'Messages', kinds: ['message'], icon: 'ph-arrows-left-right', box: 'msg', color: 'var(--msg)' },
    { label: 'State', kinds: ['state'], icon: 'ph-swap', box: '', color: 'var(--tx-label)' },
    { label: 'Hub', kinds: ['hub'], icon: 'ph-info', box: 'hub', color: 'var(--hub)' },
    { label: 'Asks', kinds: ['ask'], icon: 'ph-question', box: 'need', color: 'var(--need)' },
  ],
};

/** The two agents a message is between, as one key, or null. */
function pair(r: Row): string | null {
  if (r.kind !== 'message' || !r.who || !r.to) return null;
  if (r.who.kind !== 'agent' || r.to.kind !== 'agent') return null;
  return [r.who.name, r.to.name].sort().join('↔');
}

export function items(rows: Row[], hidden: Set<string>, expanded: Set<string>, foldPairs: boolean): Item[] {
  const shown = rows.filter((r) => !hidden.has(r.kind as Kind));
  // Runs of 2+ consecutive messages between the same two agents.
  const runs: { start: number; end: number }[] = [];
  for (let i = 0; i < shown.length; ) {
    const p = pair(shown[i]);
    let j = i + 1;
    if (p) while (j < shown.length && pair(shown[j]) === p) j++;
    if (p && j - i > 1) runs.push({ start: i, end: j });
    i = j;
  }
  const lastRun = runs.at(-1);
  const out: Item[] = [];
  let ri = 0;
  for (let i = 0; i < shown.length; i++) {
    const run = runs[ri];
    if (run && i === run.start) {
      const rs = shown.slice(run.start, run.end);
      const key = `run:${rs[0].id}`;
      const open = run === lastRun || !foldPairs || expanded.has(key);
      out.push({ key, row: rs[rs.length - 1], run: rs, folded: !open });
      i = run.end - 1;
      ri++;
      continue;
    }
    out.push({ key: `row:${shown[i].id}`, row: shown[i] });
  }
  return out;
}

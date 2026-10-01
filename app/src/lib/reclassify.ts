// Reclassifying a filed signal (18c chronology, 18d search). `t` moves the
// whole signal: its old topic link becomes `wrong`, the new one `confirmed`
// (filed straight away; one topic per signal). The old spot shows "moved to X
// · u undo" until you leave the view; `undo` puts every link back.
import { call } from './api';

export interface Moved {
  item_id: number;
  from_state: string | null;
  to_state: string | null;
  seen: boolean;
}

export interface Move {
  itemId: number;
  from: number | null;
  to: number;
  toName: string;
  moved: Moved[];
}

export async function moveSignal(itemId: number, from: number | null, to: number, toName: string): Promise<Move> {
  const r = await call<{ moved: Moved[] }>('incoming.reclassify', { item_id: itemId, from_topic_id: from, to_topic_id: to });
  return { itemId, from, to, toName, moved: r.moved };
}

export async function undoMove(m: Move): Promise<void> {
  await call('incoming.unreclassify', { moved: m.moved, from_topic_id: m.from, to_topic_id: m.to });
}

/** Shape of `incoming.detail`: an entry expanded in place. */
export interface Detail {
  item_id: number;
  source: string;
  url: string | null;
  summary: string | null;
  msg_count: number | null;
  messages: import('../views/incoming/types').Message[];
  context: import('../views/incoming/types').Message | null;
}

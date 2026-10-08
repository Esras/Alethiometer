// Live queries (app/docs/db-plan.md, Phase 4): subscribe to a read command
// and get its result again whenever a write changes what it read. The core
// tracks the tables each query read (`src-tauri/src/live.rs`); this side
// keeps one `live.wait` long poll open and hands each new result to its
// subscription.

import { call } from './api';

type Sub = {
  cmd: string;
  args: Record<string, unknown>;
  onResult: (r: never) => void;
  onError?: (e: unknown) => void;
  id: number | null;
  /** Orders results: an older one never replaces a newer one. */
  seq: number;
  closed: boolean;
};

const client =
  typeof crypto !== 'undefined' && 'randomUUID' in crypto
    ? crypto.randomUUID()
    : `c${Math.random().toString(36).slice(2)}${Date.now()}`;
const subs = new Map<number, Sub>();
/** Results that came by `live.wait` before their `live.subscribe` returned. */
const early = new Map<number, { seq: number; result: unknown }>();
let since = 0;
let waiting = false;

function deliver(s: Sub, seq: number, result: unknown) {
  if (s.closed || seq < s.seq) return;
  s.seq = seq;
  s.onResult(result as never);
}

async function open(s: Sub) {
  try {
    const r = await call<{ id: number; result: unknown; seq: number }>('live.subscribe', {
      client,
      cmd: s.cmd,
      args: s.args,
    });
    if (s.closed) {
      early.delete(r.id);
      call('live.unsubscribe', { id: r.id }).catch(() => {});
      return;
    }
    s.id = r.id;
    subs.set(r.id, s);
    deliver(s, r.seq, r.result);
    const e = early.get(r.id);
    early.delete(r.id);
    if (e) deliver(s, e.seq, e.result);
  } catch (e) {
    console.error(`live ${s.cmd}:`, e);
    if (!s.closed) s.onError?.(e);
  }
  startWaiting();
}

async function startWaiting() {
  if (waiting) return;
  waiting = true;
  let pause = 1000;
  while (subs.size > 0) {
    try {
      const r = await call<{ seq: number; changed: { id: number; result: unknown; seq: number }[]; gone: number[] }>(
        'live.wait',
        { client, since, ids: [...subs.keys()] },
      );
      pause = 1000;
      since = r.seq;
      for (const c of r.changed) {
        const s = subs.get(c.id);
        if (s) deliver(s, c.seq, c.result);
        else if (early.size < 100) early.set(c.id, { seq: c.seq, result: c.result });
      }
      // Dropped while this window slept (or the core restarted, and counts
      // from zero again): ask again.
      if (r.gone.length) since = 0;
      for (const id of r.gone) {
        const s = subs.get(id);
        subs.delete(id);
        if (s && !s.closed) {
          s.id = null;
          s.seq = 0;
          void open(s);
        }
      }
    } catch {
      await new Promise((ok) => setTimeout(ok, pause));
      pause = Math.min(pause * 2, 30_000);
    }
  }
  waiting = false;
}

/** Run `cmd` now and again whenever its result changes; returns the
 *  unsubscribe, so it can be an `$effect`'s return value. `onError` hears
 *  a subscription the core refused (bad arguments, say). */
export function live<T>(
  cmd: string,
  args: Record<string, unknown>,
  onResult: (r: T) => void,
  onError?: (e: unknown) => void,
): () => void {
  const s: Sub = { cmd, args, onResult: onResult as (r: never) => void, onError, id: null, seq: 0, closed: false };
  void open(s);
  return () => {
    s.closed = true;
    if (s.id !== null) {
      subs.delete(s.id);
      call('live.unsubscribe', { id: s.id }).catch(() => {});
    }
  };
}

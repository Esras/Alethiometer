// The Agents tab's client state: the tree and the open thread, kept current
// by polling `agents.poll` with the last revision seen (the core streams into
// an in-memory store; see src-tauri/src/agents/store.rs).

import { call } from '../../lib/api';
import { ui } from '../../lib/app.svelte';
import { DEFAULT_LOOK } from './look';
import type { Group, Kind, Request, Row, Session, ThreadData, Tree } from './types';

export type Zone = 'tree' | 'thread' | 'composer';
export type Node =
  | { type: 'project'; key: string; label: string }
  | { type: 'group'; key: string; group: Group; open: boolean; sessions: Session[] }
  | { type: 'room'; key: string; group: Group }
  | { type: 'session'; key: string; s: Session; depth: number; unsorted: boolean }
  | { type: 'unsorted'; key: string };

export type Form = { kind: 'scion'; group: string; room: string | null } | { kind: 'local'; group: string | null };

export const ag = $state({
  tree: null as Tree | null,
  rev: 0,
  /** Tree selection: `g:<group id>` or a thread key (`s:<session>`, `r:<room>`). */
  sel: null as string | null,
  /** The thread shown on the right (the last session or room opened). */
  open: null as string | null,
  zone: 'tree' as Zone,
  tab: 'transcript' as 'transcript' | 'messages',
  rows: [] as Row[],
  threadRev: 0,
  threadKey: '',
  /** Selected row in the thread (j/k), or null while following the latest. */
  rowSel: null as number | null,
  /** Rows opened with e (thinking, long output) or folded runs opened. */
  expanded: new Set<string>(),
  hidden: new Set<Kind>(),
  /** Requests put off with Later (Esc); they stay under Needs you. */
  later: new Set<string>(),
  folded: new Set<string>(),
  form: null as Form | null,
  keys: false,
  filter: '',
  error: null as string | null,
  /** Composer text per thread, and an ask being answered with R. */
  drafts: {} as Record<string, string>,
  replying: null as string | null,
});

let timer: ReturnType<typeof setInterval> | null = null;
let busy = false;

export function startPolling() {
  if (timer) return;
  poll();
  timer = setInterval(poll, 400);
}

export function stopPolling() {
  if (timer) clearInterval(timer);
  timer = null;
}

export async function poll() {
  if (busy) return;
  busy = true;
  const key = ag.open;
  try {
    const r = await call<{ rev: number; tree: Tree | null; thread: ThreadData | null }>('agents.poll', {
      rev: ag.rev,
      thread: key ?? '',
      thread_rev: key === ag.threadKey ? ag.threadRev : 0,
      tab: ag.tab,
    });
    if (r.tree) {
      ag.tree = r.tree;
      if (ui.info) ui.info.agents_waiting = r.tree.requests.length;
    }
    ag.rev = r.rev;
    if (r.thread && r.thread.key === ag.open && r.thread.tab === ag.tab) merge(r.thread);
    ag.error = null;
  } catch (e) {
    ag.error = String((e as Error).message ?? e);
  } finally {
    busy = false;
  }
}

function merge(t: ThreadData) {
  if (t.reset || t.key !== ag.threadKey) {
    ag.rows = t.rows;
  } else if (t.rows.length) {
    const byId = new Map(ag.rows.map((r) => [r.id, r]));
    for (const r of t.rows) byId.set(r.id, r);
    const keep = new Set(t.ids);
    ag.rows = [...byId.values()].filter((r) => keep.has(r.id)).sort((a, b) => a.id - b.id);
  } else if (t.ids.length !== ag.rows.length) {
    const keep = new Set(t.ids);
    ag.rows = ag.rows.filter((r) => keep.has(r.id));
  }
  ag.threadKey = t.key;
  ag.threadRev = t.rev;
}

/** Open a thread on the right. */
export function openThread(key: string) {
  if (ag.open !== key) {
    ag.open = key;
    ag.rows = [];
    ag.threadRev = 0;
    ag.threadKey = '';
    ag.rowSel = null;
    ag.tab = 'transcript';
    ag.replying = null;
    if (ag.form && !formFits(key)) ag.form = null;
    poll();
  }
}

function formFits(key: string): boolean {
  const f = ag.form;
  if (!f) return false;
  if (f.kind === 'scion') return f.room ? key === `r:${f.room}` : false;
  return false;
}

export function setTab(tab: 'transcript' | 'messages') {
  if (ag.tab === tab) return;
  ag.tab = tab;
  ag.rows = [];
  ag.threadRev = 0;
  ag.threadKey = '';
  ag.rowSel = null;
  poll();
}

export const look = () => ui.info?.agents ?? DEFAULT_LOOK;

export function sessionById(id: string | null | undefined): Session | undefined {
  if (!id) return undefined;
  return ag.tree?.sessions.find((s) => s.id === id);
}

export function groupById(id: string | null | undefined): Group | undefined {
  if (!id) return undefined;
  return ag.tree?.groups.find((g) => g.id === id);
}

export function roomGroup(roomId: string): Group | undefined {
  return ag.tree?.groups.find((g) => g.room?.id === roomId);
}

/** The session or room a thread key names. */
export function threadOf(key: string | null): { session?: Session; group?: Group } {
  if (!key) return {};
  if (key.startsWith('s:')) {
    const s = sessionById(key.slice(2));
    return { session: s, group: groupById(s?.group_id) };
  }
  if (key.startsWith('r:')) return { group: roomGroup(key.slice(2)) };
  return {};
}

/** Requests pinned in a thread, oldest first; Later'd ones are skipped. */
export function pinned(key: string | null): Request | undefined {
  if (!key || !ag.tree) return undefined;
  const id = key.startsWith('s:') ? key.slice(2) : null;
  return ag.tree.requests.find((r) => !ag.later.has(r.id) && (r.thread === key || (id && r.session_id === id && r.thread.startsWith('s:'))));
}

function matches(s: Session, q: string): boolean {
  return [s.name, s.title, s.cwd, s.harness, s.model].some((x) => x?.toLowerCase().includes(q));
}

/** The tree as a flat list of rows, for drawing and for j/k. */
export function nodes(): Node[] {
  const t = ag.tree;
  if (!t) return [];
  const q = ag.filter.trim().toLowerCase();
  const out: Node[] = [];
  const kids = (parent: Session, depth: number, list: Session[]) => {
    for (const c of t.sessions.filter((s) => s.parent_id === parent.id)) {
      list.push(c);
      if (depth < 3) kids(c, depth + 1, list);
    }
  };
  const ordered = (group: string | null) => {
    const list: Session[] = [];
    for (const s of t.sessions.filter((s) => (s.group_id ?? null) === group && !s.parent_id)) {
      list.push(s);
      kids(s, 1, list);
    }
    return list;
  };
  const depth = (s: Session) => {
    let d = 0;
    let p = s.parent_id;
    while (p && d < 4) {
      d++;
      p = sessionById(p)?.parent_id ?? null;
    }
    return d;
  };
  let project: string | null = null;
  for (const g of t.groups) {
    const sessions = ordered(g.id);
    const hit = !q || g.name.toLowerCase().includes(q) || g.goal.toLowerCase().includes(q) || sessions.some((s) => matches(s, q));
    if (!hit) continue;
    const label = g.project || (g.source === 'acp' ? 'local' : 'scion');
    if (label !== project) {
      out.push({ type: 'project', key: `p:${label}:${g.id}`, label });
      project = label;
    }
    const open = !ag.folded.has(g.id) || !!q;
    out.push({ type: 'group', key: `g:${g.id}`, group: g, open, sessions });
    if (!open) continue;
    if (g.room) out.push({ type: 'room', key: `r:${g.room.id}`, group: g });
    for (const s of sessions) if (!q || matches(s, q) || g.name.toLowerCase().includes(q)) out.push({ type: 'session', key: `s:${s.id}`, s, depth: depth(s), unsorted: false });
  }
  const loose = ordered(null).filter((s) => !q || matches(s, q));
  if (loose.length) {
    out.push({ type: 'unsorted', key: 'unsorted' });
    for (const s of loose) out.push({ type: 'session', key: `s:${s.id}`, s, depth: depth(s), unsorted: true });
  }
  return out;
}

/** Nodes j/k can land on. */
export const selectable = (n: Node) => n.type === 'group' || n.type === 'room' || n.type === 'session';

/** "4s", "2m", "3h", "2d" since a local timestamp. */
export function age(ts: string, now: number): string {
  if (!ts) return '';
  const t = new Date(ts).getTime();
  const s = Math.max(0, Math.round((now - t) / 1000));
  if (s < 60) return `${s}s`;
  if (s < 3600) return `${Math.floor(s / 60)}m`;
  if (s < 86400) return `${Math.floor(s / 3600)}h`;
  return `${Math.floor(s / 86400)}d`;
}

export const hm = (ts: string) => ts.slice(11, 16);

export async function act(cmd: string, args: Record<string, unknown>) {
  try {
    const r = await call(`agents.${cmd}`, args);
    ag.error = null;
    poll();
    return r;
  } catch (e) {
    ag.error = String((e as Error).message ?? e);
    throw e;
  }
}

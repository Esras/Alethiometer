// The Agents tab's client state: the tree and the open thread, kept current
// by polling `agents.poll` with the last revision seen (the core streams into
// an in-memory store; see src-tauri/src/agents/store.rs).

import { call } from '../../lib/api';
import { ui } from '../../lib/app.svelte';
import { DEFAULT_LOOK } from './look';
import type { ArchivedItem, ArchivedOpen, Group, Kind, Plan, Request, Row, Session, ThreadData, Tree } from './types';

export type Zone = 'tree' | 'thread' | 'composer';
export type Node =
  | { type: 'project'; key: string; label: string }
  | { type: 'group'; key: string; group: Group; open: boolean; sessions: Session[] }
  | { type: 'room'; key: string; group: Group }
  | { type: 'session'; key: string; s: Session; depth: number; unsorted: boolean }
  | { type: 'unsorted'; key: string }
  | { type: 'archived-head'; key: string; groups: number; sessions: number; total: number }
  | { type: 'archived-label'; key: string; label: string; project: boolean }
  | { type: 'archived'; key: string; item: ArchivedItem };

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
  /** Archived items matching the filter, and how many sessions are archived. */
  hits: [] as ArchivedItem[],
  hitsTotal: 0,
  /** An archived item opened read-only, and the filter it was found with. */
  archived: null as ArchivedOpen | null,
  archivedQ: '',
  /** The archive confirm (02). */
  plan: null as Plan | null,
  /** Archived but still in the undo window (03), newest last. */
  pending: [] as Pending[],
  /** The right-click menu (05). */
  menu: null as { key: string; x: number; y: number } | null,
});

/** A local session that ended a turn since you looked (the core keeps the count, so the tab badge sees it too). */
export function turnEnded(s: Session): boolean {
  return s.source === 'acp' && s.status === 'idle' && s.seen_turns != null && s.stats.turns > s.seen_turns;
}

/** Looking at a session's thread counts as seeing its turns. */
function markSeen(sessions: Session[]) {
  for (const s of sessions) if (turnEnded(s) && ag.open === `s:${s.id}`) call('agents.seen', { session: s.id }).catch(() => {});
}

export type Pending = { target: string; until: number; force: boolean; label: string };

const UNDO_MS = 10_000;

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
      thread: ag.archived ? '' : (key ?? ''),
      thread_rev: key === ag.threadKey ? ag.threadRev : 0,
      tab: ag.tab,
    });
    if (r.tree) {
      ag.tree = r.tree;
      markSeen(r.tree.sessions);
      if (ui.info) ui.info.agents_waiting = r.tree.requests.length + r.tree.sessions.filter(turnEnded).length;
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
  ag.archived = null;
  if (ag.open !== key) {
    ag.open = key;
    if (ag.tree) markSeen(ag.tree.sessions);
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
  if (q && ag.hits.length) {
    const groups = ag.hits.filter((h) => h.kind === 'group').length;
    out.push({ type: 'archived-head', key: 'archived', groups, sessions: ag.hits.length - groups, total: ag.hitsTotal });
    let project: string | null = null;
    let group: string | null = null;
    for (const h of [...ag.hits].sort((a, b) => a.project.localeCompare(b.project))) {
      const p = h.project || (h.group ? h.source === 'acp' ? 'local' : 'scion' : 'Unsorted');
      if (p !== project) {
        out.push({ type: 'archived-label', key: `ap:${p}:${h.id}`, label: p, project: true });
        project = p;
        group = null;
      }
      const g = h.kind === 'group' ? h.name : h.group;
      if (h.kind === 'session' && g && g !== group) out.push({ type: 'archived-label', key: `ag:${h.id}`, label: g, project: false });
      group = g;
      out.push({ type: 'archived', key: `a:${h.id}`, item: h });
    }
  }
  return out;
}

/** Nodes j/k can land on. */
export const selectable = (n: Node) => n.type === 'group' || n.type === 'room' || n.type === 'session' || n.type === 'archived';

let searchTimer: ReturnType<typeof setTimeout> | null = null;

/** Look for archived items as the filter changes (they come back only through it). */
export function searchArchived(q: string) {
  if (searchTimer) clearTimeout(searchTimer);
  if (!q.trim()) {
    ag.hits = [];
    return;
  }
  searchTimer = setTimeout(async () => {
    try {
      const r = await call<{ items: ArchivedItem[]; total: number }>('agents.archived', { q });
      if (ag.filter === q) {
        ag.hits = r.items;
        ag.hitsTotal = r.total;
      }
    } catch (e) {
      ag.error = String((e as Error).message ?? e);
    }
  }, 150);
}

export async function openArchived(id: string) {
  try {
    ag.archived = await call<ArchivedOpen>('agents.archived_open', { id });
    ag.archivedQ = ag.filter.trim();
    ag.sel = `a:${id}`;
  } catch (e) {
    ag.error = String((e as Error).message ?? e);
  }
}

export function closeArchived() {
  ag.archived = null;
}

/** What `a` and the menu archive for a tree row: a room is its group. */
function archiveTarget(key: string): string | null {
  if (key.startsWith('r:')) {
    const g = roomGroup(key.slice(2));
    return g ? `g:${g.id}` : null;
  }
  return key.startsWith('g:') || key.startsWith('s:') ? key : null;
}

/** Open the archive confirm (02) for a tree row. */
export async function askArchive(key: string | null) {
  const target = key && archiveTarget(key);
  if (!target || ag.pending.some((p) => p.target === target)) return;
  try {
    ag.plan = await call<Plan>('agents.archive_plan', { target });
  } catch (e) {
    ag.error = String((e as Error).message ?? e);
  }
}

/** ⌘⏎ (or ⌘⌫, force): start the undo window; the archive goes through when it ends. */
export function confirmArchive(force: boolean) {
  const p = ag.plan;
  if (!p || p.state === 'blocked' || (p.state === 'not_done') !== force) return;
  ag.plan = null;
  const label = p.hub ? `deleting ${p.hub} agent${p.hub === 1 ? '' : 's'}` : 'archiving';
  ag.pending = [...ag.pending, { target: p.target, until: Date.now() + UNDO_MS, force, label }];
  setTimeout(() => commitArchive(p.target), UNDO_MS);
}

function commitArchive(target: string) {
  const p = ag.pending.find((x) => x.target === target);
  if (!p) return;
  ag.pending = ag.pending.filter((x) => x !== p);
  act('archive', { target, force: p.force }).catch(() => {});
}

/** u: take back the newest archive still in its undo window. */
export function undoArchive(): boolean {
  if (!ag.pending.length) return false;
  ag.pending = ag.pending.slice(0, -1);
  return true;
}

export function pendingFor(key: string): Pending | undefined {
  return ag.pending.find((p) => p.target === key);
}

/** Nothing running or waiting under a row: the menu's Archive is on. */
export function archivable(key: string): boolean {
  const target = archiveTarget(key);
  if (!target || !ag.tree) return false;
  const busy = (s: Session) => s.status === 'running' || s.status === 'waiting' || ag.tree!.requests.some((r) => r.session_id === s.id);
  if (target.startsWith('g:')) return !ag.tree.sessions.some((s) => s.group_id === target.slice(2) && busy(s));
  const under = (id: string): Session[] => ag.tree!.sessions.filter((s) => s.parent_id === id).flatMap((c) => [c, ...under(c.id)]);
  const s = sessionById(target.slice(2));
  return !!s && ![s, ...under(s.id)].some(busy);
}

/** "4s", "2m", "3h", "2d" since an instant. */
export function age(ts: string, now: number): string {
  if (!ts) return '';
  const t = new Date(ts).getTime();
  const s = Math.max(0, Math.round((now - t) / 1000));
  if (s < 60) return `${s}s`;
  if (s < 3600) return `${Math.floor(s / 60)}m`;
  if (s < 86400) return `${Math.floor(s / 3600)}h`;
  return `${Math.floor(s / 86400)}d`;
}

export { clockHM as hm } from '../../lib/app.svelte';

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

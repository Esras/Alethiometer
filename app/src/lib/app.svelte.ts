// Shell state shared by every view: current tab, current topic (drives the day
// strip highlight), overlays, and the ticking clock.

import { call } from './api';
import type { AppInfo, Category } from './types';

/** `search` (18d) is a tab only while it's open: it isn't in TABS. */
export type Tab = 'incoming' | 'priorities' | 'topics' | 'people' | 'todos' | 'notes' | 'reflect' | 'agents' | 'search';
export const TABS: { id: Tab; label: string }[] = [
  { id: 'incoming', label: 'Incoming' },
  { id: 'priorities', label: 'Priorities' },
  { id: 'topics', label: 'Topics' },
  { id: 'people', label: 'People' },
  { id: 'todos', label: 'Todos' },
  { id: 'notes', label: 'Notes' },
  { id: 'reflect', label: 'Reflect' },
  { id: 'agents', label: 'Agents' },
];

export const ui = $state({
  info: null as AppInfo | null,
  tab: 'incoming' as Tab,
  /** Topic the current view is about; lights its meetings on the day strip. */
  topicId: null as number | null,
  /** Topic opened in the Topics tab, and the tab it was opened from (breadcrumb). */
  openTopicId: null as number | null,
  openedFrom: null as Tab | null,
  /** Todo to select when the Todos tab next shows it. */
  openTodoId: null as number | null,
  assistantOpen: false,
  searchOpen: false,
  settingsOpen: false,
  /** The Search results view's query, and the tab it was opened from (Esc goes back). */
  searchQuery: '',
  /** Whether ⌘K had asked Glean (⌥↵) when it opened the Search view. */
  searchGlean: false,
  searchFrom: null as Tab | null,
  /** Bumped after writes so views can reload. */
  rev: 0,
  nowMs: Date.now(),
  error: null as string | null,
});

export async function loadInfo() {
  setInfo(await call<AppInfo>('app.info'));
  tick();
  setInterval(tick, 15_000);
  watchSettings();
  setInterval(watchSettings, 3_000);
  setInterval(watchAgents, 3_000);
}

/** The Agents tab's badge moves while you're on another tab. */
async function watchAgents() {
  const n = await call<number>('agents.badge').catch(() => null);
  if (n !== null && ui.info) ui.info.agents_waiting = n;
}

const inTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

function setInfo(info: AppInfo) {
  ui.info = info;
  const root = document.documentElement;
  for (const [key, c] of Object.entries(info.categories)) root.style.setProperty(`--cat-${key}`, c.color);
  applyType(info);
}

const quote = (f: string) => (/^[\w-]+$/.test(f) ? f : `"${f.replace(/"/g, '')}"`);

/** Type settings as CSS: the root size scales every rem; reading text has its own size. */
function applyType(info: AppInfo) {
  const root = document.documentElement.style;
  root.setProperty(
    '--font-ui',
    info.ui_font ? `${quote(info.ui_font)}, 'Inter', system-ui, sans-serif` : `'Inter', system-ui, sans-serif`,
  );
  root.setProperty(
    '--font-mono',
    info.mono_font ? `${quote(info.mono_font)}, ui-monospace, monospace` : `ui-monospace, 'SF Mono', Menlo, monospace`,
  );
  root.setProperty('font-size', `${info.ui_font_size || 13}px`);
  root.setProperty('--fs-read', `${info.reading_font_size || 14}px`);
  // The desktop window zooms natively (set_zoom); a plain browser gets CSS zoom.
  if (!inTauri) root.setProperty('zoom', String(info.ui_scale || 1));
}

let settingsRev = -1;
/** Settings changed (from the modal or a hand edit): reload what depends on them. */
export async function watchSettings() {
  const st = await call<{ rev: number }>('settings.state').catch(() => null);
  if (!st) return;
  if (settingsRev >= 0 && st.rev !== settingsRev) refresh();
  settingsRev = st.rev;
}

function tick() {
  ui.nowMs = Date.now() + (ui.info?.clock_offset_ms ?? 0);
}

const formats = new Map<string, Intl.DateTimeFormat>();

/** An instant's wall-clock parts in the core's zone (`info.tz`), so the UI
 *  and the core agree on "today" and "10:52". Instants arrive as UTC
 *  ("2026-10-03T08:52:00.000Z") or epoch ms. */
export function wall(at: string | number) {
  const tz = ui.info?.tz ?? '';
  let f = formats.get(tz);
  if (!f) {
    const opts: Intl.DateTimeFormatOptions = {
      year: 'numeric',
      month: '2-digit',
      day: '2-digit',
      weekday: 'short',
      hour: '2-digit',
      minute: '2-digit',
      second: '2-digit',
      hourCycle: 'h23',
    };
    try {
      f = new Intl.DateTimeFormat('en-US', { ...opts, timeZone: tz || undefined });
    } catch {
      f = new Intl.DateTimeFormat('en-US', opts); // a zone this browser doesn't know: its own
    }
    formats.set(tz, f);
  }
  const p = Object.fromEntries(
    f.formatToParts(typeof at === 'number' ? at : new Date(at)).map((x) => [x.type, x.value]),
  );
  const h = Number(p.hour);
  const m = Number(p.minute);
  return {
    day: `${p.year}-${p.month}-${p.day}`,
    weekday: p.weekday,
    minute: h * 60 + m + Number(p.second) / 60,
    hm: `${p.hour}:${p.minute}`,
  };
}

/** "10:52" for an instant, on the app's clock. */
export const clockHM = (at: string | number | null | undefined) => (at ? wall(at).hm : '');

/** "Tue 2026-10-21 10:52", for a hover. */
export const clockFull = (at: string | number | null | undefined) => {
  if (!at) return '';
  const w = wall(at);
  return `${w.weekday} ${w.day} ${w.hm}`;
};

/** Minutes since local midnight, from the (possibly offset) clock. */
export function nowMinute(): number {
  return wall(ui.nowMs).minute;
}

export const UNCATEGORIZED: Category = { label: 'Uncategorized', color: '#504945' };

export function category(key: string): Category {
  if (!key) return UNCATEGORIZED;
  return ui.info?.categories[key] ?? { label: key, color: '#a89984' };
}

export function catColor(key: string): string {
  return category(key).color;
}

/** Switch tabs. Clears the strip highlight; the new view sets its own topic. */
export function setTab(tab: Tab) {
  if (tab === ui.tab) return;
  ui.topicId = null;
  ui.tab = tab;
}

/** ⌘↵ in ⌘K: the full Search results view (18d) for `q`. */
export function openSearch(q: string) {
  if (ui.tab !== 'search') ui.searchFrom = ui.tab;
  ui.searchQuery = q;
  ui.searchOpen = false;
  ui.topicId = null;
  ui.tab = 'search';
}

export function closeSearch() {
  setTab(ui.searchFrom && ui.searchFrom !== 'search' ? ui.searchFrom : 'incoming');
}

export function openTopic(id: number) {
  ui.openedFrom = ui.tab === 'topics' ? ui.openedFrom : ui.tab;
  ui.openTopicId = id;
  ui.tab = 'topics';
}

export function openTodo(id: number) {
  ui.openTodoId = id;
  setTab('todos');
}

/** Call after a write: views reload, and the Incoming count updates. */
export function refresh() {
  ui.rev++;
  call<AppInfo>('app.info').then(setInfo);
}

/** Countdown color, three steps (ui-plan decision 11): green above 15 min, yellow 6–15, red at 5 or less. */
export function urgencyColor(minutesLeft: number): string {
  if (minutesLeft > 15) return 'var(--time-far)';
  if (minutesLeft > 5) return 'var(--time-near)';
  return 'var(--time-now)';
}

/** Next-meeting pill in the Topics index: red within 30 min, yellow later today, green on another day. */
export function meetingPill(start: string): { label: string; weight: 'red' | 'yellow' | 'green' } {
  const t = new Date(start).getTime();
  const mins = Math.max(0, Math.round((t - ui.nowMs) / 60000));
  if (mins <= 30) return { label: `in ${mins} min`, weight: 'red' };
  const w = wall(t);
  if (w.day === wall(ui.nowMs).day) {
    return { label: `in ${Math.floor(mins / 60)}h ${mins % 60}m`, weight: 'yellow' };
  }
  return { label: `${w.weekday} ${w.hm}`, weight: 'green' };
}

export function hm(min: number): string {
  const h = Math.floor(min / 60);
  const m = Math.round(min % 60);
  return `${String(h).padStart(2, '0')}:${String(m).padStart(2, '0')}`;
}

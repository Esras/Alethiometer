// Shell state shared by every view: current tab, current topic (drives the day
// strip highlight), overlays, and the ticking clock.

import { call } from './api';
import type { AppInfo, Category } from './types';

export type Tab = 'incoming' | 'priorities' | 'topics' | 'notes' | 'reflect';
export const TABS: { id: Tab; label: string }[] = [
  { id: 'incoming', label: 'Incoming' },
  { id: 'priorities', label: 'Priorities' },
  { id: 'topics', label: 'Topics' },
  { id: 'notes', label: 'Notes' },
  { id: 'reflect', label: 'Reflect' },
];

export const ui = $state({
  info: null as AppInfo | null,
  tab: 'incoming' as Tab,
  /** Topic the current view is about; lights its meetings on the day strip. */
  topicId: null as number | null,
  /** Topic opened in the Topics tab, and the tab it was opened from (breadcrumb). */
  openTopicId: null as number | null,
  openedFrom: null as Tab | null,
  assistantOpen: false,
  searchOpen: false,
  /** Bumped after writes so views can reload. */
  rev: 0,
  nowMs: Date.now(),
  error: null as string | null,
});

export async function loadInfo() {
  const info = await call<AppInfo>('app.info');
  ui.info = info;
  const root = document.documentElement;
  for (const [key, c] of Object.entries(info.categories)) root.style.setProperty(`--cat-${key}`, c.color);
  tick();
  setInterval(tick, 15_000);
}

function tick() {
  ui.nowMs = Date.now() + (ui.info?.clock_offset_ms ?? 0);
}

/** Minutes since local midnight, from the (possibly offset) clock. */
export function nowMinute(): number {
  const d = new Date(ui.nowMs);
  return d.getHours() * 60 + d.getMinutes() + d.getSeconds() / 60;
}

export function category(key: string): Category {
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

export function openTopic(id: number) {
  ui.openedFrom = ui.tab === 'topics' ? ui.openedFrom : ui.tab;
  ui.openTopicId = id;
  ui.tab = 'topics';
}

/** Call after a write: views reload, and the Incoming count updates. */
export function refresh() {
  ui.rev++;
  call<AppInfo>('app.info').then((info) => (ui.info = info));
}

/** Countdown color: green > 15 min, yellow 5–15, orange 2–5, red < 2. */
export function urgencyColor(minutesLeft: number): string {
  if (minutesLeft > 15) return 'var(--green)';
  if (minutesLeft > 5) return 'var(--yellow)';
  if (minutesLeft > 2) return 'var(--orange)';
  return 'var(--red)';
}

/** Next-meeting pill in the Topics index: red within 30 min, yellow later today, green on another day. */
export function meetingPill(start: string): { label: string; weight: 'red' | 'yellow' | 'green' } {
  const t = new Date(start);
  const mins = Math.max(0, Math.round((t.getTime() - ui.nowMs) / 60000));
  if (mins <= 30) return { label: `in ${mins} min`, weight: 'red' };
  if (t.toDateString() === new Date(ui.nowMs).toDateString()) {
    return { label: `in ${Math.floor(mins / 60)}h ${mins % 60}m`, weight: 'yellow' };
  }
  const day = t.toLocaleDateString('en-US', { weekday: 'short' });
  return { label: `${day} ${hm(t.getHours() * 60 + t.getMinutes())}`, weight: 'green' };
}

export function hm(min: number): string {
  const h = Math.floor(min / 60);
  const m = Math.round(min % 60);
  return `${String(h).padStart(2, '0')}:${String(m).padStart(2, '0')}`;
}

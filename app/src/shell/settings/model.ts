// The Settings page as `settings.get` returns it, plus path helpers.

import { call } from '../../lib/api';

export type Seg = string | number;

export interface Field {
  path: Seg[];
  key: string;
  section: string;
  group: string;
  label: string;
  desc: string;
  widget: string;
  opts?: {
    unit?: string;
    step?: number;
    min?: number;
    max?: number;
    enum?: string[];
    readonly?: boolean;
    restart?: boolean;
    dev?: boolean;
    advanced?: boolean;
    secret?: boolean;
    note?: string;
    optional?: boolean;
    pair?: string;
    mono?: boolean;
  };
}

export interface Secret {
  name: string;
  label: string;
  note: string;
  optional: boolean;
  oauth: boolean;
  set: boolean;
  set_at: string | null;
}

export interface SourceRow {
  index: number;
  kind: string;
  name: string;
  tag: string;
  enabled: boolean;
  status: { id: number; last_sync_at: string | null; last_error: string | null; syncing: boolean; live?: boolean | null } | null;
  secrets: Secret[];
  attention: boolean;
}

export interface Kind {
  kind: string;
  label: string;
  fields: Field[];
  defaults: Record<string, unknown>;
}

export interface State {
  rev: number;
  message: string;
  error: string | null;
}

export interface Page {
  values: Record<string, unknown>;
  fields: Field[];
  kinds: Kind[];
  sources: SourceRow[];
  usage: { model: string; calls: number; tokens: number }[];
  state: State;
  dev: boolean;
  path: string;
  tag_rules: string;
}

export type Edit = { op?: 'set' | 'remove' | 'push'; path: Seg[]; value?: unknown };

export const SECTIONS = [
  { id: 'general', label: 'General', icon: 'sliders-horizontal', blurb: 'Window, type, day strip and Topics.' },
  { id: 'categories', label: 'Categories', icon: 'tag', blurb: 'Topic categories, in the order the app shows them. Keys never change.' },
  { id: 'people', label: 'People', icon: 'users', blurb: 'You, and when to suggest a People note for a new sender.' },
  { id: 'sources', label: 'Sources', icon: 'plugs-connected', blurb: '' },
  { id: 'ai', label: 'AI', icon: 'sparkle', blurb: 'Work data only goes to allowed providers. Anything else fails closed.' },
  { id: 'developer', label: 'Developer', icon: 'wrench', blurb: 'For working on Alethiometer itself.' },
] as const;
export type Section = (typeof SECTIONS)[number]['id'];

/** Group order per page (anything unlisted keeps its schema order, after these). */
export const GROUP_ORDER = ['', 'Window', 'Type', 'Day strip', 'Topics', 'Notes', 'Data', 'You', 'New person suggestions', 'Allowed providers', 'Bedrock', 'Models', 'Prices', 'Clock', 'identity', 'secrets', 'reads', 'limits', 'Advanced'];

export const GROUP_LABELS: Record<string, string> = {
  identity: 'Identity',
  secrets: 'Secrets',
  reads: 'What it reads',
  limits: 'Limits',
};

export function get(obj: unknown, path: Seg[]): unknown {
  let cur: unknown = obj;
  for (const seg of path) {
    if (cur === null || cur === undefined) return undefined;
    cur = (cur as Record<string | number, unknown>)[seg];
  }
  return cur;
}

/** A source config value, falling back to the kind's default. */
export function sourceValue(page: Page, index: number, kind: string, rel: Seg[]): unknown {
  const v = get(page.values, ['sources', index, ...rel]);
  if (v !== undefined) return v;
  if (rel[0] === 'config') return get(page.kinds.find((k) => k.kind === kind)?.defaults, rel.slice(1));
  if (rel[0] === 'poll_secs') return 300;
  if (rel[0] === 'enabled') return true;
  return undefined;
}

export async function loadPage(): Promise<Page> {
  return call<Page>('settings.get');
}

export async function patch(edits: Edit[]): Promise<State> {
  return call<State>('settings.patch', { edits });
}

/** "Sep 14" from "2026-09-14T10:02:00". */
export function shortDate(ts: string | null): string {
  if (!ts) return '';
  const d = new Date(ts.replace(' ', 'T'));
  return d.toLocaleDateString('en-US', { month: 'short', day: 'numeric' });
}

export function ago(ts: string | null, nowMs: number): string {
  if (!ts) return 'never';
  const mins = Math.round((nowMs - new Date(ts.replace(' ', 'T')).getTime()) / 60000);
  if (mins < 1) return 'just now';
  if (mins < 60) return `${mins}m`;
  if (mins < 60 * 24) return `${Math.round(mins / 60)}h`;
  return `${Math.round(mins / 1440)}d`;
}

/** What the sidebar and page header say about a source's state. */
export function sourceState(s: SourceRow, nowMs: number): { label: string; tone: 'ok' | 'warn' | 'off' } {
  if (!s.enabled) return { label: 'off', tone: 'off' };
  const missing = s.secrets.find((k) => !k.set && !k.optional);
  if (missing) return { label: missing.oauth ? 'not connected' : 'no token', tone: 'warn' };
  if (s.status?.last_error) return { label: 'error', tone: 'warn' };
  if (s.kind === 'glean') return { label: 'search only', tone: 'ok' };
  if (s.status?.syncing) return { label: 'syncing', tone: 'ok' };
  if (s.status?.live) return { label: 'live', tone: 'ok' };
  if (s.kind === 'obsidian' && s.status?.last_sync_at) return { label: 'watching', tone: 'ok' };
  if (s.status?.last_sync_at) return { label: ago(s.status.last_sync_at, nowMs), tone: 'ok' };
  return { label: 'not synced yet', tone: 'off' };
}

/** Matches for find (25d): labels, descriptions and key paths, sources included. */
export function find(page: Page, q: string): { field: Field; where: string; section: Section; source: number | null; path: Seg[] }[] {
  const needle = q.trim().toLowerCase();
  if (!needle) return [];
  const hit = (f: Field) => [f.label, f.desc, f.key].some((t) => t.toLowerCase().includes(needle));
  const out: ReturnType<typeof find> = [];
  for (const f of page.fields) {
    if (f.opts?.dev && !page.dev) continue;
    if (hit(f)) out.push({ field: f, where: SECTIONS.find((s) => s.id === f.section)?.label ?? f.section, section: f.section as Section, source: null, path: f.path });
  }
  for (const s of page.sources) {
    const kind = page.kinds.find((k) => k.kind === s.kind);
    for (const f of kind?.fields ?? []) {
      if (hit(f) || s.name.toLowerCase().includes(needle)) {
        const path = ['sources', s.index, ...f.path];
        out.push({ field: { ...f, key: f.key.replace('sources[]', `sources[${s.index}]`) }, where: `Sources · ${s.name}`, section: 'sources', source: s.index, path });
      }
    }
  }
  return out.slice(0, 40);
}

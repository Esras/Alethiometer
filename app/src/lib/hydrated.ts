// Hydrated search (Glean + local), as `search.hydrated` returns it. Shared by
// ⌘K (3b/3c) and the Search view (18d).

import { call } from './api';

export type Route = 'glean' | 'local';

export interface HRow {
  key: string;
  title: string;
  url: string | null;
  icon: string;
  system: string;
  routes: Route[];
  at: string | null;
  when: string;
  snippet: string;
  item_id: number | null;
  topic: { id: number; name: string; category: string; state: 'confirmed' | 'suggested' } | null;
  kept: boolean;
  reason: string;
  similarity: number | null;
  pinned: boolean;
  container: string;
  msgs: number;
}

export interface Facet {
  route: Route;
  count: number;
  sources: { icon: string; system: string; count: number }[];
  status: { state: string; message?: string };
}

export interface Hydrated {
  q: string;
  anchor: { id: number; name: string; category: string } | null;
  rows: HRow[];
  facets: Facet[];
  hidden: number;
  topics: { id: number; name: string; category: string }[];
  embeddings: string;
}

export const ROUTE_LABEL: Record<Route, string> = { glean: 'Glean', local: 'Local' };
/** Each route keeps its own colour; Glean's lavender comes from its icon. */
export const ROUTE_COLOR: Record<Route, string> = { glean: '#9a9df5', local: 'var(--aqua)' };

export function hydrated(q: string, anchor: number | null, glean: boolean): Promise<Hydrated> {
  return call<Hydrated>('search.hydrated', { q, anchor, glean });
}

export function feedback(topicId: number, r: Pick<HRow, 'key' | 'title' | 'url' | 'system'>, verdict: 'pin' | 'reject' | 'clear'): Promise<unknown> {
  return call('search.feedback', { topic_id: topicId, key: r.key, verdict, title: r.title, url: r.url ?? '', source: r.system });
}

/** What the facet bar is pointing at: a route, or one source within a route. */
export interface Lens {
  route: Route;
  icon?: string;
}

export function inLens(r: HRow, l: Lens | null): boolean {
  if (!l) return true;
  return r.routes.includes(l.route) && (!l.icon || r.icon === l.icon);
}

/** "a \u0001b\u0002 c" → text and marked parts. */
export function parts(snippet: string): { text: string; hit: boolean }[] {
  return snippet
    .split(/(\u0001[^\u0002]*\u0002)/)
    .filter(Boolean)
    .map((p) => (p.startsWith('\u0001') ? { text: p.slice(1, -1), hit: true } : { text: p, hit: false }));
}

/** What a route chip says when it has nothing to show. */
export function routeNote(f: Facet, isMac: boolean): string {
  switch (f.status.state) {
    case 'not_asked':
      return `${isMac ? '⌥' : 'Alt+'}↵ to ask`;
    case 'off':
      return 'not set up';
    case 'no_token':
      return 'needs its token';
    case 'no_server':
      return 'needs a server';
    case 'limit':
      return 'daily limit reached';
    case 'failed':
      return 'unavailable';
    default:
      return '';
  }
}

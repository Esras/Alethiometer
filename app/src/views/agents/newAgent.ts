// The New agent form's logic (decisions.md › "New agent form and the Agents tree"), kept out of the
// component so it can be tested: what a saved agent fills in, which fields differ from it, what saving
// writes back, and the warnings under the form.

import type { SavedAgent } from './types';

export type Field = Exclude<keyof SavedAgent, 'name'>;

/** Least to most specific, as the form lists them. The session's name (the top line's) isn't one: it's per launch. */
export const FIELDS: Field[] = ['runs', 'sandbox', 'harness', 'model', 'thinking', 'image', 'dirs', 'task'];

export const LABEL: Record<Field, string> = {
  runs: 'Runs',
  sandbox: 'Sandbox',
  harness: 'Harness',
  model: 'Model',
  thinking: 'Thinking',
  image: 'Image',
  dirs: 'Directories',
  task: 'Task',
};

export type Harness = { key: string; label: string; sandbox_from: string; sandbox_command: string };

/** A new saved agent (or New's one-off) at every field's default. */
export function blank(harness: string, name = ''): SavedAgent {
  return {
    name,
    runs: 'local',
    sandbox: '',
    harness,
    model: '',
    thinking: '',
    image: '',
    dirs: [],
    task: '',
  };
}

/** A saved agent as the launch form starts: missing fields filled, empty Directories as where it's opened from. */
export function resolve(raw: SavedAgent, from: string[], harnesses: Harness[]): SavedAgent {
  const r = { ...blank(harnesses[0]?.key ?? ''), ...raw, dirs: [...(raw.dirs ?? [])] };
  if (r.runs !== 'openshell') r.runs = 'local';
  if (!harnesses.some((h) => h.key === r.harness) && harnesses.length) r.harness = harnesses[0].key;
  if (!r.dirs.length) r.dirs = [...from];
  return r;
}

const same = (a: unknown, b: unknown) =>
  Array.isArray(a) && Array.isArray(b) ? a.length === b.length && a.every((x, i) => x === b[i]) : a === b;

/** Fields that don't apply to how it runs; they're removed from the form, so they don't count either. */
export function applies(f: SavedAgent, k: Field): boolean {
  if (k === 'sandbox') return f.runs === 'openshell';
  if (k === 'image') return f.runs === 'openshell' && !f.sandbox;
  return true;
}

/** Fields that differ from what the saved agent filled in. The Task typed for a launch is never one: a saved
 *  agent's task is set in ⌘, only (user, 2026-11-13). */
export function changed(raw: SavedAgent, base: SavedAgent, f: SavedAgent): Field[] {
  return FIELDS.filter((k) => {
    if (k === 'task') return false;
    if (!applies(f, k)) return false;
    return !same(f[k], base[k]);
  });
}

/** The saved agent with the changed fields written in; the rest stay as saved (empty stays empty). */
export function merged(raw: SavedAgent, base: SavedAgent, f: SavedAgent): SavedAgent {
  const out: SavedAgent = { ...blank(base.harness), ...raw, dirs: [...(raw.dirs ?? [])] };
  for (const k of changed(raw, base, f)) (out as Record<Field, unknown>)[k] = Array.isArray(f[k]) ? [...f[k]] : f[k];
  return out;
}

/** A folder's name: what a chip shows and where an upload lands. */
export const base = (d: string) => d.replace(/\/+$/, '').split('/').pop() || d;

/** Chip labels: the folder's name, or parent/name when two have the same name. */
export function chipLabels(dirs: string[]): string[] {
  const names = dirs.map(base);
  return dirs.map((d, i) => {
    if (names.filter((n) => n === names[i]).length < 2) return names[i];
    const parts = d.replace(/\/+$/, '').split('/');
    return parts.slice(-2).join('/');
  });
}

/** Directories that would land in the same place in a new sandbox (uploads land by folder name). */
export function collisions(dirs: string[]): string[] {
  const names = dirs.map(base);
  return dirs.filter((_, i) => names.indexOf(names[i]) !== i || names.lastIndexOf(names[i]) !== i);
}

/** An image another harness uses and this one doesn't: it likely lacks this harness's adapter. */
export function imageFlag(f: SavedAgent, harnesses: Harness[]): string | null {
  if (!applies(f, 'image') || !f.image.trim()) return null;
  const h = harnesses.find((x) => x.key === f.harness);
  const img = f.image.trim();
  if (!h || img === h.sandbox_from) return null;
  const other = harnesses.find((x) => x.key !== h.key && x.sandbox_from && x.sandbox_from === img);
  if (!other) return null;
  return `${img} is ${other.label}'s image; ${h.label}'s is ${h.sandbox_from || "the gateway's default"}`;
}

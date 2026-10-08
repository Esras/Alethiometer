// Merging topics (user decisions 2026-10-01). `m` stages a merge of one topic
// into another: the topic shows "merged into B · u undo" and nothing is
// written until you leave the screen (the Topics index for a topic, one topic
// for another, or another tab). `u` cancels it before then. A merge still
// staged when the app closes is dropped. The merge itself (`topics.merge`):
// signals, people, repos and Notes lines move to B, B's note gets A's text
// appended, Gmail threads are relabelled when the category changes, A is
// deleted.
import { call } from './api';
import { refresh } from './app.svelte';

export interface Merge {
  from: number;
  fromName: string;
  into: number;
  intoName: string;
}

export const merges = $state<{ list: Merge[]; err: string | null }>({ list: [], err: null });

export function stageMerge(m: Merge) {
  merges.list = [...merges.list.filter((x) => x.from !== m.from), m];
  merges.err = null;
}

/** Cancel the merge of `from` (or the latest one). */
export function cancelMerge(from?: number): boolean {
  const m = from === undefined ? merges.list[merges.list.length - 1] : merges.list.find((x) => x.from === from);
  if (!m) return false;
  merges.list = merges.list.filter((x) => x !== m);
  return true;
}

export function pendingMerge(from: number): Merge | undefined {
  return merges.list.find((x) => x.from === from);
}

/** Topics that can't be a merge target: ones already staged to go away. */
export function mergingAway(): number[] {
  return merges.list.map((m) => m.from);
}

/** Leaving the screen: write every staged merge, in the order they were made. */
export async function commitMerges() {
  const list = merges.list;
  if (!list.length) return;
  merges.list = [];
  for (const m of list) {
    try {
      const r = await call<{ note: string }>('topics.merge', { from: m.from, into: m.into });
      // Merged; only the note text couldn't be appended.
      if (r.note.startsWith('failed'))
        merges.err = `Merged ${m.fromName} into ${m.intoName}, but its note wasn't appended (${r.note}). ${m.fromName}'s note file is still there.`;
    } catch (e) {
      merges.err = `Merging ${m.fromName} into ${m.intoName} failed: ${String((e as Error).message ?? e)}`;
    }
  }
  refresh();
}

// Shapes returned by incoming.list (src-tauri/src/queries/incoming.rs).
import type { SourceKind, TopicRef } from '../../lib/types';

export interface Signal {
  item_id: number;
  source: SourceKind;
  kind: string;
  title: string;
  /** Muted line next to the title: "42 new · 09:05–10:40", "Sam · 10:41". */
  meta: string;
  summary: string | null;
  url: string | null;
  /** Messages behind the signal (e.g. all Slack messages since you last looked). */
  msg_count: number | null;
  /** `id` is what `incoming.split` takes. */
  messages: { id: number; time: string; author: string; text: string }[];
}

export interface TopicSignals extends TopicRef {
  signals: Signal[];
}

export interface Queue {
  topics: TopicSignals[];
  unsorted: Signal[];
}

/** A topic id, or Unsorted. */
export type QueueKey = number | 'unsorted';

// Shapes returned by incoming.list (src-tauri/src/queries/incoming.rs).
import type { NewPerson, Who } from '../../lib/people';
import type { SourceKind, TopicRef } from '../../lib/types';

export interface Message {
  id: number;
  time: string;
  author: string;
  text: string;
  /** Person colour and People note (16c, 17a). */
  who: Who;
  /** Another part of this split came in just before this message (⋯). */
  gap?: boolean;
  /** The split part holding it (`incoming.family` only). */
  part?: number;
  /** Thread replies to this message; they move with it. */
  replies?: Message[];
}

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
  messages: Message[];
  context: Message[];
  /** Already filed under this topic: new activity rather than a suggestion. */
  update: boolean;
  /** A todo waiting for this signal to be confirmed (22c). */
  held: Held | null;
  /** One part of a split whose other parts still hold messages (15g). */
  split: boolean;
  /** An unknown sender here is worth a People note (17a). */
  new_person: NewPerson | null;
}

export interface Held {
  target: 'local' | 'jira';
  text: string;
  due: string | null;
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

/** One part of a split family in the review (15g/15h). */
export interface Part {
  item_id: number;
  /** Its topic; null is Unsorted. */
  topic: { id: number; name: string; category: string } | null;
  /** The family's dismissed messages. */
  dismissed: boolean;
}

/** `incoming.family`: every part still in Incoming, messages in time order. */
export interface Family {
  root: number;
  title: string;
  source: SourceKind;
  url: string | null;
  conv: 'dm' | 'group' | 'channel' | 'thread' | 'other';
  parts: Part[];
  messages: Message[];
}

// Shapes returned by incoming.list (src-tauri/src/queries/incoming.rs).
import type { NewPerson, Who } from "../../lib/people";
import type { SourceKind, TopicRef } from "../../lib/types";

export interface Message {
  id: number;
  /** Source key of the message (a Slack `ts`). */
  key?: string;
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
  read?: boolean;
}

export interface Signal {
  item_id: number;
  source: SourceKind;
  kind: string;
  title: string;
  /** Muted line next to the title: "42 new · 09:05–10:40", "Sam · 10:41". */
  meta: string;
  /** An agent signal: who, why, the session, its request. */
  agent?: AgentInfo;
  /** Agents waiting: the topic it's filed under (absent: Unsorted). */
  topic?: string;
  topic_id?: number;
  summary: string | null;
  url: string | null;
  /** Messages behind the signal (e.g. all Slack messages since you last looked). */
  msg_count: number | null;
  /** `id` is what `incoming.split` takes. `incoming.list` leaves these
   *  empty (but for agent signals); an expanded row gets them from
   *  `incoming.messages` (`Bodies`). */
  messages: Message[];
  context: Message[];
  /** How many top-level messages the signal has: a collapsed row's count. */
  n_messages: number;
  /** Already filed under this topic: new activity rather than a suggestion. */
  update: boolean;
  /** A todo waiting for this signal to be confirmed (22c). */
  held: Held | null;
  /** One part of a split whose other parts still hold messages (15g). */
  split: boolean;
  /** An unknown sender here is worth a People note (17a). */
  new_person: NewPerson | null;
  /** The item's author as `meta` names it, matched like a message sender (16c, 17b). */
  author: { name: string; who: Who } | null;
  /** Gmail's own current label, matched to a category key (`config.category_labels`). */
  category?: string | null;
  change?: string | null;
  description?: string | null;
  /** The item's revision as listed, and the keys of the messages listed.
   * Triage sends both back so it applies to what you saw. */
  rev: number;
  keys: string[];
  /** The split family it belongs to (its first part's item). */
  root: number;
  /** A Slack thread item: a reply goes in its thread by default. */
  thread?: boolean;
  /** An invite's Accept/Decline line (14a). */
  rsvp?: Rsvp;
  /** The latest Slack reply sent from this signal. */
  reply?: Answer;
}

/** `queries/respond.rs`: an invite's RSVP line. */
export interface Rsvp {
  /** Your answer in Google as last synced. */
  status: 'needs_action' | 'accepted' | 'declined' | 'tentative';
  /** The answer sent from here. */
  answer?: Answer;
  /** "DD prep 16:45–17:15", "Life 17:30–18:00". */
  overlaps: string[];
  /** The event in Google Calendar, where an answer can be changed. */
  url: string | null;
}

/** An answer (RSVP or Slack reply) as its outbox row stands. */
export interface Answer {
  outbox_id: number;
  state: 'pending' | 'sent' | 'confirmed' | 'failed';
  response?: 'accepted' | 'declined';
  text?: string;
  /** "10:58", "Thu 14:00". */
  at: string;
  error?: string;
}

export interface Held {
  target: "local" | "jira";
  text: string;
  due: string | null;
  sched: string | null;
}

export interface TopicSignals extends TopicRef {
  signals: Signal[];
}

export interface Queue {
  topics: TopicSignals[];
  unsorted: Signal[];
  /** Agents waiting on you, across topics (2b): the same signals again. */
  agents: Signal[];
}

/** A topic id, Unsorted, or Agents waiting. */
export type QueueKey = number | "unsorted" | "agents";

/** An agent signal (`agents::incoming`): who, why, and what to answer it with. */
export interface AgentInfo {
  session: string;
  name: string;
  /** approval | ask | undelivered | failed_delivery | failed | wrote | turn | done */
  kind: string;
  reason: string;
  waiting: boolean;
  project: string;
  group: string;
  harness: string;
  host: string;
  cwd: string;
  why: string;
  request?: string;
  thread?: string;
  row?: number;
  count?: number;
  facts?: {
    task: string;
    ran: string;
    turns: number;
    pr: { label: string; detail: string; url: string } | null;
    ticket: string | null;
  };
}

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
  conv: "dm" | "group" | "channel" | "thread" | "other";
  parts: Part[];
  messages: Message[];
}

/** An expanded row's messages (`incoming.messages`), with the revision and
 *  keys triage sends back for what it showed. */
export interface Bodies {
  item_id: number;
  rev: number;
  keys: string[];
  messages: Message[];
  context: Message[];
}

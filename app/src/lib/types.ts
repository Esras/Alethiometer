// Shapes returned by the Rust core (see src-tauri/src/queries/*). snake_case on purpose.
import type { Rsvp } from '../views/incoming/types';

export type Weight = 'red' | 'yellow' | 'green';
export interface Pill {
  label: string;
  weight: Weight;
}

export interface Category {
  label: string;
  color: string;
}

export interface AppInfo {
  categories: Record<string, Category>;
  day_start_hour: number;
  day_end_hour: number;
  life_buffer_min: number;
  quiet_days: number;
  clock_offset_ms: number;
  /** The core's time zone (IANA name); wall-clock times in the UI use it. */
  tz: string | null;
  ai_label: string;
  incoming_count: number;
  todo_count: number;
  platform: string;
  ui_scale: number;
  ui_font: string;
  mono_font: string;
  ui_font_size: number;
  reading_font_size: number;
  settings_rev: number;
  glean: boolean;
  agents: import('../views/agents/types').AgentsLook;
  agents_waiting: number;
}

export interface Block {
  id: number;
  start_min: number;
  end_min: number;
  title: string | null;
  category: string;
  tentative: boolean;
  life: boolean;
  buffer_min: number | null;
  topic_ids: number[];
  item_id: number | null;
  attendees: string | null;
}

export interface DayStripData {
  day_start_min: number;
  day_end_min: number;
  blocks: Block[];
  note: string | null;
}

export interface CategoryRow extends Category {
  key: string;
}

export interface TopicRef {
  id: number;
  name: string;
  category: string;
}

export interface TopicRow extends TopicRef {
  new_count: number;
  open_count: number;
  obsidian_path: string | null;
  last_signal_at: string | null;
  /** "10:41 · Slack", "Yesterday", "Mon", "Sep 25". */
  last_signal: string | null;
  /** No signal within settings.quiet_days. */
  quiet: boolean;
  next_meeting: { start: string; title: string } | null;
}

/** Source kinds; each has an icon in public/icons/<kind>.png. */
export type SourceKind =
  | 'gcal'
  | 'slack'
  | 'jira'
  | 'github'
  | 'obsidian'
  | 'gmail'
  | 'sheets'
  | 'notion'
  | 'marvin'
  | 'protoncal'
  | 'protonmail'
  | 'agents';

export interface TopicDetail {
  topic: TopicRef & { score: number | null; pills: Pill[] };
  upcoming: { item_id: number; when: string; today: boolean; source: SourceKind; title: string; sub: string | null; rsvp?: Rsvp }[];
  open: { item_id: number; source: SourceKind; title: string; due: Pill | null; owner: string | null }[];
  chronology: {
    item_id: number;
    when: string;
    today: boolean;
    source: SourceKind;
    title: string;
    range: string | null;
    sub: string | null;
    right: string;
    open: boolean;
    /** An invite's Accept/Decline line (14b). */
    rsvp?: Rsvp;
  }[];
  filters: { source: SourceKind; count: number }[];
  /** Whoever wrote in its confirmed signals, plus anyone linked by hand (`people::for_topic`). */
  people: TopicPerson[];
  repos: string[];
  note: { path: string; abs_path: string; text: string } | null;
}

export interface SearchResults {
  topics: (TopicRef & { type: 'topic'; title: string })[];
  items: { type: 'item'; id: number; title: string; source: SourceKind; at: string; topic_id: number | null }[];
}

export interface TopicPerson {
  /** The People note's person; null for someone without a note. */
  id: number | null;
  name: string;
  initials: string;
  role: string | null;
  /** Person palette index (16c). */
  color: number;
  known: boolean;
  account: import('./people').Account | null;
  signals: number;
  last: string | null;
}

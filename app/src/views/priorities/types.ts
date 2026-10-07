// Shapes returned by `priorities.list` (src-tauri/src/queries/priorities.rs).
import type { Pill, TopicRef } from '../../lib/types';

export type ColumnKey = 'priority' | 'quick' | 'neglect';

export interface DateMark {
  /** due: a hard date; sched: a soft one, never overdue. */
  kind: 'due' | 'sched';
  label: string;
  overdue: boolean;
}

export interface PriorityRow {
  item_id: number;
  source: 'local' | 'jira' | 'obsidian';
  title: string;
  url: string | null;
  topic: TopicRef | null;
  topic_priority: 'H' | 'M' | 'L' | null;
  date: DateMark | null;
  /** Date filter bucket: od | today | week (next 7 days) | later | none. */
  when: 'od' | 'today' | 'week' | 'later' | 'none';
  estimate: { minutes: number; by: 'ai' | 'you' } | null;
  score: number;
  value: number;
  pressure: number;
  effort: number;
  /** Score filter: top, middle or bottom third of the Priority column. */
  band: 'hi' | 'mid' | 'lo';
  neglect: number;
  age_days: number;
  quiet_days: number;
  /** Factors present: due, sched, meeting, waiting, blocks, topic_priority, who_asked, breadth, estimate, tag:<name>. */
  factors: string[];
  /** Factors that added to the score, by group; Effort leads with where its estimate came from. */
  reasons: { value: Pill[]; pressure: Pill[]; effort: Pill[] };
}

export interface PrioritiesData {
  todos: PriorityRow[];
  orders: Record<ColumnKey, number[]>;
  quick_max: number;
  /** The tag vocabulary. */
  tags: string[];
}

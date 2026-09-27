// Shapes returned by `priorities.list` (src-tauri/src/queries/priorities.rs).
import type { Pill, SourceKind, TopicRef } from '../../lib/types';

export type QuadrantKey = 'now' | 'make_room' | 'quick' | 'can_wait';

export interface PriorityRow {
  item_id: number;
  source: SourceKind;
  kind: string;
  title: string;
  topic: TopicRef | null;
  /** Reasons only (never the topic), red/yellow/green by how much each adds. */
  reasons: Pill[];
  score: number;
  /** Time part of the Selected bar meta ("11:00–11:45", "due Wed"). */
  when: string | null;
  /** Attendees or author. */
  people: string | null;
  summary: string | null;
  body: string;
  url: string | null;
}

export interface PriorityQuadrant {
  key: QuadrantKey;
  items: PriorityRow[];
}

export interface PrioritiesData {
  quadrants: PriorityQuadrant[];
}

/** Compact view: one chip per topic with a count. */
export interface TopicChip {
  topic: TopicRef | null;
  count: number;
}

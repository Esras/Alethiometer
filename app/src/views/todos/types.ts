// Shapes returned by todos.list (src-tauri/src/queries/todos.rs).
import type { TopicRef } from '../../lib/types';

export type TodoExt =
  | { type: 'local'; from: string | null; from_item: number | null; notes_line_id: number | null }
  | { type: 'jira'; key: string; status: string; issue_type: string | null }
  | { type: 'obsidian'; path: string; text: string; line: number | null };

export interface Todo {
  item_id: number;
  title: string;
  from: string | null;
  due: string | null;
  due_label: string | null;
  overdue: boolean;
  topic: TopicRef | null;
  priority: 'now' | 'make_room' | 'quick' | 'can_wait' | null;
  url: string | null;
  /** Minutes; `by: 'ai'` is the small model's guess, `'you'` an override. Never sent to Jira. */
  estimate: { minutes: number; by: 'ai' | 'you' } | null;
  ext: TodoExt;
}

export type Draft = { target: 'local' | 'jira'; text: string; due: string | null; topic_id?: number | null };

export interface JiraTarget {
  project: string;
  issue_type: string;
}

export interface TodosData {
  todos: Todo[];
  jira: JiraTarget | null;
}

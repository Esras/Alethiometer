// Shapes returned by reflect.* (src-tauri/src/queries/reflect.rs). snake_case on purpose.

export type Scope = 'day' | 'week' | 'month' | 'quarter';
export type Lens = 'both' | 'work' | 'life';
export type CarryAction = 'tomorrow' | 'schedule' | 'let_go' | 'undo';

export interface ReflectNote {
  path: string;
  abs_path: string;
  text: string;
  /** Disk content when loaded; null when the file doesn't exist yet. */
  base: string | null;
}

export interface Bar {
  start_min: number;
  end_min: number;
  category: string;
  over: boolean;
}

export interface DayBriefData {
  tiles: { value: string; label: string }[];
  range: { start_min: number; end_min: number };
  plan: Bar[];
  did: Bar[];
  did_note: string;
  where: { label: string; category: string; minutes: number; dur: string }[];
  decided: { title: string; topic: string | null }[];
}

export interface CarryRow {
  item_id: number;
  period: string;
  glyph: 'do' | 'research' | 'delegated';
  title: string;
  owner: string | null;
  action: Exclude<CarryAction, 'undo'> | null;
  outbox: 'pending' | 'sent' | 'confirmed' | 'failed' | null;
}

export interface ChildRow {
  label: string;
  period: string;
  summary: string | null;
  has_entry: boolean;
  cats: { category: string; minutes: number }[];
}

export interface ReflectData {
  scope: Scope;
  lens: Lens;
  period: string;
  today: string;
  is_current: boolean;
  prev: string;
  next: string;
  title: string;
  context: string;
  note: ReflectNote;
  brief: DayBriefData | null;
  children: ChildRow[] | null;
  carry: CarryRow[];
}

export type SaveResult = { ok: true; base: string; summary: string | null } | { conflict: true; disk: string | null };

export interface NudgeData {
  show: boolean;
  at: string;
  title: string;
  line: string;
  carry_count: number;
  fires_at: string | null;
  answer: string | null;
}

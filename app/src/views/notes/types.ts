// Shapes returned by queries/notes.rs. snake_case on purpose.

export type Kind = 'note' | 'do' | 'research' | 'delegated' | 'life';
export type LineState = 'draft' | 'held' | 'incoming' | 'pending' | 'sent' | 'confirmed' | 'failed';

export interface Target {
  type: 'topic' | 'person' | 'app' | 'keep';
  id?: number | null;
  label: string;
  category?: string | null;
  app?: string | null;
  key?: string | null;
  topic_id?: number | null;
  offer?: string | null;
  create?: boolean;
  /** true: a suggestion; false: the user's own pick. */
  auto: boolean;
  reason?: string | null;
}

export interface Line {
  id: number | null;
  key?: string;
  kind: Kind;
  bare: boolean;
  text: string;
  target: Target | null;
  state: LineState;
  receipt_at: string | null;
  error: string | null;
  locked: boolean;
  /** Jira task requested from this line: queued, then the key once Jira confirms. */
  jira?: { state: 'pending' | 'sent' | 'confirmed' | 'failed'; key: string | null; url: string | null; error: string | null } | null;
}

export interface Page {
  date: string;
  label: string;
  today: boolean;
  prev: string;
  next: string | null;
  lines: Line[];
}

export interface Options {
  options: Target[];
  create: Target | null;
}

/** Client-side line: a stable key plus "waiting for a fresh suggestion". */
export interface EditLine extends Line {
  key: string;
  stale: boolean;
}

export const KEEP: Target = { type: 'keep', label: 'Just keep as a note', auto: false };

export const GLYPHS: Record<Kind, { icon: string; color: string }> = {
  note: { icon: 'minus', color: '#a89984' },
  do: { icon: 'square', color: 'var(--blue)' },
  research: { icon: 'circle', color: 'var(--green)' },
  delegated: { icon: 'triangle', color: 'var(--red)' },
  life: { icon: 'house-simple', color: 'var(--blue)' },
};

/** Prefixes typed at the start of a line. `^` before `@name` keeps the @name. */
export const PREFIXES: { re: RegExp; kind: Kind }[] = [
  { re: /^- /, kind: 'note' },
  { re: /^\[ ?\] /, kind: 'do' },
  { re: /^\(\) /, kind: 'research' },
  { re: /^\^ ?(?=@)/, kind: 'delegated' },
  { re: /^\^ /, kind: 'delegated' },
  { re: /^~ /, kind: 'life' },
];

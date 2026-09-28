// The topic's note as `topics.get` returns it. `base` is the text an edit
// starts from (null: the file doesn't exist yet).
export interface TopicNote {
  path: string;
  abs_path: string;
  text: string;
  base?: string | null;
}

export type SaveState =
  | { state: 'saved' }
  | { state: 'saving'; outbox_id?: number }
  | { state: 'conflict'; disk: string | null }
  | { state: 'offline'; error: string }
  | { state: 'failed'; error: string };

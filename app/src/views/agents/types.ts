// Shapes from src-tauri/src/agents/model.rs (snake_case on purpose).

export type Status = 'running' | 'waiting' | 'idle' | 'done' | 'failed';
export type Kind = 'you' | 'output' | 'thinking' | 'tool' | 'message' | 'state' | 'hub' | 'ask' | 'approval' | 'failed';

export interface Room {
  id: string;
  name: string;
  members: number;
  unread: number;
  active_at: string;
  hub: string;
  thread_ref: string;
  mode: string;
  loop_cap: string;
  messages: number;
  live: boolean;
  url: string;
}

export interface Group {
  id: string;
  source: 'acp' | 'scion';
  project: string;
  name: string;
  goal: string;
  host: string;
  room: Room | null;
  created_at: string;
}

export interface Fact {
  label: string;
  detail: string;
  url: string;
}

export interface Session {
  id: string;
  source: 'acp' | 'scion';
  group_id: string | null;
  parent_id: string | null;
  name: string;
  template: string;
  harness: string;
  model: string;
  host: string;
  cwd: string;
  status: Status;
  title: string;
  subtitle: string;
  context: string;
  created_at: string;
  active_at: string;
  color: number;
  stats: { ctx_used: number; ctx_size: number; turns: number; cost_usd: number | null; cache: string; started_at: string };
  facts: { ticket: Fact | null; pr: Fact | null; branch: Fact | null; todo: Fact | null };
  live: boolean;
  problem: string;
  guess: { group_id: string; label: string } | null;
  url: string;
  ext: string;
}

export interface Who {
  name: string;
  session: string | null;
  kind: 'you' | 'agent' | 'room' | 'hub';
}

export interface Tool {
  name: string;
  detail: string;
  status: string;
  ms: number | null;
  result: string;
  added: number | null;
  removed: number | null;
  summary: string;
}

export interface Delivery {
  state: 'delivered' | 'deferred' | 'unreachable' | 'failed' | 'pending';
  count: number;
  at: string;
  delivered_at: string;
  reason: string;
  retry_at: string;
  seen_by: string[];
}

export interface Row {
  id: number;
  rev: number;
  at: string;
  kind: Kind;
  who: Who | null;
  to: Who | null;
  label: string;
  text: string;
  note: string;
  tool: Tool | null;
  delivery: Delivery | null;
  children: Row[];
  request: string | null;
  tokens: number | null;
  open: boolean;
  ext: string;
}

export interface Step {
  n: string;
  text: string;
  tags: string[];
  depth: number;
}

export interface Request {
  id: string;
  session_id: string;
  thread: string;
  kind: 'approval' | 'ask';
  at: string;
  title: string;
  tool: string;
  cwd: string;
  command: string;
  options: { id: string; name: string; kind: string; key: string }[];
  parsed: { verdict: string; steps: Step[]; checks: { ok: boolean; text: string }[] } | null;
  explained: {
    state: 'pending' | 'ok' | 'error' | 'off';
    text: string;
    model: string;
    fits: boolean | null;
    fit_note: string;
    misuse: string[];
    worst: string;
    read: string[];
  } | null;
  tag: string;
  question: string;
  choices: { key: string; title: string; detail: string }[];
  via: string;
}

export interface Tree {
  rev: number;
  groups: Group[];
  sessions: Session[];
  requests: Request[];
  counts: { running: number; waiting: number; failed: number };
  harnesses: { key: string; label: string; command: string }[];
  hubs: { source: string; name: string; url: string; state: string; error: string }[];
  host: string;
}

export interface ThreadData {
  key: string;
  tab: string;
  rev: number;
  reset: boolean;
  rows: Row[];
  ids: number[];
}

export interface AgentsLook {
  colors: { you: string; tool: string; message: string; hub: string; needs_you: string };
  names: { color_by: 'agent' | 'template' | 'off'; palette: string[]; self_in_transcript: boolean };
  boxes: { strength: 'subtle' | 'normal' | 'strong'; fold_pairs: boolean };
}

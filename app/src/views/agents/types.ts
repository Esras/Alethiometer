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
  /** A local group's project folder; empty for Scion. */
  project_dir: string;
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
  done?: boolean;
}

export interface ConfigChoice {
  id: string;
  current: string;
  options: { value: string; name: string }[];
}

export interface Session {
  id: string;
  seen_turns?: number | null;
  /** A file its direnv environment came from has changed since it started. */
  env_changed?: boolean;
  source: 'acp' | 'scion';
  group_id: string | null;
  parent_id: string | null;
  name: string;
  template: string;
  harness: string;
  model: string;
  thinking?: ConfigChoice | null;
  host: string;
  cwd: string;
  /** Local only: the project folder and its label. */
  project_dir: string;
  project: string;
  status: Status;
  title: string;
  subtitle: string;
  context: string;
  created_at: string;
  active_at: string;
  color: number;
  stats: {
    ctx_used: number;
    ctx_size: number;
    turns: number;
    cost_usd: number | null;
    cache: string;
    started_at: string;
  };
  facts: { ticket: Fact | null; pr: Fact | null; branch: Fact | null; todo: Fact | null };
  live: boolean;
  problem: string;
  guess: { group_id: string; label: string } | null;
  url: string;
  ext: string;
  /** Held back while mid-turn, oldest first; not real rows until sent. */
  queued: { text: string; images?: { data: string; mime: string }[] }[];
  /** Local only: the OpenShell sandbox it runs in (`cwd` is a path inside it). */
  sandbox?: { gateway: string; name: string; upload: string; allow_all: boolean; synced_turns: number } | null;
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
  diff?: string;
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
  images?: { data: string; mime: string }[];
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
  notes?: string[];
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
  /** Text the agent sent with an approval (a gate's prompt). Shown, never explained. */
  message?: string;
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
  /** `free`: the agent asks for your own words after it ("Type something."). */
  choices: { key: string; title: string; detail: string; value?: string; free?: boolean }[];
  via: string;
  /** An ACP elicitation's requestedSchema (JSON Schema, an object of fields). */
  form?: FormSchema | null;
}

export interface FormField {
  type?: string;
  title?: string;
  description?: string;
  default?: unknown;
  oneOf?: { const: string; title?: string; description?: string }[];
  anyOf?: { const: string; title?: string; description?: string }[];
  enum?: string[];
  items?: FormField;
}

export interface FormSchema {
  type: 'object';
  properties: Record<string, FormField>;
  required?: string[];
}

export interface Tree {
  rev: number;
  groups: Group[];
  sessions: Session[];
  requests: Request[];
  counts: { running: number; waiting: number; failed: number };
  harnesses: {
    key: string;
    label: string;
    command: string;
    cache_ttl_min: number | null;
    sandbox_command: string;
    sandbox_from: string;
  }[];
  /** The OpenShell CLI is on the PATH: agents can run in its sandboxes. */
  openshell: boolean;
  hubs: { source: string; name: string; url: string; state: string; error: string }[];
  host: string;
  /** The agent host (app/agentd) local agents run in. */
  agentd: {
    state: 'idle' | 'starting' | 'running' | 'missing' | 'error' | 'unsupported' | '';
    version: string;
    pid: number;
    error: string;
    stale: boolean;
    binary: string;
  };
  /** Done nudges by group id: "completed, #208 merged". */
  nudges: Record<string, string>;
  /** Hub deletes that failed after archiving (retried with ⌘R). */
  hub_failed: number;
}

/** What archiving a row would do (agents.archive_plan). */
export interface Plan {
  target: string;
  kind: 'session' | 'group';
  name: string;
  place: string;
  state: 'ok' | 'not_done' | 'blocked';
  rows: {
    kind: 'group' | 'room' | 'session';
    name: string;
    session: string | null;
    status: Status | null;
    harness: string;
    depth: number;
    lit: boolean;
    tag: string;
  }[];
  reasons: { text: string; note: string }[];
  going: number;
  hub: number;
  /** OpenShell sandboxes it deletes (ones Alethiometer made that nothing else runs in). */
  sandboxes: number;
  local: number;
}

/** An archived group or session, as listed by the tree filter. */
export interface ArchivedItem {
  id: string;
  kind: 'group' | 'session';
  source: 'acp' | 'scion';
  project: string;
  group: string;
  group_id: string | null;
  at: string;
  name: string;
  goal: string | null;
  harness: string | null;
  model: string | null;
  room: string | null;
  line?: string;
  who?: string;
}

/** An archived item opened read-only (agents.archived_open). */
export interface ArchivedOpen {
  item: ArchivedItem;
  data: Partial<Session> & Partial<Group>;
  thread: string;
  rows: Row[];
  members: ArchivedItem[];
  kept: { messages: number; transcripts: number };
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

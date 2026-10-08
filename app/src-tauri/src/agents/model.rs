//! What the Agents tab shows, the same for both sources: groups and sessions
//! for the tree, rows for a thread (a session's transcript or a Scion room),
//! and the requests pinned above the composer (approvals, asks).
//!
//! A thread is keyed `s:<session id>` or `r:<room id>`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Running,
    /// Waiting on you: a pinned approval or ask.
    Waiting,
    #[default]
    Idle,
    Done,
    Failed,
}

/// A group in the tree: a local group you made, or a Scion group thread's
/// agents. Sessions with no group are Unsorted.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Group {
    /// `local:<n>` or `scion:<hub source>:<project id>:<conversation id or "agents">`.
    pub id: String,
    /// `acp` or `scion`.
    pub source: String,
    /// The label above the group: the repo for local groups, the Scion project.
    pub project: String,
    pub name: String,
    pub goal: String,
    /// Host tag: "laptop", "Scion · gcp-1".
    pub host: String,
    /// A Scion group thread, shown as the group's first row.
    pub room: Option<Room>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Room {
    /// The thread key's id: `r:<id>`.
    pub id: String,
    pub name: String,
    pub members: usize,
    pub unread: usize,
    pub active_at: String,
    /// Status line: Hub, thread id, mode, loop cap, message count.
    pub hub: String,
    pub thread_ref: String,
    pub mode: String,
    pub loop_cap: String,
    pub messages: usize,
    pub live: bool,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Session {
    /// `acp-<hex>` or `scion:<hub source>:<agent id>`.
    pub id: String,
    pub source: String,
    pub group_id: Option<String>,
    /// A subagent sits under its parent, indented.
    pub parent_id: Option<String>,
    pub name: String,
    /// Settings harness key (local) or Scion template name: what `names.color_by = template` colours by.
    pub template: String,
    /// "Claude Code", "Pi".
    pub harness: String,
    /// Short model name: "sonnet".
    pub model: String,
    pub host: String,
    pub cwd: String,
    pub status: Status,
    /// Pane header title and subtitle.
    pub title: String,
    pub subtitle: String,
    /// Context line under the name: "Scion Hub · project ingest-svc".
    pub context: String,
    pub created_at: String,
    pub active_at: String,
    /// Index into `agents.names.palette`, unique within the group (persisted).
    pub color: usize,
    pub stats: Stats,
    pub facts: Facts,
    /// A process (local) or Hub agent is attached and can take a message now.
    pub live: bool,
    /// Why it isn't live, when it should be ("pi-acp not found", "not signed in").
    pub problem: String,
    /// Unsorted only: the AI's guess at a group.
    pub guess: Option<Guess>,
    /// Open on Hub / in Zed.
    pub url: String,
    /// Agent-side id: the ACP sessionId (to resume or load), the Scion agent id.
    #[serde(default)]
    pub ext: String,
    /// Composer queue: held back while mid-turn, oldest first; not real rows until sent.
    #[serde(default)]
    pub queued: Vec<QueuedMessage>,
    /// The queue marker's peak count this busy spell, so it never counts back down.
    #[serde(default)]
    pub queue_peak: usize,
    /// Row id of the active "N queued" marker, if any; cleared once the queue drains.
    #[serde(default)]
    pub queue_marker: Option<u64>,
    /// The agent's thinking option (ACP config option of category `thought_level`), if it offers one.
    #[serde(default)]
    pub thinking: Option<ConfigChoice>,
    /// Turns you had seen the last time you looked (or the count before its first turn ended, for a session that never had one); `stats.turns` past it is an ended turn you haven't seen.
    #[serde(default)]
    pub seen_turns: Option<u64>,
    /// What direnv watched for the environment it started in (local), and
    /// whether one of those files has changed since: the agent still has the old one.
    #[serde(default)]
    pub env_watches: Vec<EnvWatch>,
    #[serde(default)]
    pub env_changed: bool,
    /// Local only: the OpenShell sandbox its adapter runs in (`cwd` is then a path inside it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sandbox: Option<Sandbox>,
}

/// Where an OpenShell session runs (`agents/openshell.rs`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct Sandbox {
    /// Gateway name as `openshell gateway list` shows it; empty is the CLI's active gateway.
    pub gateway: String,
    pub name: String,
    /// The local directory uploaded into it, when Alethiometer created it.
    pub upload: String,
    /// Its permission requests are answered with the agent's own allow option (any agent but Claude Code, which runs in auto mode).
    pub allow_all: bool,
    /// `stats.turns` when its changes were last brought back (or found to be none): archiving deletes the sandbox, so turns past it are work that would go with it.
    pub synced_turns: u64,
}

/// One of direnv's watched files (`.envrc`, `flake.lock`…) as it was.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct EnvWatch {
    pub path: String,
    pub modtime: i64,
    pub exists: bool,
}

impl Session {
    pub fn turn_unseen(&self) -> bool {
        self.source == "acp" && self.status == Status::Idle && self.seen_turns.is_some_and(|n| self.stats.turns > n)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ConfigChoice {
    pub id: String,
    pub current: String,
    pub options: Vec<ChoiceOption>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ChoiceOption {
    pub value: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct QueuedMessage {
    pub text: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<RowImage>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Stats {
    pub ctx_used: u64,
    pub ctx_size: u64,
    pub turns: u64,
    pub cost_usd: Option<f64>,
    /// "warm ~3m": when the prompt cache was last written, if the agent says.
    pub cache: String,
    pub started_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Facts {
    pub ticket: Option<Fact>,
    pub pr: Option<Fact>,
    pub branch: Option<Fact>,
    pub todo: Option<Fact>,
}

/// One facts-box line: "ALE-92" + "In progress".
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Fact {
    pub label: String,
    pub detail: String,
    pub url: String,
    /// A ticket in Jira's done category, a merged PR.
    #[serde(default)]
    pub done: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Guess {
    pub group_id: String,
    /// "alethiometer › Agents tab".
    pub label: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, Default)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// You: a prompt, instruction, mention or DM.
    You,
    /// The session's own text.
    #[default]
    Output,
    Thinking,
    /// A tool call; its result is `tool.result` (the second line in the box).
    Tool,
    /// Agent ↔ agent, agent → room, agent → you, subagent task and result.
    Message,
    /// Started, running → completed, joined.
    State,
    /// Hub notice (loop cap, rate limit).
    Hub,
    /// Needs you: a question.
    Ask,
    /// Needs you: a tool waiting for approval.
    Approval,
    /// A message that couldn't be delivered.
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Row {
    /// Unique and increasing within the thread.
    pub id: u64,
    /// Store revision of the last change (the UI asks for rows newer than its own).
    #[serde(default)]
    pub rev: u64,
    pub at: String,
    pub kind: Kind,
    /// Who: None is the thread's own session (its rows drop the name).
    pub who: Option<Who>,
    /// To whom: an agent, `#room` or you.
    pub to: Option<Who>,
    /// "prompt", "task", "reply", "instruction", "mention", "input-needed", "needs approval", "result".
    pub label: String,
    pub text: String,
    /// Right of the header, before the time: "subagent · haiku".
    pub note: String,
    pub tool: Option<Tool>,
    /// Pasted into the composer; sent as ACP `image` content blocks.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<RowImage>,
    pub delivery: Option<Delivery>,
    /// Rows inside this row's box (a subagent's task, its folded tool calls, its result).
    pub children: Vec<Row>,
    /// The pinned request this row stands for.
    pub request: Option<String>,
    /// Thinking: rough token count.
    pub tokens: Option<u64>,
    /// Still streaming.
    pub open: bool,
    /// Agent-side id (ACP toolCallId, Scion message id) for updates in place.
    #[serde(default)]
    pub ext: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Who {
    /// "You", an agent name, `#room`, "Hub".
    pub name: String,
    /// Session id, for its colour and to open it.
    pub session: Option<String>,
    /// you | agent | room | hub
    pub kind: String,
}

impl Who {
    pub fn you() -> Who {
        Who { name: "You".into(), session: None, kind: "you".into() }
    }
    pub fn agent(name: &str, session: Option<String>) -> Who {
        Who { name: name.into(), session, kind: "agent".into() }
    }
    pub fn room(name: &str, id: &str) -> Who {
        Who { name: format!("#{name}"), session: Some(format!("r:{id}")), kind: "room".into() }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Tool {
    /// Short verb in the box: read, edit, grep, run, bash, fetch.
    pub name: String,
    /// What it acts on: a path, a pattern, a command.
    pub detail: String,
    /// pending | in_progress | completed | failed
    pub status: String,
    pub ms: Option<u64>,
    /// The result line(s), as the agent reported them.
    pub result: String,
    /// Lines added/removed, for edits.
    pub added: Option<u64>,
    pub removed: Option<u64>,
    /// Folded summary for a subagent's calls: "5 tool calls · read ×3, grep ×2 · 4.8s".
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct RowImage {
    /// Base64-encoded, no data: URI prefix.
    pub data: String,
    pub mime: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Delivery {
    /// delivered | deferred | unreachable | failed | pending
    pub state: String,
    /// Recipients it reached (the ✓ count).
    pub count: usize,
    pub at: String,
    pub delivered_at: String,
    /// Why (failed: the Hub's reason; deferred: "handler was mid-turn").
    pub reason: String,
    /// Failed: when the Hub retries next.
    pub retry_at: String,
    pub seen_by: Vec<String>,
}

/// Pinned above the composer and listed under Needs you.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Request {
    pub id: String,
    pub session_id: String,
    /// The thread it's pinned in (the session's, or the room it was asked in).
    pub thread: String,
    /// approval | ask
    pub kind: String,
    pub at: String,
    /// Plain-language title: "Run a shell command".
    pub title: String,
    /// Tool name ("bash") and where: "bash · ~/src/alethiometer".
    pub tool: String,
    pub cwd: String,
    /// The full command or path the approval is about.
    pub command: String,
    /// Text the agent sent with an approval (a gate's prompt, a description).
    /// Shown as is; never given to the explainer.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub message: String,
    pub options: Vec<PermOption>,
    pub parsed: Option<crate::agents::shell::Parsed>,
    pub explained: Option<Explained>,
    /// Needs you tag: low | outside repo | question | writes | network.
    pub tag: String,
    /// Ask: the question, and choices read from the message.
    pub question: String,
    pub choices: Vec<Choice>,
    /// "input-needed · 4m" source label, and the room it came through.
    pub via: String,
    /// An ACP elicitation's `requestedSchema`: the fields the agent asked for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub form: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct PermOption {
    pub id: String,
    /// The agent's own label.
    pub name: String,
    /// allow_once | allow_always | reject_once | reject_always
    pub kind: String,
    /// O, S, A, D.
    pub key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Explained {
    /// pending | ok | error | off
    pub state: String,
    pub text: String,
    /// "luna · Bedrock US".
    pub model: String,
    /// Read as if the command was planted: whether it does only what its
    /// stated purpose and the task need, and what goes beyond them.
    #[serde(default)]
    pub fits: Option<bool>,
    #[serde(default)]
    pub fit_note: String,
    /// Concrete ways this command could do harm; empty when it's benign.
    #[serde(default)]
    pub misuse: Vec<String>,
    #[serde(default)]
    pub worst: String,
    /// Project files read to see into the command (scripts it runs).
    #[serde(default)]
    pub read: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Choice {
    pub key: String,
    pub title: String,
    pub detail: String,
    /// What answering with it sends (an elicitation option's `const`).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub value: String,
    /// The agent asks for your own words after this one (pi's "Type something."):
    /// the card takes them with the choice, as a deny takes its reason.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub free: bool,
}

/// Rough tokens for text the agent didn't count (thinking): ~4 chars a token.
pub fn rough_tokens(text: &str) -> u64 {
    (text.chars().count() as u64).div_ceil(4)
}

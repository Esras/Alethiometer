//! The Agents tab's stored state (local groups, sessions, transcripts, the
//! agent host's log positions, colours), and what the facts box and Agents in
//! Incoming read. The models are the tab's own (`agents::model`).

pub mod model;

use self::model::{Group, Row, Session};
use crate::error::Result;
use crate::time::{Due, Ts};
use std::collections::{HashMap, HashSet};

/// A local agent's place in the agent host's log, and its turn state.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct HostState {
    pub seq: u64,
    pub turn: serde_json::Value,
}

/// Local state at startup. Transcripts load when opened.
#[derive(Debug, Default)]
pub struct SavedAgents {
    pub groups: Vec<Group>,
    pub sessions: Vec<Session>,
    pub hosts: Vec<(String, HostState)>,
    /// The last stored row id of each thread.
    pub last_row_ids: HashMap<String, u64>,
    /// Ids of every archived group and session: never put back in the tree.
    pub archived: HashSet<String>,
    /// Done nudges put off with Not now, until when.
    pub snoozed: HashMap<String, Ts>,
}

/// An archived group or session, as it was when archived.
#[derive(Debug, Clone)]
pub struct Archived {
    pub id: String,
    /// group | session
    pub kind: String,
    pub source: String,
    pub project: String,
    pub group_name: String,
    pub group_id: Option<String>,
    pub at: Ts,
    /// The `Group` or `Session` as JSON.
    pub data: String,
}

/// An archived item matching the tree filter, and the first row that matched.
#[derive(Debug, Clone)]
pub struct ArchivedMatch {
    pub item: Archived,
    /// The matching row's thread and JSON, when the match was in a transcript.
    pub row: Option<(String, String)>,
}

/// A Jira ticket for the facts box.
#[derive(Debug, Clone)]
pub struct TicketRow {
    pub summary: Option<String>,
    pub url: Option<String>,
    /// Its status is in Jira's done category.
    pub done: bool,
}

/// A GitHub PR for the facts box.
#[derive(Debug, Clone)]
pub struct PrRow {
    pub url: Option<String>,
    pub status: String,
    pub draft: bool,
    pub merged: bool,
}

/// An open task naming a ticket.
#[derive(Debug, Clone)]
pub struct TaskRow {
    pub title: String,
    pub source_kind: String,
    pub due: Option<Due>,
    pub url: Option<String>,
}

/// A signal of the built-in agents source as stored.
#[derive(Debug, Clone)]
pub struct AgentSignal {
    pub external_id: String,
    pub seen: bool,
    /// Marked seen by us because it no longer applied (`$.auto_seen`).
    pub auto_seen: bool,
    pub session: String,
}

pub trait AgentViews {
    fn saved(&self) -> Result<SavedAgents>;
    /// A thread's stored rows, in order. Rows that don't parse are left out.
    fn transcript(&self, thread: &str) -> Result<Vec<Row>>;
    /// The Jira issue with this key.
    fn ticket(&self, key: &str) -> Result<Option<TicketRow>>;
    /// Archived items whose name, goal or title contains `q`, or one of whose
    /// rows does (its text or a tool's target or result), newest first. ASCII
    /// case-insensitive.
    fn archived_matching(&self, q: &str, limit: usize) -> Result<Vec<ArchivedMatch>>;
    fn archived(&self, id: &str) -> Result<Option<Archived>>;
    /// Archived sessions in a group archived with them.
    fn archived_members(&self, group_id: &str) -> Result<Vec<Archived>>;
    /// How many sessions have been archived.
    fn archived_sessions(&self) -> Result<usize>;
    /// The latest GitHub PR for this branch, or with this key in its title.
    fn pr(&self, branch: Option<&str>, key: Option<&str>) -> Result<Option<PrRow>>;
    /// Tasks not done whose id is the key or whose title names it.
    fn open_tasks_for(&self, key: &str) -> Result<Vec<TaskRow>>;
    fn signals(&self, source_id: i64) -> Result<Vec<AgentSignal>>;
    /// The topic a Jira ticket is filed (or suggested) under, confirmed first.
    fn ticket_topic(&self, key: &str) -> Result<Option<i64>>;
    /// The topic another signal of this session is filed (or suggested) under.
    fn session_topic(&self, source_id: i64, except_item: i64, session: &str) -> Result<Option<i64>>;
}

pub trait AgentState {
    fn put_host(&self, id: &str, h: &HostState) -> Result<()>;
    fn remove_host(&self, id: &str) -> Result<()>;
    fn put_group(&self, g: &Group) -> Result<()>;
    fn remove_group(&self, id: &str) -> Result<()>;
    fn put_session(&self, s: &Session) -> Result<()>;
    /// A session and its transcript.
    fn remove_session(&self, id: &str) -> Result<()>;
    fn put_row(&self, thread: &str, r: &Row) -> Result<()>;
    /// Move a group or session out of the tree's tables into the archive (a
    /// session's agent-host state goes too). Its rows stay.
    fn archive(&self, a: &Archived) -> Result<()>;
    /// Put off a group's done nudge.
    fn snooze_nudge(&self, group: &str, until: Ts) -> Result<()>;
    /// `name`'s colour in `group`: assigned in order of creation, unique
    /// within the group while the palette lasts, and kept.
    fn color(&self, group: &str, name: &str, palette_len: usize) -> Result<usize>;
    /// Mark a signal of the agents source seen because it no longer applies.
    fn auto_seen(&self, source_id: i64, external_id: &str) -> Result<()>;
}

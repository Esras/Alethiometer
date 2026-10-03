//! The Agents tab's stored state (local groups, sessions, transcripts, the
//! agent host's log positions, colours), and what the facts box and Agents in
//! Incoming read. The models are the tab's own (`agents::model`).

use crate::agents::model::{Group, Row, Session};
use crate::error::Result;
use crate::time::Due;
use std::collections::HashMap;

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
}

/// A Jira ticket for the facts box.
#[derive(Debug, Clone)]
pub struct TicketRow {
    pub summary: Option<String>,
    pub url: Option<String>,
}

/// A GitHub PR for the facts box.
#[derive(Debug, Clone)]
pub struct PrRow {
    pub url: Option<String>,
    pub status: String,
    pub draft: bool,
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
    /// `name`'s colour in `group`: assigned in order of creation, unique
    /// within the group while the palette lasts, and kept.
    fn color(&self, group: &str, name: &str, palette_len: usize) -> Result<usize>;
    /// Mark a signal of the agents source seen because it no longer applies.
    fn auto_seen(&self, source_id: i64, external_id: &str) -> Result<()>;
}

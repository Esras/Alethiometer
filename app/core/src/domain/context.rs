//! What the work AI is told: rows for its prompts (work sources only).

use crate::error::Result;
use crate::time::{Due, Ts};

/// An item as a prompt shows it.
#[derive(Debug, Clone)]
pub struct ContextItem {
    pub id: i64,
    /// The source's tag; prompts check it again (work only).
    pub tag: String,
    pub source_kind: String,
    pub kind: String,
    pub title: String,
    pub summary: Option<String>,
    pub body: String,
    pub at: Ts,
    pub ended: Option<Ts>,
    pub due: Option<Due>,
    pub status: String,
    pub owner: Option<String>,
    /// Seen in Incoming and never filed under a topic (dismissed, or left unfiled).
    pub dismissed: bool,
}

/// An active topic for the topics overview.
#[derive(Debug, Clone)]
pub struct TopicOverview {
    pub id: i64,
    pub name: String,
    pub note_path: Option<String>,
    /// Confirmed work items.
    pub items: i64,
    pub last: Option<Ts>,
}

/// A titled work calendar event.
#[derive(Debug, Clone)]
pub struct WorkEvent {
    pub item_id: Option<i64>,
    pub tag: String,
    pub start: Ts,
    pub end: Ts,
    pub title: String,
    pub attendees: Option<String>,
    pub tentative: bool,
}

pub trait ContextViews {
    /// Work items linked to the topic in `state` ("suggested": only unseen ones), newest first.
    fn topic_items(&self, topic_id: i64, state: &str) -> Result<Vec<ContextItem>>;
    /// An item's latest `n` messages, oldest first: (at, author, text).
    fn latest_messages(&self, item_id: i64, n: usize) -> Result<Vec<(Ts, String, String)>>;
    /// Active topics by name.
    fn topics_overview(&self) -> Result<Vec<TopicOverview>>;
    /// Titles of a topic's latest confirmed work items.
    fn recent_titles(&self, topic_id: i64, n: usize) -> Result<Vec<String>>;
    /// Titled work events (no life blocks) starting in `[from, to)`.
    fn work_events(&self, from: Ts, to: Ts) -> Result<Vec<WorkEvent>>;
    /// Work items (not events) with activity in `[from, to)`, newest first.
    fn touched(&self, from: Ts, to: Ts) -> Result<Vec<ContextItem>>;
}

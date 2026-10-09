//! Notes: a page of lines per day, each with a destination.

use crate::error::Result;
use crate::time::Date;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Where a line goes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Target {
    /// topic | person | app | keep
    #[serde(rename = "type")]
    pub ty: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<i64>,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    /// For app targets: jira | marvin.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app: Option<String>,
    /// Jira key for app targets ("PLAT-1201").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    /// Topic behind an app/person target, used to file the Incoming signal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub topic_id: Option<i64>,
    /// Quiet offer shown under the destination ("create Jira task"). Never acted on by itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offer: Option<String>,
    /// The user accepted the offer: on send, a create-issue write is queued in the outbox.
    #[serde(default, skip_serializing_if = "is_false")]
    pub create: bool,
    /// True for a suggestion; false for the user's own pick.
    #[serde(default)]
    pub auto: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl Target {
    /// Same destination, ignoring suggestion details.
    pub fn same(&self, o: &Target) -> bool {
        self.ty == o.ty && self.id == o.id && self.key == o.key && self.label == o.label
    }
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// A line's own choices and receipt: the user's destination (if any), whether
/// it has no glyph, and the life-line receipt fields.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LineMeta {
    #[serde(flatten, default)]
    pub to: Option<Target>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub bare: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct NoteLine {
    pub id: i64,
    pub kind: String,
    pub text: String,
    pub state: String,
    pub meta: LineMeta,
}

/// The Jira task a line asked for, as the outbox last knows it.
#[derive(Debug, Clone)]
pub struct JiraRequest {
    pub state: String,
    pub error: Option<String>,
    /// Once Jira confirmed it.
    pub key: Option<String>,
    pub url: Option<String>,
}

/// A topic as the Notes suggestion index matches lines against it. Its
/// linked items' text is read separately (`NotesViews::topic_text`), only
/// when `corpus_rev` moved; its note is read from the vault.
#[derive(Debug, Clone)]
pub struct TopicText {
    pub id: i64,
    pub name: String,
    pub category: String,
    pub obsidian_path: Option<String>,
    /// Bumped by the database when its links or a linked item's text change.
    pub corpus_rev: i64,
    /// People linked to it by hand.
    pub people: Vec<i64>,
}

/// What destination suggestions match against.
#[derive(Debug, Clone, Default)]
pub struct Corpus {
    /// (id, name)
    pub people: Vec<(i64, String)>,
    /// Topics not archived, busiest first.
    pub topics: Vec<TopicText>,
    /// Jira issue titles ("KEY · summary") and their confirmed topic, newest first.
    pub issues: Vec<(String, Option<i64>)>,
}

pub trait NotesViews {
    fn lines(&self, date: Date) -> Result<Vec<NoteLine>>;
    /// The latest Jira task request per line.
    fn jira_requests(&self, line_ids: &[i64]) -> Result<HashMap<i64, JiraRequest>>;
    fn corpus(&self) -> Result<Corpus>;
    /// Titles, summaries and bodies of a topic's linked items.
    fn topic_text(&self, topic_id: i64) -> Result<String>;
    /// Lines on a day's page, and how many of them are held.
    fn page_counts(&self, date: Date) -> Result<(i64, i64)>;
}

/// A day page's lines.
pub trait NoteLines {
    fn insert(&self, date: Date, position: i64, kind: &str, text: &str, meta: &LineMeta) -> Result<i64>;
    fn update(&self, id: i64, position: i64, kind: &str, text: &str, meta: &LineMeta) -> Result<()>;
    fn set_position(&self, id: i64, position: i64) -> Result<()>;
    fn set_state(&self, id: i64, state: &str, meta: &LineMeta) -> Result<()>;
    fn delete(&self, id: i64) -> Result<()>;
}

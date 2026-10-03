//! Topics: the root object signals are filed under.

use crate::error::Result;
use crate::time::{Due, Ts};

/// A topic by name, as lists show it.
#[derive(Debug, Clone, serde::Serialize)]
pub struct TopicRef {
    pub id: i64,
    pub name: String,
    pub category: String,
}

/// A topic as the Topics index lists it.
#[derive(Debug, Clone)]
pub struct TopicSummary {
    pub id: i64,
    pub name: String,
    pub category: String,
    pub obsidian_path: Option<String>,
    /// Unseen signals linked to it (suggested or confirmed).
    pub new_count: i64,
    /// Open tasks, issues and emails filed under it that someone owns.
    pub open_count: i64,
    /// Latest activity of a confirmed signal up to now, and its source kind.
    pub last_signal_at: Option<Ts>,
    pub last_source: Option<String>,
    /// The next linked meeting within the window: start and title.
    pub next_meeting: Option<(Ts, String)>,
}

#[derive(Debug, Clone)]
pub struct TopicHead {
    pub name: String,
    pub category: String,
    pub obsidian_path: Option<String>,
    pub score: Option<f64>,
    /// Reason pills, as JSON.
    pub pills_json: String,
    pub archived: bool,
}

/// An item confirmed under a topic.
#[derive(Debug, Clone)]
pub struct LinkedItem {
    pub id: i64,
    pub source_kind: String,
    pub kind: String,
    pub title: String,
    pub summary: Option<String>,
    pub at: Ts,
    pub ended: Option<Ts>,
    pub due: Option<Due>,
    pub status: String,
    pub msg_count: Option<i64>,
    pub owner: Option<String>,
    /// The recurring series an event instance belongs to.
    pub series: Option<String>,
}

pub trait TopicViews {
    /// Active or archived topics. `now` bounds the last signal; the next
    /// meeting is looked for in `(now, meetings_until]`.
    fn list(&self, archived: bool, now: Ts, meetings_until: Ts) -> Result<Vec<TopicSummary>>;
    fn head(&self, id: i64) -> Result<Option<TopicHead>>;
    fn get_ref(&self, id: i64) -> Result<Option<TopicRef>>;
    /// Topics not archived with a note: (vault path as stored, id).
    fn note_paths(&self) -> Result<Vec<(String, i64)>>;
    fn confirmed_items(&self, id: i64) -> Result<Vec<LinkedItem>>;
    /// GitHub repos linked to the topic, sorted.
    fn repos(&self, id: i64) -> Result<Vec<String>>;
}

pub trait Topics {
    /// A topic not archived with this name, ignoring case.
    fn find_by_name(&self, name: &str) -> Result<Option<i64>>;
    fn create(&self, name: &str, category: &str) -> Result<i64>;
    /// False when there's no such topic.
    fn rename(&self, id: i64, name: &str) -> Result<bool>;
    fn set_category(&self, id: i64, category: &str) -> Result<bool>;
    fn set_archived(&self, id: i64, archived: bool) -> Result<bool>;
    /// Record where its note is, unless it already has one.
    fn set_note_path_if_unset(&self, id: i64, path: &str) -> Result<()>;
    /// Items confirmed under the topic.
    fn filed_items(&self, id: i64) -> Result<Vec<i64>>;
    /// Merge `from` into `into` (named `into_name`, category `into_category`):
    /// every link moves (where the item already has one to `into`, the stronger
    /// state wins: confirmed, suggested, wrong), then everything else naming
    /// `from` (people and repos linked by hand, Glean verdicts, Notes lines)
    /// moves, `from`'s vector goes, `into`'s is marked stale, and `from` is
    /// deleted. The one place that knows every table referring to a topic; a
    /// test checks it against the schema's foreign keys.
    fn merge(&self, from: i64, into: i64, into_name: &str, into_category: &str) -> Result<()>;
}

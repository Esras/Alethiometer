//! Items: every signal a source (or the app) produced.

use crate::error::Result;
use crate::time::{Date, Ts};

/// An item the app makes itself (a Notes line sent to Incoming, a local todo,
/// a Jira issue it just created).
#[derive(Debug, Clone, Default)]
pub struct NewItem {
    pub source_id: i64,
    pub external_id: String,
    pub kind: String,
    pub title: String,
    pub summary: Option<String>,
    pub url: Option<String>,
    pub occurred_at: Ts,
    pub due_day: Option<Date>,
    pub status: String,
    pub owner: Option<String>,
    /// False: a new signal in Incoming.
    pub seen: bool,
    /// Source-specific fields, as JSON.
    pub raw_json: String,
}

/// An item with its source, as actions on it need it.
#[derive(Debug, Clone)]
pub struct ItemRef {
    pub id: i64,
    pub source_id: i64,
    pub source_kind: String,
    pub external_id: String,
    pub kind: String,
    pub title: String,
    pub url: Option<String>,
    pub parent_id: Option<i64>,
    pub seen: bool,
    /// Source-specific fields, as JSON.
    pub raw_json: String,
}

pub trait Items {
    fn insert(&self, item: &NewItem) -> Result<i64>;
    /// Insert unless the source already has that external id; the item's id either way.
    fn insert_or_get(&self, item: &NewItem) -> Result<i64>;
    fn get(&self, id: i64) -> Result<Option<ItemRef>>;
    fn find(&self, source_id: i64, external_id: &str) -> Result<Option<i64>>;
    fn set_external_id(&self, id: i64, external_id: &str) -> Result<()>;
    /// False when there's no such item.
    fn set_seen(&self, id: i64, seen: bool) -> Result<bool>;
    fn set_seen_many(&self, ids: &[i64], seen: bool) -> Result<()>;
    fn set_status(&self, id: i64, status: &str) -> Result<()>;
    fn set_status_by_external(&self, source_id: i64, external_id: &str, status: &str) -> Result<()>;
    /// Open tasks of a source whose external id starts with `prefix`: (id, external id).
    fn open_tasks(&self, source_id: i64, prefix: &str) -> Result<Vec<(i64, String)>>;
    /// Mark (or unmark) a split part as the family's dismissed messages.
    fn set_dismissed(&self, id: i64, dismissed: bool) -> Result<()>;
    /// A created Jira issue's link and label on the item that asked for it.
    fn set_url_and_summary(&self, id: i64, url: Option<&str>, summary: &str) -> Result<()>;
    /// The other instances of a recurring event's series, in the same source.
    fn series_siblings(&self, id: i64) -> Result<Vec<i64>>;
    /// The item's split family: root first, then the parts.
    fn family(&self, id: i64) -> Result<Vec<ItemRef>>;
}

/// Reads about single items, outside a unit of work.
pub trait ItemViews {
    /// Whether the source's item is seen, if there is one.
    fn seen_by_external(&self, source_id: i64, external_id: &str) -> Result<Option<bool>>;
    fn seen(&self, id: i64) -> Result<Option<bool>>;
    /// The item holding message `key` of `channel` as its own (not context,
    /// not a reply): where that message's thread replies go.
    fn holder(&self, source_id: i64, channel: Option<&str>, key: &str) -> Result<Option<i64>>;
}

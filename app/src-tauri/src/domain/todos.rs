//! Todos.

use crate::domain::topics::TopicRef;
use crate::error::Result;
use crate::time::Due;
use serde::Serialize;

/// A todo waiting on a signal in Incoming, made when the signal is filed.
#[derive(Debug, Serialize, Clone)]
pub struct Held {
    pub target: String,
    pub text: String,
    pub due: Option<String>,
}

/// An open work item you own in a system that holds todos.
#[derive(Debug, Clone)]
pub struct TodoRow {
    pub item_id: i64,
    pub source_kind: String,
    pub title: String,
    pub due: Option<Due>,
    pub url: Option<String>,
    /// The source's fields, as JSON.
    pub raw_json: String,
    pub summary: Option<String>,
    /// Confirmed, else suggested.
    pub topic: Option<TopicRef>,
    pub priority: Option<String>,
    /// Minutes, and who estimated them.
    pub estimate: Option<(i64, String)>,
}

pub trait TodoViews {
    /// Open todos, by item id.
    fn open(&self) -> Result<Vec<TodoRow>>;
    /// The todos held on these items, by item.
    fn held(&self, item_ids: &[i64]) -> Result<Vec<(i64, Held)>>;
}

/// Todo state the app keeps: todos held on signals, and estimates.
pub trait Todos {
    /// The todo held (not discarded) on an item.
    fn held(&self, item_id: i64) -> Result<Option<Held>>;
    /// Hold a todo on an item (replacing one already there).
    fn hold(&self, item_id: i64, todo: &Held) -> Result<()>;
    fn unhold(&self, item_id: i64) -> Result<()>;
    /// Set aside while the item is dismissed, or held again.
    fn set_discarded(&self, item_id: i64, discarded: bool) -> Result<()>;
    /// Your estimate in minutes; None clears it.
    fn set_estimate(&self, item_id: i64, minutes: Option<i64>) -> Result<()>;
}

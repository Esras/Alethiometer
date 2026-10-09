//! The small model's triage: Unsorted signals it suggests topics for (or splits).

use crate::error::Result;
use crate::time::Ts;

/// An unseen work item with no live link.
#[derive(Debug, Clone)]
pub struct UnsortedItem {
    pub id: i64,
    pub source_kind: String,
    pub kind: String,
    pub title: String,
    pub summary: Option<String>,
    pub parent_id: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct PromptMessage {
    pub id: i64,
    pub at: Ts,
    pub author: String,
    pub text: String,
}

/// A message shown before the item for context, with the topic the user
/// filed it under elsewhere, if they did.
#[derive(Debug, Clone)]
pub struct ContextMessage {
    pub at: Ts,
    pub author: String,
    pub text: String,
    pub topic: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SuggestRun {
    pub status: String,
    pub message: Option<String>,
    pub suggested: i64,
    pub at: Ts,
    pub last_ok_at: Option<Ts>,
}

pub trait SuggestViews {
    /// Unsorted work items, newest activity first.
    fn unsorted(&self, limit: usize) -> Result<Vec<UnsortedItem>>;
    fn unsorted_count(&self) -> Result<i64>;
    /// Topics the user marked wrong for the item.
    fn wrong_topics(&self, item_id: i64) -> Result<Vec<i64>>;
    /// The item's own top-level messages, oldest first.
    fn own_messages(&self, item_id: i64) -> Result<Vec<PromptMessage>>;
    /// Thread replies, oldest first, with the id of the message they reply to.
    fn replies(&self, item_id: i64) -> Result<Vec<(i64, PromptMessage)>>;
    fn context_messages(&self, item_id: i64) -> Result<Vec<ContextMessage>>;
    fn last_run(&self) -> Result<Option<SuggestRun>>;
    /// Whether the item has a link other than `wrong`.
    fn has_live_link(&self, item_id: i64) -> Result<bool>;
}

pub trait SuggestRuns {
    /// The latest run's outcome; a success also stamps `last_ok_at`.
    fn record(&self, status: &str, message: Option<&str>, suggested: i64) -> Result<()>;
}

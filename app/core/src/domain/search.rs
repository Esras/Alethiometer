//! Search over items (full text) and topics (by name).

use crate::error::Result;
use crate::time::Ts;

#[derive(Debug, Clone)]
pub struct TopicHit {
    pub id: i64,
    pub name: String,
    pub category: String,
}

/// An item ⌘K lists.
#[derive(Debug, Clone)]
pub struct QuickHit {
    pub id: i64,
    pub title: String,
    pub source_kind: String,
    pub at: Ts,
    /// A topic it's linked to (not marked wrong).
    pub topic_id: Option<i64>,
}

/// The topic a search result is filed or suggested under.
#[derive(Debug, Clone)]
pub struct HitTopic {
    pub id: i64,
    pub name: String,
    pub category: String,
    pub state: String,
}

/// A result in the Search view.
#[derive(Debug, Clone)]
pub struct SearchHit {
    pub id: i64,
    pub title: String,
    pub source_kind: String,
    /// Last activity.
    pub at: Ts,
    /// The matching passage, matches between `\u{1}` and `\u{2}`.
    pub snippet: String,
    pub summary: String,
    /// Confirmed, else the latest suggested, among topics not archived.
    pub topic: Option<HitTopic>,
    /// Top-level messages.
    pub msgs: i64,
}

pub trait SearchViews {
    /// Topics not archived whose name contains `text`, by name.
    fn topics_named(&self, text: &str, limit: usize) -> Result<Vec<TopicHit>>;
    /// Work items matching a full-text query (`queries::search::fts_query`), best first.
    fn quick(&self, fts: &str, limit: usize) -> Result<Vec<QuickHit>>;
    fn results(&self, fts: &str, limit: usize) -> Result<Vec<SearchHit>>;
}

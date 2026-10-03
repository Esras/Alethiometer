//! Items: every signal a source (or the app) produced.

use crate::error::Result;
use crate::time::Ts;

/// An item the app makes itself (a Notes line sent to Incoming, a local todo).
#[derive(Debug, Clone)]
pub struct NewItem {
    pub source_id: i64,
    pub external_id: String,
    pub kind: String,
    pub title: String,
    pub summary: Option<String>,
    pub occurred_at: Ts,
    pub status: String,
    pub owner: Option<String>,
    /// False: a new signal in Incoming.
    pub seen: bool,
    /// Source-specific fields, as JSON.
    pub raw_json: String,
}

pub trait Items {
    fn insert(&self, item: &NewItem) -> Result<i64>;
}

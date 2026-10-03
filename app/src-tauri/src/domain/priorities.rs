//! Priorities (8b): scored work items in four quadrants.

use crate::domain::topics::TopicRef;
use crate::error::Result;
use crate::time::{Due, Ts};

/// A scored item with what the Selected bar shows about it.
#[derive(Debug, Clone)]
pub struct PriorityRow {
    pub item_id: i64,
    pub quadrant: String,
    pub score: f64,
    /// Stored reasons, as JSON.
    pub reasons_json: String,
    pub title: String,
    pub summary: Option<String>,
    pub body: String,
    pub url: Option<String>,
    pub at: Ts,
    pub due: Option<Due>,
    pub kind: String,
    pub source_kind: String,
    pub author: Option<String>,
    /// The item's own calendar event.
    pub event: Option<(Ts, Ts)>,
    pub attendees: Option<String>,
    /// Confirmed, else suggested.
    pub topic: Option<TopicRef>,
}

pub trait PriorityViews {
    /// Open work items with a priority, highest score first.
    fn list(&self) -> Result<Vec<PriorityRow>>;
    /// Start and end of the item's own meeting, else the next one linked to
    /// its topic: not a life block, not over at `now`, starting in `[day_start, day_end)`.
    fn next_meeting(&self, item_id: i64, topic_id: Option<i64>, now: Ts, day_start: Ts, day_end: Ts) -> Result<Option<(Ts, Ts)>>;
}

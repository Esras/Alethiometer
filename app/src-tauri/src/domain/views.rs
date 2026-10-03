//! Read models: rows as screens list them.

use crate::time::Ts;

/// A calendar event on the day strip.
#[derive(Debug, Clone, PartialEq)]
pub struct DayEvent {
    pub id: i64,
    pub start: Ts,
    pub end: Ts,
    pub title: Option<String>,
    pub category: String,
    pub tentative: bool,
    /// A personal busy block: shown without a title.
    pub life: bool,
    /// Minutes kept free around a life block, when set on it.
    pub buffer_min: Option<i64>,
    pub item_id: Option<i64>,
    pub attendees: Option<String>,
    /// Topics its item is linked to (suggested or confirmed).
    pub topic_ids: Vec<i64>,
}

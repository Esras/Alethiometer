//! The day strip: calendar events today.

use crate::error::Result;
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

pub trait DayStripViews {
    /// Events starting in `[from, to)`, in start order.
    fn blocks(&self, from: Ts, to: Ts) -> Result<Vec<DayEvent>>;
    /// When the next event linked to `topic_id` (not marked wrong) starts after `after`.
    fn next_linked_meeting(&self, topic_id: i64, after: Ts) -> Result<Option<Ts>>;
}

/// Calendar events.
pub trait Events {
    /// Set the buffer kept around a life block. False when `id` isn't one.
    fn set_buffer(&self, id: i64, minutes: i64) -> Result<bool>;
}

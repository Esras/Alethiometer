//! Reflect: reflections per period, the carry list and what the calendar says happened.

use crate::error::Result;
use crate::time::{Date, Ts};

/// A calendar event as Reflect reads it.
#[derive(Debug, Clone)]
pub struct CalendarEvent {
    pub start: Ts,
    pub end: Ts,
    pub title: Option<String>,
    pub category: String,
    pub life: bool,
    /// The event item's source fields, as JSON (join/leave times when known).
    pub raw_json: Option<String>,
    /// The first topic its item is confirmed under.
    pub topic: Option<String>,
}

/// An item on a day's carry list.
#[derive(Debug, Clone)]
pub struct CarryItem {
    pub item_id: i64,
    /// The day it was carried from.
    pub period: Date,
    pub title: String,
    pub owner: Option<String>,
    pub raw_json: String,
    pub action: Option<String>,
}

pub trait ReflectViews {
    /// Events starting in `[from, to)`, in start order.
    fn events(&self, from: Ts, to: Ts) -> Result<Vec<CalendarEvent>>;
    /// Work decisions made in `[from, to)`: title and confirmed topic, oldest first.
    fn decisions(&self, from: Ts, to: Ts) -> Result<Vec<(String, Option<String>)>>;
    /// The stored one-line summary of a reflection: None when there's no
    /// reflection, Some(None) when it has no summary.
    fn summary(&self, scope: &str, period: Date, lens: &str) -> Result<Option<Option<String>>>;
    /// Carry rows from days in `[from, to)`. `latest_open`: only rows not yet
    /// acted on that are an item's latest carry before `to` (a week and up).
    fn carry(&self, from: Date, to: Date, latest_open: bool) -> Result<Vec<CarryItem>>;
    /// Rows from `day` not acted on yet.
    fn open_carry(&self, day: Date) -> Result<i64>;
    /// The outbox state of the calendar hold for an item carried from `period`.
    fn hold_state(&self, item_id: i64, period: Date) -> Result<Option<String>>;
}

pub trait Reflections {
    fn save_summary(&self, scope: &str, period: Date, lens: &str, path: &str, summary: Option<&str>) -> Result<()>;
    /// The action taken on an item carried from `period`: None when it isn't on that list.
    fn carry_action(&self, item_id: i64, period: Date) -> Result<Option<Option<String>>>;
    fn set_carry_action(&self, item_id: i64, period: Date, action: Option<&str>) -> Result<()>;
    /// Put the item on `period`'s list (if it isn't).
    fn carry_to(&self, item_id: i64, period: Date) -> Result<()>;
    /// Take it off `period`'s list, unless it was acted on there.
    fn uncarry(&self, item_id: i64, period: Date) -> Result<()>;
}

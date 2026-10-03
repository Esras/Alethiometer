//! Ports: what the application asks of storage. Implemented in `crate::store`.
//!
//! A service gets `&dyn Reads` (a consistent snapshot) or `&dyn UnitOfWork`
//! (one transaction, committed by the edge if the service returns `Ok`).
//! Neither hands out a connection or runs SQL given as text.

use crate::domain::views::DayEvent;
use crate::error::Result;
use crate::time::Ts;

/// Read models, one port per screen.
pub trait Reads {
    fn day_strip(&self) -> &dyn DayStripViews;
}

/// One request's changes. Reads inside it see its own writes.
pub trait UnitOfWork {
    fn reads(&self) -> &dyn Reads;
    fn events(&self) -> &dyn Events;
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

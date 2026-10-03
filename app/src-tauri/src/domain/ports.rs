//! What the application asks of storage, implemented in `crate::store`.
//!
//! A service gets `&dyn Reads` (a consistent snapshot) or `&dyn UnitOfWork`
//! (one transaction, committed by the edge if the service returns `Ok`).
//! Neither hands out a connection or runs SQL given as text. Each area's
//! ports and models are in `domain/<area>.rs`.

use super::daystrip::{DayStripViews, Events};
use super::incoming::IncomingViews;
use super::people::PeopleViews;
use super::sources::SourceViews;
use super::todos::TodoViews;

/// Read models, one port per screen or area.
pub trait Reads {
    fn day_strip(&self) -> &dyn DayStripViews;
    fn incoming(&self) -> &dyn IncomingViews;
    fn people(&self) -> &dyn PeopleViews;
    fn sources(&self) -> &dyn SourceViews;
    fn todos(&self) -> &dyn TodoViews;
}

/// One request's changes. Reads inside it see its own writes.
pub trait UnitOfWork {
    fn reads(&self) -> &dyn Reads;
    fn events(&self) -> &dyn Events;
}

//! What the application asks of storage, implemented in `crate::store`.
//!
//! A service gets `&dyn Reads` (a consistent snapshot) or `&dyn UnitOfWork`
//! (one transaction, committed by the edge if the service returns `Ok`).
//! Neither hands out a connection or runs SQL given as text. Each area's
//! ports and models are in `domain/<area>.rs`.

use super::daystrip::{DayStripViews, Events};
use super::incoming::IncomingViews;
use super::items::Items;
use super::links::Links;
use super::notes::{NoteLines, NotesViews};
use super::outbox::Outbox;
use super::people::PeopleViews;
use super::search::SearchViews;
use super::sources::{SourceViews, Sources};
use super::todos::TodoViews;
use super::topics::TopicViews;

/// Read models, one port per screen or area.
pub trait Reads {
    fn day_strip(&self) -> &dyn DayStripViews;
    fn incoming(&self) -> &dyn IncomingViews;
    fn notes(&self) -> &dyn NotesViews;
    fn people(&self) -> &dyn PeopleViews;
    fn search(&self) -> &dyn SearchViews;
    fn sources(&self) -> &dyn SourceViews;
    fn todos(&self) -> &dyn TodoViews;
    fn topics(&self) -> &dyn TopicViews;
}

/// One request's changes. Reads inside it see its own writes.
pub trait UnitOfWork {
    fn reads(&self) -> &dyn Reads;
    fn events(&self) -> &dyn Events;
    fn items(&self) -> &dyn Items;
    fn links(&self) -> &dyn Links;
    fn note_lines(&self) -> &dyn NoteLines;
    fn outbox(&self) -> &dyn Outbox;
    fn sources(&self) -> &dyn Sources;
}

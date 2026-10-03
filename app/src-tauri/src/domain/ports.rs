//! What the application asks of storage, implemented in `crate::store`.
//!
//! A service gets `&dyn Reads` (a consistent snapshot) or `&dyn UnitOfWork`
//! (one transaction, committed by the edge if the service returns `Ok`).
//! Neither hands out a connection or runs SQL given as text. Each area's
//! ports and models are in `domain/<area>.rs`.

use super::categories::{Categories, CategoryViews};
use super::daystrip::{DayStripViews, Events};
use super::incoming::IncomingViews;
use super::items::Items;
use super::links::Links;
use super::notes::{NoteLines, NotesViews};
use super::outbox::Outbox;
use super::people::{People, PeopleViews};
use super::priorities::PriorityViews;
use super::search::SearchViews;
use super::sources::{SourceViews, Sources};
use super::splits::Splits;
use super::todos::{TodoViews, Todos};
use super::topics::{TopicViews, Topics};

/// Read models, one port per screen or area.
pub trait Reads {
    fn categories(&self) -> &dyn CategoryViews;
    fn day_strip(&self) -> &dyn DayStripViews;
    fn incoming(&self) -> &dyn IncomingViews;
    fn notes(&self) -> &dyn NotesViews;
    fn people(&self) -> &dyn PeopleViews;
    fn priorities(&self) -> &dyn PriorityViews;
    fn search(&self) -> &dyn SearchViews;
    fn sources(&self) -> &dyn SourceViews;
    fn todos(&self) -> &dyn TodoViews;
    fn topics(&self) -> &dyn TopicViews;
}

/// One request's changes. Reads inside it see its own writes.
pub trait UnitOfWork {
    fn reads(&self) -> &dyn Reads;
    fn categories(&self) -> &dyn Categories;
    fn events(&self) -> &dyn Events;
    fn items(&self) -> &dyn Items;
    fn links(&self) -> &dyn Links;
    fn note_lines(&self) -> &dyn NoteLines;
    fn outbox(&self) -> &dyn Outbox;
    fn people(&self) -> &dyn People;
    fn sources(&self) -> &dyn Sources;
    fn splits(&self) -> &dyn Splits;
    fn todos(&self) -> &dyn Todos;
    fn topics(&self) -> &dyn Topics;
}

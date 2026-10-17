//! What the application asks of storage, implemented in `crate::store`.
//!
//! A service gets `&dyn Reads` (a consistent snapshot) or `&dyn UnitOfWork`
//! (one transaction, committed by the edge if the service returns `Ok`).
//! Neither hands out a connection or runs SQL given as text. Each area's
//! ports and models are in `domain/<area>.rs`.

use super::agents::{AgentState, AgentViews};
use super::categories::{Categories, CategoryViews};
use super::context::ContextViews;
use super::daystrip::{DayStripViews, Events};
use super::embed::{VectorViews, Vectors};
use super::glean::{GleanViews, SearchFeedback};
use super::incoming::IncomingViews;
use super::ingest::Ingest;
use super::items::{ItemViews, Items};
use super::links::Links;
use super::notes::{NoteLines, NotesViews};
use super::outbox::{Outbox, OutboxViews};
use super::people::{People, PeopleViews};
use super::priorities::PriorityViews;
use super::reflect::{ReflectViews, Reflections};
use super::respond::RespondViews;
use super::sample::SampleData;
use super::search::SearchViews;
use super::sources::{SourceViews, Sources};
use super::splits::Splits;
use super::suggest::{SuggestRuns, SuggestViews};
use super::todos::{TodoViews, Todos};
use super::topics::{TopicViews, Topics};
use super::triage::{TriageLog, TriageViews};
use super::usage::{Usage, UsageViews};

/// Read models, one port per screen or area.
pub trait Reads {
    fn agents(&self) -> &dyn AgentViews;
    fn categories(&self) -> &dyn CategoryViews;
    fn context(&self) -> &dyn ContextViews;
    fn day_strip(&self) -> &dyn DayStripViews;
    fn glean(&self) -> &dyn GleanViews;
    fn incoming(&self) -> &dyn IncomingViews;
    fn items(&self) -> &dyn ItemViews;
    fn notes(&self) -> &dyn NotesViews;
    fn outbox(&self) -> &dyn OutboxViews;
    fn people(&self) -> &dyn PeopleViews;
    fn priorities(&self) -> &dyn PriorityViews;
    fn reflect(&self) -> &dyn ReflectViews;
    fn respond(&self) -> &dyn RespondViews;
    fn search(&self) -> &dyn SearchViews;
    fn sources(&self) -> &dyn SourceViews;
    fn suggest(&self) -> &dyn SuggestViews;
    fn todos(&self) -> &dyn TodoViews;
    fn topics(&self) -> &dyn TopicViews;
    fn triage(&self) -> &dyn TriageViews;
    fn usage(&self) -> &dyn UsageViews;
    fn vectors(&self) -> &dyn VectorViews;
}

/// One request's changes. Reads inside it see its own writes.
pub trait UnitOfWork {
    fn reads(&self) -> &dyn Reads;
    fn agents(&self) -> &dyn AgentState;
    fn categories(&self) -> &dyn Categories;
    fn events(&self) -> &dyn Events;
    fn ingest(&self) -> &dyn Ingest;
    fn items(&self) -> &dyn Items;
    fn links(&self) -> &dyn Links;
    fn note_lines(&self) -> &dyn NoteLines;
    fn outbox(&self) -> &dyn Outbox;
    fn people(&self) -> &dyn People;
    fn reflections(&self) -> &dyn Reflections;
    fn sample(&self) -> &dyn SampleData;
    fn search_feedback(&self) -> &dyn SearchFeedback;
    fn sources(&self) -> &dyn Sources;
    fn splits(&self) -> &dyn Splits;
    fn suggest_runs(&self) -> &dyn SuggestRuns;
    fn todos(&self) -> &dyn Todos;
    fn topics(&self) -> &dyn Topics;
    fn triage(&self) -> &dyn TriageLog;
    fn usage(&self) -> &dyn Usage;
    fn vectors(&self) -> &dyn Vectors;
}

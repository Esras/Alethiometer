//! Priorities: what the database knows about each open todo, for scoring
//! (`queries::priorities`, `app/docs/priorities.md`).

use crate::domain::topics::TopicRef;
use crate::error::Result;
use crate::time::{Date, Due, Ts};

/// An author as the source stores them, for the Resolver.
#[derive(Debug, Clone, PartialEq)]
pub struct Author {
    /// The source the message or item came from (`slack`, `jira`…).
    pub source_kind: String,
    /// The source's account id, when it gave one.
    pub author_id: Option<String>,
    pub name: String,
}

/// One open todo you own, with the raw facts its factors are scored from.
/// Nothing here depends on the time it's read at.
#[derive(Debug, Clone)]
pub struct TodoFacts {
    pub item_id: i64,
    pub source_kind: String,
    pub title: String,
    pub url: Option<String>,
    /// The source's fields, as JSON.
    pub raw_json: String,
    pub summary: Option<String>,
    pub due: Option<Due>,
    /// A local todo's scheduled day.
    pub sched: Option<Date>,
    /// Minutes, and who estimated them.
    pub estimate: Option<(i64, String)>,
    /// Confirmed, else suggested.
    pub topic: Option<TopicRef>,
    /// H, M or L.
    pub topic_priority: Option<String>,
    /// Open Jira issues this one blocks.
    pub blocks_open: i64,
    /// People on the topic (`topic_people`).
    pub topic_people: i64,
    /// Distinct sources of the topic's confirmed signals.
    pub topic_sources: i64,
    /// When it was made: the source's own time where it gives one, else when it was stored.
    pub created: Ts,
    /// The last change to it: the item row, or its topic link.
    pub changed: Ts,
    /// The newest message on its thread by anyone but a bot.
    pub last_signal: Option<Ts>,
    /// Who wrote that message.
    pub last_author: Option<Author>,
    /// Who asked for it: the author of the signal a local todo came from, or a Jira issue's reporter.
    pub asker: Option<Author>,
    /// Confirmed tags, the todo's own and its topic's.
    pub tags: Vec<String>,
}

pub trait PriorityViews {
    /// Open todos you own (local, Jira and Obsidian tasks), by item id.
    fn facts(&self) -> Result<Vec<TodoFacts>>;
    /// The next meeting (not a life block) linked to each topic that isn't
    /// over at `now` and starts before `until`: (topic id, start, end).
    fn next_meetings(&self, now: Ts, until: Ts) -> Result<Vec<(i64, Ts, Ts)>>;
}

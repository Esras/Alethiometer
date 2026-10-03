//! Incoming's read models: unseen work signals, their messages, and a split
//! family's parts.

use crate::error::Result;
use crate::time::Ts;

/// An item as Incoming lists it.
#[derive(Debug, Clone)]
pub struct SignalRow {
    pub item_id: i64,
    pub source_kind: String,
    pub kind: String,
    pub title: String,
    pub summary: Option<String>,
    pub url: Option<String>,
    pub occurred_at: Ts,
    pub ended_at: Option<Ts>,
    pub msg_count: Option<i64>,
    /// The item author's person, when matched, and their handle in this source.
    pub author_name: Option<String>,
    pub author_handle: Option<String>,
    /// Gmail's own current label, as a category key.
    pub category: Option<String>,
    /// A calendar change ("moved", "invite"…) and where it moved from.
    pub change: Option<String>,
    pub moved_from: Option<Ts>,
    /// An event's description.
    pub description: Option<String>,
    /// An agent signal's details, as JSON.
    pub agent_json: Option<String>,
    /// A Slack thread item (its parent wasn't captured).
    pub thread: bool,
    /// `items.rev`: what triage sends back so it applies to what you saw.
    pub rev: i64,
}

/// An unseen signal with a live link to a topic.
#[derive(Debug, Clone)]
pub struct FiledSignal {
    pub signal: SignalRow,
    pub topic_id: i64,
    pub topic_name: String,
    pub topic_category: String,
    /// Confirmed already: new activity, not a suggestion.
    pub update: bool,
}

/// An agent waiting on you, with the topic it's filed or suggested under.
#[derive(Debug, Clone)]
pub struct WaitingAgent {
    pub signal: SignalRow,
    pub topic: Option<(i64, String)>,
}

#[derive(Debug, Clone)]
pub struct MessageRow {
    pub id: i64,
    pub item_id: i64,
    pub at: Ts,
    pub author: String,
    pub text: String,
    /// Shown before the signal for context; not part of it.
    pub context: bool,
    pub ext_key: Option<String>,
    /// The `ext_key` of the message this replies to.
    pub reply_to: Option<String>,
    pub author_id: Option<String>,
    /// There when the item was last triaged.
    pub read: bool,
}

/// The item a split review or an expanded entry is about.
#[derive(Debug, Clone)]
pub struct ItemHeader {
    pub title: String,
    pub source_kind: String,
    pub work: bool,
    pub url: Option<String>,
    pub summary: Option<String>,
    pub msg_count: Option<i64>,
    pub raw_json: String,
}

pub use super::topics::TopicRef;

/// One item of a split family, as the review needs it.
#[derive(Debug, Clone)]
pub struct PartRow {
    pub item_id: i64,
    pub seen: bool,
    /// It holds messages of its own (not only context).
    pub has_messages: bool,
    /// The family's dismissed messages.
    pub dismissed: bool,
    /// Confirmed under a topic.
    pub filed: bool,
    /// The most recent live (suggested or confirmed) link to a topic not archived.
    pub topic: Option<TopicRef>,
}

pub trait IncomingViews {
    /// Every unseen item, any source.
    fn unseen_count(&self) -> Result<i64>;
    /// Unseen work signals linked (suggested or confirmed) to a topic not
    /// archived, newest activity first.
    fn filed_signals(&self) -> Result<Vec<FiledSignal>>;
    /// Unseen work signals with no live link, newest activity first.
    fn unsorted_signals(&self) -> Result<Vec<SignalRow>>;
    /// Agent signals waiting on you, oldest first.
    fn waiting_agents(&self) -> Result<Vec<WaitingAgent>>;
    /// The messages of these items, by item, oldest first.
    fn messages(&self, item_ids: &[i64]) -> Result<Vec<MessageRow>>;
    /// Times of the top-level messages the other parts of an item's split family hold.
    fn split_sibling_times(&self, item_id: i64) -> Result<Vec<Ts>>;
    fn item_header(&self, item_id: i64) -> Result<Option<ItemHeader>>;
    /// An item's split family: its root and every part, root first.
    fn family(&self, item_id: i64) -> Result<(i64, Vec<i64>)>;
    fn parts(&self, item_ids: &[i64]) -> Result<Vec<PartRow>>;
}

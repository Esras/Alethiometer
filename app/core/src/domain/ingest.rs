//! What connectors produce: a batch of normalized records for one source,
//! applied in one unit of work (`sync::apply`). Raw source fields stay JSON.

use crate::error::Result;
use crate::time::{Due, Ts};
use serde_json::Value;

#[derive(Debug, Default, Clone)]
pub struct Batch {
    pub items: Vec<ItemUpsert>,
    /// For personal calendars: the full set of busy blocks in the synced window.
    /// Replaces this source's previous busy blocks.
    pub busy: Option<Vec<Busy>>,
    /// New cursor to store (sync token, updated-since timestamp, etc.).
    pub cursor: Option<String>,
    /// External ids the remote side cancelled or deleted. The item is kept (no
    /// deletions of user-visible records), marked done and resurfaced as new
    /// signal; only its calendar block, a cache of remote times, is dropped.
    /// Unknown ids are ignored.
    pub cancelled: Vec<String>,
    /// External ids the user has read in the remote app (e.g. Slack's read
    /// marker moved past them): marked seen, nothing else changes. Never new
    /// signal. Unknown ids are ignored.
    pub seen: Vec<String>,
    /// What the source said about the accounts it saw (Slack profiles):
    /// stored in `account_profiles`, newest wins.
    pub profiles: Vec<Profile>,
    /// How each stream (a Slack conversation or thread) went this poll; a
    /// failed one is skipped, recorded, and retried by the next poll.
    pub streams: Vec<StreamOutcome>,
    /// Queued repairs this poll read or found gone: written over any stored revision, never created.
    pub repaired: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StreamOutcome {
    pub key: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Profile {
    /// Account kind as `queries::people` names it (`slack`).
    pub kind: String,
    pub account: String,
    pub real_name: Option<String>,
    pub email: Option<String>,
    pub is_bot: bool,
}

#[derive(Debug, Default, Clone)]
pub struct ItemUpsert {
    /// Stable id in the remote system. (source, external_id) is unique.
    pub external_id: String,
    /// message | thread | issue | pr | event | note | email | doc | task
    pub kind: String,
    pub title: String,
    pub body: String,
    pub summary: Option<String>,
    pub url: Option<String>,
    /// Author handle in the source (Slack user id, Jira accountId, GitHub login…).
    pub author: Option<Handle>,
    pub occurred_at: Ts,
    /// When the item was made in its own system (Jira's `created`), if the source says.
    pub source_created_at: Option<Ts>,
    pub ended_at: Option<Ts>,
    pub due: Option<Due>,
    /// open | done
    pub status: String,
    pub msg_count: Option<i64>,
    pub owner: Option<String>,
    /// Source fields. `msgs`, when it lists one id per message (in order), keys
    /// the messages so a topic split can move some to a child item for good.
    pub raw_json: Value,
    /// Expanded context (e.g. Slack messages since last look). Upserted by
    /// key; stored ones the batch leaves out stay, and messages a split child
    /// owns are skipped (see `queries::split`).
    pub messages: Vec<Message>,
    pub field_changes: Vec<FieldChange>,
    /// Set when this is a calendar event.
    pub event: Option<EventFields>,
    /// True when this update is new signal (new messages, reassigned, state change).
    /// New items are always new signal.
    pub bump: bool,
    /// Remove this item's calendar block if it has one (declined, or now all-day).
    /// The item itself stays. Ignored when `event` is set.
    pub drop_event: bool,
    /// The source's revision of this item (an `updated` time, an etag). When
    /// it equals the stored one the item hasn't changed and nothing is written.
    pub remote_rev: Option<String>,
    /// A thread: the key of its parent message. Unless this item already exists,
    /// when another item holds the parent its messages are stored there as the
    /// parent's replies (that item bumps instead) and this item is never created.
    pub attach_to: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Handle {
    pub handle: String,
    pub display_name: String,
}

#[derive(Debug, Clone, Default)]
pub struct Message {
    pub at: Ts,
    pub author: String,
    pub text: String,
    pub author_id: Option<String>,
    pub context: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct FieldChange {
    pub key: String,
    pub at: Ts,
    pub author: String,
    pub author_id: Option<String>,
    pub field: String,
    pub from: Option<String>,
    pub to: Option<String>,
}

#[derive(Debug, Clone)]
pub struct EventFields {
    pub start: Ts,
    pub end: Ts,
    /// Category key from settings (e.g. work, self).
    pub category: String,
    pub tentative: bool,
    pub attendees: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Busy {
    pub start: Ts,
    pub end: Ts,
}

/// Writing a batch: the storage side of `sync::apply`, which decides what
/// each record means (new signal, quiet, attached to its parent).
pub trait Ingest {
    /// The person behind a source handle, made (unnamed note) if new.
    fn handle_person(&self, source_kind: &str, handle: &Handle) -> Result<i64>;
    /// Whether a reply stored on `holder` was there when it was triaged.
    fn reply_read(&self, holder: i64, key: &str, parent: &str) -> Result<bool>;
    /// Store (or replace) a thread reply on the item holding its parent.
    fn put_reply(&self, holder: i64, m: &Message, key: &str, parent: &str, read: bool) -> Result<()>;
    fn insert_item(&self, source_id: i64, it: &ItemUpsert, author: Option<i64>) -> Result<i64>;
    /// The revision stored with an item (`ItemUpsert::remote_rev`).
    fn remote_rev(&self, item_id: i64) -> Result<Option<String>>;
    /// Update an item from the source; `bump` makes it new signal again.
    fn update_item(&self, id: i64, it: &ItemUpsert, author: Option<i64>, bump: bool) -> Result<()>;
    /// Add or update an item's top-level messages by key; ones the batch
    /// leaves out stay, except earlier versions of a hashed key ("h:").
    /// A new message is unread; an updated one keeps its read state.
    fn upsert_messages(&self, item_id: i64, messages: &[(&Message, &str)]) -> Result<()>;
    fn upsert_field_changes(&self, item_id: i64, changes: &[FieldChange]) -> Result<()>;
    /// A triaged item a sync brings back: what it held so far stays read.
    fn mark_read_if_triaged(&self, item_id: i64) -> Result<()>;
    fn put_event(&self, item_id: i64, source_id: i64, title: &str, e: &EventFields) -> Result<()>;
    /// Remove an item's calendar block (not a life block).
    fn drop_event(&self, item_id: i64) -> Result<()>;
    /// The remote side cancelled or deleted it: done, back in Incoming, block
    /// dropped. Returns how many items that made new signal.
    fn cancel(&self, source_id: i64, external_id: &str) -> Result<usize>;
    fn put_profile(&self, p: &Profile) -> Result<()>;
    /// Read in the remote app: seen, nothing else.
    fn mark_seen(&self, source_id: i64, external_id: &str) -> Result<()>;
    /// A new instance of a known series takes the series' links; true when it
    /// stays quiet (the series isn't in Incoming, or is filed).
    fn inherit_series(&self, source_id: i64, item_id: i64, series: &str) -> Result<bool>;
    /// A thread split out of its parent's triaged item takes that item's confirmed topics.
    fn inherit_confirmed(&self, from: i64, to: i64) -> Result<()>;
    /// One unseen instance per series: the next upcoming, else the latest past.
    fn collapse_series(&self, source_id: i64, series: &str, now: Ts) -> Result<()>;
    /// Replace a personal calendar's busy blocks, keeping buffers set on them (by start).
    fn replace_busy(&self, source_id: i64, busy: &[Busy]) -> Result<()>;
    /// A stream was read (`error` None) or failed: its row in `sync_streams`.
    fn stream_outcome(&self, source_id: i64, stream: &str, error: Option<&str>) -> Result<()>;
    /// The sync went through: its cursor (if any), the time, no error.
    fn finish(&self, source_id: i64, cursor: Option<&str>) -> Result<()>;
}

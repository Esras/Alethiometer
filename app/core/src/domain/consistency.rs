//! Consistency checks (Settings › Sources): rows today's connectors would never leave behind.

use crate::error::Result;

/// Sources whose messages always carry an author id (Slack's and agents' bots don't).
pub const AUTHORED_KINDS: &[&str] = &["gmail", "jira", "github"];

/// A source's items with messages that have no author id.
#[derive(Debug, Clone, PartialEq)]
pub struct MissingAuthors {
    pub source_id: i64,
    /// External ids, in id order.
    pub items: Vec<String>,
    pub messages: i64,
}

/// One Gmail sender on two people: `bare` holds the address, `named` the address and a display name.
#[derive(Debug, Clone, PartialEq)]
pub struct SplitSender {
    pub address: String,
    pub bare: i64,
    pub named: i64,
    /// Items each one is the author of.
    pub bare_items: i64,
    pub named_items: i64,
    /// Either row is a People note's.
    pub noted: bool,
}

/// A calendar item marked done with no block and no reason the connector gives.
#[derive(Debug, Clone, PartialEq)]
pub struct Leftover {
    pub source_id: i64,
    pub item_id: i64,
    pub title: String,
}

pub trait ConsistencyViews {
    /// Per source of one of `kinds`: items whose own messages (not context) have no author id.
    fn missing_authors(&self, kinds: &[&str]) -> Result<Vec<MissingAuthors>>;
    /// Gmail senders with a bare-address person and exactly one `address name` twin.
    fn split_senders(&self) -> Result<Vec<SplitSender>>;
    /// Google Calendar items done, off the strip, neither cancelled nor declined.
    fn calendar_leftovers(&self) -> Result<Vec<Leftover>>;
}

pub trait Consistency {
    /// Forget these items' stored revisions, so the next copy applies.
    fn forget_revs(&self, source_id: i64, external_ids: &[String]) -> Result<()>;
    /// Move everything pointing at person `from` onto `into`, then delete `from`.
    fn merge_person(&self, from: i64, into: i64) -> Result<()>;
}

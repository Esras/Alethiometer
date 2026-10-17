//! Triage history for ignore suggestions: what was dismissed or kept in
//! Incoming, by sender, list, channel, repo or project.

use crate::error::Result;
use crate::time::Ts;

/// What a triage action was about: `from`, `domain`, `list`, `channel`, `repo` or `project`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TriageKey {
    pub kind: String,
    pub key: String,
}

/// Actions on one key in a source since a time.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KeyStats {
    /// Dismissed or trashed.
    pub dismissed: i64,
    /// Dismissed, trashed or kept.
    pub total: i64,
    /// Different `from` addresses among the dismissed (counts for a `domain`).
    pub senders: i64,
}

pub trait TriageViews {
    fn key_stats(&self, source_id: i64, key: &TriageKey, since: Ts) -> Result<KeyStats>;
    /// Until when "Not now" holds for the key, if it was said.
    fn declined_until(&self, source_id: i64, key: &TriageKey) -> Result<Option<Ts>>;
}

pub trait TriageLog {
    /// `action` is `dismiss`, `trash` or `keep`. Returns the event's id.
    fn record(&self, item_id: i64, source_id: i64, action: &str, at: Ts, keys: &[TriageKey]) -> Result<i64>;
    /// `u`: forget the event, if it was about `item_id`.
    fn forget(&self, event_id: i64, item_id: i64) -> Result<()>;
    fn decline(&self, source_id: i64, key: &TriageKey, until: Ts) -> Result<()>;
}

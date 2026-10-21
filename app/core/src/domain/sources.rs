//! Sources: each configured connector's row and its sync state.

use crate::error::Result;

#[derive(Debug, Clone)]
pub struct SourceRow {
    pub id: i64,
    pub kind: String,
    pub tag: String,
    pub name: String,
    pub enabled: bool,
    /// The connector's settings, as JSON.
    pub config_json: String,
    /// The connector's own sync state, as JSON.
    pub cursor: Option<String>,
    pub last_sync_at: Option<crate::time::Ts>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Conversation {
    pub channel: String,
    pub title: String,
    pub conv: Option<String>,
    pub last: crate::time::Ts,
}

pub trait SourceViews {
    /// Sources of a kind, in id order.
    fn of_kind(&self, kind: &str) -> Result<Vec<SourceRow>>;
    fn get(&self, id: i64) -> Result<Option<SourceRow>>;
    /// Every source, by kind and name.
    fn all(&self) -> Result<Vec<SourceRow>>;
    fn find(&self, kind: &str, name: &str) -> Result<Option<i64>>;
    /// Per Slack channel of a source: the latest message since `since` and how many.
    fn channel_activity(&self, source_id: i64, since: crate::time::Ts) -> Result<Vec<(String, crate::time::Ts, i64)>>;
    /// Each chat conversation of a source, most recently active first.
    fn conversations(&self, source_id: i64) -> Result<Vec<Conversation>>;
    /// Stored revisions of a source's items, by external id.
    fn item_revs(&self, source_id: i64) -> Result<std::collections::HashMap<String, String>>;
    /// Open items of a source not read by id since `before` (and not failing
    /// for good), least recently read first (`sync::Polling::stale_open`).
    fn stale_open(&self, source_id: i64, before: crate::time::Ts, limit: usize) -> Result<Vec<String>>;
    /// A source's streams that failed last time they were read.
    fn failing_streams(&self, source_id: i64) -> Result<Vec<FailingStream>>;
}

/// A stream whose last read failed (`sync_streams`).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct FailingStream {
    pub stream: String,
    pub error: String,
    /// Failures in a row.
    pub failures: i64,
}

pub trait Sources {
    /// The app's own work source of `kind` ("notes"), made if missing.
    fn ensure_local(&self, kind: &str, name: &str) -> Result<i64>;
    /// A source from settings: its row (by kind and name) made or updated.
    fn register(&self, kind: &str, name: &str, tag: &str, config_json: &str, enabled: bool) -> Result<()>;
    /// A source renamed in settings keeps its data. False when there's no such source.
    fn rename(&self, kind: &str, from: &str, to: &str) -> Result<bool>;
    /// Gone from settings: kept, but off.
    fn disable(&self, kind: &str, name: &str) -> Result<()>;
    /// The last sync failed.
    fn set_error(&self, id: i64, error: &str) -> Result<()>;
    /// Mark it synced now (the UI reloads on this).
    fn touch_synced(&self, id: i64) -> Result<()>;
}

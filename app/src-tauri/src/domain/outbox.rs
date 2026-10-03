//! The outbox: writes to other apps, sent in the background.

use crate::error::Result;
use crate::time::{Date, Ts};

/// A queued write.
#[derive(Debug, Clone)]
pub struct OutboxEntry {
    pub id: i64,
    pub source_id: i64,
    pub source_name: String,
    pub op: String,
    pub payload_json: String,
    /// pending | sent | confirmed | failed
    pub state: String,
    pub error: Option<String>,
    pub item_id: Option<i64>,
    pub created_at: Ts,
    pub confirmed_at: Option<Ts>,
}

pub trait OutboxViews {
    fn get(&self, id: i64) -> Result<Option<OutboxEntry>>;
    /// Everything not confirmed, and what was confirmed after `since`; newest first.
    fn recent(&self, since: Ts, limit: usize) -> Result<Vec<OutboxEntry>>;
    /// The latest pending or failed `op` for the source whose payload's `path` is `path`.
    fn open_for_path(&self, source_id: i64, op: &str, path: &str) -> Result<Option<OutboxEntry>>;
}

pub trait Outbox {
    /// Queue `op` for the source; returns its id.
    fn enqueue(&self, source_id: i64, op: &str, payload_json: &str, item_id: Option<i64>) -> Result<i64>;
    /// Take the oldest pending row for sending: it's `sent` from now on.
    fn claim_next(&self) -> Result<Option<OutboxEntry>>;
    fn confirm(&self, id: i64, receipt_json: &str) -> Result<()>;
    fn fail(&self, id: i64, error: &str) -> Result<()>;
    /// Failed rows go back to pending: one, or all of them. How many did.
    fn retry(&self, id: Option<i64>) -> Result<usize>;
    /// Replace the payload of a row not sent yet (pending or failed). False when it's gone out.
    fn update_open(&self, id: i64, payload_json: &str) -> Result<bool>;
    /// A failed row goes again with a new payload. False when it isn't failed.
    fn resend(&self, id: i64, payload_json: &str) -> Result<bool>;
    /// A calendar hold for an item carried from `period` that hasn't gone out
    /// is cancelled (failed, "cancelled before sending"). How many were.
    fn cancel_pending_hold(&self, item_id: i64, period: Date) -> Result<usize>;
    /// Failed Jira task requests these Notes lines made go back to pending.
    fn retry_line_requests(&self, line_ids: &[i64]) -> Result<usize>;
}

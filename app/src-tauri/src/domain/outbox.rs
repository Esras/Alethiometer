//! The outbox: writes to other apps, sent in the background.

use crate::error::Result;

pub trait Outbox {
    /// Queue `op` for the source; returns its id.
    fn enqueue(&self, source_id: i64, op: &str, payload_json: &str, item_id: Option<i64>) -> Result<i64>;
    /// A calendar hold for an item carried from `period` that hasn't gone out
    /// is cancelled (failed, "cancelled before sending"). How many were.
    fn cancel_pending_hold(&self, item_id: i64, period: crate::time::Date) -> Result<usize>;
    /// Failed Jira task requests these Notes lines made go back to pending.
    fn retry_line_requests(&self, line_ids: &[i64]) -> Result<usize>;
}

//! The outbox.

use super::Sql;
use crate::domain::outbox::Outbox;
use crate::error::Result;
use rusqlite::params;

impl Outbox for Sql<'_> {
    fn enqueue(&self, source_id: i64, op: &str, payload_json: &str, item_id: Option<i64>) -> Result<i64> {
        self.conn().execute(
            "INSERT INTO outbox (source_id, op, payload_json, item_id) VALUES (?1, ?2, ?3, ?4)",
            params![source_id, op, payload_json, item_id],
        )?;
        Ok(self.conn().last_insert_rowid())
    }

    fn cancel_pending_hold(&self, item_id: i64, period: crate::time::Date) -> Result<usize> {
        Ok(self.conn().execute(
            "UPDATE outbox SET state = 'failed', error = 'cancelled before sending'
              WHERE op = 'gcal.hold' AND state = 'pending'
                AND json_extract(payload_json, '$.item_id') = ?1 AND json_extract(payload_json, '$.carried_from') = ?2",
            params![item_id, period],
        )?)
    }

    fn retry_line_requests(&self, line_ids: &[i64]) -> Result<usize> {
        Ok(self.conn().execute(
            "UPDATE outbox SET state = 'pending' WHERE state = 'failed' AND op = 'jira.create_issue'
               AND json_extract(payload_json, '$.notes_line_id') IN (SELECT value FROM json_each(?1))",
            [serde_json::to_string(line_ids)?],
        )?)
    }
}

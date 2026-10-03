//! The outbox.

use super::Sql;
use crate::domain::outbox::{Outbox, OutboxEntry, OutboxViews};
use crate::error::Result;
use crate::time::{Date, Ts, SQL_NOW};
use rusqlite::{params, OptionalExtension, Row};

const ENTRY: &str = "o.id, o.source_id, s.name, o.op, o.payload_json, o.state, o.error, o.item_id, o.created_at, o.confirmed_at";

fn entry(r: &Row) -> rusqlite::Result<OutboxEntry> {
    Ok(OutboxEntry {
        id: r.get(0)?,
        source_id: r.get(1)?,
        source_name: r.get(2)?,
        op: r.get(3)?,
        payload_json: r.get(4)?,
        state: r.get(5)?,
        error: r.get(6)?,
        item_id: r.get(7)?,
        created_at: r.get(8)?,
        confirmed_at: r.get(9)?,
    })
}

impl OutboxViews for Sql<'_> {
    fn get(&self, id: i64) -> Result<Option<OutboxEntry>> {
        Ok(self.conn().query_row(&format!("SELECT {ENTRY} FROM outbox o JOIN sources s ON s.id = o.source_id WHERE o.id = ?1"), [id], entry).optional()?)
    }

    fn recent(&self, since: Ts, limit: usize) -> Result<Vec<OutboxEntry>> {
        Ok(self
            .conn()
            .prepare_cached(&format!(
                "SELECT {ENTRY} FROM outbox o JOIN sources s ON s.id = o.source_id
                  WHERE o.state != 'confirmed' OR o.confirmed_at > ?1
                  ORDER BY o.id DESC LIMIT ?2"
            ))?
            .query_map(params![since, limit as i64], entry)?
            .collect::<rusqlite::Result<_>>()?)
    }

    fn open_for_path(&self, source_id: i64, op: &str, path: &str) -> Result<Option<OutboxEntry>> {
        Ok(self
            .conn()
            .query_row(
                &format!(
                    "SELECT {ENTRY} FROM outbox o JOIN sources s ON s.id = o.source_id
                      WHERE o.source_id = ?1 AND o.op = ?2 AND o.state IN ('pending', 'failed')
                        AND json_extract(o.payload_json, '$.path') = ?3
                      ORDER BY o.id DESC LIMIT 1"
                ),
                params![source_id, op, path],
                entry,
            )
            .optional()?)
    }
}

impl Outbox for Sql<'_> {
    fn enqueue(&self, source_id: i64, op: &str, payload_json: &str, item_id: Option<i64>) -> Result<i64> {
        self.conn().execute(
            "INSERT INTO outbox (source_id, op, payload_json, item_id) VALUES (?1, ?2, ?3, ?4)",
            params![source_id, op, payload_json, item_id],
        )?;
        Ok(self.conn().last_insert_rowid())
    }

    fn claim_next(&self) -> Result<Option<OutboxEntry>> {
        let next: Option<i64> =
            self.conn().query_row("SELECT id FROM outbox WHERE state = 'pending' ORDER BY id LIMIT 1", [], |r| r.get(0)).optional()?;
        let Some(id) = next else { return Ok(None) };
        self.conn().execute(&format!("UPDATE outbox SET state = 'sent', sent_at = {SQL_NOW}, attempts = attempts + 1 WHERE id = ?1"), [id])?;
        self.get(id)
    }

    fn confirm(&self, id: i64, receipt_json: &str) -> Result<()> {
        self.conn().execute(
            &format!("UPDATE outbox SET state = 'confirmed', confirmed_at = {SQL_NOW}, receipt_json = ?2, error = NULL WHERE id = ?1"),
            params![id, receipt_json],
        )?;
        Ok(())
    }

    fn fail(&self, id: i64, error: &str) -> Result<()> {
        self.conn().execute("UPDATE outbox SET state = 'failed', error = ?2 WHERE id = ?1", params![id, error])?;
        Ok(())
    }

    fn retry(&self, id: Option<i64>) -> Result<usize> {
        Ok(match id {
            Some(id) => self.conn().execute("UPDATE outbox SET state = 'pending' WHERE id = ?1 AND state = 'failed'", [id])?,
            None => self.conn().execute("UPDATE outbox SET state = 'pending' WHERE state = 'failed'", [])?,
        })
    }

    fn update_open(&self, id: i64, payload_json: &str) -> Result<bool> {
        Ok(self.conn().execute("UPDATE outbox SET payload_json = ?2 WHERE id = ?1 AND state IN ('pending', 'failed')", params![id, payload_json])? == 1)
    }

    fn resend(&self, id: i64, payload_json: &str) -> Result<bool> {
        Ok(self
            .conn()
            .execute("UPDATE outbox SET payload_json = ?2, state = 'pending', error = NULL WHERE id = ?1 AND state = 'failed'", params![id, payload_json])?
            == 1)
    }

    fn cancel_pending_hold(&self, item_id: i64, period: Date) -> Result<usize> {
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

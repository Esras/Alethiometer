//! Source rows.

use super::Sql;
use crate::domain::sources::{SourceRow, SourceViews};
use crate::error::Result;

const COLS: &str = "id, kind, tag, name, enabled, config_json, cursor, last_sync_at, last_error";

fn row(r: &rusqlite::Row) -> rusqlite::Result<SourceRow> {
    Ok(SourceRow {
        id: r.get(0)?,
        kind: r.get(1)?,
        tag: r.get(2)?,
        name: r.get(3)?,
        enabled: r.get(4)?,
        config_json: r.get(5)?,
        cursor: r.get(6)?,
        last_sync_at: r.get(7)?,
        last_error: r.get(8)?,
    })
}

impl SourceViews for Sql<'_> {
    fn get(&self, id: i64) -> Result<Option<SourceRow>> {
        use rusqlite::OptionalExtension;
        Ok(self.conn().query_row(&format!("SELECT {COLS} FROM sources WHERE id = ?1"), [id], row).optional()?)
    }

    fn of_kind(&self, kind: &str) -> Result<Vec<SourceRow>> {
        Ok(self.conn().prepare_cached(&format!("SELECT {COLS} FROM sources WHERE kind = ?1 ORDER BY id"))?.query_map([kind], row)?.collect::<rusqlite::Result<_>>()?)
    }

    fn find(&self, kind: &str, name: &str) -> Result<Option<i64>> {
        use rusqlite::OptionalExtension;
        Ok(self.conn().query_row("SELECT id FROM sources WHERE kind = ?1 AND name = ?2", [kind, name], |r| r.get(0)).optional()?)
    }

    fn channel_activity(&self, source_id: i64, since: crate::time::Ts) -> Result<Vec<(String, crate::time::Ts, i64)>> {
        Ok(self
            .conn()
            .prepare_cached(
                "SELECT json_extract(i.raw_json, '$.channel'), max(m.at), count(m.id)
                   FROM items i JOIN item_messages m ON m.item_id = i.id
                  WHERE i.source_id = ?1 AND m.at >= ?2 AND m.context = 0 GROUP BY 1",
            )?
            .query_map(rusqlite::params![source_id, since], |r| Ok((r.get::<_, Option<String>>(0)?.unwrap_or_default(), r.get(1)?, r.get(2)?)))?
            .collect::<rusqlite::Result<_>>()?)
    }

    fn failing_streams(&self, source_id: i64) -> Result<Vec<crate::domain::sources::FailingStream>> {
        Ok(self
            .conn()
            .prepare_cached("SELECT stream, error, failures FROM sync_streams WHERE source_id = ?1 AND error IS NOT NULL ORDER BY stream")?
            .query_map([source_id], |r| Ok(crate::domain::sources::FailingStream { stream: r.get(0)?, error: r.get(1)?, failures: r.get(2)? }))?
            .collect::<rusqlite::Result<_>>()?)
    }

    fn all(&self) -> Result<Vec<SourceRow>> {
        Ok(self.conn().prepare_cached(&format!("SELECT {COLS} FROM sources ORDER BY kind, name"))?.query_map([], row)?.collect::<rusqlite::Result<_>>()?)
    }
}

impl crate::domain::sources::Sources for Sql<'_> {
    fn register(&self, kind: &str, name: &str, tag: &str, config_json: &str, enabled: bool) -> Result<()> {
        use rusqlite::{params, OptionalExtension};
        let c = self.conn();
        let existing: Option<i64> = c.query_row("SELECT id FROM sources WHERE kind = ?1 AND name = ?2", params![kind, name], |r| r.get(0)).optional()?;
        match existing {
            Some(id) => c.execute("UPDATE sources SET tag = ?1, config_json = ?2, enabled = ?3 WHERE id = ?4", params![tag, config_json, enabled, id])?,
            None => c.execute(
                "INSERT INTO sources (kind, tag, name, config_json, enabled) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![kind, tag, name, config_json, enabled],
            )?,
        };
        Ok(())
    }

    fn rename(&self, kind: &str, from: &str, to: &str) -> Result<bool> {
        Ok(self.conn().execute("UPDATE sources SET name = ?3 WHERE kind = ?1 AND name = ?2", rusqlite::params![kind, from, to])? == 1)
    }

    fn disable(&self, kind: &str, name: &str) -> Result<()> {
        self.conn().execute("UPDATE sources SET enabled = 0 WHERE kind = ?1 AND name = ?2", rusqlite::params![kind, name])?;
        Ok(())
    }

    fn set_error(&self, id: i64, error: &str) -> Result<()> {
        self.conn().execute("UPDATE sources SET last_error = ?2 WHERE id = ?1", rusqlite::params![id, error])?;
        Ok(())
    }

    fn touch_synced(&self, id: i64) -> Result<()> {
        self.conn().execute(&format!("UPDATE sources SET last_sync_at = {} WHERE id = ?1", crate::time::SQL_NOW), [id])?;
        Ok(())
    }

    fn ensure_local(&self, kind: &str, name: &str) -> Result<i64> {
        use rusqlite::OptionalExtension;
        let id: Option<i64> = self.conn().query_row("SELECT id FROM sources WHERE kind = ?1", [kind], |r| r.get(0)).optional()?;
        match id {
            Some(id) => Ok(id),
            None => {
                self.conn().execute("INSERT INTO sources (kind, tag, name) VALUES (?1, 'work', ?2)", [kind, name])?;
                Ok(self.conn().last_insert_rowid())
            }
        }
    }
}

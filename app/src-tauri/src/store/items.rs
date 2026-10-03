//! Items.

use super::Sql;
use crate::domain::items::{Items, NewItem};
use crate::error::Result;
use rusqlite::params;

impl Items for Sql<'_> {
    fn insert(&self, i: &NewItem) -> Result<i64> {
        self.conn().execute(
            "INSERT INTO items (source_id, external_id, kind, title, summary, occurred_at, status, owner, seen, raw_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![i.source_id, i.external_id, i.kind, i.title, i.summary, i.occurred_at, i.status, i.owner, i.seen, i.raw_json],
        )?;
        Ok(self.conn().last_insert_rowid())
    }
}

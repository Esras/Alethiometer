//! Items.

use super::Sql;
use crate::domain::items::{ItemRef, ItemViews, Items, NewItem};
use crate::error::Result;
use crate::time::Date;
use rusqlite::{params, OptionalExtension, Row};

const REF: &str = "i.id, i.source_id, s.kind, i.external_id, i.kind, i.title, i.url, i.parent_id, i.seen, i.raw_json";

fn item_ref(r: &Row) -> rusqlite::Result<ItemRef> {
    Ok(ItemRef {
        id: r.get(0)?,
        source_id: r.get(1)?,
        source_kind: r.get(2)?,
        external_id: r.get(3)?,
        kind: r.get(4)?,
        title: r.get(5)?,
        url: r.get(6)?,
        parent_id: r.get(7)?,
        seen: r.get(8)?,
        raw_json: r.get(9)?,
    })
}

impl Items for Sql<'_> {
    fn insert(&self, i: &NewItem) -> Result<i64> {
        self.conn().execute(
            "INSERT INTO items (source_id, external_id, kind, title, summary, url, occurred_at, due_day, sched_day, status, owner, seen, raw_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![i.source_id, i.external_id, i.kind, i.title, i.summary, i.url, i.occurred_at, i.due_day, i.sched_day, i.status, i.owner, i.seen, i.raw_json],
        )?;
        Ok(self.conn().last_insert_rowid())
    }

    fn insert_or_get(&self, i: &NewItem) -> Result<i64> {
        match self.find(i.source_id, &i.external_id)? {
            Some(id) => Ok(id),
            None => self.insert(i),
        }
    }

    fn get(&self, id: i64) -> Result<Option<ItemRef>> {
        Ok(self
            .conn()
            .query_row(
                &format!("SELECT {REF} FROM items i JOIN sources s ON s.id = i.source_id WHERE i.id = ?1"),
                [id],
                item_ref,
            )
            .optional()?)
    }

    fn find(&self, source_id: i64, external_id: &str) -> Result<Option<i64>> {
        Ok(self
            .conn()
            .query_row(
                "SELECT id FROM items WHERE source_id = ?1 AND external_id = ?2",
                params![source_id, external_id],
                |r| r.get(0),
            )
            .optional()?)
    }

    fn set_external_id(&self, id: i64, external_id: &str) -> Result<()> {
        self.conn().execute(
            "UPDATE items SET external_id = ?2 WHERE id = ?1",
            params![id, external_id],
        )?;
        Ok(())
    }

    fn set_seen(&self, id: i64, seen: bool) -> Result<bool> {
        Ok(self
            .conn()
            .execute("UPDATE items SET seen = ?2 WHERE id = ?1", params![id, seen])?
            == 1)
    }

    fn rev(&self, id: i64) -> Result<Option<i64>> {
        Ok(self
            .conn()
            .query_row("SELECT rev FROM items WHERE id = ?1", [id], |r| r.get(0))
            .optional()?)
    }

    fn mark_read(&self, id: i64, keys: &[String]) -> Result<()> {
        let keys = serde_json::to_string(keys)?;
        let c = self.conn();
        c.execute(
            "UPDATE item_messages SET read = 1 WHERE item_id = ?1 AND ext_key IN (SELECT value FROM json_each(?2))",
            params![id, keys],
        )?;
        c.execute(
            "UPDATE item_field_changes SET read = 1 WHERE item_id = ?1 AND ext_key IN (SELECT value FROM json_each(?2))",
            params![id, keys],
        )?;
        Ok(())
    }

    fn unread_keys(&self, id: i64) -> Result<Vec<String>> {
        let mut st = self.conn().prepare_cached(
            "SELECT ext_key FROM item_messages WHERE item_id = ?1 AND read = 0 AND ext_key IS NOT NULL
             UNION ALL SELECT ext_key FROM item_field_changes WHERE item_id = ?1 AND read = 0",
        )?;
        let keys = st
            .query_map([id], |r| r.get(0))?
            .collect::<rusqlite::Result<Vec<String>>>()?;
        Ok(keys)
    }

    fn mark_unread(&self, id: i64, keys: &[String]) -> Result<()> {
        let keys = serde_json::to_string(keys)?;
        let c = self.conn();
        c.execute(
            "UPDATE item_messages SET read = 0 WHERE item_id = ?1 AND ext_key IN (SELECT value FROM json_each(?2))",
            params![id, keys],
        )?;
        c.execute(
            "UPDATE item_field_changes SET read = 0 WHERE item_id = ?1 AND ext_key IN (SELECT value FROM json_each(?2))",
            params![id, keys],
        )?;
        Ok(())
    }

    fn set_seen_many(&self, ids: &[i64], seen: bool) -> Result<()> {
        self.conn().execute(
            "UPDATE items SET seen = ?2 WHERE id IN (SELECT value FROM json_each(?1))",
            params![serde_json::to_string(ids)?, seen],
        )?;
        Ok(())
    }

    fn set_status(&self, id: i64, status: &str) -> Result<()> {
        self.conn()
            .execute("UPDATE items SET status = ?2 WHERE id = ?1", params![id, status])?;
        Ok(())
    }

    fn set_sched_day(&self, id: i64, day: Option<Date>) -> Result<()> {
        self.conn()
            .execute("UPDATE items SET sched_day = ?2 WHERE id = ?1", params![id, day])?;
        Ok(())
    }

    fn set_status_by_external(&self, source_id: i64, external_id: &str, status: &str) -> Result<()> {
        self.conn().execute(
            "UPDATE items SET status = ?3 WHERE source_id = ?1 AND external_id = ?2",
            params![source_id, external_id, status],
        )?;
        Ok(())
    }

    fn open_tasks(&self, source_id: i64, prefix: &str) -> Result<Vec<(i64, String)>> {
        Ok(self
            .conn()
            .prepare_cached(
                "SELECT id, external_id FROM items WHERE source_id = ?1 AND kind = 'task' AND status = 'open'
                   AND substr(external_id, 1, length(?2)) = ?2",
            )?
            .query_map(params![source_id, prefix], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?)
    }

    fn set_dismissed(&self, id: i64, dismissed: bool) -> Result<()> {
        let sql = if dismissed {
            "UPDATE items SET raw_json = json_set(raw_json, '$.dismissed', json('true')) WHERE id = ?1"
        } else {
            "UPDATE items SET raw_json = json_remove(raw_json, '$.dismissed') WHERE id = ?1"
        };
        self.conn().execute(sql, [id])?;
        Ok(())
    }

    fn set_url_and_summary(&self, id: i64, url: Option<&str>, summary: &str) -> Result<()> {
        self.conn().execute(
            "UPDATE items SET url = coalesce(?2, url), summary = ?3 WHERE id = ?1",
            params![id, url, summary],
        )?;
        Ok(())
    }

    fn series_siblings(&self, id: i64) -> Result<Vec<i64>> {
        Ok(self
            .conn()
            .prepare_cached(
                "SELECT o.id FROM items i JOIN items o ON o.source_id = i.source_id AND o.id != i.id
                   AND json_extract(o.raw_json, '$.recurringEventId') = json_extract(i.raw_json, '$.recurringEventId')
                  WHERE i.id = ?1",
            )?
            .query_map([id], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?)
    }

    fn family(&self, id: i64) -> Result<Vec<ItemRef>> {
        Ok(self
            .conn()
            .prepare_cached(&format!(
                "SELECT {REF} FROM items i JOIN sources s ON s.id = i.source_id
                  WHERE i.id = (SELECT coalesce(parent_id, id) FROM items WHERE id = ?1)
                     OR i.parent_id = (SELECT coalesce(parent_id, id) FROM items WHERE id = ?1)
                  ORDER BY i.parent_id IS NOT NULL, i.id"
            ))?
            .query_map([id], item_ref)?
            .collect::<rusqlite::Result<_>>()?)
    }
}

impl ItemViews for Sql<'_> {
    fn item(&self, id: i64) -> Result<Option<ItemRef>> {
        Items::get(self, id)
    }

    fn seen_by_external(&self, source_id: i64, external_id: &str) -> Result<Option<bool>> {
        Ok(self
            .conn()
            .query_row(
                "SELECT seen FROM items WHERE source_id = ?1 AND external_id = ?2",
                params![source_id, external_id],
                |r| r.get(0),
            )
            .optional()?)
    }

    fn seen(&self, id: i64) -> Result<Option<bool>> {
        Ok(self
            .conn()
            .query_row("SELECT seen FROM items WHERE id = ?1", [id], |r| r.get(0))
            .optional()?)
    }

    fn holder(&self, source_id: i64, channel: Option<&str>, key: &str) -> Result<Option<i64>> {
        Ok(self
            .conn()
            .query_row(
                "SELECT m.item_id FROM item_messages m JOIN items i ON i.id = m.item_id
                  WHERE m.ext_key = ?2 AND m.context = 0 AND m.reply_to IS NULL AND i.source_id = ?1
                    AND json_extract(i.raw_json, '$.channel') IS ?3
                  ORDER BY m.id DESC LIMIT 1",
                params![source_id, key, channel],
                |r| r.get(0),
            )
            .optional()?)
    }
}

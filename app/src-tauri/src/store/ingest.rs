//! Writing a sync batch (`domain::ingest`).

use super::Sql;
use crate::domain::ingest::{Busy, EventFields, Handle, Ingest, ItemUpsert, Message, Profile};
use crate::error::Result;
use crate::time::{Due, Ts, SQL_NOW};
use rusqlite::{params, OptionalExtension};
use std::collections::HashMap;

const SERIES: &str = "json_extract(raw_json, '$.recurringEventId')";

impl Ingest for Sql<'_> {
    fn handle_person(&self, source_kind: &str, h: &Handle) -> Result<i64> {
        let c = self.conn();
        if let Some(id) = c
            .query_row("SELECT person_id FROM person_handles WHERE source_kind = ?1 AND handle = ?2", params![source_kind, h.handle], |r| r.get(0))
            .optional()?
        {
            return Ok(id);
        }
        c.execute("INSERT INTO people (display_name) VALUES (?1)", [&h.display_name])?;
        let id = c.last_insert_rowid();
        c.execute("INSERT INTO person_handles (person_id, source_kind, handle) VALUES (?1, ?2, ?3)", params![id, source_kind, h.handle])?;
        Ok(id)
    }

    fn reply_read(&self, holder: i64, key: &str, parent: &str) -> Result<bool> {
        Ok(self
            .conn()
            .query_row("SELECT read FROM item_messages WHERE item_id = ?1 AND ext_key = ?2 AND reply_to = ?3", params![holder, key, parent], |r| r.get(0))
            .optional()?
            .unwrap_or(false))
    }

    fn put_reply(&self, holder: i64, m: &Message, key: &str, parent: &str, read: bool) -> Result<()> {
        self.conn().execute(
            "INSERT INTO item_messages (item_id, at, author, text, ext_key, author_id, context, reply_to, read) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, ?7, ?8)
             ON CONFLICT (item_id, ext_key) DO UPDATE SET at = excluded.at, author = excluded.author, text = excluded.text,
                author_id = excluded.author_id, reply_to = excluded.reply_to, read = read OR excluded.read",
            params![holder, m.at, m.author, m.text, key, m.author_id, parent, read],
        )?;
        Ok(())
    }

    fn insert_item(&self, source_id: i64, it: &ItemUpsert, author: Option<i64>) -> Result<i64> {
        let (due_at, due_day) = Due::columns(it.due);
        self.conn().execute(
            "INSERT INTO items (source_id, external_id, kind, title, body, summary, url, author_person_id,
               occurred_at, ended_at, due_at, due_day, status, msg_count, owner, seen, raw_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, 0, ?16)",
            params![
                source_id, it.external_id, it.kind, it.title, it.body, it.summary, it.url, author,
                it.occurred_at, it.ended_at, due_at, due_day, it.status, it.msg_count, it.owner, it.raw_json.to_string()
            ],
        )?;
        Ok(self.conn().last_insert_rowid())
    }

    fn update_item(&self, id: i64, it: &ItemUpsert, author: Option<i64>, bump: bool) -> Result<()> {
        let (due_at, due_day) = Due::columns(it.due);
        self.conn().execute(
            "UPDATE items SET kind = ?2, title = ?3, body = ?4, summary = ?5, url = ?6, author_person_id = ?7,
               occurred_at = ?8, ended_at = ?9, due_at = ?10, due_day = ?11, status = ?12, msg_count = ?13, owner = ?14,
               raw_json = ?15, seen = CASE WHEN ?16 THEN 0 ELSE seen END
             WHERE id = ?1",
            params![
                id, it.kind, it.title, it.body, it.summary, it.url, author, it.occurred_at, it.ended_at,
                due_at, due_day, it.status, it.msg_count, it.owner, it.raw_json.to_string(), bump
            ],
        )?;
        Ok(())
    }

    fn upsert_messages(&self, item_id: i64, messages: &[(&Message, &str)]) -> Result<()> {
        let c = self.conn();
        let mut upsert = c.prepare_cached(
            "INSERT INTO item_messages (item_id, at, author, text, ext_key, author_id, context, read) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0)
             ON CONFLICT (item_id, ext_key) DO UPDATE SET at = excluded.at, author = excluded.author, text = excluded.text,
                author_id = excluded.author_id, context = excluded.context",
        )?;
        for (m, key) in messages {
            upsert.execute(params![item_id, m.at, m.author, m.text, key, m.author_id, m.context])?;
        }
        // A hashed key changes with the message: what the source no longer
        // sends under it is an earlier version, not a message of its own.
        let keys: Vec<&str> = messages.iter().map(|(_, k)| *k).collect();
        c.execute(
            "DELETE FROM item_messages WHERE item_id = ?1 AND reply_to IS NULL AND ext_key LIKE 'h:%'
               AND ext_key NOT IN (SELECT value FROM json_each(?2))",
            params![item_id, serde_json::to_string(&keys)?],
        )?;
        Ok(())
    }

    fn mark_read_if_triaged(&self, item_id: i64) -> Result<()> {
        self.conn().execute(
            "UPDATE item_messages SET read = 1 WHERE item_id = ?1 AND EXISTS (SELECT 1 FROM items WHERE id = ?1 AND seen = 1)",
            [item_id],
        )?;
        Ok(())
    }

    fn put_event(&self, item_id: i64, source_id: i64, title: &str, e: &EventFields) -> Result<()> {
        let c = self.conn();
        let ev: Option<i64> = c.query_row("SELECT id FROM events WHERE item_id = ?1", [item_id], |r| r.get(0)).optional()?;
        match ev {
            Some(ev) => c.execute(
                "UPDATE events SET start = ?2, end = ?3, title = ?4, category = ?5, tentative = ?6, attendees = ?7 WHERE id = ?1",
                params![ev, e.start, e.end, title, e.category, e.tentative, e.attendees],
            )?,
            None => c.execute(
                "INSERT INTO events (item_id, source_id, start, end, title, category, tentative, attendees)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![item_id, source_id, e.start, e.end, title, e.category, e.tentative, e.attendees],
            )?,
        };
        Ok(())
    }

    fn drop_event(&self, item_id: i64) -> Result<()> {
        self.conn().execute("DELETE FROM events WHERE item_id = ?1 AND is_life_busy = 0", [item_id])?;
        Ok(())
    }

    fn cancel(&self, source_id: i64, external_id: &str) -> Result<usize> {
        let c = self.conn();
        let id: Option<i64> =
            c.query_row("SELECT id FROM items WHERE source_id = ?1 AND external_id = ?2", params![source_id, external_id], |r| r.get(0)).optional()?;
        let Some(id) = id else { return Ok(0) };
        let n = c.execute(
            "UPDATE items SET status = 'done', seen = 0, summary = 'cancelled', raw_json = json_set(raw_json, '$.change', 'cancelled')
              WHERE id = ?1 AND status != 'done'",
            [id],
        )?;
        self.drop_event(id)?;
        Ok(n)
    }

    fn put_profile(&self, p: &Profile) -> Result<()> {
        self.conn().execute(
            &format!(
                "INSERT INTO account_profiles (kind, account, real_name, email, is_bot) VALUES (?1, ?2, ?3, lower(?4), ?5)
                 ON CONFLICT (kind, account) DO UPDATE SET real_name = ?3, email = lower(?4), is_bot = ?5, updated_at = {SQL_NOW}"
            ),
            params![p.kind, p.account, p.real_name, p.email, p.is_bot],
        )?;
        Ok(())
    }

    fn mark_seen(&self, source_id: i64, external_id: &str) -> Result<()> {
        self.conn().execute("UPDATE items SET seen = 1 WHERE source_id = ?1 AND external_id = ?2", params![source_id, external_id])?;
        Ok(())
    }

    fn inherit_series(&self, source_id: i64, item_id: i64, key: &str) -> Result<bool> {
        let c = self.conn();
        let sibs = format!("SELECT id FROM items WHERE source_id = ?1 AND id != ?2 AND {SERIES} = ?3");
        let (n, unseen, confirmed): (i64, i64, bool) = c.query_row(
            &format!(
                "SELECT count(*), coalesce(sum(seen = 0), 0),
                        EXISTS (SELECT 1 FROM topic_links WHERE state = 'confirmed' AND item_id IN ({sibs}))
                   FROM items WHERE id IN ({sibs})"
            ),
            params![source_id, item_id, key],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        if n == 0 {
            return Ok(false);
        }
        c.execute(
            &format!(
                "INSERT OR IGNORE INTO topic_links (item_id, topic_id, state, confidence, reason)
                 SELECT ?2, topic_id, state, confidence, reason FROM topic_links WHERE item_id IN ({sibs})
                  ORDER BY state = 'confirmed' DESC"
            ),
            params![source_id, item_id, key],
        )?;
        let quiet = unseen == 0 || confirmed;
        if quiet {
            c.execute("UPDATE items SET seen = 1 WHERE id = ?1", [item_id])?;
        }
        Ok(quiet)
    }

    fn collapse_series(&self, source_id: i64, key: &str, now: Ts) -> Result<()> {
        self.conn().execute(
            &format!(
                "UPDATE items SET seen = 1 WHERE source_id = ?1 AND seen = 0 AND {SERIES} = ?2 AND id != (
                   SELECT id FROM items WHERE source_id = ?1 AND seen = 0 AND {SERIES} = ?2
                    ORDER BY coalesce(ended_at, occurred_at) < ?3,
                             CASE WHEN coalesce(ended_at, occurred_at) >= ?3 THEN occurred_at END,
                             occurred_at DESC
                    LIMIT 1)"
            ),
            params![source_id, key, now],
        )?;
        Ok(())
    }

    fn replace_busy(&self, source_id: i64, busy: &[Busy]) -> Result<()> {
        let c = self.conn();
        let buffers: HashMap<Ts, i64> = c
            .prepare_cached("SELECT start, buffer_min FROM events WHERE source_id = ?1 AND buffer_min IS NOT NULL")?
            .query_map([source_id], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        c.execute("DELETE FROM events WHERE source_id = ?1 AND is_life_busy = 1", [source_id])?;
        for b in busy {
            c.execute(
                "INSERT INTO events (source_id, start, end, category, is_life_busy, buffer_min) VALUES (?1, ?2, ?3, 'life', 1, ?4)",
                params![source_id, b.start, b.end, buffers.get(&b.start)],
            )?;
        }
        Ok(())
    }

    fn finish(&self, source_id: i64, cursor: Option<&str>) -> Result<()> {
        self.conn().execute(
            &format!("UPDATE sources SET cursor = coalesce(?2, cursor), last_sync_at = {SQL_NOW}, last_error = NULL WHERE id = ?1"),
            params![source_id, cursor],
        )?;
        Ok(())
    }
}

//! Incoming's read models.

use super::Sql;
use crate::domain::incoming::{
    FieldChangeRow, FiledSignal, IncomingViews, ItemHeader, MessageRow, PartRow, SignalRow, TopicRef, WaitingAgent,
};
use crate::error::{msg, Result};
use crate::time::Ts;
use rusqlite::{params, OptionalExtension, Row};

/// Top-level messages a view shows per item: the latest. Storage keeps all.
const SHOWN_MESSAGES: i64 = 50;

/// `SignalRow`'s columns, from `items i JOIN sources s`; `signal` reads them back.
const SIGNAL: &str = "i.id, s.kind, i.kind, i.title, i.summary, i.url, i.occurred_at, i.ended_at, i.msg_count,
     (SELECT display_name FROM people p WHERE p.id = i.author_person_id),
     (SELECT h.handle FROM person_handles h WHERE h.person_id = i.author_person_id AND h.source_kind = s.kind),
     json_extract(i.raw_json, '$.category'), json_extract(i.raw_json, '$.change'), json_extract(i.raw_json, '$.moved_from'),
     CASE WHEN i.kind = 'event' THEN i.body END,
     CASE WHEN s.kind = 'agents' THEN json_extract(i.raw_json, '$.agent') END,
     json_extract(i.raw_json, '$.thread_ts') IS NOT NULL, i.rev, coalesce(i.parent_id, i.id)";
const SIGNAL_COLS: usize = 19;

fn signal(r: &Row) -> rusqlite::Result<SignalRow> {
    Ok(SignalRow {
        item_id: r.get(0)?,
        source_kind: r.get(1)?,
        kind: r.get(2)?,
        title: r.get(3)?,
        summary: r.get(4)?,
        url: r.get(5)?,
        occurred_at: r.get(6)?,
        ended_at: r.get(7)?,
        msg_count: r.get(8)?,
        author_name: r.get(9)?,
        author_handle: r.get(10)?,
        category: r.get(11)?,
        change: r.get(12)?,
        // Written by the app; a value that isn't an instant reads as none.
        moved_from: r.get::<_, Option<String>>(13)?.and_then(|s| Ts::parse(&s)),
        description: r.get(14)?,
        agent_json: r.get(15)?,
        thread: r.get(16)?,
        rev: r.get(17)?,
        root: r.get(18)?,
    })
}

/// A JSON array of ids, for `json_each`.
fn id_list(ids: &[i64]) -> String {
    serde_json::to_string(ids).expect("ids serialize")
}

impl Sql<'_> {
    /// `messages`, with `text` as the text column (`''` leaves it unread).
    fn message_rows(&self, item_ids: &[i64], text: &str) -> Result<Vec<MessageRow>> {
        Ok(self
            .conn()
            .prepare_cached(&format!(
                // An item keeps every message a sync gave it; a view shows its
                // latest SHOWN_MESSAGES top-level ones (and their replies).
                "WITH m AS (
                   SELECT id, item_id, at, author, {text} AS text, context, ext_key, reply_to, author_id, read, CASE WHEN reply_to IS NULL
                                  THEN row_number() OVER (PARTITION BY item_id, reply_to IS NULL ORDER BY at DESC, id DESC) END AS newest
                     FROM item_messages WHERE item_id IN (SELECT value FROM json_each(?1)))
                 SELECT id, item_id, at, author, text, context, ext_key, reply_to, author_id, read FROM m
                  WHERE newest IS NULL OR newest <= ?2
                  ORDER BY item_id, at, id"
            ))?
            .query_map(params![id_list(item_ids), SHOWN_MESSAGES], |r| {
                Ok(MessageRow {
                    id: r.get(0)?,
                    item_id: r.get(1)?,
                    at: r.get(2)?,
                    author: r.get(3)?,
                    text: r.get(4)?,
                    context: r.get(5)?,
                    ext_key: r.get(6)?,
                    reply_to: r.get(7)?,
                    author_id: r.get(8)?,
                    read: r.get(9)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?)
    }
}

impl IncomingViews for Sql<'_> {
    fn unseen_count(&self) -> Result<i64> {
        Ok(self
            .conn()
            .query_row("SELECT count(*) FROM items WHERE seen = 0", [], |r| r.get(0))?)
    }

    fn filed_signals(&self) -> Result<Vec<FiledSignal>> {
        let n = SIGNAL_COLS;
        Ok(self
            .conn()
            .prepare_cached(&format!(
                "SELECT {SIGNAL}, t.id, t.name, t.category, tl.state = 'confirmed'
                   FROM topic_links tl
                   JOIN items i ON i.id = tl.item_id
                   JOIN sources s ON s.id = i.source_id
                   JOIN topics t ON t.id = tl.topic_id
                  WHERE tl.state IN ('suggested', 'confirmed') AND i.seen = 0 AND t.archived_at IS NULL AND s.tag = 'work'
                  ORDER BY coalesce(i.ended_at, i.occurred_at) DESC, i.id DESC"
            ))?
            .query_map([], |r| {
                Ok(FiledSignal { signal: signal(r)?, topic_id: r.get(n)?, topic_name: r.get(n + 1)?, topic_category: r.get(n + 2)?, update: r.get(n + 3)? })
            })?
            .collect::<rusqlite::Result<_>>()?)
    }

    fn unsorted_signals(&self) -> Result<Vec<SignalRow>> {
        Ok(self
            .conn()
            .prepare_cached(&format!(
                // CROSS JOIN: start from the unseen items (a partial index), not
                // from each work source's every item.
                "SELECT {SIGNAL}
                   FROM items i CROSS JOIN sources s ON s.id = i.source_id
                  WHERE i.seen = 0 AND s.tag = 'work'
                    AND NOT EXISTS (SELECT 1 FROM topic_links tl WHERE tl.item_id = i.id AND tl.state != 'wrong')
                  ORDER BY coalesce(i.ended_at, i.occurred_at) DESC, i.id DESC"
            ))?
            .query_map([], signal)?
            .collect::<rusqlite::Result<_>>()?)
    }

    fn waiting_agents(&self) -> Result<Vec<WaitingAgent>> {
        let n = SIGNAL_COLS;
        Ok(self
            .conn()
            .prepare_cached(&format!(
                "SELECT {SIGNAL}, t.id, t.name
                   FROM items i JOIN sources s ON s.id = i.source_id
                   LEFT JOIN topics t ON t.id = (
                        SELECT tl.topic_id FROM topic_links tl JOIN topics t2 ON t2.id = tl.topic_id
                         WHERE tl.item_id = i.id AND tl.state IN ('suggested', 'confirmed') AND t2.archived_at IS NULL
                         ORDER BY tl.state = 'confirmed' DESC LIMIT 1)
                  WHERE i.seen = 0 AND s.kind = 'agents' AND json_extract(i.raw_json, '$.agent.waiting') = 1
                  ORDER BY coalesce(i.ended_at, i.occurred_at), i.id"
            ))?
            .query_map([], |r| {
                let topic = match r.get::<_, Option<i64>>(n)? {
                    Some(id) => Some((id, r.get(n + 1)?)),
                    None => None,
                };
                Ok(WaitingAgent {
                    signal: signal(r)?,
                    topic,
                })
            })?
            .collect::<rusqlite::Result<_>>()?)
    }

    fn messages(&self, item_ids: &[i64]) -> Result<Vec<MessageRow>> {
        self.message_rows(item_ids, "text")
    }

    fn message_heads(&self, item_ids: &[i64]) -> Result<Vec<MessageRow>> {
        self.message_rows(item_ids, "''")
    }

    fn field_changes(&self, item_ids: &[i64]) -> Result<Vec<FieldChangeRow>> {
        Ok(self
            .conn()
            .prepare_cached(
                "WITH f AS (
                   SELECT *, row_number() OVER (PARTITION BY item_id ORDER BY at DESC, id DESC) AS newest
                     FROM item_field_changes WHERE item_id IN (SELECT value FROM json_each(?1)))
                 SELECT item_id, ext_key, at, author, author_id, field, from_text, to_text, read FROM f
                  WHERE newest <= ?2 ORDER BY item_id, at, id",
            )?
            .query_map(params![id_list(item_ids), SHOWN_MESSAGES], |r| {
                Ok(FieldChangeRow {
                    item_id: r.get(0)?,
                    ext_key: r.get(1)?,
                    at: r.get(2)?,
                    author: r.get(3)?,
                    author_id: r.get(4)?,
                    field: r.get(5)?,
                    from: r.get(6)?,
                    to: r.get(7)?,
                    read: r.get(8)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?)
    }

    fn signal(&self, item_id: i64) -> Result<Option<SignalRow>> {
        Ok(self
            .conn()
            .query_row(&format!("SELECT {SIGNAL} FROM items i JOIN sources s ON s.id = i.source_id WHERE i.id = ?1 AND s.tag = 'work'"), [item_id], signal)
            .optional()?)
    }
    fn split_sibling_times(&self, item_id: i64) -> Result<Vec<Ts>> {
        Ok(self
            .conn()
            .prepare_cached(
                "WITH root AS (SELECT coalesce(parent_id, id) AS id FROM items WHERE id = ?1)
                 SELECT m.at FROM item_messages m
                  WHERE m.item_id IN (SELECT id FROM root UNION SELECT i.id FROM items i, root WHERE i.parent_id = root.id)
                    AND m.item_id != ?1 AND m.context = 0 AND m.reply_to IS NULL",
            )?
            .query_map([item_id], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?)
    }

    fn item_header(&self, item_id: i64) -> Result<Option<ItemHeader>> {
        Ok(self
            .conn()
            .query_row(
                "SELECT i.title, s.kind, s.tag = 'work', i.url, i.summary, i.msg_count, i.raw_json
                   FROM items i JOIN sources s ON s.id = i.source_id WHERE i.id = ?1",
                [item_id],
                |r| {
                    Ok(ItemHeader {
                        title: r.get(0)?,
                        source_kind: r.get(1)?,
                        work: r.get(2)?,
                        url: r.get(3)?,
                        summary: r.get(4)?,
                        msg_count: r.get(5)?,
                        raw_json: r.get(6)?,
                    })
                },
            )
            .optional()?)
    }

    fn family(&self, item_id: i64) -> Result<(i64, Vec<i64>)> {
        let parent: Option<i64> = self
            .conn()
            .query_row("SELECT parent_id FROM items WHERE id = ?1", [item_id], |r| r.get(0))
            .optional()?
            .ok_or_else(|| msg("no such item"))?;
        let root = parent.unwrap_or(item_id);
        let parts = self
            .conn()
            .prepare_cached("SELECT id FROM items WHERE id = ?1 OR parent_id = ?1 ORDER BY id = ?1 DESC, id")?
            .query_map([root], |r| r.get(0))?
            .collect::<rusqlite::Result<Vec<i64>>>()?;
        Ok((root, parts))
    }

    fn parts(&self, item_ids: &[i64]) -> Result<Vec<PartRow>> {
        Ok(self
            .conn()
            .prepare_cached(
                "SELECT i.id, i.seen,
                        EXISTS (SELECT 1 FROM item_messages m WHERE m.item_id = i.id AND m.context = 0),
                        coalesce(json_extract(i.raw_json, '$.dismissed'), 0),
                        t.id, t.name, t.category,
                        EXISTS (SELECT 1 FROM topic_links f WHERE f.item_id = i.id AND f.state = 'confirmed')
                   FROM json_each(?1) j JOIN items i ON i.id = j.value
                   LEFT JOIN topics t ON t.id = (
                        SELECT tl.topic_id FROM topic_links tl JOIN topics t2 ON t2.id = tl.topic_id
                         WHERE tl.item_id = i.id AND tl.state IN ('suggested', 'confirmed') AND t2.archived_at IS NULL
                         ORDER BY tl.updated_at DESC, tl.rowid DESC LIMIT 1)
                  ORDER BY j.key",
            )?
            .query_map([id_list(item_ids)], |r| {
                let topic = match r.get::<_, Option<i64>>(4)? {
                    Some(id) => Some(TopicRef {
                        id,
                        name: r.get(5)?,
                        category: r.get(6)?,
                    }),
                    None => None,
                };
                Ok(PartRow {
                    item_id: r.get(0)?,
                    seen: r.get(1)?,
                    has_messages: r.get(2)?,
                    dismissed: r.get(3)?,
                    filed: r.get(7)?,
                    topic,
                })
            })?
            .collect::<rusqlite::Result<_>>()?)
    }
}

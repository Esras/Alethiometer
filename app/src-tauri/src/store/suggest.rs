//! The small model's triage.

use super::Sql;
use crate::domain::suggest::{ContextMessage, PromptMessage, SuggestRun, SuggestRuns, SuggestViews, UnsortedItem};
use crate::error::Result;
use crate::time::SQL_NOW;
use rusqlite::{params, OptionalExtension};

const UNSORTED: &str = "FROM items i JOIN sources s ON s.id = i.source_id
    WHERE i.seen = 0 AND s.tag = 'work'
      AND NOT EXISTS (SELECT 1 FROM topic_links tl WHERE tl.item_id = i.id AND tl.state != 'wrong')";

impl SuggestViews for Sql<'_> {
    fn unsorted(&self, limit: usize) -> Result<Vec<UnsortedItem>> {
        Ok(self
            .conn()
            .prepare_cached(&format!(
                "SELECT i.id, s.kind, i.kind, i.title, i.summary, i.parent_id {UNSORTED}
                  ORDER BY coalesce(i.ended_at, i.occurred_at) DESC, i.id DESC LIMIT ?1"
            ))?
            .query_map([limit as i64], |r| {
                Ok(UnsortedItem { id: r.get(0)?, source_kind: r.get(1)?, kind: r.get(2)?, title: r.get(3)?, summary: r.get(4)?, parent_id: r.get(5)? })
            })?
            .collect::<rusqlite::Result<_>>()?)
    }

    fn unsorted_count(&self) -> Result<i64> {
        Ok(self.conn().query_row(&format!("SELECT count(*) {UNSORTED}"), [], |r| r.get(0))?)
    }

    fn wrong_topics(&self, item_id: i64) -> Result<Vec<i64>> {
        Ok(self
            .conn()
            .prepare_cached("SELECT topic_id FROM topic_links WHERE item_id = ?1 AND state = 'wrong'")?
            .query_map([item_id], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?)
    }

    fn own_messages(&self, item_id: i64) -> Result<Vec<PromptMessage>> {
        Ok(self
            .conn()
            .prepare_cached("SELECT id, at, author, text FROM item_messages WHERE item_id = ?1 AND context = 0 AND reply_to IS NULL ORDER BY at, id")?
            .query_map([item_id], |r| Ok(PromptMessage { id: r.get(0)?, at: r.get(1)?, author: r.get(2)?, text: r.get(3)? }))?
            .collect::<rusqlite::Result<_>>()?)
    }

    fn replies(&self, item_id: i64) -> Result<Vec<(i64, PromptMessage)>> {
        Ok(self
            .conn()
            .prepare_cached(
                "SELECT p.id, r.id, r.at, r.author, r.text FROM item_messages r
                   JOIN item_messages p ON p.item_id = r.item_id AND p.ext_key = r.reply_to AND p.reply_to IS NULL AND p.context = 0
                  WHERE r.item_id = ?1 ORDER BY r.at, r.id",
            )?
            .query_map([item_id], |r| Ok((r.get(0)?, PromptMessage { id: r.get(1)?, at: r.get(2)?, author: r.get(3)?, text: r.get(4)? })))?
            .collect::<rusqlite::Result<_>>()?)
    }

    fn context_messages(&self, item_id: i64) -> Result<Vec<ContextMessage>> {
        Ok(self
            .conn()
            .prepare_cached(
                "SELECT m.at, m.author, m.text,
                        (SELECT t.name FROM item_messages o
                           JOIN items oi ON oi.id = o.item_id
                           JOIN topic_links l ON l.item_id = oi.id AND l.state = 'confirmed'
                           JOIN topics t ON t.id = l.topic_id
                          WHERE o.ext_key = m.ext_key AND o.context = 0 AND oi.source_id = i.source_id AND oi.id != i.id
                          LIMIT 1)
                   FROM item_messages m JOIN items i ON i.id = m.item_id
                  WHERE m.item_id = ?1 AND m.context = 1 ORDER BY m.at, m.id",
            )?
            .query_map([item_id], |r| Ok(ContextMessage { at: r.get(0)?, author: r.get(1)?, text: r.get(2)?, topic: r.get(3)? }))?
            .collect::<rusqlite::Result<_>>()?)
    }

    fn last_run(&self) -> Result<Option<SuggestRun>> {
        Ok(self
            .conn()
            .query_row("SELECT status, message, suggested, at, last_ok_at FROM ai_suggest_run", [], |r| {
                Ok(SuggestRun { status: r.get(0)?, message: r.get(1)?, suggested: r.get(2)?, at: r.get(3)?, last_ok_at: r.get(4)? })
            })
            .optional()?)
    }

    fn has_live_link(&self, item_id: i64) -> Result<bool> {
        Ok(self.conn().query_row("SELECT EXISTS (SELECT 1 FROM topic_links WHERE item_id = ?1 AND state != 'wrong')", [item_id], |r| r.get(0))?)
    }
}

impl SuggestRuns for Sql<'_> {
    fn record(&self, status: &str, message: Option<&str>, suggested: i64) -> Result<()> {
        self.conn().execute(
            &format!(
                "INSERT INTO ai_suggest_run (id, status, message, suggested, at, last_ok_at)
                 VALUES (1, ?1, ?2, ?3, {SQL_NOW}, CASE ?1 WHEN 'ok' THEN {SQL_NOW} END)
                 ON CONFLICT (id) DO UPDATE SET status = excluded.status, message = excluded.message, suggested = excluded.suggested,
                   at = excluded.at, last_ok_at = coalesce(excluded.last_ok_at, ai_suggest_run.last_ok_at)"
            ),
            params![status, message, suggested],
        )?;
        Ok(())
    }
}

//! Notes pages.

use super::Sql;
use crate::domain::notes::{Corpus, JiraRequest, LineMeta, NoteLine, NoteLines, NotesViews, TopicText};
use crate::error::Result;
use crate::time::Date;
use rusqlite::params;
use serde_json::Value;
use std::collections::HashMap;

fn meta_json(meta: &LineMeta) -> Result<String> {
    Ok(serde_json::to_string(meta)?)
}

impl NotesViews for Sql<'_> {
    fn lines(&self, date: Date) -> Result<Vec<NoteLine>> {
        Ok(self
            .conn()
            .prepare_cached("SELECT id, kind, text, state, target_json FROM notes_lines WHERE page_date = ?1 ORDER BY position, id")?
            .query_map([date], |r| {
                let meta: Option<String> = r.get(4)?;
                Ok(NoteLine {
                    id: r.get(0)?,
                    kind: r.get(1)?,
                    text: r.get(2)?,
                    state: r.get(3)?,
                    meta: meta.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default(),
                })
            })?
            .collect::<rusqlite::Result<_>>()?)
    }

    fn jira_requests(&self, line_ids: &[i64]) -> Result<HashMap<i64, JiraRequest>> {
        let rows = self
            .conn()
            .prepare_cached(
                "SELECT json_extract(payload_json, '$.notes_line_id'), state, error, receipt_json FROM outbox
                  WHERE op = 'jira.create_issue' AND json_extract(payload_json, '$.notes_line_id') IN (SELECT value FROM json_each(?1))
                  ORDER BY id",
            )?
            .query_map([serde_json::to_string(line_ids)?], |r| {
                let receipt: Value = r.get::<_, Option<String>>(3)?.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(Value::Null);
                let field = |k: &str| receipt.get(k).and_then(Value::as_str).map(str::to_owned);
                Ok((r.get::<_, i64>(0)?, JiraRequest { state: r.get(1)?, error: r.get(2)?, key: field("key"), url: field("url") }))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        // In id order, so the latest request per line wins.
        Ok(rows.into_iter().collect())
    }

    fn corpus(&self) -> Result<Corpus> {
        let c = self.conn();
        let people = c
            .prepare_cached("SELECT id, display_name FROM people ORDER BY id")?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        let mut topics: Vec<TopicText> = c
            .prepare_cached(
                "SELECT id, name, category, obsidian_path, corpus_rev FROM topics WHERE archived_at IS NULL ORDER BY coalesce(score, 0) DESC, name",
            )?
            .query_map([], |r| {
                Ok(TopicText { id: r.get(0)?, name: r.get(1)?, category: r.get(2)?, obsidian_path: r.get(3)?, corpus_rev: r.get(4)?, people: vec![] })
            })?
            .collect::<rusqlite::Result<_>>()?;
        let mut linked = c.prepare_cached("SELECT person_id FROM topic_people WHERE topic_id = ?1")?;
        for t in &mut topics {
            t.people = linked.query_map([t.id], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
        }
        let issues = c
            .prepare_cached(
                "SELECT i.title, (SELECT tl.topic_id FROM topic_links tl WHERE tl.item_id = i.id AND tl.state = 'confirmed' LIMIT 1)
                   FROM items i JOIN sources s ON s.id = i.source_id
                  WHERE s.kind = 'jira' AND i.kind = 'issue' ORDER BY i.occurred_at DESC",
            )?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        Ok(Corpus { people, topics, issues })
    }

    fn topic_text(&self, topic_id: i64) -> Result<String> {
        let mut text = String::new();
        let mut items = self.conn().prepare_cached(
            "SELECT i.title, coalesce(i.summary, ''), i.body FROM topic_links tl JOIN items i ON i.id = tl.item_id
              WHERE tl.topic_id = ?1 AND tl.state != 'wrong'",
        )?;
        for r in items.query_map([topic_id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)))? {
            let (a, b, c) = r?;
            text.push_str(&format!("{a}\n{b}\n{c}\n"));
        }
        Ok(text)
    }

    fn page_counts(&self, date: Date) -> Result<(i64, i64)> {
        self.page_counts_impl(date)
    }
}

impl Sql<'_> {
    fn page_counts_impl(&self, date: Date) -> Result<(i64, i64)> {
        Ok(self.conn().query_row(
            "SELECT count(*), coalesce(sum(state = 'held'), 0) FROM notes_lines WHERE page_date = ?1",
            [date],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?)
    }
}

impl NoteLines for Sql<'_> {
    fn insert(&self, date: Date, position: i64, kind: &str, text: &str, meta: &LineMeta) -> Result<i64> {
        self.conn().execute(
            "INSERT INTO notes_lines (page_date, position, kind, text, target_json, state) VALUES (?1, ?2, ?3, ?4, ?5, 'draft')",
            params![date, position, kind, text, meta_json(meta)?],
        )?;
        Ok(self.conn().last_insert_rowid())
    }

    fn update(&self, id: i64, position: i64, kind: &str, text: &str, meta: &LineMeta) -> Result<()> {
        self.conn().execute(
            "UPDATE notes_lines SET position = ?1, kind = ?2, text = ?3, target_json = ?4 WHERE id = ?5",
            params![position, kind, text, meta_json(meta)?, id],
        )?;
        Ok(())
    }

    fn set_position(&self, id: i64, position: i64) -> Result<()> {
        self.conn().execute("UPDATE notes_lines SET position = ?1 WHERE id = ?2", params![position, id])?;
        Ok(())
    }

    fn set_state(&self, id: i64, state: &str, meta: &LineMeta) -> Result<()> {
        self.conn().execute("UPDATE notes_lines SET state = ?1, target_json = ?2 WHERE id = ?3", params![state, meta_json(meta)?, id])?;
        Ok(())
    }

    fn delete(&self, id: i64) -> Result<()> {
        self.conn().execute("DELETE FROM notes_lines WHERE id = ?1", [id])?;
        Ok(())
    }
}

//! Local reads for Glean search and Ask Glean, and search feedback.

use super::Sql;
use crate::domain::glean::{EntitySignal, EvidenceItem, GleanViews, LocalHit, SearchFeedback};
use crate::domain::search::HitTopic;
use crate::domain::topics::TopicRef;
use crate::error::Result;
use crate::time::Ts;
use rusqlite::{params, OptionalExtension};
use std::collections::HashMap;

impl GleanViews for Sql<'_> {
    fn entity_signals(&self, topic_id: i64) -> Result<Vec<EntitySignal>> {
        Ok(self
            .conn()
            .prepare_cached(
                "SELECT i.title, coalesce(i.url, ''), coalesce(json_extract(i.raw_json, '$.channel_name'), '')
                   FROM topic_links tl JOIN items i ON i.id = tl.item_id JOIN sources s ON s.id = i.source_id
                  WHERE tl.topic_id = ?1 AND tl.state = 'confirmed' AND s.tag = 'work'",
            )?
            .query_map([topic_id], |r| {
                Ok(EntitySignal {
                    title: r.get(0)?,
                    url: r.get(1)?,
                    channel: r.get(2)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?)
    }

    fn email_of(&self, person_id: i64) -> Result<Option<String>> {
        Ok(self
            .conn()
            .query_row(
                "SELECT account FROM person_accounts WHERE person_id = ?1 AND kind = 'email' LIMIT 1",
                [person_id],
                |r| r.get(0),
            )
            .optional()?)
    }

    fn recent_signals(&self, topic_id: i64, limit: usize) -> Result<Vec<(i64, String)>> {
        Ok(self
            .conn()
            .prepare_cached(
                "SELECT i.id, i.title FROM topic_links tl JOIN items i ON i.id = tl.item_id JOIN sources s ON s.id = i.source_id
                  WHERE tl.topic_id = ?1 AND (tl.state = 'confirmed' OR (tl.state = 'suggested' AND i.seen = 0)) AND s.tag = 'work'
                  ORDER BY i.occurred_at DESC LIMIT ?2",
            )?
            .query_map(params![topic_id, limit as i64], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?)
    }

    fn fts_items(&self, fts: &str, limit: usize) -> Result<Vec<i64>> {
        Ok(self
            .conn()
            .prepare_cached("SELECT rowid FROM items_fts WHERE items_fts MATCH ?1 ORDER BY rank LIMIT ?2")?
            .query_map(params![fts, limit as i64], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?)
    }

    fn evidence_item(&self, id: i64) -> Result<Option<EvidenceItem>> {
        Ok(self
            .conn()
            .query_row(
                "SELECT i.title, i.url, s.kind, s.tag, coalesce(i.summary, ''), coalesce(i.body, ''), i.occurred_at
                   FROM items i JOIN sources s ON s.id = i.source_id WHERE i.id = ?1",
                [id],
                |r| {
                    Ok(EvidenceItem {
                        title: r.get(0)?,
                        url: r.get(1)?,
                        source_kind: r.get(2)?,
                        tag: r.get(3)?,
                        summary: r.get(4)?,
                        body: r.get(5)?,
                        at: r.get(6)?,
                    })
                },
            )
            .optional()?)
    }

    fn confirmed_topic(&self, item_id: i64) -> Result<Option<TopicRef>> {
        Ok(self
            .conn()
            .query_row(
                "SELECT t.id, t.name, t.category FROM topic_links tl JOIN topics t ON t.id = tl.topic_id
                  WHERE tl.item_id = ?1 AND tl.state = 'confirmed' LIMIT 1",
                [item_id],
                |r| {
                    Ok(TopicRef {
                        id: r.get(0)?,
                        name: r.get(1)?,
                        category: r.get(2)?,
                    })
                },
            )
            .optional()?)
    }

    fn local_hits(&self, fts: &str, limit: usize) -> Result<Vec<LocalHit>> {
        Ok(self
            .conn()
            .prepare_cached(
                "SELECT i.id, i.title, i.url, s.kind, coalesce(i.ended_at, i.occurred_at), coalesce(i.summary, ''),
                        snippet(items_fts, 1, char(1), char(2), '…', 14),
                        (SELECT count(*) FROM item_messages m WHERE m.item_id = i.id AND m.context = 0 AND m.reply_to IS NULL)
                   FROM items_fts f JOIN items i ON i.id = f.rowid JOIN sources s ON s.id = i.source_id
                  WHERE items_fts MATCH ?1 AND s.tag = 'work'
                  ORDER BY rank LIMIT ?2",
            )?
            .query_map(params![fts, limit as i64], |r| {
                let summary: String = r.get(5)?;
                Ok(LocalHit {
                    id: r.get(0)?,
                    title: r.get(1)?,
                    url: r.get(2)?,
                    source_kind: r.get(3)?,
                    at: r.get(4)?,
                    summary: Some(summary).filter(|s| !s.is_empty()),
                    snippet: r.get(6)?,
                    msgs: r.get(7)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?)
    }

    fn hit_topic(&self, item_id: i64) -> Result<Option<HitTopic>> {
        Ok(self
            .conn()
            .query_row(
                "SELECT t.id, t.name, t.category, tl.state FROM topic_links tl JOIN topics t ON t.id = tl.topic_id
                  WHERE tl.item_id = ?1 AND tl.state != 'wrong' AND t.archived_at IS NULL
                  ORDER BY tl.state = 'confirmed' DESC, tl.updated_at DESC LIMIT 1",
                [item_id],
                |r| {
                    Ok(HitTopic {
                        id: r.get(0)?,
                        name: r.get(1)?,
                        category: r.get(2)?,
                        state: r.get(3)?,
                    })
                },
            )
            .optional()?)
    }

    fn has_person_named(&self, name: &str) -> Result<bool> {
        Ok(self.conn().query_row(
            "SELECT EXISTS (SELECT 1 FROM people WHERE note_path IS NOT NULL AND lower(display_name) = lower(?1))",
            [name],
            |r| r.get(0),
        )?)
    }

    fn feedback(&self, topic_id: i64) -> Result<HashMap<String, (String, Ts)>> {
        Ok(self
            .conn()
            .prepare_cached("SELECT key, verdict, at FROM search_feedback WHERE topic_id = ?1")?
            .query_map([topic_id], |r| Ok((r.get(0)?, (r.get(1)?, r.get(2)?))))?
            .collect::<rusqlite::Result<_>>()?)
    }
}

impl SearchFeedback for Sql<'_> {
    fn set(&self, topic_id: i64, key: &str, verdict: &str, title: &str, url: &str, source: &str) -> Result<()> {
        self.conn().execute(
            "INSERT INTO search_feedback (topic_id, key, verdict, title, url, source) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT (topic_id, key) DO UPDATE SET verdict = excluded.verdict, at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
            params![topic_id, key, verdict, title, url, source],
        )?;
        Ok(())
    }

    fn clear(&self, topic_id: i64, key: &str) -> Result<()> {
        self.conn().execute(
            "DELETE FROM search_feedback WHERE topic_id = ?1 AND key = ?2",
            params![topic_id, key],
        )?;
        Ok(())
    }
}

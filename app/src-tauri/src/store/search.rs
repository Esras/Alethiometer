//! Full-text search over items and topic names.

use super::Sql;
use crate::domain::search::{HitTopic, QuickHit, SearchHit, SearchViews, TopicHit};
use crate::error::Result;
use rusqlite::params;

impl SearchViews for Sql<'_> {
    fn topics_named(&self, text: &str, limit: usize) -> Result<Vec<TopicHit>> {
        Ok(self
            .conn()
            .prepare_cached("SELECT id, name, category FROM topics WHERE archived_at IS NULL AND name LIKE ?1 ORDER BY name LIMIT ?2")?
            .query_map(params![format!("%{}%", text.trim()), limit as i64], |r| Ok(TopicHit { id: r.get(0)?, name: r.get(1)?, category: r.get(2)? }))?
            .collect::<rusqlite::Result<_>>()?)
    }

    fn quick(&self, fts: &str, limit: usize) -> Result<Vec<QuickHit>> {
        Ok(self
            .conn()
            .prepare_cached(
                "SELECT i.id, i.title, s.kind, i.occurred_at,
                        (SELECT tl.topic_id FROM topic_links tl WHERE tl.item_id = i.id AND tl.state != 'wrong' LIMIT 1)
                   FROM items_fts f JOIN items i ON i.id = f.rowid JOIN sources s ON s.id = i.source_id
                  WHERE items_fts MATCH ?1 AND s.tag = 'work'
                  ORDER BY rank LIMIT ?2",
            )?
            .query_map(params![fts, limit as i64], |r| {
                Ok(QuickHit { id: r.get(0)?, title: r.get(1)?, source_kind: r.get(2)?, at: r.get(3)?, topic_id: r.get(4)? })
            })?
            .collect::<rusqlite::Result<_>>()?)
    }

    fn results(&self, fts: &str, limit: usize) -> Result<Vec<SearchHit>> {
        Ok(self
            .conn()
            .prepare_cached(
                "WITH hits AS (
                   SELECT rowid AS id, rank, snippet(items_fts, 1, char(1), char(2), '…', 14) AS snip
                     FROM items_fts WHERE items_fts MATCH ?1 ORDER BY rank LIMIT ?2 * 3)
                 SELECT i.id, i.title, s.kind, coalesce(i.ended_at, i.occurred_at), h.snip, coalesce(i.summary, ''),
                        t.id, t.name, t.category, tl.state,
                        (SELECT count(*) FROM item_messages m WHERE m.item_id = i.id AND m.context = 0 AND m.reply_to IS NULL)
                   FROM hits h JOIN items i ON i.id = h.id JOIN sources s ON s.id = i.source_id
                   LEFT JOIN topic_links tl ON tl.rowid = (
                        SELECT x.rowid FROM topic_links x JOIN topics y ON y.id = x.topic_id
                         WHERE x.item_id = i.id AND x.state != 'wrong' AND y.archived_at IS NULL
                         ORDER BY x.state = 'confirmed' DESC, x.updated_at DESC LIMIT 1)
                   LEFT JOIN topics t ON t.id = tl.topic_id
                  WHERE s.tag = 'work'
                  ORDER BY h.rank LIMIT ?2",
            )?
            .query_map(params![fts, limit as i64], |r| {
                let topic = match r.get::<_, Option<i64>>(6)? {
                    Some(id) => Some(HitTopic { id, name: r.get(7)?, category: r.get(8)?, state: r.get(9)? }),
                    None => None,
                };
                Ok(SearchHit {
                    id: r.get(0)?,
                    title: r.get(1)?,
                    source_kind: r.get(2)?,
                    at: r.get(3)?,
                    snippet: r.get(4)?,
                    summary: r.get(5)?,
                    topic,
                    msgs: r.get(10)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?)
    }
}

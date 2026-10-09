//! Topics.

use super::Sql;
use crate::domain::topics::{LinkedItem, TopicHead, TopicSummary, TopicViews, Topics};
use crate::error::Result;
use crate::time::SQL_NOW;
use crate::time::{Date, Due, Ts};
use rusqlite::{params, OptionalExtension};

impl TopicViews for Sql<'_> {
    fn list(&self, archived: bool, now: Ts, meetings_until: Ts) -> Result<Vec<TopicSummary>> {
        // Counts from the views; the latest signal is one seek on
        // `topic_links_latest`; the next meeting comes from the week's events.
        Ok(self
            .conn()
            .prepare_cached(
                "WITH latest AS (
                   SELECT t.id AS topic_id,
                          (SELECT tl.item_id FROM topic_links tl
                            WHERE tl.topic_id = t.id AND tl.state = 'confirmed' AND tl.item_at <= ?1
                            ORDER BY tl.item_at DESC LIMIT 1) AS item_id
                     FROM topics t WHERE (t.archived_at IS NOT NULL) = ?3),
                 meetings AS (
                   SELECT tl.topic_id, e.start, e.title,
                          row_number() OVER (PARTITION BY tl.topic_id ORDER BY e.start, e.id) AS n
                     FROM events e JOIN topic_links tl ON tl.item_id = e.item_id AND tl.state = 'confirmed'
                    WHERE e.start > ?1 AND e.start <= ?2)
                 SELECT t.id, t.name, t.category, t.obsidian_path,
                        coalesce(nc.n, 0), coalesce(oc.n, 0),
                        coalesce(li.ended_at, li.occurred_at), ls.kind,
                        m.start, m.title, t.priority
                   FROM topics t
                   JOIN latest l ON l.topic_id = t.id
                   LEFT JOIN items li ON li.id = l.item_id
                   LEFT JOIN sources ls ON ls.id = li.source_id
                   LEFT JOIN topic_new_counts nc ON nc.topic_id = t.id
                   LEFT JOIN topic_open_counts oc ON oc.topic_id = t.id
                   LEFT JOIN meetings m ON m.topic_id = t.id AND m.n = 1",
            )?
            .query_map(params![now, meetings_until, archived], |r| {
                let start: Option<Ts> = r.get(8)?;
                Ok(TopicSummary {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    category: r.get(2)?,
                    obsidian_path: r.get(3)?,
                    new_count: r.get(4)?,
                    open_count: r.get(5)?,
                    last_signal_at: r.get(6)?,
                    last_source: r.get(7)?,
                    next_meeting: match start {
                        Some(s) => Some((s, r.get::<_, Option<String>>(9)?.unwrap_or_default())),
                        None => None,
                    },
                    priority: r.get(10)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?)
    }

    fn head(&self, id: i64) -> Result<Option<TopicHead>> {
        Ok(self
            .conn()
            .query_row("SELECT name, category, obsidian_path, score, pills_json, archived_at IS NOT NULL, priority FROM topics WHERE id = ?1", [id], |r| {
                Ok(TopicHead { name: r.get(0)?, category: r.get(1)?, obsidian_path: r.get(2)?, score: r.get(3)?, pills_json: r.get(4)?, archived: r.get(5)?, priority: r.get(6)? })
            })
            .optional()?)
    }

    fn get_ref(&self, id: i64) -> Result<Option<crate::domain::topics::TopicRef>> {
        Ok(self
            .conn()
            .query_row("SELECT id, name, category FROM topics WHERE id = ?1", [id], |r| {
                Ok(crate::domain::topics::TopicRef {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    category: r.get(2)?,
                })
            })
            .optional()?)
    }

    fn note_paths(&self) -> Result<Vec<(String, i64)>> {
        Ok(self
            .conn()
            .prepare_cached(
                "SELECT obsidian_path, id FROM topics WHERE archived_at IS NULL AND obsidian_path IS NOT NULL",
            )?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?)
    }

    fn confirmed_items(&self, id: i64) -> Result<Vec<LinkedItem>> {
        Ok(self
            .conn()
            .prepare_cached(
                "SELECT i.id, s.kind, i.kind, i.title, i.summary, i.occurred_at, i.ended_at, i.due_at, i.due_day, i.status,
                        i.msg_count, i.owner, json_extract(i.raw_json, '$.recurringEventId')
                   FROM topic_links tl JOIN items i ON i.id = tl.item_id JOIN sources s ON s.id = i.source_id
                  WHERE tl.topic_id = ?1 AND tl.state = 'confirmed'",
            )?
            .query_map([id], |r| {
                Ok(LinkedItem {
                    id: r.get(0)?,
                    source_kind: r.get(1)?,
                    kind: r.get(2)?,
                    title: r.get(3)?,
                    summary: r.get(4)?,
                    at: r.get(5)?,
                    ended: r.get(6)?,
                    due: Due::from_columns(r.get(7)?, r.get::<_, Option<Date>>(8)?),
                    status: r.get(9)?,
                    msg_count: r.get(10)?,
                    owner: r.get(11)?,
                    series: r.get(12)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?)
    }

    fn repos(&self, id: i64) -> Result<Vec<String>> {
        Ok(self
            .conn()
            .prepare_cached("SELECT repo FROM topic_repos WHERE topic_id = ?1 ORDER BY repo")?
            .query_map([id], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?)
    }
}

impl Topics for Sql<'_> {
    fn find_by_name(&self, name: &str) -> Result<Option<i64>> {
        Ok(self
            .conn()
            .query_row(
                "SELECT id FROM topics WHERE archived_at IS NULL AND name = ?1 COLLATE NOCASE",
                [name],
                |r| r.get(0),
            )
            .optional()?)
    }

    fn create(&self, name: &str, category: &str) -> Result<i64> {
        self.conn().execute(
            "INSERT INTO topics (name, category) VALUES (?1, ?2)",
            params![name, category],
        )?;
        Ok(self.conn().last_insert_rowid())
    }

    fn rename(&self, id: i64, name: &str) -> Result<bool> {
        Ok(self
            .conn()
            .execute("UPDATE topics SET name = ?2 WHERE id = ?1", params![id, name])?
            == 1)
    }

    fn set_category(&self, id: i64, category: &str) -> Result<bool> {
        Ok(self
            .conn()
            .execute("UPDATE topics SET category = ?2 WHERE id = ?1", params![id, category])?
            == 1)
    }

    fn set_priority(&self, id: i64, priority: Option<&str>) -> Result<bool> {
        Ok(self
            .conn()
            .execute("UPDATE topics SET priority = ?2 WHERE id = ?1", params![id, priority])?
            == 1)
    }

    fn set_archived(&self, id: i64, archived: bool) -> Result<bool> {
        Ok(self.conn().execute(
            &format!("UPDATE topics SET archived_at = CASE WHEN ?2 THEN {SQL_NOW} END WHERE id = ?1"),
            params![id, archived],
        )? == 1)
    }

    fn set_note_path_if_unset(&self, id: i64, path: &str) -> Result<()> {
        self.conn().execute(
            "UPDATE topics SET obsidian_path = ?2 WHERE id = ?1 AND obsidian_path IS NULL",
            params![id, path],
        )?;
        Ok(())
    }

    fn filed_items(&self, id: i64) -> Result<Vec<i64>> {
        Ok(self
            .conn()
            .prepare_cached("SELECT item_id FROM topic_links WHERE topic_id = ?1 AND state = 'confirmed'")?
            .query_map([id], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?)
    }

    fn merge(&self, from: i64, into: i64, into_name: &str, into_category: &str) -> Result<()> {
        let c = self.conn();
        c.execute(
            "INSERT INTO topic_links (item_id, topic_id, state, confidence, reason, updated_at)
             SELECT item_id, ?2, state, confidence, reason, updated_at FROM topic_links WHERE topic_id = ?1 AND true
             ON CONFLICT (item_id, topic_id) DO UPDATE SET state = CASE
               WHEN 'confirmed' IN (state, excluded.state) THEN 'confirmed'
               WHEN 'suggested' IN (state, excluded.state) THEN 'suggested'
               ELSE 'wrong' END",
            params![from, into],
        )?;
        c.execute("DELETE FROM topic_links WHERE topic_id = ?1", [from])?;
        for table in ["topic_people", "topic_repos", "search_feedback", "topic_tags"] {
            c.execute(
                &format!("UPDATE OR IGNORE {table} SET topic_id = ?2 WHERE topic_id = ?1"),
                params![from, into],
            )?;
            c.execute(&format!("DELETE FROM {table} WHERE topic_id = ?1"), [from])?;
        }
        c.execute(
            "UPDATE notes_lines SET target_json = json_set(target_json, '$.id', ?2, '$.label', ?3, '$.category', ?4)
              WHERE json_extract(target_json, '$.type') = 'topic' AND json_extract(target_json, '$.id') = ?1",
            params![from, into, into_name, into_category],
        )?;
        // Ids aren't AUTOINCREMENT: a later topic can get `from`'s id and must not inherit its vector.
        c.execute("DELETE FROM vec_topics WHERE rowid = ?1", [from])?;
        c.execute(
            "DELETE FROM embed_meta WHERE kind = 'topic' AND id IN (?1, ?2)",
            params![from, into],
        )?;
        c.execute("DELETE FROM topics WHERE id = ?1", [from])?;
        Ok(())
    }
}

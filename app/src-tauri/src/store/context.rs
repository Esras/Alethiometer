//! Rows for the work AI's prompts.

use super::Sql;
use crate::domain::context::{ContextItem, ContextViews, TopicOverview, WorkEvent};
use crate::error::Result;
use crate::time::{Date, Due, Ts};
use rusqlite::{params, Row};

const ITEM: &str = "i.id, s.tag, s.kind, i.kind, i.title, i.summary, i.body, i.occurred_at, i.ended_at, i.due_at, i.due_day, i.status, i.owner";

fn item(r: &Row) -> rusqlite::Result<ContextItem> {
    Ok(ContextItem {
        id: r.get(0)?,
        tag: r.get(1)?,
        source_kind: r.get(2)?,
        kind: r.get(3)?,
        title: r.get(4)?,
        summary: r.get(5)?,
        body: r.get(6)?,
        at: r.get(7)?,
        ended: r.get(8)?,
        due: Due::from_columns(r.get(9)?, r.get::<_, Option<Date>>(10)?),
        status: r.get(11)?,
        owner: r.get(12)?,
    })
}

impl ContextViews for Sql<'_> {
    fn topic_items(&self, topic_id: i64, state: &str) -> Result<Vec<ContextItem>> {
        Ok(self
            .conn()
            .prepare_cached(&format!(
                "SELECT {ITEM}
                   FROM topic_links tl JOIN items i ON i.id = tl.item_id JOIN sources s ON s.id = i.source_id
                  WHERE tl.topic_id = ?1 AND tl.state = ?2 AND s.tag = 'work' AND (?2 = 'confirmed' OR i.seen = 0)
                  ORDER BY coalesce(i.ended_at, i.occurred_at) DESC, i.id DESC"
            ))?
            .query_map(params![topic_id, state], item)?
            .collect::<rusqlite::Result<_>>()?)
    }

    fn latest_messages(&self, item_id: i64, n: usize) -> Result<Vec<(Ts, String, String)>> {
        let mut rows: Vec<(Ts, String, String)> = self
            .conn()
            .prepare_cached("SELECT at, author, text FROM item_messages WHERE item_id = ?1 ORDER BY at DESC, id DESC LIMIT ?2")?
            .query_map(params![item_id, n as i64], |m| Ok((m.get(0)?, m.get(1)?, m.get(2)?)))?
            .collect::<rusqlite::Result<_>>()?;
        rows.reverse();
        Ok(rows)
    }

    fn topics_overview(&self) -> Result<Vec<TopicOverview>> {
        Ok(self
            .conn()
            .prepare_cached(
                "SELECT t.id, t.name, t.obsidian_path,
                        (SELECT count(*) FROM topic_links tl JOIN items i ON i.id = tl.item_id JOIN sources s ON s.id = i.source_id
                          WHERE tl.topic_id = t.id AND tl.state = 'confirmed' AND s.tag = 'work'),
                        (SELECT max(i.occurred_at) FROM topic_links tl JOIN items i ON i.id = tl.item_id JOIN sources s ON s.id = i.source_id
                          WHERE tl.topic_id = t.id AND tl.state = 'confirmed' AND s.tag = 'work')
                   FROM topics t WHERE t.archived_at IS NULL ORDER BY t.name",
            )?
            .query_map([], |r| Ok(TopicOverview { id: r.get(0)?, name: r.get(1)?, note_path: r.get(2)?, items: r.get(3)?, last: r.get(4)? }))?
            .collect::<rusqlite::Result<_>>()?)
    }

    fn recent_titles(&self, topic_id: i64, n: usize) -> Result<Vec<String>> {
        Ok(self
            .conn()
            .prepare_cached(
                "SELECT i.title FROM topic_links tl JOIN items i ON i.id = tl.item_id JOIN sources s ON s.id = i.source_id
                  WHERE tl.topic_id = ?1 AND tl.state = 'confirmed' AND s.tag = 'work'
                  ORDER BY i.occurred_at DESC LIMIT ?2",
            )?
            .query_map(params![topic_id, n as i64], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?)
    }

    fn work_events(&self, from: Ts, to: Ts) -> Result<Vec<WorkEvent>> {
        Ok(self
            .conn()
            .prepare_cached(
                "SELECT e.item_id, s.tag, e.start, e.end, e.title, e.attendees, e.tentative
                   FROM events e JOIN sources s ON s.id = e.source_id
                  WHERE s.tag = 'work' AND e.is_life_busy = 0 AND e.title IS NOT NULL AND e.start >= ?1 AND e.start < ?2
                  ORDER BY e.start",
            )?
            .query_map(params![from, to], |r| {
                Ok(WorkEvent { item_id: r.get(0)?, tag: r.get(1)?, start: r.get(2)?, end: r.get(3)?, title: r.get(4)?, attendees: r.get(5)?, tentative: r.get(6)? })
            })?
            .collect::<rusqlite::Result<_>>()?)
    }

    fn touched(&self, from: Ts, to: Ts) -> Result<Vec<ContextItem>> {
        Ok(self
            .conn()
            .prepare_cached(&format!(
                "SELECT {ITEM}
                   FROM items i JOIN sources s ON s.id = i.source_id
                  WHERE s.tag = 'work' AND i.kind != 'event'
                    AND ((i.occurred_at >= ?1 AND i.occurred_at < ?2) OR (i.ended_at >= ?1 AND i.ended_at < ?2)
                         OR EXISTS (SELECT 1 FROM item_messages m WHERE m.item_id = i.id AND m.at >= ?1 AND m.at < ?2))
                  ORDER BY coalesce(i.ended_at, i.occurred_at) DESC, i.id DESC"
            ))?
            .query_map(params![from, to], item)?
            .collect::<rusqlite::Result<_>>()?)
    }
}

//! Priorities.

use super::Sql;
use crate::domain::priorities::{PriorityRow, PriorityViews};
use crate::domain::topics::TopicRef;
use crate::error::Result;
use crate::time::{Date, Due, Ts};
use rusqlite::{params, OptionalExtension};

impl PriorityViews for Sql<'_> {
    fn list(&self) -> Result<Vec<PriorityRow>> {
        Ok(self
            .conn()
            .prepare_cached(
                "SELECT p.item_id, p.quadrant, p.score, p.reasons_json,
                        i.title, i.summary, i.body, i.url, i.occurred_at, i.due_at, i.due_day, i.kind, s.kind,
                        per.display_name, e.start, e.end, e.attendees, t.id, t.name, t.category
                   FROM priorities p
                   JOIN items i ON i.id = p.item_id
                   JOIN sources s ON s.id = i.source_id
                   LEFT JOIN people per ON per.id = i.author_person_id
                   LEFT JOIN events e ON e.item_id = i.id
                   LEFT JOIN topics t ON t.id = (SELECT tl.topic_id FROM topic_links tl
                                                  WHERE tl.item_id = i.id AND tl.state != 'wrong'
                                                  ORDER BY tl.state = 'confirmed' DESC LIMIT 1)
                  WHERE s.tag = 'work' AND i.status != 'done'
                  ORDER BY p.score DESC, i.id",
            )?
            .query_map([], |r| {
                let event = match (r.get::<_, Option<Ts>>(14)?, r.get::<_, Option<Ts>>(15)?) {
                    (Some(s), Some(e)) => Some((s, e)),
                    _ => None,
                };
                let topic = match r.get::<_, Option<i64>>(17)? {
                    Some(id) => Some(TopicRef { id, name: r.get(18)?, category: r.get(19)? }),
                    None => None,
                };
                Ok(PriorityRow {
                    item_id: r.get(0)?,
                    quadrant: r.get(1)?,
                    score: r.get(2)?,
                    reasons_json: r.get(3)?,
                    title: r.get(4)?,
                    summary: r.get(5)?,
                    body: r.get(6)?,
                    url: r.get(7)?,
                    at: r.get(8)?,
                    due: Due::from_columns(r.get(9)?, r.get::<_, Option<Date>>(10)?),
                    kind: r.get(11)?,
                    source_kind: r.get(12)?,
                    author: r.get(13)?,
                    event,
                    attendees: r.get(16)?,
                    topic,
                })
            })?
            .collect::<rusqlite::Result<_>>()?)
    }

    fn next_meeting(&self, item_id: i64, topic_id: Option<i64>, now: Ts, day_start: Ts, day_end: Ts) -> Result<Option<(Ts, Ts)>> {
        Ok(self
            .conn()
            .query_row(
                "SELECT e.start, e.end FROM events e
                  WHERE e.is_life_busy = 0 AND e.end > ?1 AND e.start >= ?2 AND e.start < ?5
                    AND (e.item_id = ?3 OR e.item_id IN
                         (SELECT tl.item_id FROM topic_links tl WHERE tl.topic_id = ?4 AND tl.state != 'wrong'))
                  ORDER BY e.item_id = ?3 DESC, e.start LIMIT 1",
                params![now, day_start, item_id, topic_id, day_end],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?)
    }
}

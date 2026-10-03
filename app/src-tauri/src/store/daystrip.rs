//! Day strip reads and calendar event writes.

use super::Sql;
use crate::domain::daystrip::{DayEvent, DayStripViews, Events};
use crate::error::Result;
use crate::time::Ts;
use rusqlite::{params, OptionalExtension};

impl DayStripViews for Sql<'_> {
    fn blocks(&self, from: Ts, to: Ts) -> Result<Vec<DayEvent>> {
        let mut stmt = self.conn().prepare_cached(
            "SELECT e.id, e.start, e.end, e.title, e.category, e.tentative, e.is_life_busy, e.buffer_min, e.item_id, e.attendees,
                    (SELECT group_concat(tl.topic_id) FROM topic_links tl
                      WHERE tl.item_id = e.item_id AND tl.state != 'wrong')
               FROM events e
              WHERE e.start >= ?1 AND e.start < ?2
              ORDER BY e.start",
        )?;
        let rows = stmt.query_map(params![from, to], |r| {
            let topics: Option<String> = r.get(10)?;
            Ok(DayEvent {
                id: r.get(0)?,
                start: r.get(1)?,
                end: r.get(2)?,
                title: r.get(3)?,
                category: r.get(4)?,
                tentative: r.get::<_, i64>(5)? == 1,
                life: r.get::<_, i64>(6)? == 1,
                buffer_min: r.get(7)?,
                item_id: r.get(8)?,
                attendees: r.get(9)?,
                topic_ids: topics.map(|s| s.split(',').filter_map(|x| x.parse().ok()).collect()).unwrap_or_default(),
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    fn next_linked_meeting(&self, topic_id: i64, after: Ts) -> Result<Option<Ts>> {
        Ok(self
            .conn()
            .query_row(
                "SELECT e.start FROM events e JOIN topic_links tl ON tl.item_id = e.item_id
                  WHERE tl.topic_id = ?1 AND tl.state != 'wrong' AND e.start > ?2
                  ORDER BY e.start LIMIT 1",
                params![topic_id, after],
                |r| r.get(0),
            )
            .optional()?)
    }
}

impl Events for Sql<'_> {
    fn set_buffer(&self, id: i64, minutes: i64) -> Result<bool> {
        Ok(self.conn().execute("UPDATE events SET buffer_min = ?1 WHERE id = ?2 AND is_life_busy = 1", params![minutes, id])? == 1)
    }
}

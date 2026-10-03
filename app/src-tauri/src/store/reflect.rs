//! Reflect.

use super::Sql;
use crate::domain::reflect::{CalendarEvent, CarryItem, ReflectViews, Reflections};
use crate::error::Result;
use crate::time::{Date, Ts};
use rusqlite::{params, OptionalExtension};

impl ReflectViews for Sql<'_> {
    fn events(&self, from: Ts, to: Ts) -> Result<Vec<CalendarEvent>> {
        Ok(self
            .conn()
            .prepare_cached(
                "SELECT e.start, e.end, e.title, e.category, e.is_life_busy, i.raw_json,
                        (SELECT t.name FROM topic_links tl JOIN topics t ON t.id = tl.topic_id
                          WHERE tl.item_id = e.item_id AND tl.state = 'confirmed' ORDER BY t.id LIMIT 1)
                   FROM events e LEFT JOIN items i ON i.id = e.item_id
                  WHERE e.start >= ?1 AND e.start < ?2
                  ORDER BY e.start",
            )?
            .query_map(params![from, to], |r| {
                Ok(CalendarEvent {
                    start: r.get(0)?,
                    end: r.get(1)?,
                    title: r.get(2)?,
                    category: r.get(3)?,
                    life: r.get::<_, i64>(4)? == 1,
                    raw_json: r.get(5)?,
                    topic: r.get(6)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?)
    }

    fn decisions(&self, from: Ts, to: Ts) -> Result<Vec<(String, Option<String>)>> {
        Ok(self
            .conn()
            .prepare_cached(
                "SELECT i.title, (SELECT t.name FROM topic_links tl JOIN topics t ON t.id = tl.topic_id
                                    WHERE tl.item_id = i.id AND tl.state = 'confirmed' LIMIT 1)
                   FROM items i JOIN sources s ON s.id = i.source_id
                  WHERE i.kind = 'decision' AND s.tag = 'work' AND i.occurred_at >= ?1 AND i.occurred_at < ?2
                  ORDER BY i.occurred_at",
            )?
            .query_map(params![from, to], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?)
    }

    fn summary(&self, scope: &str, period: Date, lens: &str) -> Result<Option<Option<String>>> {
        Ok(self
            .conn()
            .query_row(
                "SELECT summary FROM reflections WHERE scope = ?1 AND period_start = ?2 AND lens = ?3",
                params![scope, period, lens],
                |r| r.get(0),
            )
            .optional()?)
    }

    fn carry(&self, from: Date, to: Date, latest_open: bool) -> Result<Vec<CarryItem>> {
        let sql = if latest_open {
            "SELECT c.item_id, c.period, i.title, i.owner, i.raw_json, c.action
               FROM carry c JOIN items i ON i.id = c.item_id
              WHERE c.period >= ?1 AND c.period < ?2 AND c.action IS NULL
                AND c.period = (SELECT max(c2.period) FROM carry c2 WHERE c2.item_id = c.item_id AND c2.period < ?2)
              ORDER BY c.period, c.rowid"
        } else {
            "SELECT c.item_id, c.period, i.title, i.owner, i.raw_json, c.action
               FROM carry c JOIN items i ON i.id = c.item_id
              WHERE c.period >= ?1 AND c.period < ?2
              ORDER BY c.rowid"
        };
        Ok(self
            .conn()
            .prepare_cached(sql)?
            .query_map(params![from, to], |r| {
                Ok(CarryItem { item_id: r.get(0)?, period: r.get(1)?, title: r.get(2)?, owner: r.get(3)?, raw_json: r.get(4)?, action: r.get(5)? })
            })?
            .collect::<rusqlite::Result<_>>()?)
    }

    fn open_carry(&self, day: Date) -> Result<i64> {
        Ok(self.conn().query_row("SELECT count(*) FROM carry WHERE period = ?1 AND action IS NULL", [day], |r| r.get(0))?)
    }

    fn hold_state(&self, item_id: i64, period: Date) -> Result<Option<String>> {
        Ok(self
            .conn()
            .query_row(
                "SELECT state FROM outbox WHERE op = 'gcal.hold'
                   AND json_extract(payload_json, '$.item_id') = ?1 AND json_extract(payload_json, '$.carried_from') = ?2
                 ORDER BY id DESC LIMIT 1",
                params![item_id, period],
                |r| r.get(0),
            )
            .optional()?)
    }
}

impl Reflections for Sql<'_> {
    fn save_summary(&self, scope: &str, period: Date, lens: &str, path: &str, summary: Option<&str>) -> Result<()> {
        self.conn().execute(
            "INSERT INTO reflections (scope, period_start, lens, obsidian_path, summary) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (scope, period_start, lens) DO UPDATE SET obsidian_path = excluded.obsidian_path, summary = excluded.summary",
            params![scope, period, lens, path, summary],
        )?;
        Ok(())
    }

    fn carry_action(&self, item_id: i64, period: Date) -> Result<Option<Option<String>>> {
        Ok(self
            .conn()
            .query_row("SELECT action FROM carry WHERE item_id = ?1 AND period = ?2", params![item_id, period], |r| r.get(0))
            .optional()?)
    }

    fn set_carry_action(&self, item_id: i64, period: Date, action: Option<&str>) -> Result<()> {
        self.conn().execute("UPDATE carry SET action = ?1 WHERE item_id = ?2 AND period = ?3", params![action, item_id, period])?;
        Ok(())
    }

    fn carry_to(&self, item_id: i64, period: Date) -> Result<()> {
        self.conn().execute("INSERT OR IGNORE INTO carry (item_id, period) VALUES (?1, ?2)", params![item_id, period])?;
        Ok(())
    }

    fn uncarry(&self, item_id: i64, period: Date) -> Result<()> {
        self.conn().execute("DELETE FROM carry WHERE item_id = ?1 AND period = ?2 AND action IS NULL", params![item_id, period])?;
        Ok(())
    }
}

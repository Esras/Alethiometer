//! Todos.

use super::Sql;
use crate::domain::todos::{Held, TodoRow, TodoViews, Todos};
use crate::domain::topics::TopicRef;
use crate::time::{Date, Due};
use crate::time::SQL_NOW;
use rusqlite::{params, OptionalExtension};
use crate::error::Result;

impl TodoViews for Sql<'_> {
    fn open(&self) -> Result<Vec<TodoRow>> {
        Ok(self
            .conn()
            .prepare_cached(
                "SELECT i.id, s.kind, i.title, i.due_at, i.due_day, i.url, i.raw_json, i.summary, t.id, t.name, t.category,
                        (SELECT quadrant FROM priorities p WHERE p.item_id = i.id), e.minutes, e.by
                   FROM items i JOIN sources s ON s.id = i.source_id LEFT JOIN todo_estimates e ON e.item_id = i.id
                   LEFT JOIN topics t ON t.id = (SELECT tl.topic_id FROM topic_links tl WHERE tl.item_id = i.id AND tl.state != 'wrong'
                                                  ORDER BY tl.state = 'confirmed' DESC LIMIT 1)
                  WHERE s.tag = 'work' AND i.owner = 'you' AND i.status = 'open'
                    AND ((s.kind IN ('notes', 'obsidian') AND i.kind = 'task') OR (s.kind = 'jira' AND i.kind = 'issue'))
                  ORDER BY i.id",
            )?
            .query_map([], |r| {
                let topic = match r.get::<_, Option<i64>>(8)? {
                    Some(id) => Some(TopicRef { id, name: r.get(9)?, category: r.get(10)? }),
                    None => None,
                };
                let estimate = match (r.get::<_, Option<i64>>(12)?, r.get::<_, Option<String>>(13)?) {
                    (Some(m), Some(by)) => Some((m, by)),
                    _ => None,
                };
                Ok(TodoRow {
                    item_id: r.get(0)?,
                    source_kind: r.get(1)?,
                    title: r.get(2)?,
                    due: Due::from_columns(r.get(3)?, r.get::<_, Option<Date>>(4)?),
                    url: r.get(5)?,
                    raw_json: r.get(6)?,
                    summary: r.get(7)?,
                    topic,
                    priority: r.get(11)?,
                    estimate,
                })
            })?
            .collect::<rusqlite::Result<_>>()?)
    }

    fn held(&self, item_ids: &[i64]) -> Result<Vec<(i64, Held)>> {
        Ok(self
            .conn()
            .prepare_cached("SELECT item_id, target, text, due FROM held_todos WHERE item_id IN (SELECT value FROM json_each(?1)) AND state = 'held'")?
            .query_map([serde_json::to_string(item_ids)?], |r| Ok((r.get(0)?, Held { target: r.get(1)?, text: r.get(2)?, due: r.get(3)? })))?
            .collect::<rusqlite::Result<_>>()?)
    }
}

impl Todos for Sql<'_> {
    fn held(&self, item_id: i64) -> Result<Option<Held>> {
        Ok(self
            .conn()
            .query_row("SELECT target, text, due FROM held_todos WHERE item_id = ?1 AND state = 'held'", [item_id], |r| {
                Ok(Held { target: r.get(0)?, text: r.get(1)?, due: r.get(2)? })
            })
            .optional()?)
    }

    fn hold(&self, item_id: i64, t: &Held) -> Result<()> {
        self.conn().execute(
            "INSERT INTO held_todos (item_id, target, text, due) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (item_id) DO UPDATE SET target = excluded.target, text = excluded.text, due = excluded.due, state = 'held'",
            params![item_id, t.target, t.text, t.due],
        )?;
        Ok(())
    }

    fn unhold(&self, item_id: i64) -> Result<()> {
        self.conn().execute("DELETE FROM held_todos WHERE item_id = ?1", [item_id])?;
        Ok(())
    }

    fn set_discarded(&self, item_id: i64, discarded: bool) -> Result<()> {
        self.conn().execute(
            "UPDATE held_todos SET state = ?2 WHERE item_id = ?1",
            params![item_id, if discarded { "discarded" } else { "held" }],
        )?;
        Ok(())
    }

    fn set_estimate(&self, item_id: i64, minutes: Option<i64>) -> Result<()> {
        match minutes {
            Some(m) => self.conn().execute(
                &format!(
                    "INSERT INTO todo_estimates (item_id, minutes, by) VALUES (?1, ?2, 'you')
                     ON CONFLICT (item_id) DO UPDATE SET minutes = excluded.minutes, by = 'you', updated_at = {SQL_NOW}"
                ),
                params![item_id, m],
            )?,
            None => self.conn().execute("DELETE FROM todo_estimates WHERE item_id = ?1", [item_id])?,
        };
        Ok(())
    }
}

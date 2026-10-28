//! Consistency checks and their repairs (`domain::consistency`).

use super::Sql;
use crate::domain::consistency::{Consistency, ConsistencyViews, Leftover, MissingAuthors, SplitSender};
use crate::error::Result;
use rusqlite::params;

impl ConsistencyViews for Sql<'_> {
    fn missing_authors(&self, kinds: &[&str]) -> Result<Vec<MissingAuthors>> {
        let rows: Vec<(i64, String, i64)> = self
            .conn()
            .prepare_cached(
                "SELECT r.source_id, r.external_id, count(*)
                   FROM item_messages m JOIN items i ON i.id = m.item_id
                   JOIN items r ON r.id = coalesce(i.parent_id, i.id)
                   JOIN sources s ON s.id = r.source_id
                  WHERE m.author_id IS NULL AND m.context = 0 AND s.kind IN (SELECT value FROM json_each(?1))
                  GROUP BY r.id ORDER BY r.source_id, r.id",
            )?
            .query_map([serde_json::to_string(kinds)?], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })?
            .collect::<rusqlite::Result<_>>()?;
        let mut out: Vec<MissingAuthors> = Vec::new();
        for (source_id, ext, n) in rows {
            match out.last_mut().filter(|m| m.source_id == source_id) {
                Some(m) => {
                    m.items.push(ext);
                    m.messages += n;
                }
                None => out.push(MissingAuthors {
                    source_id,
                    items: vec![ext],
                    messages: n,
                }),
            }
        }
        Ok(out)
    }

    fn split_senders(&self) -> Result<Vec<SplitSender>> {
        Ok(self
            .conn()
            .prepare_cached(
                "WITH pairs AS (
                   SELECT b.handle AS address, b.person_id AS bare, n.person_id AS named
                     FROM person_handles b JOIN person_handles n
                       ON n.source_kind = 'gmail' AND n.handle >= b.handle || ' ' AND n.handle < b.handle || '!'
                    WHERE b.source_kind = 'gmail' AND instr(b.handle, ' ') = 0 AND n.person_id != b.person_id)
                 SELECT p.address, p.bare, p.named,
                        (SELECT count(*) FROM items WHERE author_person_id = p.bare),
                        (SELECT count(*) FROM items WHERE author_person_id = p.named),
                        EXISTS (SELECT 1 FROM people WHERE id IN (p.bare, p.named) AND note_path IS NOT NULL)
                   FROM pairs p
                  WHERE (SELECT count(*) FROM pairs q WHERE q.address = p.address) = 1
                  ORDER BY p.address",
            )?
            .query_map([], |r| {
                Ok(SplitSender {
                    address: r.get(0)?,
                    bare: r.get(1)?,
                    named: r.get(2)?,
                    bare_items: r.get(3)?,
                    named_items: r.get(4)?,
                    noted: r.get(5)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?)
    }

    fn calendar_leftovers(&self) -> Result<Vec<Leftover>> {
        Ok(self
            .conn()
            .prepare_cached(
                "SELECT i.source_id, i.id, i.title FROM items i JOIN sources s ON s.id = i.source_id
                  WHERE s.kind = 'gcal' AND i.status = 'done'
                    AND NOT EXISTS (SELECT 1 FROM events e WHERE e.item_id = i.id)
                    AND coalesce(i.summary, '') != 'cancelled'
                    AND coalesce(json_extract(i.raw_json, '$.change'), '') != 'cancelled'
                    AND NOT EXISTS (SELECT 1 FROM json_each(i.raw_json, '$.attendees') a
                         WHERE json_extract(a.value, '$.self') = 1 AND json_extract(a.value, '$.responseStatus') = 'declined')
                  ORDER BY i.id",
            )?
            .query_map([], |r| {
                Ok(Leftover {
                    source_id: r.get(0)?,
                    item_id: r.get(1)?,
                    title: r.get(2)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?)
    }

    fn repairs(&self, source_id: i64, limit: usize) -> Result<Vec<String>> {
        Ok(self
            .conn()
            .prepare_cached(
                "SELECT external_id FROM repair_queue WHERE source_id = ?1 ORDER BY created_at, external_id LIMIT ?2",
            )?
            .query_map(params![source_id, limit as i64], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?)
    }

    fn queued_repairs(&self) -> Result<Vec<(i64, i64)>> {
        Ok(self
            .conn()
            .prepare_cached("SELECT source_id, count(*) FROM repair_queue GROUP BY source_id ORDER BY source_id")?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?)
    }
}

impl Consistency for Sql<'_> {
    fn queue_repairs(&self, source_id: i64, external_ids: &[String]) -> Result<usize> {
        Ok(self.conn().execute(
            "INSERT OR IGNORE INTO repair_queue (source_id, external_id) SELECT ?1, value FROM json_each(?2)",
            params![source_id, serde_json::to_string(external_ids)?],
        )?)
    }

    fn repaired(&self, source_id: i64, external_ids: &[String]) -> Result<()> {
        self.conn().execute(
            "DELETE FROM repair_queue WHERE source_id = ?1 AND external_id IN (SELECT value FROM json_each(?2))",
            params![source_id, serde_json::to_string(external_ids)?],
        )?;
        Ok(())
    }

    fn merge_person(&self, from: i64, into: i64) -> Result<()> {
        let c = self.conn();
        c.execute(
            "UPDATE person_handles SET person_id = ?2 WHERE person_id = ?1",
            params![from, into],
        )?;
        c.execute(
            "UPDATE person_accounts SET person_id = ?2 WHERE person_id = ?1",
            params![from, into],
        )?;
        c.execute(
            "UPDATE items SET author_person_id = ?2 WHERE author_person_id = ?1",
            params![from, into],
        )?;
        c.execute(
            "INSERT OR IGNORE INTO topic_people (topic_id, person_id) SELECT topic_id, ?2 FROM topic_people WHERE person_id = ?1",
            params![from, into],
        )?;
        c.execute("DELETE FROM topic_people WHERE person_id = ?1", [from])?;
        c.execute("DELETE FROM people WHERE id = ?1", [from])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;

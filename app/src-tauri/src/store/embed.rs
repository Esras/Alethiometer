//! Vectors (sqlite-vec `vec_items`, `vec_topics`) and `embed_meta`.

use super::Sql;
use crate::domain::embed::{ItemSource, Stored, TopicSource, VecKind, VectorViews, Vectors};
use crate::error::Result;
use rusqlite::{params, OptionalExtension};

fn table(kind: VecKind) -> &'static str {
    match kind {
        VecKind::Item => "vec_items",
        VecKind::Topic => "vec_topics",
    }
}

fn to_blob(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|f| f.to_le_bytes()).collect()
}

fn stored(hash: Option<String>, model: Option<String>) -> Option<Stored> {
    Some(Stored { hash: hash?, model: model? })
}

impl Sql<'_> {
    fn topic_titles(&self, id: i64) -> Result<Vec<String>> {
        Ok(self
            .conn()
            .prepare_cached(
                "SELECT i.title FROM topic_links tl JOIN items i ON i.id = tl.item_id JOIN sources s ON s.id = i.source_id
                  WHERE tl.topic_id = ?1 AND tl.state = 'confirmed' AND s.tag = 'work'
                  ORDER BY i.occurred_at DESC LIMIT 20",
            )?
            .query_map([id], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?)
    }

    fn topic_sources(&self, filter: &str, arg: Option<i64>) -> Result<Vec<TopicSource>> {
        let sql = format!(
            "SELECT t.id, t.name, t.obsidian_path, m.hash, m.model FROM topics t
               LEFT JOIN embed_meta m ON m.kind = 'topic' AND m.id = t.id
              WHERE {filter} ORDER BY t.id"
        );
        let mut stmt = self.conn().prepare_cached(&sql)?;
        let map = |r: &rusqlite::Row| Ok((r.get::<_, i64>(0)?, r.get(1)?, r.get(2)?, stored(r.get(3)?, r.get(4)?)));
        let rows: Vec<(i64, String, Option<String>, Option<Stored>)> = match arg {
            Some(a) => stmt.query_map([a], map)?.collect::<rusqlite::Result<_>>()?,
            None => stmt.query_map([], map)?.collect::<rusqlite::Result<_>>()?,
        };
        rows.into_iter()
            .map(|(id, name, note_path, stored)| Ok(TopicSource { id, name, note_path, titles: self.topic_titles(id)?, stored }))
            .collect()
    }
}

impl VectorViews for Sql<'_> {
    fn vector(&self, kind: VecKind, id: i64) -> Result<Option<Vec<f32>>> {
        let blob: Option<Vec<u8>> = self
            .conn()
            .query_row(&format!("SELECT embedding FROM {} WHERE rowid = ?1", table(kind)), [id], |r| r.get(0))
            .optional()?;
        Ok(blob.map(|b| b.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect()))
    }

    fn nearest_items(&self, v: &[f32], k: usize) -> Result<Vec<(i64, f32)>> {
        let mut stmt = self.conn().prepare_cached(
            "SELECT v.rowid, v.distance FROM vec_items v
              WHERE v.embedding MATCH ?1 AND k = ?2 ORDER BY v.distance",
        )?;
        let rows = stmt.query_map(params![to_blob(v), k as i64], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, f64>(1)?)))?;
        // vec0 reports L2 distance; for unit vectors cos = 1 - d²/2.
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?.into_iter().map(|(id, d)| (id, 1.0 - (d * d / 2.0) as f32)).collect())
    }

    fn each_work_item(&self, f: &mut dyn FnMut(ItemSource) -> bool) -> Result<()> {
        let mut stmt = self.conn().prepare_cached(
            "SELECT i.id, i.title, coalesce(i.summary, ''), coalesce(i.body, ''), m.hash, m.model
               FROM items i JOIN sources s ON s.id = i.source_id
               LEFT JOIN embed_meta m ON m.kind = 'item' AND m.id = i.id
              WHERE s.tag = 'work'
              ORDER BY i.occurred_at DESC",
        )?;
        let mut rows = stmt.query([])?;
        while let Some(r) = rows.next()? {
            let item = ItemSource { id: r.get(0)?, title: r.get(1)?, summary: r.get(2)?, body: r.get(3)?, stored: stored(r.get(4)?, r.get(5)?) };
            if !f(item) {
                break;
            }
        }
        Ok(())
    }

    fn topic_source(&self, id: i64) -> Result<Option<TopicSource>> {
        Ok(self.topic_sources("t.id = ?1", Some(id))?.pop())
    }

    fn active_topics(&self) -> Result<Vec<TopicSource>> {
        self.topic_sources("t.archived_at IS NULL", None)
    }

    fn counts(&self) -> Result<(i64, i64)> {
        let c = self.conn();
        let indexed = c.query_row("SELECT count(*) FROM embed_meta WHERE kind = 'item'", [], |r| r.get(0))?;
        let work = c.query_row("SELECT count(*) FROM items i JOIN sources s ON s.id = i.source_id WHERE s.tag = 'work'", [], |r| r.get(0))?;
        Ok((indexed, work))
    }
}

impl Vectors for Sql<'_> {
    fn store(&self, kind: VecKind, id: i64, model: &str, text_hash: &str, v: &[f32]) -> Result<()> {
        let t = table(kind);
        let c = self.conn();
        c.execute(&format!("DELETE FROM {t} WHERE rowid = ?1"), [id])?;
        c.execute(&format!("INSERT INTO {t} (rowid, embedding) VALUES (?1, ?2)"), params![id, to_blob(v)])?;
        c.execute(
            "INSERT INTO embed_meta (kind, id, model, hash) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (kind, id) DO UPDATE SET model = excluded.model, hash = excluded.hash, at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
            params![kind.as_str(), id, model, text_hash],
        )?;
        Ok(())
    }
}

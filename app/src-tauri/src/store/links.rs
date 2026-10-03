//! Item–topic links.

use super::Sql;
use crate::domain::links::Links;
use crate::error::Result;
use crate::time::SQL_NOW;
use rusqlite::{params, OptionalExtension};

impl Links for Sql<'_> {
    fn state(&self, item_id: i64, topic_id: i64) -> Result<Option<String>> {
        Ok(self
            .conn()
            .query_row("SELECT state FROM topic_links WHERE item_id = ?1 AND topic_id = ?2", params![item_id, topic_id], |r| r.get(0))
            .optional()?)
    }

    fn set_state(&self, item_id: i64, topic_id: i64, state: &str) -> Result<bool> {
        Ok(self.conn().execute(
            &format!("UPDATE topic_links SET state = ?3, updated_at = {SQL_NOW} WHERE item_id = ?1 AND topic_id = ?2"),
            params![item_id, topic_id, state],
        )? == 1)
    }

    fn confirm(&self, item_ids: &[i64], topic_id: i64, reason: &str) -> Result<()> {
        self.conn().execute(
            &format!(
                "INSERT INTO topic_links (item_id, topic_id, state, confidence, reason)
                 SELECT value, ?2, 'confirmed', 1.0, ?3 FROM json_each(?1) WHERE true
                 ON CONFLICT (item_id, topic_id) DO UPDATE SET state = 'confirmed', updated_at = {SQL_NOW}"
            ),
            params![serde_json::to_string(item_ids)?, topic_id, reason],
        )?;
        Ok(())
    }

    fn add_confirmed(&self, item_id: i64, topic_id: i64, reason: &str) -> Result<()> {
        self.conn().execute(
            "INSERT INTO topic_links (item_id, topic_id, state, confidence, reason) VALUES (?1, ?2, 'confirmed', 1.0, ?3)
             ON CONFLICT (item_id, topic_id) DO NOTHING",
            params![item_id, topic_id, reason],
        )?;
        Ok(())
    }

    fn pick(&self, item_id: i64, topic_id: i64, state: &str) -> Result<()> {
        self.conn().execute(
            &format!(
                "INSERT INTO topic_links (item_id, topic_id, state, confidence, reason) VALUES (?1, ?2, ?3, 1.0, 'picked by you')
                 ON CONFLICT (item_id, topic_id) DO UPDATE SET state = ?3, confidence = 1.0, reason = 'picked by you', updated_at = {SQL_NOW}"
            ),
            params![item_id, topic_id, state],
        )?;
        Ok(())
    }

    fn restore(&self, item_id: i64, topic_id: i64, state: Option<&str>) -> Result<()> {
        match state {
            None => self.conn().execute("DELETE FROM topic_links WHERE item_id = ?1 AND topic_id = ?2", params![item_id, topic_id])?,
            Some(st) => self.conn().execute(
                &format!(
                    "INSERT INTO topic_links (item_id, topic_id, state, confidence, reason) VALUES (?1, ?2, ?3, 1.0, 'picked by you')
                     ON CONFLICT (item_id, topic_id) DO UPDATE SET state = ?3, updated_at = {SQL_NOW}"
                ),
                params![item_id, topic_id, st],
            )?,
        };
        Ok(())
    }

    fn wrong_all(&self, item_id: i64) -> Result<()> {
        self.conn().execute("UPDATE topic_links SET state = 'wrong' WHERE item_id = ?1 AND state != 'wrong'", [item_id])?;
        Ok(())
    }

    fn suggest_from_repos(&self, source_id: i64) -> Result<usize> {
        Ok(self.conn().execute(
            "WITH repo_topic AS (
               SELECT lower(tr.repo) AS repo, min(tr.topic_id) AS topic_id, min(tr.repo) AS name
                 FROM topic_repos tr JOIN topics t ON t.id = tr.topic_id
                WHERE t.archived_at IS NULL
                GROUP BY lower(tr.repo) HAVING count(DISTINCT tr.topic_id) = 1)
             INSERT INTO topic_links (item_id, topic_id, state, confidence, reason)
             SELECT i.id, rt.topic_id, 'suggested', 0.9, 'linked repo ' || rt.name
               FROM items i JOIN repo_topic rt ON rt.repo = lower(substr(i.external_id, 1, instr(i.external_id, '#') - 1))
              WHERE i.source_id = ?1
                AND NOT EXISTS (SELECT 1 FROM topic_links l WHERE l.item_id = i.id)",
            [source_id],
        )?)
    }

    fn suggest_if_unlinked(&self, item_id: i64, topic_id: i64, confidence: f64, reason: &str) -> Result<bool> {
        Ok(self.conn().execute(
            "INSERT INTO topic_links (item_id, topic_id, state, confidence, reason)
             SELECT ?1, ?2, 'suggested', ?3, ?4
              WHERE NOT EXISTS (SELECT 1 FROM topic_links WHERE item_id = ?1 AND state != 'wrong')
             ON CONFLICT (item_id, topic_id) DO NOTHING",
            params![item_id, topic_id, confidence, reason],
        )? == 1)
    }

    fn suggest(&self, item_id: i64, topic_id: i64, confidence: f64, reason: &str) -> Result<()> {
        self.conn().execute(
            "INSERT INTO topic_links (item_id, topic_id, state, confidence, reason) VALUES (?1, ?2, 'suggested', ?3, ?4)",
            params![item_id, topic_id, confidence, reason],
        )?;
        Ok(())
    }
}

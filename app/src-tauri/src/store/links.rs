//! Item–topic links.

use super::Sql;
use crate::domain::links::Links;
use crate::error::Result;
use rusqlite::params;

impl Links for Sql<'_> {
    fn suggest(&self, item_id: i64, topic_id: i64, confidence: f64, reason: &str) -> Result<()> {
        self.conn().execute(
            "INSERT INTO topic_links (item_id, topic_id, state, confidence, reason) VALUES (?1, ?2, 'suggested', ?3, ?4)",
            params![item_id, topic_id, confidence, reason],
        )?;
        Ok(())
    }
}

//! Topic categories.

use super::Sql;
use crate::domain::categories::{Categories, Category, CategoryViews};
use crate::error::Result;
use rusqlite::params;

impl CategoryViews for Sql<'_> {
    fn list(&self) -> Result<Vec<Category>> {
        Ok(self
            .conn()
            .prepare_cached("SELECT key, label, color FROM categories ORDER BY position, label")?
            .query_map([], |r| Ok(Category { key: r.get(0)?, label: r.get(1)?, color: r.get(2)? }))?
            .collect::<rusqlite::Result<_>>()?)
    }

    fn uses(&self, key: &str) -> Result<(i64, i64)> {
        Ok(self.conn().query_row(
            "SELECT (SELECT count(*) FROM topics WHERE category = ?1), (SELECT count(*) FROM events WHERE category = ?1)",
            [key],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?)
    }
}

impl Categories for Sql<'_> {
    fn replace(&self, rows: &[Category]) -> Result<()> {
        self.conn().execute("DELETE FROM categories", [])?;
        for (i, c) in rows.iter().enumerate() {
            self.conn().execute(
                "INSERT INTO categories (key, label, color, position) VALUES (?1, ?2, ?3, ?4)",
                params![c.key, c.label, c.color, i as i64],
            )?;
        }
        Ok(())
    }

    fn add(&self, c: &Category) -> Result<()> {
        self.conn().execute(
            "INSERT INTO categories (key, label, color, position) VALUES (?1, ?2, ?3, (SELECT coalesce(max(position), 0) + 1 FROM categories))",
            params![c.key, c.label, c.color],
        )?;
        Ok(())
    }

    fn update(&self, key: &str, label: Option<&str>, color: Option<&str>) -> Result<bool> {
        Ok(self.conn().execute(
            "UPDATE categories SET label = coalesce(?2, label), color = coalesce(?3, color) WHERE key = ?1",
            params![key, label, color],
        )? == 1)
    }

    fn delete(&self, key: &str) -> Result<()> {
        self.conn().execute("DELETE FROM categories WHERE key = ?1", [key])?;
        Ok(())
    }
}

//! Todos.

use super::Sql;
use crate::domain::todos::{Held, TodoViews};
use crate::error::Result;

impl TodoViews for Sql<'_> {
    fn held(&self, item_ids: &[i64]) -> Result<Vec<(i64, Held)>> {
        Ok(self
            .conn()
            .prepare_cached("SELECT item_id, target, text, due FROM held_todos WHERE item_id IN (SELECT value FROM json_each(?1)) AND state = 'held'")?
            .query_map([serde_json::to_string(item_ids)?], |r| Ok((r.get(0)?, Held { target: r.get(1)?, text: r.get(2)?, due: r.get(3)? })))?
            .collect::<rusqlite::Result<_>>()?)
    }
}

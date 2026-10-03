//! Source rows.

use super::Sql;
use crate::domain::sources::{SourceRow, SourceViews};
use crate::error::Result;

impl SourceViews for Sql<'_> {
    fn of_kind(&self, kind: &str) -> Result<Vec<SourceRow>> {
        Ok(self
            .conn()
            .prepare_cached("SELECT id, kind, tag, name, enabled, config_json, cursor FROM sources WHERE kind = ?1 ORDER BY id")?
            .query_map([kind], |r| {
                Ok(SourceRow { id: r.get(0)?, kind: r.get(1)?, tag: r.get(2)?, name: r.get(3)?, enabled: r.get(4)?, config_json: r.get(5)?, cursor: r.get(6)? })
            })?
            .collect::<rusqlite::Result<_>>()?)
    }
}

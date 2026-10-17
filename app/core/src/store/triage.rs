//! Triage history and "Not now" on ignore suggestions.

use super::Sql;
use crate::domain::triage::{KeyStats, TriageKey, TriageLog, TriageViews};
use crate::error::Result;
use crate::time::Ts;
use rusqlite::{params, OptionalExtension};

impl TriageViews for Sql<'_> {
    fn key_stats(&self, source_id: i64, key: &TriageKey, since: Ts) -> Result<KeyStats> {
        Ok(self.conn().prepare_cached(
            "SELECT count(*) FILTER (WHERE e.action IN ('dismiss', 'trash')),
                    count(*),
                    (SELECT count(DISTINCT f.key) FROM triage_keys f
                      WHERE f.kind = 'from'
                        AND f.event_id IN (SELECT e2.id FROM triage_events e2 JOIN triage_keys k2 ON k2.event_id = e2.id
                                            WHERE e2.source_id = ?1 AND e2.at >= ?4 AND e2.action IN ('dismiss', 'trash')
                                              AND k2.kind = ?2 AND k2.key = ?3))
               FROM triage_events e JOIN triage_keys k ON k.event_id = e.id
              WHERE e.source_id = ?1 AND k.kind = ?2 AND k.key = ?3 AND e.at >= ?4",
        )?
        .query_row(params![source_id, key.kind, key.key, since], |r| {
            Ok(KeyStats {
                dismissed: r.get(0)?,
                total: r.get(1)?,
                senders: r.get(2)?,
            })
        })?)
    }

    fn declined_until(&self, source_id: i64, key: &TriageKey) -> Result<Option<Ts>> {
        Ok(self
            .conn()
            .query_row(
                "SELECT until FROM ignore_declines WHERE source_id = ?1 AND kind = ?2 AND key = ?3",
                params![source_id, key.kind, key.key],
                |r| r.get(0),
            )
            .optional()?)
    }
}

impl TriageLog for Sql<'_> {
    fn record(&self, item_id: i64, source_id: i64, action: &str, at: Ts, keys: &[TriageKey]) -> Result<i64> {
        self.conn().execute(
            "INSERT INTO triage_events (item_id, source_id, action, at) VALUES (?1, ?2, ?3, ?4)",
            params![item_id, source_id, action, at],
        )?;
        let id = self.conn().last_insert_rowid();
        let mut add = self
            .conn()
            .prepare_cached("INSERT INTO triage_keys (event_id, kind, key) VALUES (?1, ?2, ?3)")?;
        for k in keys {
            add.execute(params![id, k.kind, k.key])?;
        }
        Ok(id)
    }

    fn forget(&self, event_id: i64, item_id: i64) -> Result<()> {
        self.conn().execute(
            "DELETE FROM triage_events WHERE id = ?1 AND item_id = ?2",
            params![event_id, item_id],
        )?;
        Ok(())
    }

    fn decline(&self, source_id: i64, key: &TriageKey, until: Ts) -> Result<()> {
        self.conn().execute(
            "INSERT INTO ignore_declines (source_id, kind, key, until) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (source_id, kind, key) DO UPDATE SET until = excluded.until",
            params![source_id, key.kind, key.key, until],
        )?;
        Ok(())
    }
}

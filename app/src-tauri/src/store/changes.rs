//! Which tables committed writes changed, for live queries (`crate::live`).
//!
//! The writer's update hook collects the tables a transaction writes;
//! `Db::write` publishes them once the commit has gone through (so a reader
//! started on the news sees the data), and drops them on a rollback.
//!
//! The update hook doesn't see a `DELETE` without `WHERE`, which SQLite
//! runs by truncating the table, so the writer's authorizer (`authorize`)
//! turns that off: SQLite documents that answering `SQLITE_IGNORE` to a
//! delete makes it delete row by row (R-17228-37124).

use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
use rusqlite::Connection;
use std::collections::BTreeSet;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::Duration;

#[derive(Default)]
pub struct Changes {
    /// Tables written by the transaction in progress.
    pending: Mutex<BTreeSet<String>>,
    /// Tables changed by commits since the last `take`.
    committed: Mutex<BTreeSet<String>>,
    cv: Condvar,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Changes {
    pub(super) fn install(self: &Arc<Self>, conn: &Connection) -> rusqlite::Result<()> {
        conn.authorizer(Some(|ctx: AuthContext<'_>| authorize(ctx, None)))?;
        let c = self.clone();
        conn.update_hook(Some(move |_: rusqlite::hooks::Action, _: &str, table: &str, _: i64| {
            lock(&c.pending).insert(table.to_owned());
        }))?;
        let c = self.clone();
        conn.rollback_hook(Some(move || lock(&c.pending).clear()))?;
        Ok(())
    }

    /// The transaction committed: what it wrote is news.
    pub(super) fn publish(&self) {
        let tables = std::mem::take(&mut *lock(&self.pending));
        if !tables.is_empty() {
            lock(&self.committed).extend(tables);
            self.cv.notify_all();
        }
    }

    /// The transaction didn't commit.
    pub(super) fn discard(&self) {
        lock(&self.pending).clear();
    }

    /// Tables changed where no hook saw it (another process, a settings change).
    pub fn touch(&self, tables: impl IntoIterator<Item = String>) {
        lock(&self.committed).extend(tables);
        self.cv.notify_all();
    }

    /// Wait up to `timeout` for changes; the tables changed since the last
    /// call (empty if nothing changed in time).
    pub fn take(&self, timeout: Duration) -> BTreeSet<String> {
        let g = lock(&self.committed);
        let (mut g, _) = self
            .cv
            .wait_timeout_while(g, timeout, |t| t.is_empty())
            .unwrap_or_else(|e| e.into_inner());
        std::mem::take(&mut *g)
    }
}

/// The writer's authorizer: deletes go row by row (so the update hook sees
/// them), and the tables read are noted in `reads` when it's given.
pub(super) fn authorize(ctx: AuthContext<'_>, reads: Option<&Mutex<BTreeSet<String>>>) -> Authorization {
    match ctx.action {
        AuthAction::Delete { .. } => Authorization::Ignore,
        AuthAction::Read { table_name, .. } => {
            if let Some(reads) = reads {
                lock(reads).insert(table_name.to_owned());
            }
            Authorization::Allow
        }
        _ => Authorization::Allow,
    }
}

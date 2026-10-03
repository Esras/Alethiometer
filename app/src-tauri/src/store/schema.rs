//! SQLite connection and migrations (PRAGMA user_version).

use rusqlite::Connection;
use std::path::Path;

/// One schema step: SQL, or Rust for what SQL can't do (time zones, JSON).
enum Step {
    Sql(&'static str),
    Rust(fn(&Connection) -> anyhow::Result<()>),
}
use Step::{Rust, Sql};

const MIGRATIONS: &[Step] = &[
    Sql(include_str!("../../migrations/0001_init.sql")),
    Sql(include_str!("../../migrations/0002_sync.sql")),
    Sql(include_str!("../../migrations/0003_splits.sql")),
    Sql(include_str!("../../migrations/0004_categories.sql")),
    Sql(include_str!("../../migrations/0005_message_context.sql")),
    Sql(include_str!("../../migrations/0006_ai_usage.sql")),
    Sql(include_str!("../../migrations/0007_ai_suggest_run.sql")),
    Sql(include_str!("../../migrations/0008_replies.sql")),
    Sql(include_str!("../../migrations/0009_held_todos.sql")),
    Sql(include_str!("../../migrations/0010_todo_estimates.sql")),
    Sql(include_str!("../../migrations/0011_people.sql")),
    Sql(include_str!("../../migrations/0012_account_profiles.sql")),
    Sql(include_str!("../../migrations/0013_bot_accounts.sql")),
    Sql(include_str!("../../migrations/0014_message_read.sql")),
    Sql(include_str!("../../migrations/0015_embeddings.sql")),
    Sql(include_str!("../../migrations/0016_glean.sql")),
    Sql(include_str!("../../migrations/0017_agents.sql")),
    Sql(include_str!("../../migrations/0018_agent_hosts.sql")),
    Sql(include_str!("../../migrations/0019_indexes.sql")),
    Sql(include_str!("../../migrations/0020_due_day.sql")),
    Rust(super::utc_migration::run),
    Sql(include_str!("../../migrations/0022_utc_defaults.sql")),
    Rust(super::message_keys::run),
    Sql(include_str!("../../migrations/0024_sync_streams.sql")),
    Sql(include_str!("../../migrations/0025_remote_rev.sql")),
    Sql(include_str!("../../migrations/0026_item_rev.sql")),
    Sql(include_str!("../../migrations/0027_outbox_lease.sql")),
    Sql(include_str!("../../migrations/0028_topic_derived.sql")),
    Sql(include_str!("../../migrations/0029_topic_corpus_rev.sql")),
];

/// sqlite-vec, loaded into every connection this process opens.
fn register_vec() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| unsafe {
        // SAFETY: sqlite3_vec_init has the extension entry point signature
        // SQLite expects; registering it before any connection is opened.
        rusqlite::ffi::sqlite3_auto_extension(Some(std::mem::transmute::<*const (), unsafe extern "C" fn(*mut rusqlite::ffi::sqlite3, *mut *mut std::os::raw::c_char, *const rusqlite::ffi::sqlite3_api_routines) -> std::os::raw::c_int>(sqlite_vec::sqlite3_vec_init as *const ())));
    });
}

pub fn open(path: &Path) -> anyhow::Result<Connection> {
    register_vec();
    let conn = Connection::open(path)?;
    init(conn)
}

pub fn open_in_memory() -> anyhow::Result<Connection> {
    register_vec();
    init(Connection::open_in_memory()?)
}

fn init(mut conn: Connection) -> anyhow::Result<Connection> {
    conn.pragma_update(None, "journal_mode", "WAL")?;
    // Another process (the devserver and the app on one data dir) waits
    // instead of failing with "database is locked".
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    migrate(&mut conn, MIGRATIONS.len())?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    // Planner statistics for the tables that changed a lot since last time.
    conn.execute_batch("PRAGMA optimize = 0x10002;")?;
    Ok(conn)
}

/// Foreign keys are off while migrating (a table rebuild drops and renames a
/// table others point at) and checked at the end: a broken reference the
/// migration made fails it; one that was already there doesn't.
fn migrate(conn: &mut Connection, to: usize) -> anyhow::Result<()> {
    let current: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    if current as usize > MIGRATIONS.len() {
        anyhow::bail!("the database is from a newer Alethiometer (schema {current}, this build knows {}): use that version", MIGRATIONS.len());
    }
    if current as usize >= to {
        return Ok(());
    }
    conn.pragma_update(None, "foreign_keys", "OFF")?;
    let before = broken_references(conn)?;
    for (i, step) in MIGRATIONS.iter().enumerate().take(to).skip(current as usize) {
        let tx = conn.transaction()?;
        match step {
            Sql(sql) => tx.execute_batch(sql)?,
            Rust(f) => f(&tx)?,
        }
        tx.pragma_update(None, "user_version", (i + 1) as i64)?;
        tx.commit()?;
    }
    let new: Vec<String> = broken_references(conn)?.difference(&before).cloned().collect();
    if !new.is_empty() {
        anyhow::bail!("migration left broken references: {}", new.join(", "));
    }
    Ok(())
}

fn broken_references(conn: &Connection) -> anyhow::Result<std::collections::BTreeSet<String>> {
    Ok(conn
        .prepare("PRAGMA foreign_key_check")?
        .query_map([], |r| Ok(format!("{} row {:?} -> {}", r.get::<_, String>(0)?, r.get::<_, Option<i64>>(1)?, r.get::<_, String>(2)?)))?
        .collect::<rusqlite::Result<_>>()?)
}

/// An in-memory database migrated only up to schema `version`, for testing a later step.
#[cfg(test)]
pub fn open_in_memory_at(version: usize) -> Connection {
    register_vec();
    let mut conn = Connection::open_in_memory().unwrap();
    migrate(&mut conn, version).unwrap();
    conn
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rebuilt_tables_keep_their_rows_and_an_old_broken_reference_doesnt_block_startup() {
        let mut conn = open_in_memory_at(19);
        conn.execute_batch(
            "INSERT INTO sources (id, kind, tag, name) VALUES (1, 'slack', 'work', 'Slack');
             INSERT INTO items (id, source_id, external_id, kind, title, occurred_at) VALUES (1, 1, 'a', 'thread', 'x', '2026-09-28T10:52:00');
             INSERT INTO topics (id, name, category) VALUES (1, 'T', 'work');
             INSERT INTO topic_links (item_id, topic_id, state, updated_at) VALUES (1, 1, 'confirmed', '2026-09-28 08:52:00');
             INSERT INTO topic_links (item_id, topic_id, state) VALUES (404, 1, 'suggested');
             INSERT INTO outbox (source_id, op, payload_json, created_at, receipt_json) VALUES (1, 'x', '{}', '2026-09-28 08:52:00', '{\"start\":\"2026-09-28T10:52:00\"}');",
        )
        .unwrap();
        migrate(&mut conn, MIGRATIONS.len()).unwrap();
        let links: Vec<(i64, String)> = conn.prepare("SELECT item_id, updated_at FROM topic_links ORDER BY item_id").unwrap().query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().map(Result::unwrap).collect();
        assert_eq!(links.len(), 2, "the dangling link (item 404) was already there and stays");
        assert_eq!(links[0], (1, "2026-09-28T08:52:00.000Z".into()));
        let (created, receipt): (String, String) = conn.query_row("SELECT created_at, receipt_json FROM outbox", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!(created, "2026-09-28T08:52:00.000Z");
        assert!(receipt.contains(".000Z"), "{receipt}");
        // New rows get UTC defaults.
        conn.execute("INSERT INTO outbox (source_id, op, payload_json) VALUES (1, 'y', '{}')", []).unwrap();
        let fresh: String = conn.query_row("SELECT created_at FROM outbox WHERE op = 'y'", [], |r| r.get(0)).unwrap();
        assert!(crate::time::Ts::parse(&fresh).is_some(), "{fresh}");
    }
}

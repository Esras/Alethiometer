//! SQLite connection and migrations (PRAGMA user_version).

use rusqlite::Connection;
use std::path::Path;

const MIGRATIONS: &[&str] = &[
    include_str!("../migrations/0001_init.sql"),
    include_str!("../migrations/0002_sync.sql"),
    include_str!("../migrations/0003_splits.sql"),
    include_str!("../migrations/0004_categories.sql"),
    include_str!("../migrations/0005_message_context.sql"),
    include_str!("../migrations/0006_ai_usage.sql"),
    include_str!("../migrations/0007_ai_suggest_run.sql"),
    include_str!("../migrations/0008_replies.sql"),
    include_str!("../migrations/0009_held_todos.sql"),
    include_str!("../migrations/0010_todo_estimates.sql"),
    include_str!("../migrations/0011_people.sql"),
    include_str!("../migrations/0012_account_profiles.sql"),
    include_str!("../migrations/0013_bot_accounts.sql"),
    include_str!("../migrations/0014_message_read.sql"),
    include_str!("../migrations/0015_embeddings.sql"),
    include_str!("../migrations/0016_glean.sql"),
    include_str!("../migrations/0017_agents.sql"),
    include_str!("../migrations/0018_agent_hosts.sql"),
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
    conn.pragma_update(None, "foreign_keys", "ON")?;
    migrate(&mut conn)?;
    Ok(conn)
}

fn migrate(conn: &mut Connection) -> anyhow::Result<()> {
    let current: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(current as usize) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", (i + 1) as i64)?;
        tx.commit()?;
    }
    Ok(())
}

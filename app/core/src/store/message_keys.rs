//! Migration 23: every message has a key, and a key names one message per
//! item (`UNIQUE(item_id, ext_key)`), so a sync upserts messages instead of
//! replacing them and splits address them by key (`app/docs/db-plan.md`, Phase 2).

use rusqlite::{params, Connection};

pub fn run(conn: &Connection) -> anyhow::Result<()> {
    // Messages stored before keys existed (the sample data's, early syncs')
    // get the key a sync would give them: a hash of time, author and text.
    let unkeyed: Vec<(i64, String, String, String)> = conn
        .prepare("SELECT id, at, author, text FROM item_messages WHERE ext_key IS NULL")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
        .collect::<rusqlite::Result<_>>()?;
    for (id, at, author, text) in &unkeyed {
        conn.execute(
            "UPDATE item_messages SET ext_key = ?1 WHERE id = ?2",
            params![crate::domain::splits::content_key(at, author, text), id],
        )?;
    }
    // One row per key in an item: the read one if any, else the first stored.
    let dropped = conn.execute(
        "DELETE FROM item_messages WHERE id IN (
           SELECT id FROM (
             SELECT id, row_number() OVER (PARTITION BY item_id, ext_key ORDER BY read DESC, id) AS n FROM item_messages)
            WHERE n > 1)",
        [],
    )?;
    if !unkeyed.is_empty() || dropped > 0 {
        eprintln!(
            "migration 23: keyed {} messages, dropped {dropped} duplicates",
            unkeyed.len()
        );
    }
    conn.execute_batch("CREATE UNIQUE INDEX item_messages_item_key ON item_messages (item_id, ext_key);")?;
    Ok(())
}

#[cfg(test)]
mod tests;

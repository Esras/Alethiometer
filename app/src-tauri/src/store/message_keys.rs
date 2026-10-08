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
mod tests {
    use super::*;

    #[test]
    fn keys_every_message_and_keeps_one_per_key() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE item_messages (id INTEGER PRIMARY KEY, item_id INTEGER, at TEXT, author TEXT, text TEXT, ext_key TEXT, read INTEGER NOT NULL DEFAULT 0);
             INSERT INTO item_messages (item_id, at, author, text, ext_key, read) VALUES
               (1, '2026-10-03T09:00:00.000Z', 'Kim', 'hi', NULL, 0),
               (1, '2026-10-03T09:01:00.000Z', 'Kim', 'again', '17.1', 0),
               (1, '2026-10-03T09:01:00.000Z', 'Kim', 'again', '17.1', 1),
               (2, '2026-10-03T09:01:00.000Z', 'Kim', 'again', '17.1', 0);",
        )
        .unwrap();
        run(&conn).unwrap();
        let rows: Vec<(i64, i64, String, bool)> = conn
            .prepare("SELECT id, item_id, ext_key, read FROM item_messages ORDER BY id")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        let hi = crate::domain::splits::content_key("2026-10-03T09:00:00.000Z", "Kim", "hi");
        assert_eq!(
            rows,
            vec![
                (1, 1, hi, false),
                (3, 1, "17.1".into(), true),
                (4, 2, "17.1".into(), false)
            ]
        );
        assert!(
            conn.execute("INSERT INTO item_messages (item_id, ext_key) VALUES (1, '17.1')", [])
                .is_err(),
            "one row per key"
        );
    }
}

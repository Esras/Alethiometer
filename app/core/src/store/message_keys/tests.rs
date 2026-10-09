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

use super::*;

fn la() -> TimeZone {
    TimeZone::get("America/Los_Angeles").unwrap()
}

/// A database at schema 20 (before this step), local times in Los Angeles.
fn old_db() -> Connection {
    let conn = crate::store::schema::open_in_memory_at(20);
    conn.execute_batch(
        "INSERT INTO sources (id, kind, tag, name, cursor, last_sync_at) VALUES
           (1, 'slack', 'work', 'Slack', NULL, '2026-09-28 10:52:00'),
           (2, 'gcal', 'work', 'Google Calendar', '{\"sync_token\":\"t\",\"full_on\":\"2026-09-28\",\"seen\":{\"ev1\":\"2026-09-28T16:00:00|0badc0de|a|2026-09-28T15:00:00||moved@2026-09-28T14:00:00\"}}', NULL);
         INSERT INTO items (id, source_id, external_id, kind, title, occurred_at, ended_at, due_at, due_day, raw_json) VALUES
           (1, 1, 'C1/1', 'thread', 'x', '2026-09-28T10:52:00', '2026-11-01T01:30:00', NULL, '2026-10-02', '{\"moved_from\":\"2026-09-28T14:00:00\",\"api\":\"2026-09-28T14:00:00+02:00\"}'),
           (2, 2, 'ev1', 'event', 'Standup', '2026-09-28T15:00:00', '2026-09-28T16:00:00', NULL, NULL,
            '{\"start\":{\"dateTime\":\"2026-09-28T15:00:00-07:00\"},\"end\":{\"dateTime\":\"2026-09-28T16:00:00-07:00\"},\"status\":\"confirmed\"}');
         INSERT INTO item_messages (item_id, at, author, text, ext_key) VALUES (1, '2026-09-28T10:52:00', 'Kim', 'hi', 'h:0000000000000000');
         INSERT INTO topics (id, name, category, archived_at) VALUES (1, 'T', 'work', '2026-09-28 17:52:00');
         INSERT INTO agent_rows (thread, id, data) VALUES ('s:acp-1', 1, '{\"at\":\"2026-09-28T10:52:00\",\"text\":\"ok\"}');",
    )
    .unwrap();
    conn
}

#[test]
fn local_and_utc_columns_and_json_become_utc() {
    let conn = old_db();
    let r = run_in(&conn, &la()).unwrap();
    let one = |sql: &str| -> String { conn.query_row(sql, [], |row| row.get(0)).unwrap() };
    // 10:52 PDT is 17:52 UTC; SQLite's datetime('now') values were UTC already.
    assert_eq!(
        one("SELECT occurred_at FROM items WHERE id = 1"),
        "2026-09-28T17:52:00.000Z"
    );
    assert_eq!(
        one("SELECT last_sync_at FROM sources WHERE id = 1"),
        "2026-09-28T17:52:00.000Z"
    );
    assert_eq!(one("SELECT archived_at FROM topics"), "2026-09-28T17:52:00.000Z");
    // 01:30 on Nov 1 happens twice in Los Angeles: the first (PDT).
    assert_eq!(
        one("SELECT ended_at FROM items WHERE id = 1"),
        "2026-11-01T08:30:00.000Z"
    );
    assert_eq!(r.ambiguous, 1);
    assert_eq!(
        one("SELECT due_day FROM items WHERE id = 1"),
        "2026-10-02",
        "calendar days stay as they are"
    );
    // JSON: our own naive times convert; the API's own (with an offset) don't.
    let raw: Value = serde_json::from_str(&one("SELECT raw_json FROM items WHERE id = 1")).unwrap();
    assert_eq!(raw["moved_from"], "2026-09-28T21:00:00.000Z");
    assert_eq!(raw["api"], "2026-09-28T14:00:00+02:00");
    let row: Value = serde_json::from_str(&one("SELECT data FROM agent_rows")).unwrap();
    assert_eq!(row["at"], "2026-09-28T17:52:00.000Z");
    // The hashed message key follows the converted time.
    assert_eq!(
        one("SELECT ext_key FROM item_messages"),
        crate::domain::splits::content_key("2026-09-28T17:52:00.000Z", "Kim", "hi")
    );
}

#[test]
fn the_calendar_cursor_converts_and_its_fingerprints_match_the_stored_events() {
    let conn = old_db();
    run_in(&conn, &la()).unwrap();
    let cursor: Value = serde_json::from_str(
        &conn
            .query_row("SELECT cursor FROM sources WHERE id = 2", [], |row| {
                row.get::<_, String>(0)
            })
            .unwrap(),
    )
    .unwrap();
    let entry = cursor["seen"]["ev1"].as_str().unwrap().to_owned();
    let raw: Value = serde_json::from_str(
        &conn
            .query_row("SELECT raw_json FROM items WHERE id = 2", [], |row| {
                row.get::<_, String>(0)
            })
            .unwrap(),
    )
    .unwrap();
    let hash = gcal_fingerprint(&raw);
    assert_eq!(
        entry,
        format!("2026-09-28T23:00:00.000Z|{hash}|a|2026-09-28T22:00:00.000Z||moved@2026-09-28T21:00:00.000Z")
    );
    assert_eq!(cursor["full_on"], "2026-09-28", "a calendar day");
}

#[test]
fn running_twice_changes_nothing_more() {
    let conn = old_db();
    run_in(&conn, &la()).unwrap();
    let before: String = conn
        .query_row(
            "SELECT group_concat(occurred_at) || group_concat(raw_json) FROM items",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let r = run_in(&conn, &la()).unwrap();
    let after: String = conn
        .query_row(
            "SELECT group_concat(occurred_at) || group_concat(raw_json) FROM items",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(before, after);
    assert_eq!((r.converted, r.unreadable), (0, 0));
}

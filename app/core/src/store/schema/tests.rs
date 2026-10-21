use super::*;

#[test]
fn rewriting_an_items_text_unchanged_leaves_the_search_index_alone() {
    let conn = open_in_memory_at(MIGRATIONS.len());
    conn.execute_batch(
        "INSERT INTO sources (id, kind, tag, name) VALUES (1, 'jira', 'work', 'Jira');
         INSERT INTO items (id, source_id, external_id, kind, title, body, occurred_at)
           VALUES (1, 1, 'PLAT-1', 'issue', 'Retry storm', 'Workers loop', '2026-09-28T10:52:00.000Z');",
    )
    .unwrap();
    let writes = |sql: &str| {
        let before = conn.total_changes();
        conn.execute(sql, []).unwrap();
        conn.total_changes() - before
    };
    // What an upsert does: the same text again. Only the item's row is written.
    assert_eq!(
        writes("UPDATE items SET title = 'Retry storm', body = 'Workers loop' WHERE id = 1"),
        1
    );
    // A real change still reaches the index.
    assert!(writes("UPDATE items SET body = 'Workers back off now' WHERE id = 1") > 1);
    let hit: i64 = conn
        .query_row("SELECT rowid FROM items_fts WHERE items_fts MATCH 'back'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(hit, 1);
    assert_eq!(
        conn.query_row("SELECT count(*) FROM items_fts WHERE items_fts MATCH 'loop'", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
}

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
    let links: Vec<(i64, String)> = conn
        .prepare("SELECT item_id, updated_at FROM topic_links ORDER BY item_id")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(
        links.len(),
        2,
        "the dangling link (item 404) was already there and stays"
    );
    assert_eq!(links[0], (1, "2026-09-28T08:52:00.000Z".into()));
    let (created, receipt): (String, String) = conn
        .query_row("SELECT created_at, receipt_json FROM outbox", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    assert_eq!(created, "2026-09-28T08:52:00.000Z");
    assert!(receipt.contains(".000Z"), "{receipt}");
    // New rows get UTC defaults.
    conn.execute(
        "INSERT INTO outbox (source_id, op, payload_json) VALUES (1, 'y', '{}')",
        [],
    )
    .unwrap();
    let fresh: String = conn
        .query_row("SELECT created_at FROM outbox WHERE op = 'y'", [], |r| r.get(0))
        .unwrap();
    assert!(crate::time::Ts::parse(&fresh).is_some(), "{fresh}");
}

#[test]
fn a_note_save_that_went_through_clears_the_earlier_failed_and_queued_saves_of_that_note() {
    let save = |id: i64, path: &str, state: &str| {
        format!(
            "INSERT INTO outbox (id, source_id, op, payload_json, state) VALUES ({id}, 1, 'obsidian.replace', '{{\"path\":\"{path}\"}}', '{state}');"
        )
    };
    let rows = [
        "INSERT INTO sources (id, kind, tag, name) VALUES (1, 'obsidian', 'work', 'Obsidian');".to_string(),
        save(1, "Daily/19.md", "failed"),
        save(2, "Other.md", "failed"),
        save(3, "Daily/19.md", "confirmed"),
        save(4, "Daily/19.md", "failed"),
        save(5, "Daily/19.md", "pending"),
    ]
    .concat();
    let ids = |conn: &Connection| -> Vec<i64> {
        conn.prepare("SELECT id FROM outbox ORDER BY id")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    };
    let mut conn = open_in_memory_at(36);
    conn.execute_batch(&rows).unwrap();
    migrate(&mut conn, MIGRATIONS.len()).unwrap();
    assert_eq!(ids(&conn), [2, 3, 4, 5], "rows already stuck, once");

    let conn = open_in_memory_at(MIGRATIONS.len());
    conn.execute_batch(&rows).unwrap();
    use crate::domain::outbox::Outbox;
    assert_eq!(crate::store::over(&conn).drop_superseded(5).unwrap(), 2);
    assert_eq!(ids(&conn), [2, 3, 5], "other notes and later saves stay");
}

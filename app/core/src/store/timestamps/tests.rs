use super::*;

fn db() -> Connection {
    crate::store::schema::open_in_memory().unwrap()
}

#[test]
fn every_table_has_both_times_and_triggers_over_all_its_columns() {
    let conn = db();
    let tables = tables(&conn).unwrap();
    assert!(tables.contains(&"items".to_string()) && tables.contains(&"topics".to_string()));
    assert!(
        !tables
            .iter()
            .any(|t| t.starts_with("items_fts") || t.starts_with("vec_")),
        "{tables:?}"
    );
    for t in &tables {
        let cols = columns(&conn, t).unwrap();
        assert!(
            cols.contains(&"created_at".into()) && cols.contains(&"updated_at".into()),
            "{t}: {cols:?}"
        );
        let sql: String = conn
            .query_row(
                "SELECT sql FROM sqlite_master WHERE type = 'trigger' AND name = ?1",
                [format!("{t}_updated_at")],
                |r| r.get(0),
            )
            .unwrap_or_else(|_| panic!("{t} has no updated_at trigger"));
        for c in cols.iter().filter(|c| *c != "updated_at") {
            assert!(
                sql.contains(&format!("NEW.\"{c}\" IS NOT OLD.\"{c}\"")),
                "{t}'s trigger misses {c}"
            );
        }
    }
}

fn times(conn: &Connection) -> (String, String) {
    conn.query_row("SELECT created_at, updated_at FROM topics WHERE id = 1", [], |r| {
        Ok((r.get(0)?, r.get(1)?))
    })
    .unwrap()
}

#[test]
fn stored_rows_get_both_and_only_a_real_change_moves_updated_at() {
    let conn = db();
    conn.execute("INSERT INTO topics (id, name, category) VALUES (1, 'Acme', 'work')", [])
        .unwrap();
    let (c, u) = times(&conn);
    assert!(crate::time::Ts::parse(&c).is_some(), "{c}");
    assert_eq!(c, u);

    let old = "2026-01-01T00:00:00.000Z";
    conn.execute("UPDATE topics SET created_at = ?1, updated_at = ?1", [old])
        .unwrap();
    assert_eq!(
        times(&conn),
        (old.into(), old.into()),
        "a write that sets updated_at itself wins"
    );

    let before = conn.total_changes();
    conn.execute("UPDATE topics SET name = 'Acme' WHERE id = 1", [])
        .unwrap();
    assert_eq!(conn.total_changes() - before, 1, "an unchanged rewrite fires nothing");
    assert_eq!(times(&conn).1, old);

    conn.execute("UPDATE topics SET name = 'Acme outage' WHERE id = 1", [])
        .unwrap();
    let (c, u) = times(&conn);
    assert_eq!(c, old);
    assert!(u.as_str() > old, "{u}");

    // A NULL to a value is a change too.
    conn.execute("UPDATE topics SET updated_at = ?1", [old]).unwrap();
    conn.execute("UPDATE topics SET obsidian_path = 'Topics/Acme.md' WHERE id = 1", [])
        .unwrap();
    assert!(times(&conn).1.as_str() > old);
}

#[test]
fn explicit_times_on_insert_are_kept() {
    let conn = db();
    conn.execute("INSERT INTO topics (id, name, category, created_at, updated_at) VALUES (1, 'A', 'work', '2026-02-01T00:00:00.000Z', '2026-03-01T00:00:00.000Z')", []).unwrap();
    assert_eq!(
        times(&conn),
        ("2026-02-01T00:00:00.000Z".into(), "2026-03-01T00:00:00.000Z".into())
    );
}

#[test]
fn existing_rows_get_the_best_time_there_is() {
    let conn = db();
    // As before this change: no times on items or topics.
    conn.execute_batch(
        "DROP TRIGGER items_created_at; DROP TRIGGER items_updated_at;
         ALTER TABLE items DROP COLUMN created_at; ALTER TABLE items DROP COLUMN updated_at;
         DROP TRIGGER topics_created_at; DROP TRIGGER topics_updated_at;
         ALTER TABLE topics DROP COLUMN created_at; ALTER TABLE topics DROP COLUMN updated_at;
         INSERT INTO sources (id, kind, tag, name) VALUES (1, 'jira', 'work', 'Jira');
         INSERT INTO items (id, source_id, external_id, kind, title, occurred_at) VALUES
           (1, 1, 'a', 'issue', 'Old', '2026-09-01T10:00:00.000Z'),
           (2, 1, 'b', 'issue', 'Future', '2999-01-01T00:00:00.000Z');
         INSERT INTO topics (id, name, category) VALUES (1, 'T', 'work');",
    )
    .unwrap();
    ensure(&conn).unwrap();
    let item = |id: i64| {
        conn.query_row("SELECT created_at, updated_at FROM items WHERE id = ?1", [id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })
        .unwrap()
    };
    assert_eq!(
        item(1),
        ("2026-09-01T10:00:00.000Z".into(), "2026-09-01T10:00:00.000Z".into())
    );
    assert!(item(2).0.as_str() < "2999", "never later than now");
    let (c, u) = times(&conn);
    assert_eq!(c, u);
    assert!(crate::time::Ts::parse(&c).is_some());
    // Running it again changes nothing.
    ensure(&conn).unwrap();
    assert_eq!(item(1).0, "2026-09-01T10:00:00.000Z");
}

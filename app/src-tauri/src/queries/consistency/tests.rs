use super::*;
use crate::store::over;
use crate::sync::{apply_now, Batch, Handle, ItemUpsert, Message};
use crate::time::t;
use rusqlite::Connection;

fn gmail() -> (Connection, crate::sync::Source) {
    let conn = crate::db::open_in_memory().unwrap();
    conn.execute(
        "INSERT INTO sources (id, kind, tag, name) VALUES (1, 'gmail', 'work', 'Work mail')",
        [],
    )
    .unwrap();
    let src = crate::sync::load_source(&conn, 1).unwrap();
    (conn, src)
}

fn mail(ext: &str, handle: &str) -> ItemUpsert {
    ItemUpsert {
        external_id: ext.into(),
        kind: "email".into(),
        title: ext.into(),
        occurred_at: t("2026-09-01T09:00:00"),
        status: "open".into(),
        author: Some(Handle {
            handle: handle.into(),
            display_name: "Anthropic".into(),
        }),
        messages: vec![Message {
            at: t("2026-09-01T09:00:00"),
            author: "Anthropic".into(),
            text: "Your receipt".into(),
            author_id: Some(handle.into()),
            ..Default::default()
        }],
        ..Default::default()
    }
}

fn count(conn: &Connection, sql: &str) -> i64 {
    conn.query_row(sql, [], |r| r.get(0)).unwrap()
}

#[test]
fn a_split_sender_is_found_then_merged_into_the_row_with_its_signals() {
    let (conn, src) = gmail();
    let bare = "no-reply@mail.anthropic.com";
    let named = "no-reply@mail.anthropic.com Anthropic";
    let batch = Batch {
        items: vec![mail("t1", bare), mail("t2", bare), mail("t3", named)],
        ..Default::default()
    };
    apply_now(&conn, &src, &batch).unwrap();
    conn.execute(
        "UPDATE items SET author_person_id = (SELECT person_id FROM person_handles WHERE handle = ?1)",
        [bare],
    )
    .unwrap();
    let found = check(&over(&conn)).unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(
        (found[0].check, found[0].source_id, found[0].count, found[0].repairable),
        ("split_senders", Some(1), 1, true)
    );
    let kept: i64 = conn
        .query_row("SELECT person_id FROM person_handles WHERE handle = ?1", [bare], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(
        merge_senders(&over(&conn)).unwrap(),
        MergedSenders { merged: 1, refused: 0 }
    );
    assert_eq!(count(&conn, "SELECT count(*) FROM people"), 1);
    assert_eq!(count(&conn, "SELECT id FROM people"), kept);
    assert_eq!(
        count(
            &conn,
            &format!("SELECT count(*) FROM person_handles WHERE person_id = {kept}")
        ),
        2
    );
    assert_eq!(
        count(
            &conn,
            &format!("SELECT count(*) FROM items WHERE author_person_id = {kept}")
        ),
        3
    );
    assert!(check(&over(&conn)).unwrap().is_empty());
}

#[test]
fn a_split_sender_with_a_people_note_is_left_alone() {
    let (conn, src) = gmail();
    let batch = Batch {
        items: vec![mail("t1", "a@x.com"), mail("t2", "a@x.com A")],
        ..Default::default()
    };
    apply_now(&conn, &src, &batch).unwrap();
    conn.execute(
        "UPDATE people SET note_path = 'People/A.md' WHERE id = (SELECT person_id FROM person_handles WHERE handle = 'a@x.com A')",
        [],
    )
    .unwrap();
    let found = check(&over(&conn)).unwrap();
    assert!(!found[0].repairable);
    assert_eq!(
        merge_senders(&over(&conn)).unwrap(),
        MergedSenders { merged: 0, refused: 1 }
    );
    assert_eq!(count(&conn, "SELECT count(*) FROM people"), 2);
}

#[test]
fn missing_authors_and_calendar_leftovers_are_listed_per_source() {
    let (conn, src) = gmail();
    let mut it = mail("t1", "kim@x.com");
    it.messages[0].author_id = None;
    apply_now(
        &conn,
        &src,
        &Batch {
            items: vec![it],
            ..Default::default()
        },
    )
    .unwrap();
    conn.execute_batch(
        "INSERT INTO sources (id, kind, tag, name) VALUES (2, 'gcal', 'work', 'Calendar');
         INSERT INTO items (source_id, external_id, kind, title, occurred_at, status) VALUES
           (2, 'e1', 'event', 'Old offsite', '2026-09-01T09:00:00.000Z', 'done');",
    )
    .unwrap();
    let found = check(&over(&conn)).unwrap();
    let got: Vec<(&str, Option<i64>, &str, usize, bool)> = found
        .iter()
        .map(|f| (f.check, f.source_id, f.source.as_str(), f.count, f.repairable))
        .collect();
    assert_eq!(
        got,
        [
            ("missing_authors", Some(1), "Work mail", 1, true),
            ("calendar_leftovers", Some(2), "Calendar", 1, false)
        ]
    );
    assert_eq!(found[1].detail, ["Old offsite (item 2)"]);
}

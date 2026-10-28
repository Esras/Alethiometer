use super::*;
use crate::store::{over, schema};
use rusqlite::Connection;

fn db() -> Connection {
    let conn = schema::open_in_memory().unwrap();
    conn.execute_batch(
        "INSERT INTO sources (id, kind, tag, name) VALUES (1, 'gmail', 'work', 'Gmail'), (2, 'slack', 'work', 'Slack'),
           (3, 'jira', 'work', 'Jira'), (4, 'gcal', 'work', 'Calendar');",
    )
    .unwrap();
    conn
}

fn item(conn: &Connection, id: i64, source: i64, ext: &str, status: &str, summary: &str, raw: &str) {
    conn.execute(
        "INSERT INTO items (id, source_id, external_id, kind, title, occurred_at, status, summary, raw_json)
         VALUES (?1, ?2, ?3, 'email', ?3, '2026-09-01T09:00:00.000Z', ?4, ?5, ?6)",
        rusqlite::params![id, source, ext, status, summary, raw],
    )
    .unwrap();
}

fn message(conn: &Connection, item: i64, key: &str, author_id: Option<&str>, context: bool) {
    conn.execute(
        "INSERT INTO item_messages (item_id, at, author, text, ext_key, author_id, context)
         VALUES (?1, '2026-09-01T09:00:00.000Z', 'Kim', 'hi', ?2, ?3, ?4)",
        rusqlite::params![item, key, author_id, context],
    )
    .unwrap();
}

#[test]
fn missing_authors_counts_own_messages_of_the_asked_kinds() {
    let conn = db();
    item(&conn, 1, 1, "t1", "open", "", "{}");
    item(&conn, 2, 1, "t2", "open", "", "{}");
    item(&conn, 3, 2, "C1/run", "open", "", "{}");
    item(&conn, 4, 3, "PLAT-1", "open", "", "{}");
    message(&conn, 1, "m1", None, false);
    message(&conn, 1, "m2", None, false);
    message(&conn, 1, "m3", None, true);
    message(&conn, 2, "m4", Some("kim@x.com"), false);
    message(&conn, 3, "s1", None, false);
    message(&conn, 4, "jira:1", None, false);
    let got = over(&conn).missing_authors(&["gmail", "jira", "github"]).unwrap();
    assert_eq!(
        got,
        vec![
            MissingAuthors {
                source_id: 1,
                items: vec!["t1".into()],
                messages: 2
            },
            MissingAuthors {
                source_id: 3,
                items: vec!["PLAT-1".into()],
                messages: 1
            },
        ],
        "context rows, Slack's bots and authored messages left out"
    );
}

fn person(conn: &Connection, id: i64, note: Option<&str>, handles: &[&str]) {
    conn.execute(
        "INSERT INTO people (id, display_name, note_path) VALUES (?1, 'Anthropic', ?2)",
        rusqlite::params![id, note],
    )
    .unwrap();
    for h in handles {
        conn.execute(
            "INSERT INTO person_handles (person_id, source_kind, handle) VALUES (?1, 'gmail', ?2)",
            rusqlite::params![id, h],
        )
        .unwrap();
    }
}

#[test]
fn split_senders_pair_a_bare_address_with_its_one_named_twin() {
    let conn = db();
    person(&conn, 1, None, &["no-reply@mail.anthropic.com"]);
    person(&conn, 2, None, &["no-reply@mail.anthropic.com Anthropic"]);
    person(&conn, 3, None, &["dana@acme.com"]);
    person(&conn, 4, None, &["comments-noreply@docs.google.com Alan Ho"]);
    person(&conn, 5, None, &["comments-noreply@docs.google.com Xuan Lam"]);
    person(&conn, 6, None, &["notifications@github.com"]);
    person(&conn, 7, None, &["notifications@github.com dan"]);
    person(&conn, 8, None, &["notifications@github.com kim"]);
    person(&conn, 9, None, &["no-reply@mail.anthropic.com.evil Anthropic"]);
    item(&conn, 1, 1, "t1", "open", "", "{}");
    conn.execute("UPDATE items SET author_person_id = 1", []).unwrap();
    let got = over(&conn).split_senders().unwrap();
    assert_eq!(
        got,
        vec![SplitSender {
            address: "no-reply@mail.anthropic.com".into(),
            bare: 1,
            named: 2,
            bare_items: 1,
            named_items: 0,
            noted: false,
        }],
        "single-format handles and an address with two named twins aren't pairs"
    );
}

#[test]
fn merge_person_moves_every_reference_and_deletes_the_row() {
    let conn = db();
    person(&conn, 1, None, &["a@x.com"]);
    person(&conn, 2, None, &["a@x.com A"]);
    item(&conn, 1, 1, "t1", "open", "", "{}");
    conn.execute_batch(
        "UPDATE items SET author_person_id = 2;
         INSERT INTO topics (id, name, category) VALUES (1, 'T', 'work'), (2, 'U', 'work');
         INSERT INTO topic_people (topic_id, person_id) VALUES (1, 1), (1, 2), (2, 2);
         INSERT INTO person_accounts (kind, account, person_id) VALUES ('email', 'a@x.com', 2);",
    )
    .unwrap();
    over(&conn).merge_person(2, 1).unwrap();
    let count = |sql: &str| -> i64 { conn.query_row(sql, [], |r| r.get(0)).unwrap() };
    assert_eq!(count("SELECT count(*) FROM people"), 1);
    assert_eq!(count("SELECT count(*) FROM person_handles WHERE person_id = 1"), 2);
    assert_eq!(count("SELECT author_person_id FROM items"), 1);
    assert_eq!(count("SELECT count(*) FROM topic_people WHERE person_id = 1"), 2);
    assert_eq!(count("SELECT person_id FROM person_accounts"), 1);
    assert_eq!(count("SELECT count(*) FROM pragma_foreign_key_check"), 0);
}

#[test]
fn merge_person_covers_every_reference_to_people() {
    let conn = db();
    let refs: Vec<(String, String)> = conn
        .prepare(
            "SELECT m.name, f.\"from\" FROM sqlite_master m, pragma_foreign_key_list(m.name) f
              WHERE m.type = 'table' AND f.\"table\" = 'people' ORDER BY 1, 2",
        )
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    let refs: Vec<(&str, &str)> = refs.iter().map(|(t, c)| (t.as_str(), c.as_str())).collect();
    assert_eq!(
        refs,
        [
            ("items", "author_person_id"),
            ("person_accounts", "person_id"),
            ("person_handles", "person_id"),
            ("topic_people", "person_id"),
        ],
        "a new reference to people needs moving in merge_person"
    );
}

#[test]
fn calendar_leftovers_skip_cancelled_and_declined_events() {
    let conn = db();
    let declined = r#"{"attendees": [{"email": "me@x.com", "self": true, "responseStatus": "declined"}]}"#;
    let accepted = r#"{"attendees": [{"email": "me@x.com", "self": true, "responseStatus": "accepted"}]}"#;
    item(&conn, 1, 4, "old", "done", "Kim", accepted);
    item(&conn, 2, 4, "gone", "done", "cancelled", r#"{"change": "cancelled"}"#);
    item(&conn, 3, 4, "no", "done", "declined", declined);
    item(&conn, 4, 4, "allday", "open", "all day", "{}");
    item(&conn, 5, 4, "ontime", "done", "", "{}");
    item(&conn, 6, 1, "mail", "done", "", "{}");
    conn.execute(
        "INSERT INTO events (item_id, source_id, start, end, category) VALUES (5, 4, '2026-09-01T09:00:00.000Z', '2026-09-01T10:00:00.000Z', 'work')",
        [],
    )
    .unwrap();
    let got = over(&conn).calendar_leftovers().unwrap();
    assert_eq!(
        got,
        vec![Leftover {
            source_id: 4,
            item_id: 1,
            title: "old".into()
        }]
    );
}

#[test]
fn forget_revs_clears_only_the_named_items() {
    let conn = db();
    item(&conn, 1, 3, "PLAT-1", "open", "", "{}");
    item(&conn, 2, 3, "PLAT-2", "open", "", "{}");
    conn.execute("UPDATE items SET remote_rev = 'r1'", []).unwrap();
    let s = over(&conn);
    s.forget_revs(3, &["PLAT-1".into()]).unwrap();
    let revs: Vec<Option<String>> = conn
        .prepare("SELECT remote_rev FROM items ORDER BY id")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    assert_eq!(revs, [None, Some("r1".into())]);
}

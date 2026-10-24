use super::*;
use crate::queries::people::scan;
use crate::time::t;
use jiff::tz::TimeZone;
use rusqlite::{params, Connection};
use std::path::Path;

/// Wednesday 2026-10-21, 10:00 UTC.
fn clock() -> Clock {
    Clock::fixed(t("2026-10-21T10:00:00Z"), TimeZone::UTC)
}

fn db() -> Connection {
    let conn = crate::db::open_in_memory().unwrap();
    for (kind, cursor) in [("slack", "{\"me\":\"U0ME\"}"), ("gmail", "{}"), ("gcal", "{}")] {
        conn.execute(
            "INSERT INTO sources (kind, tag, name, cursor) VALUES (?1, 'work', ?1, ?2)",
            params![kind, cursor],
        )
        .unwrap();
    }
    for name in ["Datadog renewal", "SOC 2 evidence", "Q4 headcount"] {
        conn.execute("INSERT INTO topics (name, category) VALUES (?1, 'work')", [name])
            .unwrap();
    }
    conn
}

fn note(vault: &Path, name: &str, text: &str) {
    std::fs::create_dir_all(vault.join("People")).unwrap();
    std::fs::write(vault.join("People").join(format!("{name}.md")), text).unwrap();
}

fn source(conn: &Connection, kind: &str) -> i64 {
    conn.query_row("SELECT id FROM sources WHERE kind = ?1", [kind], |r| r.get(0))
        .unwrap()
}

/// A signal of `kind`'s source, linked to topic `topic` in `state`, with messages (author id, at).
fn signal(conn: &Connection, kind: &str, topic: Option<(i64, &str)>, messages: &[(&str, &str)]) -> i64 {
    let n: i64 = conn.query_row("SELECT count(*) FROM items", [], |r| r.get(0)).unwrap();
    let first = messages.first().map_or("2026-10-20T09:00:00Z", |m| m.1);
    conn.execute(
        "INSERT INTO items (source_id, external_id, kind, title, occurred_at, seen) VALUES (?1, ?2, 'thread', 't', ?3, 1)",
        params![source(conn, kind), format!("x{n}"), t(first)],
    )
    .unwrap();
    let id = conn.last_insert_rowid();
    for (author, at) in messages {
        conn.execute(
            "INSERT INTO item_messages (item_id, at, author, text, author_id) VALUES (?1, ?2, ?3, 'hi', ?3)",
            params![id, t(at), author],
        )
        .unwrap();
    }
    if let Some((topic, state)) = topic {
        conn.execute(
            "INSERT INTO topic_links (item_id, topic_id, state) VALUES (?1, ?2, ?3)",
            params![id, topic, state],
        )
        .unwrap();
    }
    id
}

fn rows(conn: &Connection) -> Vec<Row> {
    index(&crate::store::over(conn), &PeopleSettings::default(), &clock()).unwrap()
}

fn row<'a>(rows: &'a [Row], name: &str) -> &'a Row {
    rows.iter()
        .find(|r| r.name == name)
        .unwrap_or_else(|| panic!("no row {name}"))
}

#[test]
fn counts_confirmed_signals_per_week_topic_and_30_days() {
    let dir = tempfile::tempdir().unwrap();
    let conn = db();
    note(
        dir.path(),
        "Kim",
        "---\naliases: [Kimberly]\nslack: U0KIM\nemail: [kim@work.com]\nrole: Procurement\n---\n",
    );
    note(dir.path(), "Jo", "---\nemail: [jo@work.com]\n---\n");
    scan(&conn, dir.path()).unwrap();

    // Two messages in one signal count once, in the week they were written.
    signal(
        &conn,
        "slack",
        Some((1, "confirmed")),
        &[
            ("U0KIM", "2026-10-20T09:40:00Z"),
            ("U0KIM", "2026-10-21T09:40:00Z"),
            ("U0ME", "2026-10-21T09:45:00Z"),
            ("U0MARCO", "2026-10-21T09:50:00Z"),
        ],
    );
    // Mail from her address is her too.
    signal(
        &conn,
        "gmail",
        Some((1, "confirmed")),
        &[("kim@work.com", "2026-10-11T12:00:00Z")],
    );
    // Older than 30 days: in the strip and the topic list, not the total.
    signal(
        &conn,
        "slack",
        Some((2, "confirmed")),
        &[("U0KIM", "2026-09-14T12:00:00Z")],
    );
    // Before the strip: only the topic list.
    signal(
        &conn,
        "slack",
        Some((3, "confirmed")),
        &[("U0KIM", "2026-03-02T12:00:00Z")],
    );
    // Not confirmed: never counted.
    signal(
        &conn,
        "slack",
        Some((2, "suggested")),
        &[("U0KIM", "2026-10-21T08:00:00Z")],
    );
    signal(&conn, "slack", None, &[("U0KIM", "2026-10-21T08:00:00Z")]);

    let rows = rows(&conn);
    let kim = row(&rows, "Kim");
    assert_eq!(kim.group, "recent");
    assert_eq!(kim.total, 2);
    assert_eq!(kim.role.as_deref(), Some("Procurement"));
    assert_eq!(kim.weeks.len(), WEEKS);
    // This week (Mon Oct 19 on), Oct 5–11, Sep 14–20 (five weeks back).
    assert_eq!(kim.weeks[11], 1);
    assert_eq!(kim.weeks[9], 1);
    assert_eq!(kim.weeks[6], 1);
    assert_eq!(kim.weeks.iter().sum::<i64>(), 3);
    let topics: Vec<(&str, i64)> = kim.topics.iter().map(|t| (t.name.as_str(), t.n)).collect();
    assert_eq!(
        topics,
        [("Datadog renewal", 2), ("SOC 2 evidence", 0), ("Q4 headcount", 0)],
        "every topic ever, most in 30 days first, then the most recent"
    );
    assert_eq!(kim.last.as_deref(), Some("today 09:40"));
    assert_eq!(kim.last_source.as_deref(), Some("slack"));
    assert!(kim.terms.contains(&"kimberly".to_string()) && kim.terms.contains(&"kim@work.com".to_string()));

    let jo = row(&rows, "Jo");
    assert_eq!(
        (jo.group, jo.total, jo.last.as_deref()),
        ("quiet", 0, None),
        "a note with no signals is quiet"
    );

    let marco = row(&rows, "U0MARCO");
    assert_eq!((marco.group, marco.known, marco.total), ("unknown", false, 1));
    assert_eq!(marco.account.as_ref().map(|a| a.id.as_str()), Some("U0MARCO"));
    assert!(!rows.iter().any(|r| r.name.contains("ME")), "you're left out");
    assert_eq!(
        [rows[0].name.as_str(), rows[1].name.as_str()],
        ["U0MARCO", "Kim"],
        "most recent signal first"
    );
}

#[test]
fn senders_without_a_note_need_a_recent_signal_and_a_person_behind_them() {
    let dir = tempfile::tempdir().unwrap();
    let conn = db();
    scan(&conn, dir.path()).unwrap();
    signal(
        &conn,
        "gmail",
        Some((1, "confirmed")),
        &[("ravi@datadoghq.com", "2026-10-15T12:00:00Z")],
    );
    signal(
        &conn,
        "gmail",
        Some((1, "confirmed")),
        &[("newsletter@datadoghq.com", "2026-10-15T12:00:00Z")],
    );
    signal(
        &conn,
        "slack",
        Some((1, "confirmed")),
        &[("U0JIRABOT", "2026-10-15T12:00:00Z")],
    );
    signal(
        &conn,
        "slack",
        Some((1, "confirmed")),
        &[("U0OLD", "2026-09-01T12:00:00Z")],
    );
    conn.execute(
        "INSERT INTO account_profiles (kind, account, real_name, is_bot) VALUES ('slack', 'U0JIRABOT', 'Jira', 1)",
        [],
    )
    .unwrap();
    let rows = rows(&conn);
    let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(
        names,
        ["ravi@datadoghq.com"],
        "no lists, no bots, nothing older than 30 days"
    );
    assert_eq!(rows[0].color, hash_color("ravi@datadoghq.com"));
}

#[test]
fn next_meeting_is_the_soonest_within_a_week_they_havent_declined() {
    let dir = tempfile::tempdir().unwrap();
    let conn = db();
    note(dir.path(), "Kim", "---\nemail: [kim@work.com]\n---\n");
    scan(&conn, dir.path()).unwrap();
    let gcal = source(&conn, "gcal");
    let event = |title: &str, start: &str, kim: &str| {
        let raw = json!({ "attendees": [
            { "email": "zach@work.com", "self": true, "responseStatus": "accepted" },
            { "email": "Kim@Work.com", "responseStatus": kim },
            { "email": "room-4@resource.calendar.google.com", "resource": true, "responseStatus": "accepted" },
        ]});
        conn.execute(
            "INSERT INTO items (source_id, external_id, kind, title, occurred_at, raw_json) VALUES (?1, ?2, 'event', ?2, ?3, ?4)",
            params![gcal, title, t(start), raw.to_string()],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO events (item_id, source_id, start, end, title, category) VALUES (?1, ?2, ?3, ?3, ?4, 'work')",
            params![conn.last_insert_rowid(), gcal, t(start), title],
        )
        .unwrap();
    };
    event("Already over", "2026-10-21T09:00:00Z", "accepted");
    event("Declined by her", "2026-10-21T15:00:00Z", "declined");
    event("Final terms call", "2026-10-22T14:00:00Z", "needsAction");
    event("Later", "2026-10-23T14:00:00Z", "accepted");
    event("Too far", "2026-10-29T14:00:00Z", "accepted");
    let rows = rows(&conn);
    let kim = row(&rows, "Kim");
    assert_eq!(
        kim.next_meeting,
        Some(Meeting {
            start: t("2026-10-22T14:00:00Z"),
            title: "Final terms call".into()
        })
    );
    assert_eq!(kim.group, "quiet", "a meeting isn't a signal");
    assert_eq!(rows.len(), 1, "rooms and you get no row");
}

#[test]
fn weeks_start_on_monday_in_the_clocks_zone() {
    let la = TimeZone::get("America/Los_Angeles").unwrap();
    let c = Clock::fixed(t("2026-10-21T10:00:00Z"), la);
    assert_eq!(strip_start(&c).to_string(), "2026-08-03");
    // Sunday 23:30 local (Monday 06:30 UTC) is last week; Monday 00:30 local is this week.
    let start = strip_start(&c);
    assert_eq!(week_of(&c, start, t("2026-10-19T06:30:00Z")), Some(10));
    assert_eq!(week_of(&c, start, t("2026-10-19T07:30:00Z")), Some(11));
    assert_eq!(week_of(&c, start, t("2026-08-03T06:59:00Z")), None, "before the strip");
}

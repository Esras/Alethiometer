use super::*;
use crate::store::over;
use crate::sync::{apply_now, Batch, ItemUpsert};
use crate::time::t;
use rusqlite::Connection;
use serde_json::json;

fn setup() -> (Connection, tempfile::TempDir) {
    let conn = crate::db::open_in_memory().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let now = crate::sync::test_clock("2026-09-27T10:52:00Z");
    crate::seed::seed(&over(&conn), &now, dir.path()).unwrap();
    (conn, dir)
}

/// A new item of the sample `kind` source, as its connector would store it.
fn item(conn: &Connection, kind: &str, external_id: &str, title: &str, raw: Value) -> i64 {
    let sid: i64 = conn
        .query_row("SELECT id FROM sources WHERE kind = ?1", [kind], |r| r.get(0))
        .unwrap();
    let src = crate::sync::load_source(conn, sid).unwrap();
    let it = ItemUpsert {
        external_id: external_id.into(),
        kind: "email".into(),
        title: title.into(),
        url: Some(format!("https://acme.atlassian.net/browse/{external_id}")),
        occurred_at: t("2026-09-27T09:00:00"),
        status: "open".into(),
        raw_json: raw,
        bump: true,
        ..Default::default()
    };
    apply_now(
        conn,
        &src,
        &Batch {
            items: vec![it],
            ..Default::default()
        },
    )
    .unwrap();
    conn.query_row(
        "SELECT id FROM items WHERE source_id = ?1 AND external_id = ?2",
        rusqlite::params![sid, external_id],
        |r| r.get(0),
    )
    .unwrap()
}

fn mail(conn: &Connection, n: usize, from: &str) -> i64 {
    item(
        conn,
        "gmail",
        &format!("thread-{from}-{n}"),
        "News",
        json!({ "thread_id": "x", "msgs": [], "from": from, "list": null, "direct": false }),
    )
}

fn get(conn: &Connection, id: i64) -> ItemRef {
    over(conn).items().get(id).unwrap().unwrap()
}

fn cfg() -> IgnoreSuggestions {
    IgnoreSuggestions::default()
}

const NOW: &str = "2026-10-17T12:00:00";

/// Dismiss each item at `now` and return the suggestion after the last.
fn dismiss_all(conn: &Connection, ids: &[i64], now: Ts) -> Option<Suggestion> {
    let uow = over(conn);
    let mut last = None;
    for &id in ids {
        record(&uow, now, id, "dismiss").unwrap();
        last = suggest(&uow, now, &cfg(), id).unwrap();
    }
    last
}

#[test]
fn mail_counts_toward_its_domain_list_and_sender_broadest_first() {
    let (conn, _d) = setup();
    let id = item(
        &conn,
        "gmail",
        "t1",
        "News",
        json!({ "from": "news@acme.com", "list": "weekly.acme.com", "direct": false }),
    );
    let s = subject(&get(&conn, id)).unwrap();
    let keys: Vec<(&str, &str, &str)> = s
        .keys
        .iter()
        .map(|(k, label, _)| (k.kind.as_str(), k.key.as_str(), label.as_str()))
        .collect();
    assert_eq!(
        keys,
        [
            ("domain", "acme.com", "@acme.com"),
            ("list", "weekly.acme.com", "List weekly.acme.com"),
            ("from", "news@acme.com", "news@acme.com"),
        ]
    );
    assert_eq!(
        s.keys[0].2,
        Offer::Copy {
            text: "from:(@acme.com)".into()
        }
    );
    assert_eq!(
        s.keys[1].2,
        Offer::Copy {
            text: "list:(weekly.acme.com)".into()
        }
    );
    assert!(!s.direct);
    // Synced before the connector stored `from`: nothing to count.
    let old = item(&conn, "gmail", "t2", "Old", json!({ "thread_id": "t2" }));
    assert_eq!(subject(&get(&conn, old)), None);
}

#[test]
fn chat_counts_toward_its_channel_and_dms_are_direct() {
    let (conn, _d) = setup();
    let ch = item(
        &conn,
        "slack",
        "C1/1.0",
        "#alerts",
        json!({ "channel": "C1", "conv": "channel", "name": "alerts" }),
    );
    let s = subject(&get(&conn, ch)).unwrap();
    assert_eq!(s.keys[0].0, key("channel", "C1"));
    assert_eq!(s.keys[0].1, "#alerts");
    assert_eq!(
        s.keys[0].2,
        Offer::Mute {
            entry: "#alerts".into()
        }
    );
    assert!(!s.direct);
    // Older items have no `name`: the title's, and the id to mute.
    let old = item(
        &conn,
        "slack",
        "C2/1.0",
        "#inc-2291 · mention",
        json!({ "channel": "C2" }),
    );
    let s = subject(&get(&conn, old)).unwrap();
    assert_eq!(
        (s.keys[0].1.as_str(), &s.keys[0].2),
        ("#inc-2291", &Offer::Mute { entry: "C2".into() })
    );
    assert!(!s.direct);
    for (ext, title, raw) in [
        ("D1/1.0", "DM · Ana", json!({ "channel": "D1", "conv": "dm" })),
        (
            "G1/1.0",
            "Group DM · ana, bo",
            json!({ "channel": "G1", "conv": "group" }),
        ),
        // Older items, without `conv`: read off the title and id.
        ("D2/1.0", "DM · Bo", json!({ "channel": "D2" })),
        ("G2/1.0", "Group DM · ana, cy", json!({ "channel": "G2" })),
    ] {
        let id = item(&conn, "slack", ext, title, raw);
        assert!(subject(&get(&conn, id)).unwrap().direct, "{title}");
    }
}

#[test]
fn github_counts_toward_its_repo_and_jira_its_project() {
    let (conn, _d) = setup();
    let pr = item(&conn, "github", "platform/ingest#482", "PR", json!({}));
    let s = subject(&get(&conn, pr)).unwrap();
    assert_eq!(s.keys[0].0, key("repo", "platform/ingest"));
    assert_eq!(
        s.keys[0].2,
        Offer::Open {
            url: "https://github.com/platform/ingest".into()
        }
    );
    let issue = item(
        &conn,
        "jira",
        "OPS-12",
        "Issue",
        json!({ "fields": { "project": { "key": "OPS" } } }),
    );
    let s = subject(&get(&conn, issue)).unwrap();
    assert_eq!(s.keys[0].0, key("project", "OPS"));
    assert_eq!(
        s.keys[0].2,
        Offer::Open {
            url: "https://acme.atlassian.net/browse/OPS".into()
        }
    );
}

#[test]
fn a_sender_is_suggested_once_enough_of_it_was_dismissed() {
    let (conn, _d) = setup();
    let now = t(NOW);
    let ids: Vec<i64> = (0..5).map(|n| mail(&conn, n, "news@acme.com")).collect();
    assert_eq!(dismiss_all(&conn, &ids[..4], now), None, "4 actions are too few");
    let s = dismiss_all(&conn, &ids[4..], now).unwrap();
    // One sender: the address, not its domain.
    assert_eq!((s.kind.as_str(), s.key.as_str()), ("from", "news@acme.com"));
    assert_eq!((s.dismissed, s.total, s.days), (5, 5, 60));
    assert_eq!(s.source_kind, "gmail");
    assert_eq!(
        s.offer,
        Offer::Copy {
            text: "from:(news@acme.com)".into()
        }
    );
}

#[test]
fn keeping_it_often_enough_holds_the_suggestion_back() {
    let (conn, _d) = setup();
    let now = t(NOW);
    let uow = over(&conn);
    // 8 dismissed of 10 is 80%: suggested. 8 of 11 isn't.
    let ids: Vec<i64> = (0..11).map(|n| mail(&conn, n, "news@acme.com")).collect();
    for &id in &ids[..2] {
        record(&uow, now, id, "keep").unwrap();
    }
    assert!(dismiss_all(&conn, &ids[2..10], now).is_some());
    record(&uow, now, ids[10], "keep").unwrap();
    assert_eq!(suggest(&uow, now, &cfg(), ids[9]).unwrap(), None);
}

#[test]
fn a_domain_needs_two_senders_and_then_wins() {
    let (conn, _d) = setup();
    let now = t(NOW);
    let mut ids: Vec<i64> = (0..4).map(|n| mail(&conn, n, "news@acme.com")).collect();
    ids.push(mail(&conn, 0, "deals@acme.com"));
    let s = dismiss_all(&conn, &ids, now).unwrap();
    assert_eq!(
        (s.kind.as_str(), s.key.as_str(), s.label.as_str()),
        ("domain", "acme.com", "@acme.com")
    );
    assert_eq!(
        s.offer,
        Offer::Copy {
            text: "from:(@acme.com)".into()
        }
    );
}

#[test]
fn only_the_window_counts() {
    let (conn, _d) = setup();
    let now = t(NOW);
    let ids: Vec<i64> = (0..5).map(|n| mail(&conn, n, "news@acme.com")).collect();
    let uow = over(&conn);
    record(&uow, now - SignedDuration::from_hours(24 * 61), ids[0], "dismiss").unwrap();
    assert_eq!(dismiss_all(&conn, &ids[1..], now), None);
}

#[test]
fn a_dm_is_never_suggested() {
    let (conn, _d) = setup();
    let now = t(NOW);
    let ids: Vec<i64> = (0..6)
        .map(|n| {
            item(
                &conn,
                "slack",
                &format!("D1/{n}.0"),
                "DM · Ana",
                json!({ "channel": "D1", "conv": "dm" }),
            )
        })
        .collect();
    assert_eq!(dismiss_all(&conn, &ids, now), None);
    let direct: Vec<i64> = (0..6)
        .map(|n| {
            item(
                &conn,
                "gmail",
                &format!("dm-{n}"),
                "Lunch?",
                json!({ "from": "ana@acme.com", "direct": true }),
            )
        })
        .collect();
    assert_eq!(dismiss_all(&conn, &direct, now), None);
}

#[test]
fn not_now_holds_a_key_back_for_thirty_days() {
    let (conn, _d) = setup();
    let now = t(NOW);
    let ids: Vec<i64> = (0..7).map(|n| mail(&conn, n, "news@acme.com")).collect();
    let s = dismiss_all(&conn, &ids[..5], now).unwrap();
    let uow = over(&conn);
    not_now(&uow, now, s.source_id, &key(&s.kind, &s.key)).unwrap();
    assert_eq!(dismiss_all(&conn, &ids[5..6], now), None);
    let later = now + SignedDuration::from_hours(24 * NOT_NOW_DAYS + 1);
    assert!(dismiss_all(&conn, &ids[6..], later).is_some());
}

#[test]
fn undo_forgets_the_action() {
    let (conn, _d) = setup();
    let now = t(NOW);
    let ids: Vec<i64> = (0..5).map(|n| mail(&conn, n, "news@acme.com")).collect();
    let uow = over(&conn);
    for &id in &ids[..4] {
        record(&uow, now, id, "dismiss").unwrap();
    }
    let e = record(&uow, now, ids[4], "dismiss").unwrap();
    assert!(suggest(&uow, now, &cfg(), ids[4]).unwrap().is_some());
    // Another item's id doesn't forget it.
    forget(&uow, e, ids[0]).unwrap();
    assert!(suggest(&uow, now, &cfg(), ids[4]).unwrap().is_some());
    forget(&uow, e, ids[4]).unwrap();
    assert_eq!(suggest(&uow, now, &cfg(), ids[4]).unwrap(), None);
}

#[test]
fn the_commands_record_suggest_and_undo() {
    use crate::state::App;
    let (conn, dir) = setup();
    let ids: Vec<i64> = (0..5).map(|n| mail(&conn, n, "news@acme.com")).collect();
    let app = App::from_parts(conn, crate::settings::Settings::default(), dir.path()).unwrap();
    let call = |cmd: &str, args: Value| crate::api::call(&app, cmd, &args).unwrap();
    for &id in &ids[..4] {
        let r = call("incoming.dismiss", json!({ "item_id": id }));
        assert!(r["event"].is_i64());
        assert!(r["suggestion"].is_null());
    }
    let r = call("incoming.trash", json!({ "item_id": ids[4] }));
    assert_eq!(r["suggestion"]["key"], "news@acme.com");
    assert_eq!(r["suggestion"]["offer"]["type"], "copy");
    call("incoming.undismiss", json!({ "item_id": ids[4], "event": r["event"] }));
    let n: i64 = app
        .db()
        .query_row("SELECT count(*) FROM triage_events", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 4, "u forgot the trash");
    let r = call("incoming.dismiss", json!({ "item_id": ids[4] }));
    let s = &r["suggestion"];
    call(
        "incoming.ignore_not_now",
        json!({ "source_id": s["source_id"], "kind": s["kind"], "key": s["key"] }),
    );
    let until: i64 = app
        .db()
        .query_row("SELECT count(*) FROM ignore_declines", [], |r| r.get(0))
        .unwrap();
    assert_eq!(until, 1);
}

#[test]
fn move_keeps_and_its_undo_forgets() {
    use crate::state::App;
    let (conn, dir) = setup();
    let id = mail(&conn, 0, "news@acme.com");
    let topic: i64 = conn
        .query_row("SELECT id FROM topics LIMIT 1", [], |r| r.get(0))
        .unwrap();
    let app = App::from_parts(conn, crate::settings::Settings::default(), dir.path()).unwrap();
    let call = |cmd: &str, args: Value| crate::api::call(&app, cmd, &args).unwrap();
    let count = || -> i64 {
        app.db()
            .query_row("SELECT count(*) FROM triage_events WHERE action = 'keep'", [], |r| {
                r.get(0)
            })
            .unwrap()
    };
    let r = call("incoming.move", json!({ "item_id": id, "topic_id": topic }));
    assert_eq!(count(), 1);
    call(
        "incoming.unfile",
        json!({ "moved": r["moved"], "to_topic_id": topic, "item_id": id, "event": r["event"] }),
    );
    assert_eq!(count(), 0);
}

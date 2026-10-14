//! Slack connector against canned API responses (shapes from the Web API docs).

use super::socket::{self, Flag, Mode};
use super::*;
use crate::settings::SourceConfig;
use crate::sync::http::{FakeHttp, Http, Response};
use crate::sync::{apply_now, register_sources, Source, StreamOutcome};
use crate::time::Ts;
use std::cell::Cell;
use std::sync::Mutex;

const USERS_CONVERSATIONS_1: &str = include_str!("../../../../tests/fixtures/slack/users_conversations_1.json");
const USERS_CONVERSATIONS_2: &str = include_str!("../../../../tests/fixtures/slack/users_conversations_2.json");
const HISTORY_INC_1: &str = include_str!("../../../../tests/fixtures/slack/history_inc_1.json");
const HISTORY_INC_2: &str = include_str!("../../../../tests/fixtures/slack/history_inc_2.json");
const REPLIES_INC: &str = include_str!("../../../../tests/fixtures/slack/replies_inc.json");
const REPLIES_RANDOM: &str = include_str!("../../../../tests/fixtures/slack/replies_random.json");
const INFO_INC: &str = include_str!("../../../../tests/fixtures/slack/info_inc.json");
const INFO_PRIYA: &str = include_str!("../../../../tests/fixtures/slack/info_priya.json");
const INFO_MPIM: &str = include_str!("../../../../tests/fixtures/slack/info_mpim.json");
const INFO_RANDOM: &str = include_str!("../../../../tests/fixtures/slack/info_random.json");
const SEARCH: &str = include_str!("../../../../tests/fixtures/slack/search_mentions.json");
const SEARCH_MINE: &str = include_str!("../../../../tests/fixtures/slack/search_mine.json");

/// 2026-09-21, a little after the last fixture message.
const NOW: f64 = 1_790_000_000.0;

thread_local! {
    static SLEPT: Cell<u64> = const { Cell::new(0) };
}

fn fake_sleep(d: std::time::Duration) {
    SLEPT.with(|s| s.set(s.get() + d.as_secs()));
}

fn now() -> Ts {
    Ts::from_unix_secs(NOW).unwrap()
}

/// Source id 1 is never connected to a socket, so its polls are `Mode::Poll`.
fn source(cursor: Option<String>) -> Source {
    source_with(1, json!({ "channels": ["#inc-2291"] }), cursor)
}

fn source_with(id: i64, config: Value, cursor: Option<String>) -> Source {
    Source {
        id,
        kind: "slack".into(),
        tag: "work".into(),
        name: "Slack".into(),
        config,
        cursor,
    }
}

fn user(id: &str, display: &str, real: &str) -> String {
    json!({
        "ok": true,
        "user": {
            "id": id, "team_id": "T0ACME", "name": display.to_lowercase(), "deleted": false, "real_name": real,
            "tz": "Europe/Berlin", "is_bot": false, "is_app_user": false, "updated": 1780000000,
            "profile": { "real_name": real, "real_name_normalized": real, "display_name": display, "display_name_normalized": display, "team": "T0ACME" }
        }
    })
    .to_string()
}

const PRIYA_HISTORY: &str = r#"{"ok": true, "messages": [{"type": "message", "user": "U0PRIYA", "text": "Can you look at the offsite agenda before Thursday?", "ts": "1789997000.000800", "team": "T0ACME"}], "has_more": false, "response_metadata": {"next_cursor": ""}}"#;

/// A workspace where `#inc-2291` is followed (read up to before the incident
/// run; its thread unread), with one unread DM, a read group DM, a mention in
/// `#random` and an 11-day-old `#random` thread you replied in, with a new
/// answer. `inc_page1` is the newest history page.
fn workspace(inc_page1: &str) -> FakeHttp {
    let http = FakeHttp::default();
    http.on("GET", "auth.test", 200, r#"{"ok": true, "url": "https://acme.slack.com/", "team": "Acme", "user": "me", "team_id": "T0ACME", "user_id": "U0ME"}"#)
        .on("GET", "users.conversations", 200, USERS_CONVERSATIONS_1)
        .on("GET", "cursor=dXNlcjpVMDYxTkZUVDI%3D", 200, USERS_CONVERSATIONS_2)
        .on("GET", "conversations.info?channel=C0INC", 200, INFO_INC)
        .on("GET", "conversations.info?channel=D0PRIYA", 200, INFO_PRIYA)
        .on("GET", "conversations.info?channel=G0MPIM", 200, INFO_MPIM)
        .on("GET", "conversations.info?channel=C0RANDOM", 200, INFO_RANDOM)
        .on("GET", "conversations.history?channel=C0INC", 200, inc_page1)
        .on("GET", "cursor=bmV4dF90czoxNzg5OTkyMDAwMDAwMzAw", 200, HISTORY_INC_2)
        .on("GET", "conversations.history?channel=C0INC&latest", 200, HISTORY_INC_2)
        .on("GET", "conversations.replies?channel=C0INC&ts=1789990000.000200", 200, REPLIES_INC)
        .on("GET", "conversations.replies?channel=C0RANDOM&ts=1789000000.000100", 200, REPLIES_RANDOM)
        .on("GET", "conversations.history?channel=D0PRIYA", 200, PRIYA_HISTORY)
        .on("GET", "search.messages?query=%3C%40U0ME", 200, SEARCH)
        .on("GET", "search.messages?query=from", 200, SEARCH_MINE);
    for (id, display, real) in [
        ("U0ME", "me", "Me Myself"),
        ("U0JORDAN", "Jordan", "Jordan Lee"),
        ("U0SAM", "Sam", "Sam Okafor"),
        ("U0PRIYA", "Priya", "Priya Nair"),
        ("U0ALICE", "alice", "Alice Moreau"),
    ] {
        http.on("GET", &format!("users.info?user={id}"), 200, user(id, display, real));
    }
    // With `users:read.email` the profile carries the address.
    let mut marco: serde_json::Value = serde_json::from_str(&user("U0MARCO", "", "Marco Rossi")).unwrap();
    marco["user"]["profile"]["email"] = json!("Marco.Rossi@acme.com");
    http.on("GET", "users.info?user=U0MARCO", 200, &marco.to_string());
    http
}

/// `#ops`, not followed: a thread you started 115 days ago gets a new reply.
fn with_ops_thread(http: &FakeHttp) {
    http.on(
        "GET",
        "conversations.info?channel=C0OPS",
        200,
        r#"{"ok": true, "channel": {"id": "C0OPS", "name": "ops", "is_channel": true, "is_im": false, "is_mpim": false, "is_private": false, "is_archived": false, "is_member": true, "created": 1600000000, "last_read": "1789000000.000000"}}"#,
    )
    .on(
        "GET",
        "conversations.replies?channel=C0OPS&ts=1780000000.000100",
        200,
        r#"{"ok": true, "messages": [
            {"type": "message", "user": "U0ME", "text": "Runbook for disk pressure alerts", "ts": "1780000000.000100", "thread_ts": "1780000000.000100", "reply_count": 1, "reply_users_count": 1, "latest_reply": "1789999900.001300", "reply_users": ["U0SAM"], "subscribed": true, "last_read": "1780000000.000100"},
            {"type": "message", "user": "U0SAM", "text": "Seeing this again after the upgrade", "ts": "1789999900.001300", "thread_ts": "1780000000.000100", "parent_user_id": "U0ME"}
        ], "has_more": false, "response_metadata": {"next_cursor": ""}}"#,
    );
}

fn run_poll(http: &dyn Http, src: &Source) -> anyhow::Result<Batch> {
    let secret = |k: &str| (k == USER_TOKEN).then(|| "xoxp-test-token".to_owned());
    let ctx = Ctx {
        source: src,
        http,
        secret: &secret,
        now: now(),
        clock: crate::sync::test_clock(&now().to_db()),
    };
    let mut cfg = Config::from_json(&src.config);
    cfg.sleep = fake_sleep;
    poll::poll(&ctx, &cfg)
}

/// Items in a batch except what the search overlap read again: those carry
/// the revision they were stored with, so applying them changes nothing.
fn fresh(b: &Batch) -> Vec<&str> {
    ids(b).into_iter().filter(|id| !id.contains("/mention/")).collect()
}

fn find<'a>(b: &'a Batch, id: &str) -> &'a crate::sync::ItemUpsert {
    b.items
        .iter()
        .find(|i| i.external_id == id)
        .unwrap_or_else(|| panic!("no item {id}: {:?}", ids(b)))
}

fn ids(b: &Batch) -> Vec<&str> {
    let mut v: Vec<&str> = b.items.iter().map(|i| i.external_id.as_str()).collect();
    v.sort();
    v
}

fn count(http: &FakeHttp, part: &str) -> usize {
    http.requests().iter().filter(|(_, url, _)| url.contains(part)).count()
}

fn texts(it: &crate::sync::ItemUpsert) -> Vec<&str> {
    it.messages
        .iter()
        .filter(|m| !m.context)
        .map(|m| m.text.as_str())
        .collect()
}

fn context(it: &crate::sync::ItemUpsert) -> Vec<&str> {
    it.messages
        .iter()
        .filter(|m| m.context)
        .map(|m| m.text.as_str())
        .collect()
}

#[test]
fn a_run_of_sixty_messages_is_sent_whole() {
    let http = workspace(HISTORY_INC_1);
    // Newest first, as conversations.history pages.
    let msgs: Vec<Value> = (0..60).rev().map(|i| json!({ "type": "message", "user": "U0PRIYA", "text": format!("m{i}"), "ts": format!("{}.000800", 1789997000 + i * 10), "team": "T0ACME" })).collect();
    http.on(
        "GET",
        "conversations.history?channel=D0PRIYA",
        200,
        json!({ "ok": true, "messages": msgs, "has_more": false, "response_metadata": { "next_cursor": "" } })
            .to_string(),
    );
    let b = run_poll(&http, &source(None)).unwrap();
    let dm = find(&b, "D0PRIYA/1789997000.000800");
    assert_eq!(
        texts(dm).len(),
        60,
        "views cap what they show; the connector sends every message"
    );
    assert_eq!(dm.raw_json["msgs"].as_array().map(Vec::len), Some(60));
}

#[test]
fn first_catch_up_turns_unread_runs_into_items() {
    let http = workspace(HISTORY_INC_1);
    let b = run_poll(&http, &source(None)).unwrap();
    assert_eq!(
        ids(&b),
        vec![
            "C0INC/1789960000.000100",
            "C0INC/thread/1789990000.000200",
            "C0RANDOM/mention/1789998000.000900",
            "C0RANDOM/thread/1789000000.000100",
            "D0PRIYA/1789997000.000800",
        ]
    );
    assert!(b.seen.is_empty());

    // Followed channel: read back to the lookback floor, both pages.
    assert_eq!(count(&http, "conversations.history?channel=C0INC&oldest"), 2);
    assert!(http
        .requests()
        .iter()
        .any(|(_, u, _)| u.contains("channel=C0INC&oldest=1789740800.000000&inclusive=true&limit=200")));
    // The group DM has nothing unread per conversations.info: no history call.
    assert_eq!(count(&http, "conversations.history?channel=G0MPIM"), 0);
    // The deleted user's DM is skipped.
    assert_eq!(count(&http, "D0GONE"), 0);

    // Top level: everything within the lookback, read in Slack or not (Sam's
    // and Marco's messages were), your own included.
    let inc = find(&b, "C0INC/1789960000.000100");
    assert_eq!(
        (inc.kind.as_str(), inc.title.as_str(), inc.msg_count),
        ("thread", "#inc-2291", Some(5))
    );
    assert_eq!(inc.occurred_at, Ts::from_unix_secs(1789960000.0).unwrap());
    assert_eq!(inc.ended_at, Ts::from_unix_secs(1789995600.0));
    let authors: Vec<&str> = inc
        .messages
        .iter()
        .filter(|m| !m.context)
        .map(|m| m.author.as_str())
        .collect();
    assert_eq!(authors, vec!["Sam", "Marco Rossi", "PagerDuty", "me", "Jordan"]);
    assert_eq!(texts(inc)[2], "Incident acknowledged by @Jordan");
    assert!(context(inc).is_empty(), "nothing older in the channel");
    assert_eq!(inc.messages[0].author_id.as_deref(), Some("U0SAM"));
    // Profiles for People: real name always, email when the token may read it.
    let marco = b.profiles.iter().find(|p| p.account == "U0MARCO").unwrap();
    assert_eq!(
        (marco.kind.as_str(), marco.real_name.as_deref(), marco.email.as_deref()),
        ("slack", Some("Marco Rossi"), Some("Marco.Rossi@acme.com"))
    );
    assert_eq!(
        b.profiles
            .iter()
            .find(|p| p.account == "U0SAM")
            .and_then(|p| p.email.clone()),
        None
    );
    assert_eq!(
        b.profiles.iter().filter(|p| p.account == "U0SAM").count(),
        1,
        "one per user per poll"
    );
    assert!(http
        .requests()
        .iter()
        .any(|(_, u, _)| u.contains("channel=C0INC&latest=1789960000.000100&inclusive=false&limit=12")));
    assert_eq!(inc.author.as_ref().map(|h| h.handle.as_str()), Some("U0JORDAN"));
    assert_eq!(
        inc.url.as_deref(),
        Some("https://acme.slack.com/archives/C0INC/p1789995600000500")
    );
    assert_eq!(inc.raw_json["channel"], "C0INC");
    assert_eq!(inc.raw_json["mark_ts"], "1789995600.000500");
    assert_eq!(
        inc.raw_json["msgs"],
        json!([
            "1789960000.000100",
            "1789990000.000200",
            "1789992000.000300",
            "1789994000.000400",
            "1789995600.000500"
        ])
    );
    assert_eq!(inc.messages.iter().filter(|m| m.context).count(), 0);
    assert!(inc.bump);

    // The thread is its own item, with its own (thread) read marker.
    let thread = find(&b, "C0INC/thread/1789990000.000200");
    assert_eq!(
        (thread.kind.as_str(), thread.title.as_str()),
        ("thread", "#inc-2291 · thread: Backoff patch is up, needs a…")
    );
    assert_eq!(
        texts(thread),
        vec!["@Marco Rossi I can review & merge", "thanks, see PR 812 and #inc-2291"]
    );
    assert_eq!(context(thread), vec!["Backoff patch is up, needs a second reviewer."]);
    assert!(thread.raw_json.get("mark_ts").is_none());
    assert_eq!(thread.raw_json["thread_ts"], "1789990000.000200");
    assert_eq!(
        thread.url.as_deref(),
        Some("https://acme.slack.com/archives/C0INC/p1789996000000700?thread_ts=1789990000.000200&cid=C0INC")
    );

    let dm = find(&b, "D0PRIYA/1789997000.000800");
    assert_eq!(
        (dm.kind.as_str(), dm.title.as_str(), dm.msg_count),
        ("message", "DM · Priya", Some(1))
    );
    assert_eq!(dm.raw_json["mark_ts"], "1789997000.000800");

    // Only the real mention outside followed channels and DMs. You're in
    // #random, so confirming it marks #random read up to the mention.
    let mention = find(&b, "C0RANDOM/mention/1789998000.000900");
    assert_eq!(mention.title, "#random · mention");
    assert_eq!(
        mention.summary.as_deref(),
        Some("@me can you approve the Q4 headcount sheet?")
    );
    assert_eq!(
        (&mention.raw_json["channel"], &mention.raw_json["mark_ts"]),
        (&json!("C0RANDOM"), &json!("1789998000.000900"))
    );

    // An old thread you replied in, found through search: its replies within the lookback, yours too.
    let old = find(&b, "C0RANDOM/thread/1789000000.000100");
    assert_eq!(old.title, "#random · thread: Dashboard migration plan: who owns what?");
    assert_eq!(
        texts(old),
        vec![
            "I think Alice's team owns it now",
            "Not anymore, @me: it moved to platform in June."
        ]
    );
    assert!(old.bump);

    // Names resolved once per user per poll.
    assert_eq!(count(&http, "users.info?user=U0MARCO"), 1);
    assert_eq!(count(&http, "users.info?user=U0JORDAN"), 1);

    let state = State::parse(b.cursor.as_deref());
    assert_eq!(
        (state.me.as_str(), state.follow.clone()),
        ("U0ME", vec!["C0INC".to_owned()])
    );
    let open: Vec<&str> = state.open.keys().map(String::as_str).collect();
    assert_eq!(
        open,
        vec![
            "C0INC",
            "C0INC/1789990000.000200",
            "C0RANDOM/1789000000.000100",
            "C0RANDOM/mention/1789998000.000900",
            "D0PRIYA"
        ]
    );
    assert_eq!(state.threads["C0RANDOM/1789000000.000100"].seen, "1789999800.001200");
    assert_eq!(state.mention_ts.as_deref(), Some("1789998000.000900"));
    assert_eq!(state.reply_ts.as_deref(), Some("1789999500.001100"));
}

fn db() -> rusqlite::Connection {
    let conn = crate::db::open_in_memory().unwrap();
    let cfg = SourceConfig {
        kind: "slack".into(),
        tag: "work".into(),
        name: "Slack".into(),
        enabled: true,
        poll_secs: 300,
        config: json!({ "channels": ["#inc-2291"] }),
    };
    register_sources(&conn, &[cfg]).unwrap();
    conn
}

fn unseen(conn: &rusqlite::Connection) -> Vec<String> {
    conn.prepare("SELECT external_id FROM items WHERE seen = 0 ORDER BY external_id")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

fn read_all_in_slack(http: &FakeHttp) {
    http.on(
        "GET",
        "conversations.info?channel=C0INC",
        200,
        INFO_INC.replace("1789991000.000250", "1789995600.000500"),
    )
    .on(
        "GET",
        "conversations.info?channel=D0PRIYA",
        200,
        INFO_PRIYA.replace(
            "\"last_read\": \"1789990000.000000\"",
            "\"last_read\": \"1789997000.000800\"",
        ),
    )
    .on(
        "GET",
        "conversations.info?channel=C0RANDOM",
        200,
        INFO_RANDOM.replace("1789990000.000000", "1789999000.000000"),
    )
    .on(
        "GET",
        "conversations.replies?channel=C0INC",
        200,
        REPLIES_INC.replace(
            "\"last_read\": \"1789990000.000200\"",
            "\"last_read\": \"1789996000.000700\"",
        ),
    );
}

#[test]
fn reading_in_slack_never_clears_incoming() {
    let conn = db();
    let src = crate::sync::load_source(&conn, 1).unwrap();
    let b1 = run_poll(&workspace(HISTORY_INC_1), &src).unwrap();
    assert_eq!(apply_now(&conn, &src, &b1).unwrap(), 5);
    let src = crate::sync::load_source(&conn, 1).unwrap();

    let http = workspace(HISTORY_INC_1);
    read_all_in_slack(&http);
    let b2 = run_poll(&http, &src).unwrap();
    assert!(fresh(&b2).is_empty() && b2.seen.is_empty(), "{:?}", ids(&b2));
    assert_eq!(count(&http, "conversations.history?channel=D0PRIYA"), 0);
    // The mention read again by the search overlap is the same revision: nothing new.
    assert_eq!(apply_now(&conn, &src, &b2).unwrap(), 0);
    // The #inc-2291 thread is stored under its parent, in the channel's run.
    assert_eq!(unseen(&conn).len(), 4);
    assert_eq!(State::parse(b2.cursor.as_deref()).open.len(), 5);
}

#[test]
fn triage_closes_a_run_and_the_next_message_starts_a_new_one() {
    let conn = db();
    let src = crate::sync::load_source(&conn, 1).unwrap();
    apply_now(&conn, &src, &run_poll(&workspace(HISTORY_INC_1), &src).unwrap()).unwrap();
    conn.execute(
        "UPDATE items SET seen = 1 WHERE external_id = 'C0INC/1789960000.000100'",
        [],
    )
    .unwrap();
    let mut src = crate::sync::load_source(&conn, 1).unwrap();
    src.cursor = Slack.before_poll(&crate::store::over(&conn), &src).unwrap();
    let state = State::parse(src.cursor.as_deref());
    assert!(!state.open.contains_key("C0INC") && !state.open.contains_key("C0INC/1789990000.000200"));
    assert_eq!(state.closed["C0INC"], "1789995600.000500");

    // Nothing new: the triaged runs stay closed although Slack still has them unread.
    let b = run_poll(&workspace(HISTORY_INC_1), &src).unwrap();
    assert!(!ids(&b).iter().any(|i| i.starts_with("C0INC")), "{:?}", ids(&b));

    let http = workspace(&with_new_message("U0JORDAN", "Postmortem draft is up."));
    let mut replies: Value = serde_json::from_str(REPLIES_INC).unwrap();
    replies["messages"].as_array_mut().unwrap().push(json!({ "type": "message", "user": "U0ME", "text": "Merged.", "ts": "1789999100.001100", "thread_ts": "1789990000.000200" }));
    http.on("GET", "conversations.replies?channel=C0INC", 200, replies.to_string());
    let b = run_poll(&http, &src).unwrap();
    let inc = find(&b, "C0INC/1789999000.001000");
    assert_eq!(
        (texts(inc), inc.msg_count, inc.bump),
        (vec!["Postmortem draft is up."], Some(1), true)
    );
    let thread = find(&b, "C0INC/thread/1789990000.000200");
    assert_eq!((texts(thread), thread.bump), (vec!["Merged."], true));
    assert_eq!(context(thread)[0], "Backoff patch is up, needs a second reviewer.");
    assert_eq!(context(thread).len(), 3);

    // The new reply lands under its parent and brings the triaged run back.
    apply_now(&conn, &src, &b).unwrap();
    assert_eq!(
        unseen(&conn),
        [
            "C0INC/1789960000.000100",
            "C0INC/1789999000.001000",
            "C0RANDOM/mention/1789998000.000900",
            "C0RANDOM/thread/1789000000.000100",
            "D0PRIYA/1789997000.000800"
        ]
    );
    let replies: Vec<(String, String)> = conn
        .prepare("SELECT m.text, m.reply_to FROM item_messages m JOIN items i ON i.id = m.item_id WHERE i.external_id = 'C0INC/1789960000.000100' AND m.reply_to IS NOT NULL ORDER BY m.at")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert_eq!(
        replies.iter().map(|r| r.0.as_str()).collect::<Vec<_>>(),
        [
            "@Marco Rossi I can review & merge",
            "thanks, see PR 812 and #inc-2291",
            "Merged."
        ]
    );
    assert!(replies.iter().all(|r| r.1 == "1789990000.000200"));
    // What you'd triaged comes back read (faded); only the new reply is new.
    let read: Vec<(String, bool)> = conn
        .prepare("SELECT m.text, m.read FROM item_messages m JOIN items i ON i.id = m.item_id WHERE i.external_id = 'C0INC/1789960000.000100' AND m.context = 0 ORDER BY m.at")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert!(read.iter().all(|(t, r)| *r == (t != "Merged.")), "{read:?}");
}

fn with_new_message(user: &str, text: &str) -> String {
    let mut page: Value = serde_json::from_str(HISTORY_INC_1).unwrap();
    let msgs = page["messages"].as_array_mut().unwrap();
    msgs.insert(
        0,
        json!({ "type": "message", "user": user, "text": text, "ts": "1789999000.001000", "team": "T0ACME" }),
    );
    page.to_string()
}

#[test]
fn new_messages_in_an_unread_run_update_and_bump_the_same_item() {
    let b1 = run_poll(&workspace(HISTORY_INC_1), &source(None)).unwrap();
    let src = source(b1.cursor.clone());

    let http = workspace(&with_new_message("U0JORDAN", "Rollback done, closing the incident."));
    let b2 = run_poll(&http, &src).unwrap();
    assert_eq!(fresh(&b2), vec!["C0INC/1789960000.000100"]);
    let inc = find(&b2, "C0INC/1789960000.000100");
    assert_eq!((inc.msg_count, inc.bump), (Some(6), true));
    assert_eq!(inc.raw_json["mark_ts"], "1789999000.001000");
    // Nothing new in the DM per conversations.info: not re-read. Nothing new in threads.
    assert_eq!(count(&http, "conversations.history?channel=D0PRIYA"), 0);

    // Your own message is new too: it's context to file like anyone else's.
    let b3 = run_poll(&workspace(&with_new_message("U0ME", "Confirmed, thanks all.")), &src).unwrap();
    assert_eq!(fresh(&b3), vec!["C0INC/1789960000.000100"]);
    assert!(find(&b3, "C0INC/1789960000.000100").bump);
}

#[test]
fn channel_mentions_without_a_label_resolve_to_names() {
    let http = workspace(&with_new_message("U0JORDAN", "Ask in <#C0RANDOM> or <#C0SECRET>"));
    http.on(
        "GET",
        "conversations.info?channel=C0SECRET",
        200,
        r#"{"ok": false, "error": "channel_not_found"}"#,
    );
    let b = run_poll(&http, &source(None)).unwrap();
    let inc = find(&b, "C0INC/1789960000.000100");
    assert!(texts(inc).contains(&"Ask in #random or #C0SECRET"), "{:?}", texts(inc));
}

#[test]
fn partly_read_run_keeps_every_message() {
    let b1 = run_poll(&workspace(HISTORY_INC_1), &source(None)).unwrap();
    let http = workspace(HISTORY_INC_1);
    http.on(
        "GET",
        "conversations.info?channel=C0INC",
        200,
        INFO_INC.replace("1789991000.000250", "1789992000.000300"),
    );
    let b2 = run_poll(&http, &source(b1.cursor.clone())).unwrap();
    assert!(!ids(&b2).contains(&"C0INC/1789960000.000100"), "{:?}", ids(&b2));
    assert_eq!(
        State::parse(b2.cursor.as_deref()).open["C0INC"].first,
        "1789960000.000100"
    );
}

#[test]
fn live_sync_reads_only_what_events_flagged() {
    let b1 = run_poll(&workspace(HISTORY_INC_1), &source(None)).unwrap();
    let src = source_with(7, json!({ "channels": ["#inc-2291"] }), b1.cursor.clone());
    socket::test_set(
        7,
        true,
        true,
        &[
            Flag::Thread("C0OPS".into(), "1780000000.000100".into()),
            Flag::Conv("D0PRIYA".into()),
        ],
    );

    let http = FakeHttp::default();
    with_ops_thread(&http);
    for (id, display) in [("U0SAM", "Sam"), ("U0PRIYA", "Priya"), ("U0ME", "me")] {
        http.on("GET", &format!("users.info?user={id}"), 200, user(id, display, display));
    }
    http.on("GET", "conversations.info?channel=D0PRIYA", 200, INFO_PRIYA.replace("1789997000.000800\"}", "1789999950.001400\"}"))
        .on(
            "GET",
            "conversations.history?channel=D0PRIYA",
            200,
            r#"{"ok": true, "messages": [
                {"type": "message", "user": "U0PRIYA", "text": "Also: can you present the Q4 slide?", "ts": "1789999950.001400"},
                {"type": "message", "user": "U0PRIYA", "text": "Can you look at the offsite agenda before Thursday?", "ts": "1789997000.000800"}
            ], "has_more": false, "response_metadata": {"next_cursor": ""}}"#,
        );
    let b = run_poll(&http, &src).unwrap();
    assert_eq!(
        ids(&b),
        vec!["C0OPS/thread/1780000000.000100", "D0PRIYA/1789997000.000800"]
    );

    // A reply to a thread you started months ago.
    let ops = find(&b, "C0OPS/thread/1780000000.000100");
    assert_eq!(
        (ops.title.as_str(), ops.bump),
        ("#ops · thread: Runbook for disk pressure alerts", true)
    );
    assert_eq!(texts(ops), vec!["Seeing this again after the upgrade"]);
    // The DM's unread run grows.
    let dm = find(&b, "D0PRIYA/1789997000.000800");
    assert_eq!((dm.msg_count, dm.bump), (Some(2), true));

    // No listing, no other conversations, no search.
    assert_eq!(count(&http, "users.conversations"), 0);
    assert_eq!(count(&http, "C0INC"), 0);
    assert_eq!(count(&http, "search.messages"), 0);
    let state = State::parse(b.cursor.as_deref());
    assert!(state.threads.contains_key("C0OPS/1780000000.000100"));
    let p = socket::plan(7);
    assert_eq!((p.mode, p.flags.len()), (Mode::Live, 0));
}

#[test]
fn without_the_socket_dms_are_capped_to_the_most_recently_active() {
    let config = json!({ "channels": [], "max_dms": 1, "mentions": false });
    // Nothing known yet: newest conversation first (the group DM).
    let http = workspace(HISTORY_INC_1);
    run_poll(&http, &source_with(1, config.clone(), None)).unwrap();
    assert_eq!(
        (
            count(&http, "conversations.info?channel=G0MPIM"),
            count(&http, "conversations.info?channel=D0PRIYA")
        ),
        (1, 0)
    );

    // Activity Alethiometer has seen wins.
    let mut state = State {
        me: "U0ME".into(),
        team_url: "https://acme.slack.com/".into(),
        ..Default::default()
    };
    state.convs.insert(
        "D0PRIYA".into(),
        poll::ConvState {
            seen: "1789997000.000800".into(),
        },
    );
    let http = workspace(HISTORY_INC_1);
    run_poll(
        &http,
        &source_with(1, config, Some(serde_json::to_string(&state).unwrap())),
    )
    .unwrap();
    assert_eq!(
        (
            count(&http, "conversations.info?channel=G0MPIM"),
            count(&http, "conversations.info?channel=D0PRIYA")
        ),
        (0, 1)
    );
}

#[test]
fn without_the_socket_quiet_threads_are_checked_hourly() {
    let config = json!({ "channels": [], "dms": false, "mentions": false });
    let mut state = State {
        me: "U0ME".into(),
        team_url: "https://acme.slack.com/".into(),
        ..Default::default()
    };
    let key = "C0OPS/1780000000.000100";
    state.threads.insert(
        key.into(),
        poll::ThreadState {
            seen: "1785000000.000000".into(),
            title: String::new(),
            checked: NOW - 600.0,
        },
    );
    let http = workspace(HISTORY_INC_1);
    with_ops_thread(&http);
    run_poll(
        &http,
        &source_with(1, config.clone(), Some(serde_json::to_string(&state).unwrap())),
    )
    .unwrap();
    assert_eq!(count(&http, "conversations.replies"), 0);

    state.threads.get_mut(key).unwrap().checked = NOW - 3600.0;
    let b = run_poll(
        &http,
        &source_with(1, config, Some(serde_json::to_string(&state).unwrap())),
    )
    .unwrap();
    assert_eq!(count(&http, "conversations.replies?channel=C0OPS"), 1);
    assert_eq!(ids(&b), vec!["C0OPS/thread/1780000000.000100"]);
}

#[test]
fn ok_false_is_an_error_with_slacks_reason_and_no_token() {
    let http = workspace(HISTORY_INC_1);
    http.on(
        "GET",
        "users.conversations",
        200,
        r#"{"ok": false, "error": "missing_scope", "needed": "im:read", "provided": "channels:history"}"#,
    );
    let err = run_poll(&http, &source(None)).unwrap_err().to_string();
    assert!(
        err.contains("users.conversations: missing_scope (needs scope im:read)"),
        "{err}"
    );
    assert!(!err.contains("xoxp"));
}

#[test]
fn all_follows_every_channel_you_are_in() {
    let http = workspace(HISTORY_INC_1);
    http.on(
        "GET",
        "conversations.history?channel=C0RANDOM",
        200,
        r#"{"ok": true, "messages": [], "has_more": false}"#,
    );
    let b = run_poll(
        &http,
        &source_with(1, json!({ "channels": "all", "dms": false, "mentions": false }), None),
    )
    .unwrap();
    let state: State = serde_json::from_str(b.cursor.as_deref().unwrap()).unwrap();
    assert_eq!(state.follow, ["C0INC", "C0RANDOM"]);
    assert_eq!(count(&http, "conversations.history?channel=C0RANDOM"), 1);
}

#[test]
fn muted_channels_are_stored_but_never_new() {
    let conn = db();
    let mut src = crate::sync::load_source(&conn, 1).unwrap();
    src.config = json!({ "channels": ["#inc-2291"], "muted": ["#inc-2291"] });
    let b = run_poll(&workspace(HISTORY_INC_1), &src).unwrap();
    assert!(b.items.iter().any(|i| i.external_id.starts_with("C0INC/")));
    apply_now(&conn, &src, &b).unwrap();
    assert!(
        unseen(&conn).iter().all(|id| !id.starts_with("C0INC/")),
        "{:?}",
        unseen(&conn)
    );
    assert!(!unseen(&conn).is_empty());
}

#[test]
fn a_muted_dm_by_the_other_users_id_is_stored_but_never_new() {
    let conn = db();
    let mut src = crate::sync::load_source(&conn, 1).unwrap();
    src.config = json!({ "channels": ["#inc-2291"], "muted": ["U0PRIYA"] });
    let b = run_poll(&workspace(HISTORY_INC_1), &src).unwrap();
    assert!(b.items.iter().any(|i| i.external_id.starts_with("D0PRIYA/")));
    apply_now(&conn, &src, &b).unwrap();
    assert!(
        unseen(&conn).iter().all(|id| !id.starts_with("D0PRIYA/")),
        "{:?}",
        unseen(&conn)
    );
    assert!(!unseen(&conn).is_empty());
}

#[test]
fn unknown_followed_channel_fails_loudly() {
    let http = workspace(HISTORY_INC_1);
    let mut src = source(None);
    src.config = json!({ "channels": ["#nope"] });
    let err = run_poll(&http, &src).unwrap_err().to_string();
    assert!(err.contains("'#nope' not found"), "{err}");
}

#[test]
fn missing_token_is_reported_by_name() {
    let http = FakeHttp::default();
    let secret = |_: &str| None;
    let src = source(None);
    let ctx = Ctx {
        source: &src,
        http: &http,
        secret: &secret,
        now: now(),
        clock: crate::sync::test_clock(&now().to_db()),
    };
    let err = Slack.poll(&ctx).unwrap_err().to_string();
    assert!(err.contains("slack.user_token"), "{err}");
}

/// Responses in order, for retry behavior.
struct Sequence(Mutex<Vec<Response>>);

impl Http for Sequence {
    fn request(&self, _: &str, _: &str, _: &[(&str, &str)], _: Option<&str>) -> anyhow::Result<Response> {
        let mut q = self.0.lock().unwrap();
        anyhow::ensure!(!q.is_empty(), "no more responses");
        Ok(q.remove(0))
    }
}

fn resp(status: u16, body: &str, retry_after: Option<&str>) -> Response {
    Response {
        status,
        body: body.into(),
        headers: retry_after
            .map(|v| vec![("retry-after".into(), v.into())])
            .unwrap_or_default(),
    }
}

#[test]
fn rate_limits_wait_for_retry_after_then_retry() {
    SLEPT.with(|s| s.set(0));
    let http = Sequence(Mutex::new(vec![
        resp(429, "", Some("7")),
        resp(429, "", Some("2")),
        resp(200, r#"{"ok": true}"#, None),
    ]));
    let mut c = api::Client::new(&http, "xoxp-test".into());
    c.sleep = fake_sleep;
    c.post("conversations.mark", &[("channel", "C0INC"), ("ts", "1.2")])
        .unwrap();
    assert_eq!(SLEPT.with(Cell::get), 9);

    // Too long a wait fails this attempt instead of blocking the thread.
    let http = Sequence(Mutex::new(vec![resp(429, "", Some("900"))]));
    let mut c = api::Client::new(&http, "xoxp-test".into());
    c.sleep = fake_sleep;
    let err = c.get("users.info", &[("user", "U1")]).unwrap_err().to_string();
    assert!(err.contains("rate limited"), "{err}");
}

fn send_op(http: &FakeHttp, op: &str, payload: Value) -> anyhow::Result<Value> {
    let secret = |k: &str| (k == USER_TOKEN).then(|| "xoxp-test-token".to_owned());
    let src = source(None);
    let ctx = Ctx {
        source: &src,
        http,
        secret: &secret,
        now: now(),
        clock: crate::sync::test_clock(&now().to_db()),
    };
    Slack.send(&ctx, op, &payload)
}

fn last_body(http: &FakeHttp) -> (String, String) {
    let (_, url, body) = http.requests().pop().unwrap();
    (url, body.unwrap_or_default())
}

#[test]
fn mark_read_calls_conversations_mark() {
    let http = FakeHttp::default();
    http.on("GET", "conversations.info?channel=C0INC", 200, INFO_INC).on(
        "POST",
        "conversations.mark",
        200,
        r#"{"ok": true}"#,
    );
    let r = send_op(
        &http,
        "slack.mark_read",
        json!({ "channel": "C0INC", "ts": "1789995600.000500" }),
    )
    .unwrap();
    assert_eq!(r, json!({ "channel": "C0INC", "ts": "1789995600.000500" }));
    assert_eq!(
        last_body(&http),
        (
            "https://slack.com/api/conversations.mark".into(),
            "channel=C0INC&ts=1789995600.000500".into()
        )
    );

    // Threads: no public API; fail visibly, call nothing.
    let n = http.requests().len();
    let err = send_op(
        &http,
        "slack.mark_read",
        json!({ "channel": "C0INC", "ts": "1789996000.000700", "thread_ts": "1789990000.000200" }),
    )
    .unwrap_err();
    assert_eq!(err.to_string(), "Slack has no API to mark a thread read");
    assert_eq!(http.requests().len(), n);
}

#[test]
fn mark_read_never_moves_the_read_marker_backwards() {
    // You confirm a mention in #random, but have since read #random further in Slack.
    let http = FakeHttp::default();
    http.on(
        "GET",
        "conversations.info?channel=C0RANDOM",
        200,
        INFO_RANDOM.replace("1789990000.000000", "1789999000.000000"),
    )
    .on("POST", "conversations.mark", 200, r#"{"ok": true}"#);
    let r = send_op(
        &http,
        "slack.mark_read",
        json!({ "channel": "C0RANDOM", "ts": "1789998000.000900" }),
    )
    .unwrap();
    assert_eq!(
        r,
        json!({ "channel": "C0RANDOM", "ts": "1789998000.000900", "already": "1789999000.000000" })
    );
    assert_eq!(count(&http, "conversations.mark"), 0);
    // Read exactly up to it: nothing to do either.
    http.on(
        "GET",
        "conversations.info?channel=C0RANDOM",
        200,
        INFO_RANDOM.replace("1789990000.000000", "1789998000.000900"),
    );
    send_op(
        &http,
        "slack.mark_read",
        json!({ "channel": "C0RANDOM", "ts": "1789998000.000900" }),
    )
    .unwrap();
    assert_eq!(count(&http, "conversations.mark"), 0);

    // Marker before the mention: marked up to the mention.
    http.on("GET", "conversations.info?channel=C0RANDOM", 200, INFO_RANDOM);
    let r = send_op(
        &http,
        "slack.mark_read",
        json!({ "channel": "C0RANDOM", "ts": "1789998000.000900" }),
    )
    .unwrap();
    assert_eq!(r, json!({ "channel": "C0RANDOM", "ts": "1789998000.000900" }));
    assert_eq!(last_body(&http).1, "channel=C0RANDOM&ts=1789998000.000900");
}

#[test]
fn confirming_a_mention_queues_mark_read_up_to_it() {
    let conn = db();
    let src = crate::sync::load_source(&conn, 1).unwrap();
    let b = run_poll(&workspace(HISTORY_INC_1), &src).unwrap();
    apply_now(&conn, &src, &b).unwrap();
    let item: i64 = conn
        .query_row(
            "SELECT id FROM items WHERE external_id = 'C0RANDOM/mention/1789998000.000900'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    conn.execute("INSERT INTO topics (name, category) VALUES ('Headcount', 'work')", [])
        .unwrap();
    let topic = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO topic_links (item_id, topic_id, state) VALUES (?1, ?2, 'suggested')",
        [item, topic],
    )
    .unwrap();
    crate::queries::incoming::confirm(
        &crate::store::over(&conn),
        crate::time::Ts::new(jiff::Timestamp::now()),
        item,
        topic,
    )
    .unwrap();
    let payload: String = conn
        .query_row(
            "SELECT payload_json FROM outbox WHERE op = 'slack.mark_read' AND item_id = ?1",
            [item],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&payload).unwrap(),
        json!({ "channel": "C0RANDOM", "ts": "1789998000.000900" })
    );
}

#[test]
fn a_mention_in_a_channel_you_are_not_in_is_not_markable() {
    let http = workspace(HISTORY_INC_1);
    http.on(
        "GET",
        "conversations.info?channel=C0RANDOM",
        200,
        INFO_RANDOM.replace("\"is_member\": true", "\"is_member\": false"),
    );
    let b = run_poll(&http, &source(None)).unwrap();
    assert!(find(&b, "C0RANDOM/mention/1789998000.000900")
        .raw_json
        .get("mark_ts")
        .is_none());
}

#[test]
fn live_mode_runs_a_full_catch_up_every_catchup_hours() {
    let b1 = run_poll(&workspace(HISTORY_INC_1), &source(None)).unwrap();
    let mut state = State::parse(b1.cursor.as_deref());
    assert_eq!(state.caught_up, NOW);
    let config = json!({ "channels": ["#inc-2291"], "catchup_hours": 3 });

    // Connected and caught up 2h59m ago: live, nothing flagged, nothing listed.
    state.caught_up = NOW - 3.0 * 3600.0 + 60.0;
    state.read_checked = NOW;
    socket::test_set(8, true, true, &[]);
    let http = workspace(HISTORY_INC_1);
    let b = run_poll(
        &http,
        &source_with(8, config.clone(), Some(serde_json::to_string(&state).unwrap())),
    )
    .unwrap();
    assert_eq!(
        (count(&http, "users.conversations"), count(&http, "search.messages")),
        (0, 0)
    );
    assert_eq!(State::parse(b.cursor.as_deref()).caught_up, NOW - 3.0 * 3600.0 + 60.0);

    // Three hours: the same pass as on reconnect, though the socket never dropped.
    state.caught_up = NOW - 3.0 * 3600.0;
    socket::test_set(8, true, true, &[]);
    let http = workspace(HISTORY_INC_1);
    let b = run_poll(
        &http,
        &source_with(8, config, Some(serde_json::to_string(&state).unwrap())),
    )
    .unwrap();
    assert_eq!(count(&http, "users.conversations"), 2);
    assert_eq!(count(&http, "search.messages"), 2);
    // Cheap where it can be: DMs with nothing new aren't re-read.
    assert_eq!(count(&http, "conversations.history?channel=D0PRIYA"), 0);
    assert_eq!(count(&http, "conversations.history?channel=G0MPIM"), 0);
    // Nothing had changed, so nothing new is written; the clock restarts.
    assert!(fresh(&b).is_empty(), "{:?}", ids(&b));
    assert_eq!(State::parse(b.cursor.as_deref()).caught_up, NOW);
    assert_eq!(socket::plan(8).mode, Mode::Live);
}

#[test]
fn reply_posts_the_users_text_in_the_thread_or_the_conversation() {
    let http = FakeHttp::default();
    http.on(
        "POST",
        "chat.postMessage",
        200,
        r#"{"ok": true, "channel": "C0INC", "ts": "1790000100.000100", "message": {"type": "message", "user": "U0ME", "text": "On it & looking", "ts": "1790000100.000100", "thread_ts": "1789990000.000200"}}"#,
    );
    let r = send_op(
        &http,
        "slack.reply",
        json!({ "channel": "C0INC", "thread_ts": "1789990000.000200", "text": "On it & looking" }),
    )
    .unwrap();
    assert_eq!(r, json!({ "channel": "C0INC", "ts": "1790000100.000100" }));
    assert_eq!(
        last_body(&http).1,
        "channel=C0INC&thread_ts=1789990000.000200&text=On%20it%20%26%20looking"
    );

    // No thread: the message goes to the conversation itself.
    send_op(&http, "slack.reply", json!({ "channel": "D0PRIYA", "text": "sure" })).unwrap();
    assert_eq!(last_body(&http).1, "channel=D0PRIYA&text=sure");

    // Nothing to send without the user's words.
    let err = send_op(
        &http,
        "slack.reply",
        json!({ "channel": "C0INC", "thread_ts": "1", "text": "  " }),
    )
    .unwrap_err();
    assert!(err.to_string().contains("'text'"));
}

#[test]
fn react_adds_a_reaction_and_treats_already_reacted_as_done() {
    let http = FakeHttp::default();
    http.on("POST", "reactions.add", 200, r#"{"ok": true}"#);
    let r = send_op(
        &http,
        "slack.react",
        json!({ "channel": "C0INC", "ts": "1789995600.000500", "name": ":eyes:" }),
    )
    .unwrap();
    assert_eq!(r["name"], "eyes");
    assert_eq!(
        last_body(&http).1,
        "channel=C0INC&timestamp=1789995600.000500&name=eyes"
    );

    http.on(
        "POST",
        "reactions.add",
        200,
        r#"{"ok": false, "error": "already_reacted"}"#,
    );
    let r = send_op(
        &http,
        "slack.react",
        json!({ "channel": "C0INC", "ts": "1789995600.000500", "name": "eyes" }),
    )
    .unwrap();
    assert_eq!(r["already"], true);
}

#[test]
fn send_failures_carry_slacks_error() {
    let http = FakeHttp::default();
    http.on(
        "GET",
        "conversations.info",
        200,
        r#"{"ok": false, "error": "channel_not_found"}"#,
    );
    let err = send_op(&http, "slack.mark_read", json!({ "channel": "C0NOPE", "ts": "1.0" })).unwrap_err();
    assert_eq!(err.to_string(), "Slack conversations.info: channel_not_found");
    assert_eq!(count(&http, "conversations.mark"), 0);
    // No marker reported (not a member): try, and show Slack's answer.
    http.on(
        "GET",
        "conversations.info",
        200,
        r#"{"ok": true, "channel": {"id": "C0NEW", "is_channel": true, "is_member": false}}"#,
    )
    .on(
        "POST",
        "conversations.mark",
        200,
        r#"{"ok": false, "error": "not_in_channel"}"#,
    );
    let err = send_op(&http, "slack.mark_read", json!({ "channel": "C0NEW", "ts": "1.0" })).unwrap_err();
    assert_eq!(err.to_string(), "Slack conversations.mark: not_in_channel");
    assert!(send_op(&http, "slack.delete", json!({})).is_err());
}

#[test]
fn web_permalinks_open_in_the_desktop_app() {
    let conn = db();
    let src = crate::sync::load_source(&conn, 1).unwrap();
    let link = |u: &str| crate::sync::connectors::slack::deep_link(&crate::store::over(&conn), u);
    assert_eq!(
        link("https://acme.slack.com/archives/C0INC/p1789960000000100"),
        None,
        "no team id before the first sync"
    );
    apply_now(&conn, &src, &run_poll(&workspace(HISTORY_INC_1), &src).unwrap()).unwrap();
    assert_eq!(
        link("https://acme.slack.com/archives/C0INC/p1789960000000100").as_deref(),
        Some("slack://channel?team=T0ACME&id=C0INC&message=1789960000.000100")
    );
    assert_eq!(
        link("https://acme.slack.com/archives/C0INC/p1789990000000300?thread_ts=1789990000.000200&cid=C0INC")
            .as_deref(),
        Some("slack://channel?team=T0ACME&id=C0INC&message=1789990000.000300&thread_ts=1789990000.000200")
    );
    assert_eq!(
        link("https://other.slack.com/archives/C0INC/p1789960000000100"),
        None,
        "another workspace"
    );
    assert_eq!(link("https://github.com/acme/x/pull/1"), None);
}

fn run_streams(
    http: &dyn Http,
    src: &Source,
    failures: &[(&str, i64)],
    committed: &mut Vec<Batch>,
) -> anyhow::Result<Batch> {
    let secret = |k: &str| (k == USER_TOKEN).then(|| "xoxp-test-token".to_owned());
    let ctx = Ctx {
        source: src,
        http,
        secret: &secret,
        now: now(),
        clock: crate::sync::test_clock(&now().to_db()),
    };
    let mut cfg = Config::from_json(&src.config);
    cfg.sleep = fake_sleep;
    let mut commit = |b: Batch| -> anyhow::Result<()> {
        committed.push(b);
        Ok(())
    };
    let mut streams = crate::sync::Polling {
        failures: failures.iter().map(|(k, n)| (k.to_string(), *n)).collect(),
        revs: Default::default(),
        stale_open: vec![],
        commit: &mut commit,
    };
    poll::poll_with(&ctx, &cfg, Some(&mut streams))
}

#[test]
fn one_failing_conversation_doesnt_stop_the_others() {
    let http = workspace(HISTORY_INC_1);
    http.on(
        "GET",
        "conversations.history?channel=C0INC",
        200,
        r#"{"ok": false, "error": "not_in_channel"}"#,
    );
    let mut committed = vec![];
    let last = run_streams(&http, &source(None), &[], &mut committed).unwrap();
    let all: Vec<&crate::sync::ItemUpsert> = committed
        .iter()
        .chain(std::iter::once(&last))
        .flat_map(|b| &b.items)
        .collect();
    assert!(
        all.iter().any(|i| i.external_id == "D0PRIYA/1789997000.000800"),
        "the DM still came in"
    );
    assert!(!all.iter().any(|i| i.external_id == "C0INC/1789960000.000100"));
    let outcomes: Vec<&StreamOutcome> = committed
        .iter()
        .chain(std::iter::once(&last))
        .flat_map(|b| &b.streams)
        .collect();
    let inc = outcomes.iter().find(|o| o.key == "C0INC").unwrap();
    assert_eq!(
        inc.error.as_deref(),
        Some("Slack conversations.history: not_in_channel")
    );
    assert!(outcomes.iter().any(|o| o.key == "D0PRIYA" && o.error.is_none()));
}

#[test]
fn a_bad_token_or_the_network_ends_the_poll_but_keeps_what_was_read() {
    // The followed channel is read first and committed; then the DM fails at the network.
    let http = workspace(HISTORY_INC_1);
    http.on("GET", "conversations.history?channel=D0PRIYA", 503, "upstream down");
    let mut committed = vec![];
    assert!(run_streams(&http, &source(None), &[], &mut committed).is_err());
    let kept: Vec<&str> = committed
        .iter()
        .flat_map(|b| &b.items)
        .map(|i| i.external_id.as_str())
        .collect();
    assert!(kept.contains(&"C0INC/1789960000.000100"), "{kept:?}");
    assert!(
        committed.iter().all(|b| b.cursor.is_some()),
        "each commit carries the cursor so far"
    );

    let http = workspace(HISTORY_INC_1);
    http.on(
        "GET",
        "conversations.history?channel=C0INC",
        200,
        r#"{"ok": false, "error": "invalid_auth"}"#,
    );
    assert!(
        run_streams(&http, &source(None), &[], &mut vec![]).is_err(),
        "every call would fail alike"
    );
}

#[test]
fn a_missing_thread_is_dropped_after_three_tries() {
    let first = run_poll(&workspace(HISTORY_INC_1), &source(None)).unwrap();
    let key = "C0RANDOM/1789000000.000100";
    assert!(State::parse(first.cursor.as_deref()).threads.contains_key(key));
    let http = workspace(HISTORY_INC_1);
    http.on(
        "GET",
        "conversations.replies?channel=C0RANDOM&ts=1789000000.000100",
        200,
        r#"{"ok": false, "error": "thread_not_found"}"#,
    );
    let b = run_streams(&http, &source(first.cursor.clone()), &[], &mut vec![]).unwrap();
    assert!(
        State::parse(b.cursor.as_deref()).threads.contains_key(key),
        "one miss: still tracked"
    );
    assert_eq!(
        b.streams.iter().find(|o| o.key == key).and_then(|o| o.error.as_deref()),
        Some("Slack conversations.replies: thread_not_found")
    );
    let b = run_streams(&http, &source(first.cursor.clone()), &[(key, 2)], &mut vec![]).unwrap();
    assert!(
        !State::parse(b.cursor.as_deref()).threads.contains_key(key),
        "the third miss in a row: forgotten"
    );
}

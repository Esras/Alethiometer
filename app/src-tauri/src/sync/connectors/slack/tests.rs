//! Slack connector against canned API responses (shapes from the Web API docs).

use super::*;
use crate::sync::http::{FakeHttp, Http, Response};
use crate::sync::{apply, local_from_unix, register_sources, Source};
use crate::settings::SourceConfig;
use std::cell::Cell;
use std::sync::Mutex;

const USERS_CONVERSATIONS_1: &str = include_str!("../../../../tests/fixtures/slack/users_conversations_1.json");
const USERS_CONVERSATIONS_2: &str = include_str!("../../../../tests/fixtures/slack/users_conversations_2.json");
const HISTORY_INC_1: &str = include_str!("../../../../tests/fixtures/slack/history_inc_1.json");
const HISTORY_INC_2: &str = include_str!("../../../../tests/fixtures/slack/history_inc_2.json");
const REPLIES_INC: &str = include_str!("../../../../tests/fixtures/slack/replies_inc.json");
const SEARCH: &str = include_str!("../../../../tests/fixtures/slack/search_mentions.json");

/// 2026-09-21, a little after the last fixture message.
const NOW: f64 = 1_790_000_000.0;

thread_local! {
    static SLEPT: Cell<u64> = const { Cell::new(0) };
}

fn fake_sleep(d: std::time::Duration) {
    SLEPT.with(|s| s.set(s.get() + d.as_secs()));
}

fn now() -> chrono::NaiveDateTime {
    chrono::NaiveDateTime::parse_from_str(&local_from_unix(NOW).unwrap(), "%Y-%m-%dT%H:%M:%S").unwrap()
}

fn source(cursor: Option<String>) -> Source {
    Source {
        id: 1,
        kind: "slack".into(),
        tag: "work".into(),
        name: "Slack".into(),
        config: json!({ "channels": ["#inc-2291"] }),
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

/// A workspace where `#inc-2291` is followed, with one DM, an empty group DM
/// and a mention in `#random`. `inc_page1` is the newest history page.
fn workspace(inc_page1: &str) -> FakeHttp {
    let http = FakeHttp::default();
    let empty = r#"{"ok": true, "messages": [], "has_more": false, "pin_count": 0, "response_metadata": {"next_cursor": ""}}"#;
    http.on("GET", "auth.test", 200, r#"{"ok": true, "url": "https://acme.slack.com/", "team": "Acme", "user": "me", "team_id": "T0ACME", "user_id": "U0ME"}"#)
        .on("GET", "users.conversations", 200, USERS_CONVERSATIONS_1)
        .on("GET", "cursor=dXNlcjpVMDYxTkZUVDI%3D", 200, USERS_CONVERSATIONS_2)
        .on("GET", "conversations.history?channel=C0INC", 200, inc_page1)
        .on("GET", "cursor=bmV4dF90czoxNzg5OTkyMDAwMDAwMzAw", 200, HISTORY_INC_2)
        .on("GET", "conversations.replies?channel=C0INC&ts=1789990000.000200", 200, REPLIES_INC)
        .on(
            "GET",
            "conversations.history?channel=D0PRIYA",
            200,
            r#"{"ok": true, "messages": [{"type": "message", "user": "U0PRIYA", "text": "Can you look at the offsite agenda before Thursday?", "ts": "1789997000.000800", "team": "T0ACME"}], "has_more": false, "response_metadata": {"next_cursor": ""}}"#,
        )
        .on("GET", "conversations.history?channel=G0MPIM", 200, empty)
        .on("GET", "search.messages", 200, SEARCH);
    for (id, display, real) in [
        ("U0ME", "me", "Me Myself"),
        ("U0JORDAN", "Jordan", "Jordan Lee"),
        ("U0MARCO", "", "Marco Rossi"),
        ("U0SAM", "Sam", "Sam Okafor"),
        ("U0PRIYA", "Priya", "Priya Nair"),
    ] {
        http.on("GET", &format!("users.info?user={id}"), 200, user(id, display, real));
    }
    http
}

fn run_poll(http: &dyn Http, src: &Source) -> anyhow::Result<Batch> {
    let secret = |k: &str| (k == USER_TOKEN).then(|| "xoxp-test-token".to_owned());
    let ctx = Ctx { source: src, http, secret: &secret, now: now() };
    let mut cfg = Config::from_json(&src.config);
    cfg.sleep = fake_sleep;
    poll::poll(&ctx, &cfg)
}

fn find<'a>(b: &'a Batch, id: &str) -> &'a ItemUpsertRef {
    b.items.iter().find(|i| i.external_id == id).unwrap_or_else(|| panic!("no item {id}: {:?}", ids(b)))
}

type ItemUpsertRef = crate::sync::ItemUpsert;

fn ids(b: &Batch) -> Vec<&str> {
    b.items.iter().map(|i| i.external_id.as_str()).collect()
}

fn count(http: &FakeHttp, part: &str) -> usize {
    http.requests().iter().filter(|(_, url, _)| url.contains(part)).count()
}

#[test]
fn first_poll_groups_messages_into_windows() {
    let http = workspace(HISTORY_INC_1);
    let b = run_poll(&http, &source(None)).unwrap();
    let mut got = ids(&b);
    got.sort();
    assert_eq!(
        got,
        vec![
            "C0INC/1789960000.000100",
            "C0INC/1789990000.000200",
            "C0INC/1789990000.000200/1789995000.000600",
            "C0RANDOM/mention/1789998000.000900",
            "D0PRIYA/1789997000.000800",
        ]
    );

    // History paging: both pages read, starting at the lookback floor.
    assert_eq!(count(&http, "conversations.history?channel=C0INC"), 2);
    assert!(http.requests().iter().any(|(_, u, _)| u.contains("channel=C0INC&oldest=1789913600.000000&inclusive=true&limit=200")));
    // The deleted user's DM is skipped.
    assert_eq!(count(&http, "D0GONE"), 0);

    // The incident run: Marco, PagerDuty, me, Jordan (the join is noise; Sam's
    // message is hours earlier, so it's its own window).
    let inc = find(&b, "C0INC/1789990000.000200");
    assert_eq!((inc.kind.as_str(), inc.title.as_str(), inc.msg_count), ("thread", "#inc-2291", Some(4)));
    assert_eq!(inc.occurred_at, local_from_unix(1789990000.0).unwrap());
    assert_eq!(inc.ended_at, local_from_unix(1789995600.0));
    assert_eq!(inc.summary.as_deref(), Some("Fix ETA 14:00. Backlog is draining."));
    let authors: Vec<&str> = inc.messages.iter().map(|m| m.author.as_str()).collect();
    assert_eq!(authors, vec!["Marco Rossi", "PagerDuty", "me", "Jordan"]);
    assert_eq!(inc.messages[1].text, "Incident acknowledged by @Jordan");
    assert_eq!(inc.author.as_ref().map(|h| h.handle.as_str()), Some("U0JORDAN"));
    assert_eq!(inc.url.as_deref(), Some("https://acme.slack.com/archives/C0INC/p1789995600000500"));
    assert_eq!(inc.raw_json["mark_ts"], "1789995600.000500");
    assert!(inc.bump);

    let thread = find(&b, "C0INC/1789990000.000200/1789995000.000600");
    assert_eq!(thread.title, "#inc-2291 · thread: Backoff patch is up, needs a…");
    assert_eq!(thread.msg_count, Some(2));
    assert_eq!(thread.messages[0].text, "@Marco Rossi I can review & merge");
    assert_eq!(thread.messages[1].text, "thanks, see PR 812 and #inc-2291");
    assert!(thread.raw_json.get("mark_ts").is_none());
    assert_eq!(
        thread.url.as_deref(),
        Some("https://acme.slack.com/archives/C0INC/p1789996000000700?thread_ts=1789990000.000200&cid=C0INC")
    );

    let dm = find(&b, "D0PRIYA/1789997000.000800");
    assert_eq!((dm.kind.as_str(), dm.title.as_str(), dm.msg_count), ("message", "DM · Priya", Some(1)));

    // Only the real mention outside followed channels and DMs.
    let mention = find(&b, "C0RANDOM/mention/1789998000.000900");
    assert_eq!(mention.title, "#random · mention");
    assert_eq!(mention.summary.as_deref(), Some("@me can you approve the Q4 headcount sheet?"));
    assert_eq!(mention.url.as_deref(), Some("https://acme.slack.com/archives/C0RANDOM/p1789998000000900"));

    // Names resolved once per user per poll, even when a user shows up many times.
    assert_eq!(count(&http, "users.info?user=U0MARCO"), 1);
    assert_eq!(count(&http, "users.info?user=U0JORDAN"), 1);

    let state = State::parse(b.cursor.as_deref());
    assert_eq!((state.me.as_str(), state.follow.clone()), ("U0ME", vec!["C0INC".to_owned()]));
    assert_eq!(state.streams["C0INC"].run.as_deref(), Some("1789990000.000200"));
    assert_eq!(state.streams["C0INC/1789990000.000200"].ts, "1789996000.000700");
    assert_eq!(state.mention_ts.as_deref(), Some("1789998000.000900"));
}

fn with_new_message(user: &str, text: &str) -> String {
    let mut page: Value = serde_json::from_str(HISTORY_INC_1).unwrap();
    let msgs = page["messages"].as_array_mut().unwrap();
    msgs.insert(0, json!({ "type": "message", "user": user, "text": text, "ts": "1789999000.001000", "team": "T0ACME" }));
    page.to_string()
}

#[test]
fn new_messages_in_an_open_window_update_and_bump_the_same_item() {
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
    let mut src = crate::sync::load_source(&conn, 1).unwrap();

    let b1 = run_poll(&workspace(HISTORY_INC_1), &src).unwrap();
    assert_eq!(apply(&conn, &src, &b1).unwrap(), 5);
    conn.execute("UPDATE items SET seen = 1", []).unwrap();
    src = crate::sync::load_source(&conn, 1).unwrap();

    let http = workspace(&with_new_message("U0JORDAN", "Rollback done, closing the incident."));
    let b2 = run_poll(&http, &src).unwrap();
    assert_eq!(ids(&b2), vec!["C0INC/1789990000.000200"]);
    let inc = &b2.items[0];
    assert_eq!(inc.msg_count, Some(5));
    assert_eq!(inc.ended_at, local_from_unix(1789999000.0));
    assert!(inc.bump);
    // Thread unchanged (latest_reply didn't move): no replies call.
    assert_eq!(count(&http, "conversations.replies"), 0);

    assert_eq!(apply(&conn, &src, &b2).unwrap(), 1);
    let (n, unseen, msgs): (i64, i64, i64) = conn
        .query_row(
            "SELECT (SELECT count(*) FROM items), (SELECT count(*) FROM items WHERE seen = 0),
                    (SELECT count(*) FROM item_messages m JOIN items i ON i.id = m.item_id WHERE i.external_id = 'C0INC/1789990000.000200')",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!((n, unseen, msgs), (5, 1, 5));

    // Your own reply updates the window without making it new signal.
    let b3 = run_poll(&workspace(&with_new_message("U0ME", "Confirmed, thanks all.")), &src).unwrap();
    assert_eq!(ids(&b3), vec!["C0INC/1789990000.000200"]);
    assert!(!b3.items[0].bump);
}

#[test]
fn ok_false_is_an_error_with_slacks_reason_and_no_token() {
    let http = workspace(HISTORY_INC_1);
    http.on("GET", "users.conversations", 200, r#"{"ok": false, "error": "missing_scope", "needed": "im:read", "provided": "channels:history"}"#);
    let err = run_poll(&http, &source(None)).unwrap_err().to_string();
    assert!(err.contains("users.conversations: missing_scope (needs scope im:read)"), "{err}");
    assert!(!err.contains("xoxp"));
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
    let ctx = Ctx { source: &src, http: &http, secret: &secret, now: now() };
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
        headers: retry_after.map(|v| vec![("retry-after".into(), v.into())]).unwrap_or_default(),
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
    c.post("conversations.mark", &[("channel", "C0INC"), ("ts", "1.2")]).unwrap();
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
    let ctx = Ctx { source: &src, http, secret: &secret, now: now() };
    Slack.send(&ctx, op, &payload)
}

fn last_body(http: &FakeHttp) -> (String, String) {
    let (_, url, body) = http.requests().pop().unwrap();
    (url, body.unwrap_or_default())
}

#[test]
fn mark_read_calls_conversations_mark() {
    let http = FakeHttp::default();
    http.on("POST", "conversations.mark", 200, r#"{"ok": true}"#);
    let r = send_op(&http, "slack.mark_read", json!({ "channel": "C0INC", "ts": "1789995600.000500" })).unwrap();
    assert_eq!(r, json!({ "channel": "C0INC", "ts": "1789995600.000500" }));
    assert_eq!(last_body(&http), ("https://slack.com/api/conversations.mark".into(), "channel=C0INC&ts=1789995600.000500".into()));
}

#[test]
fn reply_posts_the_users_text_in_the_thread() {
    let http = FakeHttp::default();
    http.on(
        "POST",
        "chat.postMessage",
        200,
        r#"{"ok": true, "channel": "C0INC", "ts": "1790000100.000100", "message": {"type": "message", "user": "U0ME", "text": "On it & looking", "ts": "1790000100.000100", "thread_ts": "1789990000.000200"}}"#,
    );
    let r = send_op(&http, "slack.reply", json!({ "channel": "C0INC", "thread_ts": "1789990000.000200", "text": "On it & looking" })).unwrap();
    assert_eq!(r, json!({ "channel": "C0INC", "ts": "1790000100.000100" }));
    assert_eq!(last_body(&http).1, "channel=C0INC&thread_ts=1789990000.000200&text=On%20it%20%26%20looking");

    // Nothing to send without the user's words.
    let err = send_op(&http, "slack.reply", json!({ "channel": "C0INC", "thread_ts": "1", "text": "  " })).unwrap_err();
    assert!(err.to_string().contains("'text'"));
}

#[test]
fn react_adds_a_reaction_and_treats_already_reacted_as_done() {
    let http = FakeHttp::default();
    http.on("POST", "reactions.add", 200, r#"{"ok": true}"#);
    let r = send_op(&http, "slack.react", json!({ "channel": "C0INC", "ts": "1789995600.000500", "name": ":eyes:" })).unwrap();
    assert_eq!(r["name"], "eyes");
    assert_eq!(last_body(&http).1, "channel=C0INC&timestamp=1789995600.000500&name=eyes");

    http.on("POST", "reactions.add", 200, r#"{"ok": false, "error": "already_reacted"}"#);
    let r = send_op(&http, "slack.react", json!({ "channel": "C0INC", "ts": "1789995600.000500", "name": "eyes" })).unwrap();
    assert_eq!(r["already"], true);
}

#[test]
fn send_failures_carry_slacks_error() {
    let http = FakeHttp::default();
    http.on("POST", "conversations.mark", 200, r#"{"ok": false, "error": "channel_not_found"}"#);
    let err = send_op(&http, "slack.mark_read", json!({ "channel": "C0NOPE", "ts": "1.0" })).unwrap_err();
    assert_eq!(err.to_string(), "Slack conversations.mark: channel_not_found");
    assert!(send_op(&http, "slack.delete", json!({})).is_err());
}

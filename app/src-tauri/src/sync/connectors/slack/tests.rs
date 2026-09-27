//! Slack connector against canned API responses (shapes from the Web API docs).

use super::socket::{self, Flag, Mode};
use super::*;
use crate::settings::SourceConfig;
use crate::sync::http::{FakeHttp, Http, Response};
use crate::sync::{apply, local_from_unix, register_sources, Source};
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

fn now() -> chrono::NaiveDateTime {
    chrono::NaiveDateTime::parse_from_str(&local_from_unix(NOW).unwrap(), "%Y-%m-%dT%H:%M:%S").unwrap()
}

/// Source id 1 is never connected to a socket, so its polls are `Mode::Poll`.
fn source(cursor: Option<String>) -> Source {
    source_with(1, json!({ "channels": ["#inc-2291"] }), cursor)
}

fn source_with(id: i64, config: Value, cursor: Option<String>) -> Source {
    Source { id, kind: "slack".into(), tag: "work".into(), name: "Slack".into(), config, cursor }
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
        .on("GET", "conversations.replies?channel=C0INC&ts=1789990000.000200", 200, REPLIES_INC)
        .on("GET", "conversations.replies?channel=C0RANDOM&ts=1789000000.000100", 200, REPLIES_RANDOM)
        .on("GET", "conversations.history?channel=D0PRIYA", 200, PRIYA_HISTORY)
        .on("GET", "search.messages?query=%3C%40U0ME", 200, SEARCH)
        .on("GET", "search.messages?query=from", 200, SEARCH_MINE);
    for (id, display, real) in [
        ("U0ME", "me", "Me Myself"),
        ("U0JORDAN", "Jordan", "Jordan Lee"),
        ("U0MARCO", "", "Marco Rossi"),
        ("U0SAM", "Sam", "Sam Okafor"),
        ("U0PRIYA", "Priya", "Priya Nair"),
        ("U0ALICE", "alice", "Alice Moreau"),
    ] {
        http.on("GET", &format!("users.info?user={id}"), 200, user(id, display, real));
    }
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
    let ctx = Ctx { source: src, http, secret: &secret, now: now() };
    let mut cfg = Config::from_json(&src.config);
    cfg.sleep = fake_sleep;
    poll::poll(&ctx, &cfg)
}

fn find<'a>(b: &'a Batch, id: &str) -> &'a crate::sync::ItemUpsert {
    b.items.iter().find(|i| i.external_id == id).unwrap_or_else(|| panic!("no item {id}: {:?}", ids(b)))
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
    it.messages.iter().map(|m| m.text.as_str()).collect()
}

#[test]
fn first_catch_up_turns_unread_runs_into_items() {
    let http = workspace(HISTORY_INC_1);
    let b = run_poll(&http, &source(None)).unwrap();
    assert_eq!(
        ids(&b),
        vec![
            "C0INC/1789992000.000300",
            "C0INC/thread/1789990000.000200",
            "C0RANDOM/mention/1789998000.000900",
            "C0RANDOM/thread/1789000000.000100",
            "D0PRIYA/1789997000.000800",
        ]
    );
    assert!(b.seen.is_empty());

    // Followed channel: read back a day (recent parents), both pages.
    assert_eq!(count(&http, "conversations.history?channel=C0INC"), 2);
    assert!(http.requests().iter().any(|(_, u, _)| u.contains("channel=C0INC&oldest=1789913600.000000&inclusive=true&limit=200")));
    // The group DM has nothing unread per conversations.info: no history call.
    assert_eq!(count(&http, "conversations.history?channel=G0MPIM"), 0);
    // The deleted user's DM is skipped.
    assert_eq!(count(&http, "D0GONE"), 0);

    // Top level: what's after your read marker (Marco's parent and Sam's
    // older message are read), your own message kept for context.
    let inc = find(&b, "C0INC/1789992000.000300");
    assert_eq!((inc.kind.as_str(), inc.title.as_str(), inc.msg_count), ("thread", "#inc-2291", Some(3)));
    assert_eq!(inc.occurred_at, local_from_unix(1789992000.0).unwrap());
    assert_eq!(inc.ended_at, local_from_unix(1789995600.0));
    let authors: Vec<&str> = inc.messages.iter().map(|m| m.author.as_str()).collect();
    assert_eq!(authors, vec!["PagerDuty", "me", "Jordan"]);
    assert_eq!(inc.messages[0].text, "Incident acknowledged by @Jordan");
    assert_eq!(inc.author.as_ref().map(|h| h.handle.as_str()), Some("U0JORDAN"));
    assert_eq!(inc.url.as_deref(), Some("https://acme.slack.com/archives/C0INC/p1789995600000500"));
    assert_eq!(inc.raw_json["channel"], "C0INC");
    assert_eq!(inc.raw_json["mark_ts"], "1789995600.000500");
    assert_eq!(inc.raw_json["msgs"], json!(["1789992000.000300", "1789994000.000400", "1789995600.000500"]));
    assert!(inc.bump);

    // The thread is its own item, with its own (thread) read marker.
    let thread = find(&b, "C0INC/thread/1789990000.000200");
    assert_eq!((thread.kind.as_str(), thread.title.as_str()), ("thread", "#inc-2291 · thread: Backoff patch is up, needs a…"));
    assert_eq!(texts(thread), vec!["@Marco Rossi I can review & merge", "thanks, see PR 812 and #inc-2291"]);
    assert!(thread.raw_json.get("mark_ts").is_none());
    assert_eq!(thread.raw_json["thread_ts"], "1789990000.000200");
    assert_eq!(
        thread.url.as_deref(),
        Some("https://acme.slack.com/archives/C0INC/p1789996000000700?thread_ts=1789990000.000200&cid=C0INC")
    );

    let dm = find(&b, "D0PRIYA/1789997000.000800");
    assert_eq!((dm.kind.as_str(), dm.title.as_str(), dm.msg_count), ("message", "DM · Priya", Some(1)));
    assert_eq!(dm.raw_json["mark_ts"], "1789997000.000800");

    // Only the real mention outside followed channels and DMs; not markable.
    let mention = find(&b, "C0RANDOM/mention/1789998000.000900");
    assert_eq!(mention.title, "#random · mention");
    assert_eq!(mention.summary.as_deref(), Some("@me can you approve the Q4 headcount sheet?"));
    assert!(mention.raw_json.get("mark_ts").is_none());

    // An old thread you replied in, found through search: only what's after your reply.
    let old = find(&b, "C0RANDOM/thread/1789000000.000100");
    assert_eq!(old.title, "#random · thread: Dashboard migration plan: who owns what?");
    assert_eq!(texts(old), vec!["Not anymore, @me: it moved to platform in June."]);
    assert!(old.bump);

    // Names resolved once per user per poll.
    assert_eq!(count(&http, "users.info?user=U0MARCO"), 1);
    assert_eq!(count(&http, "users.info?user=U0JORDAN"), 1);

    let state = State::parse(b.cursor.as_deref());
    assert_eq!((state.me.as_str(), state.follow.clone()), ("U0ME", vec!["C0INC".to_owned()]));
    let open: Vec<&str> = state.open.keys().map(String::as_str).collect();
    assert_eq!(open, vec!["C0INC", "C0INC/1789990000.000200", "C0RANDOM/1789000000.000100", "C0RANDOM/mention/1789998000.000900", "D0PRIYA"]);
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

#[test]
fn reading_in_slack_marks_items_seen() {
    let conn = db();
    let src = crate::sync::load_source(&conn, 1).unwrap();
    let b1 = run_poll(&workspace(HISTORY_INC_1), &src).unwrap();
    assert_eq!(apply(&conn, &src, &b1).unwrap(), 5);
    let src = crate::sync::load_source(&conn, 1).unwrap();

    // You read everything in Slack except the #random thread.
    let http = workspace(HISTORY_INC_1);
    http.on("GET", "conversations.info?channel=C0INC", 200, INFO_INC.replace("1789991000.000250", "1789995600.000500"))
        .on("GET", "conversations.info?channel=D0PRIYA", 200, INFO_PRIYA.replace("\"last_read\": \"1789990000.000000\"", "\"last_read\": \"1789997000.000800\""))
        .on("GET", "conversations.info?channel=C0RANDOM", 200, INFO_RANDOM.replace("1789990000.000000", "1789999000.000000"))
        .on("GET", "conversations.replies?channel=C0INC", 200, REPLIES_INC.replace("\"last_read\": \"1789990000.000200\"", "\"last_read\": \"1789996000.000700\""));
    let b2 = run_poll(&http, &src).unwrap();
    assert!(b2.items.is_empty(), "{:?}", ids(&b2));
    let mut seen = b2.seen.clone();
    seen.sort();
    assert_eq!(seen, vec!["C0INC/1789992000.000300", "C0INC/thread/1789990000.000200", "C0RANDOM/mention/1789998000.000900", "D0PRIYA/1789997000.000800"]);
    // The DM wasn't re-read: conversations.info said it's read.
    assert_eq!(count(&http, "conversations.history?channel=D0PRIYA"), 0);

    assert_eq!(apply(&conn, &src, &b2).unwrap(), 0);
    let unseen: Vec<String> = conn
        .prepare("SELECT external_id FROM items WHERE seen = 0")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert_eq!(unseen, vec!["C0RANDOM/thread/1789000000.000100"]);
    let state = State::parse(b2.cursor.as_deref());
    assert_eq!(state.open.keys().collect::<Vec<_>>(), vec!["C0RANDOM/1789000000.000100"]);
}

fn with_new_message(user: &str, text: &str) -> String {
    let mut page: Value = serde_json::from_str(HISTORY_INC_1).unwrap();
    let msgs = page["messages"].as_array_mut().unwrap();
    msgs.insert(0, json!({ "type": "message", "user": user, "text": text, "ts": "1789999000.001000", "team": "T0ACME" }));
    page.to_string()
}

#[test]
fn new_messages_in_an_unread_run_update_and_bump_the_same_item() {
    let b1 = run_poll(&workspace(HISTORY_INC_1), &source(None)).unwrap();
    let src = source(b1.cursor.clone());

    let http = workspace(&with_new_message("U0JORDAN", "Rollback done, closing the incident."));
    let b2 = run_poll(&http, &src).unwrap();
    assert_eq!(ids(&b2), vec!["C0INC/1789992000.000300"]);
    let inc = &b2.items[0];
    assert_eq!((inc.msg_count, inc.bump), (Some(4), true));
    assert_eq!(inc.raw_json["mark_ts"], "1789999000.001000");
    // Nothing new in the DM per conversations.info: not re-read. Nothing new in threads.
    assert_eq!(count(&http, "conversations.history?channel=D0PRIYA"), 0);

    // Your own message updates the run without making it new signal.
    let b3 = run_poll(&workspace(&with_new_message("U0ME", "Confirmed, thanks all.")), &src).unwrap();
    assert_eq!(ids(&b3), vec!["C0INC/1789992000.000300"]);
    assert!(!b3.items[0].bump);
}

#[test]
fn partly_read_run_shrinks_without_bumping() {
    let b1 = run_poll(&workspace(HISTORY_INC_1), &source(None)).unwrap();
    let http = workspace(HISTORY_INC_1);
    http.on("GET", "conversations.info?channel=C0INC", 200, INFO_INC.replace("1789991000.000250", "1789992000.000300"));
    let b2 = run_poll(&http, &source(b1.cursor.clone())).unwrap();
    assert_eq!(ids(&b2), vec!["C0INC/1789992000.000300"]);
    let inc = &b2.items[0];
    assert_eq!((inc.msg_count, inc.bump), (Some(2), false));
    assert_eq!(inc.raw_json["first_ts"], "1789994000.000400");
    assert_eq!(inc.raw_json["msgs"], json!(["1789994000.000400", "1789995600.000500"]));
}

#[test]
fn live_sync_reads_only_what_events_flagged() {
    let b1 = run_poll(&workspace(HISTORY_INC_1), &source(None)).unwrap();
    let src = source_with(7, json!({ "channels": ["#inc-2291"] }), b1.cursor.clone());
    socket::test_set(7, true, true, &[Flag::Thread("C0OPS".into(), "1780000000.000100".into()), Flag::Conv("D0PRIYA".into())]);

    let http = FakeHttp::default();
    with_ops_thread(&http);
    for (id, display) in [("U0SAM", "Sam"), ("U0PRIYA", "Priya")] {
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
    assert_eq!(ids(&b), vec!["C0OPS/thread/1780000000.000100", "D0PRIYA/1789997000.000800"]);

    // A reply to a thread you started months ago.
    let ops = find(&b, "C0OPS/thread/1780000000.000100");
    assert_eq!((ops.title.as_str(), ops.bump), ("#ops · thread: Runbook for disk pressure alerts", true));
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
    assert_eq!((count(&http, "conversations.info?channel=G0MPIM"), count(&http, "conversations.info?channel=D0PRIYA")), (1, 0));

    // Activity Helm has seen wins.
    let mut state = State { me: "U0ME".into(), team_url: "https://acme.slack.com/".into(), ..Default::default() };
    state.convs.insert("D0PRIYA".into(), poll::ConvState { seen: "1789997000.000800".into() });
    let http = workspace(HISTORY_INC_1);
    run_poll(&http, &source_with(1, config, Some(serde_json::to_string(&state).unwrap()))).unwrap();
    assert_eq!((count(&http, "conversations.info?channel=G0MPIM"), count(&http, "conversations.info?channel=D0PRIYA")), (0, 1));
}

#[test]
fn without_the_socket_quiet_threads_are_checked_hourly() {
    let config = json!({ "channels": [], "dms": false, "mentions": false });
    let mut state = State { me: "U0ME".into(), team_url: "https://acme.slack.com/".into(), ..Default::default() };
    let key = "C0OPS/1780000000.000100";
    state.threads.insert(key.into(), poll::ThreadState { seen: "1785000000.000000".into(), title: String::new(), checked: NOW - 600.0 });
    let http = workspace(HISTORY_INC_1);
    with_ops_thread(&http);
    run_poll(&http, &source_with(1, config.clone(), Some(serde_json::to_string(&state).unwrap()))).unwrap();
    assert_eq!(count(&http, "conversations.replies"), 0);

    state.threads.get_mut(key).unwrap().checked = NOW - 3600.0;
    let b = run_poll(&http, &source_with(1, config, Some(serde_json::to_string(&state).unwrap()))).unwrap();
    assert_eq!(count(&http, "conversations.replies?channel=C0OPS"), 1);
    assert_eq!(ids(&b), vec!["C0OPS/thread/1780000000.000100"]);
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

    // Threads: no public API; fail visibly, call nothing.
    let n = http.requests().len();
    let err = send_op(&http, "slack.mark_read", json!({ "channel": "C0INC", "ts": "1789996000.000700", "thread_ts": "1789990000.000200" }))
        .unwrap_err();
    assert_eq!(err.to_string(), "Slack has no API to mark a thread read");
    assert_eq!(http.requests().len(), n);
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

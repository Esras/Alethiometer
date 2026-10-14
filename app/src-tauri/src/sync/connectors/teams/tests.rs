//! Teams connector against canned Graph responses (shapes from the Graph v1.0 docs).

use super::*;
use crate::settings::SourceConfig;
use crate::sync::http::{FakeHttp, Http};
use crate::sync::{apply_now, register_sources, Source};
use crate::time::Ts;

const DM: &str = "19%3Ame-priya_unq%40unq.gbl.spaces";
const DM_ID: &str = "19:me-priya_unq@unq.gbl.spaces";
const GRP: &str = "19%3Alaunchprep%40thread.v2";
const GRP_ID: &str = "19:launchprep@thread.v2";
const ME: &str = "11111111-0000-0000-0000-00000000000a";
const PRIYA: &str = "22222222-0000-0000-0000-00000000000b";
const SAM: &str = "33333333-0000-0000-0000-00000000000c";
const TENANT: &str = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";
const TEAM: &str = "44444444-0000-0000-0000-00000000000d";
const CH: &str = "19:incidents0123@thread.tacv2";
const CH_ENC: &str = "19%3Aincidents0123%40thread.tacv2";
/// Posts in #incidents: one from yesterday with a reply today, one new today.
const POST_OLD: &str = "1791882000000";
const POST_NEW: &str = "1791975600000";

macro_rules! fixture {
    ($name:literal) => {
        include_str!(concat!("../../../../tests/fixtures/teams/", $name))
    };
}

/// 2026-10-14T12:00:00Z, just after the last fixture message.
fn now() -> Ts {
    Ts::parse("2026-10-14T12:00:00Z").unwrap()
}

fn source(config: Value, cursor: Option<String>) -> Source {
    Source {
        id: 1,
        kind: "teams".into(),
        tag: "work".into(),
        name: "Teams".into(),
        config,
        cursor,
    }
}

fn chats_only() -> Value {
    json!({ "client_id": "cid", "tenant": "contoso.onmicrosoft.com", "secret_prefix": "teams.test" })
}

fn with_channel() -> Value {
    let mut c = chats_only();
    c["channels"] = json!([format!(
        "https://teams.microsoft.com/l/channel/{}/incidents?groupId={TEAM}&tenantId={TENANT}",
        // Teams writes the channel id with lowercase escapes.
        CH_ENC.to_lowercase()
    )]);
    c
}

/// A tenant with an unread DM from Priya, a group chat with Sam and a bot,
/// an old meeting chat and an empty chat; #incidents in team Platform.
fn tenant() -> FakeHttp {
    let http = FakeHttp::default();
    http.on("POST", "/oauth2/v2.0/token", 200, fixture!("token.json"))
        .on("GET", "/me?", 200, fixture!("me.json"))
        .on("GET", "/organization?", 200, fixture!("organization.json"))
        .on("GET", "/me/chats?", 200, fixture!("chats.json"))
        .on("GET", &format!("/chats/{DM}/members"), 200, fixture!("members_dm.json"))
        .on(
            "GET",
            &format!("/chats/{DM}/messages?"),
            200,
            fixture!("messages_dm.json"),
        )
        .on(
            "GET",
            &format!("/chats/{GRP}/messages?"),
            200,
            fixture!("messages_group.json"),
        )
        .on("GET", &format!("/users/{PRIYA}?"), 200, fixture!("user_priya.json"))
        .on("GET", &format!("/users/{SAM}?"), 200, fixture!("user_sam.json"))
        .on("GET", &format!("/teams/{TEAM}?"), 200, fixture!("team.json"))
        .on(
            "GET",
            &format!("/teams/{TEAM}/channels/{CH_ENC}?"),
            200,
            fixture!("channel.json"),
        )
        .on(
            "GET",
            &format!("/teams/{TEAM}/channels/{CH_ENC}/messages?"),
            200,
            fixture!("channel_messages.json"),
        )
        .on(
            "GET",
            &format!("/teams/{TEAM}/channels/{CH_ENC}/messages/{POST_OLD}/replies"),
            200,
            fixture!("replies.json"),
        )
        .on(
            "GET",
            &format!("/teams/{TEAM}/channels/{CH_ENC}/messages/{POST_NEW}/replies"),
            200,
            r#"{"value": []}"#,
        );
    http
}

fn ctx_run<T>(http: &dyn Http, src: &Source, f: impl FnOnce(&Ctx) -> T) -> T {
    let secret = |k: &str| (k == "teams.test.refresh_token").then(|| "0.AQ-refresh-1".to_owned());
    let save = |_: &str, _: &str| Ok(());
    let ctx = Ctx {
        source: src,
        http,
        secret: &secret,
        save_secret: &save,
        now: now(),
        clock: crate::sync::test_clock(&now().to_db()),
    };
    f(&ctx)
}

fn run_poll(http: &dyn Http, src: &Source) -> anyhow::Result<Batch> {
    ctx_run(http, src, |ctx| {
        let mut cfg = Config::from_json(&src.config);
        cfg.sleep = |_| {};
        poll::poll_with(ctx, &cfg, None)
    })
}

fn item<'b>(b: &'b Batch, id: &str) -> &'b crate::sync::ItemUpsert {
    b.items.iter().find(|i| i.external_id == id).unwrap_or_else(|| {
        panic!(
            "no item {id}: {:?}",
            b.items.iter().map(|i| &i.external_id).collect::<Vec<_>>()
        )
    })
}

#[test]
fn a_first_poll_reads_recent_chats_as_runs() {
    let http = tenant();
    let b = run_poll(&http, &source(chats_only(), None)).unwrap();
    // The meeting chat's last message is older than the lookback; the empty chat has none.
    assert_eq!(
        b.items.len(),
        2,
        "{:?}",
        b.items.iter().map(|i| &i.external_id).collect::<Vec<_>>()
    );

    let dm = item(&b, &format!("{DM_ID}/1791907200000"));
    assert_eq!(dm.title, "Chat · Priya Shah");
    assert_eq!(dm.kind, "thread");
    // A system event and a deleted message are left out.
    let texts: Vec<&str> = dm.messages.iter().map(|m| m.text.as_str()).collect();
    assert_eq!(
        texts,
        [
            "See you tomorrow",
            "Hi Zach Example, can you check [the rollout plan](https://contoso.sharepoint.com/plan)?\n\n[file: plan.docx]",
            "Rollout plan is in the doc",
            "Fine, ship it 👍",
        ]
    );
    assert_eq!(dm.raw_json["conv"], "dm");
    assert_eq!(dm.raw_json["channel"], DM_ID);
    assert_eq!(dm.raw_json["mark_ts"], "1791978000000");
    assert_eq!(
        dm.raw_json["msgs"],
        json!(["1791907200000", "1791977400000", "1791977700000", "1791978000000"])
    );
    assert_eq!(
        dm.author.as_ref().unwrap().handle,
        PRIYA,
        "the last author who isn't you"
    );
    assert_eq!(dm.messages[2].author_id.as_deref(), Some(ME));
    assert!(dm.bump && dm.attach_to.is_none());

    let grp = item(&b, &format!("{GRP_ID}/1791973740000"));
    assert_eq!(grp.title, "Group chat · Launch prep");
    assert_eq!(grp.raw_json["conv"], "group");
    assert_eq!(grp.messages[0].author, "Polly");

    // Profiles: you (from /me), people with their email, the bot without.
    let p = |id: &str| b.profiles.iter().find(|p| p.account == id).unwrap();
    assert_eq!(p(ME).email.as_deref(), Some("zach@contoso.com"));
    assert_eq!(p(PRIYA).email.as_deref(), Some("priya.shah@contoso.com"));
    assert_eq!(p(SAM).email.as_deref(), Some("sam.okafor@contoso.com"));
    assert!(p("app-1").is_bot && p("app-1").email.is_none());
    assert!(b.profiles.iter().all(|p| p.kind == "teams"));

    let state = State::parse(b.cursor.as_deref());
    assert_eq!((state.me.as_str(), state.tenant.as_str()), (ME, TENANT));
    assert_eq!(state.chats[DM_ID].seen, "1791978000000");
    assert_eq!(state.open[DM_ID].id, format!("{DM_ID}/1791907200000"));
    // No channel was asked for.
    assert!(!http.requests().iter().any(|r| r.1.contains("/teams/")));
}

#[test]
fn a_chat_with_nothing_new_isnt_read_again() {
    let http = tenant();
    let src = source(chats_only(), None);
    let first = run_poll(&http, &src).unwrap();
    // Both runs triaged: nothing open, and the chats' newest messages are read.
    let mut state = State::parse(first.cursor.as_deref());
    state.open.clear();
    let before = http.requests().len();
    let again = run_poll(
        &http,
        &source(chats_only(), Some(serde_json::to_string(&state).unwrap())),
    )
    .unwrap();
    assert!(again.items.is_empty());
    let calls: Vec<String> = http.requests()[before..].iter().map(|r| r.1.clone()).collect();
    assert!(calls.iter().all(|u| !u.contains("/messages")), "{calls:?}");
}

#[test]
fn triage_closes_a_run_and_the_next_message_starts_a_new_one() {
    let conn = crate::db::open_in_memory().unwrap();
    register_sources(
        &conn,
        &[SourceConfig {
            kind: "teams".into(),
            tag: "work".into(),
            name: "Teams".into(),
            enabled: true,
            poll_secs: 120,
            config: chats_only(),
        }],
    )
    .unwrap();
    let http = tenant();
    let src = crate::sync::load_source(&conn, 1).unwrap();
    apply_now(&conn, &src, &run_poll(&http, &src).unwrap()).unwrap();
    conn.execute(
        "UPDATE items SET seen = 1 WHERE external_id = ?1",
        [format!("{DM_ID}/1791907200000")],
    )
    .unwrap();
    let mut src = crate::sync::load_source(&conn, 1).unwrap();
    src.cursor = Teams.before_poll(&crate::store::over(&conn), &src).unwrap();
    let state = State::parse(src.cursor.as_deref());
    assert!(!state.open.contains_key(DM_ID));
    assert_eq!(state.closed[DM_ID], "1791978000000");

    // Priya writes again: a new run, without the triaged messages.
    let mut msgs: Value = serde_json::from_str(fixture!("messages_dm.json")).unwrap();
    let mut next = msgs["value"][0].clone();
    next["id"] = json!("1791979100000");
    next["createdDateTime"] = json!("2026-10-14T11:58:20Z");
    next["lastModifiedDateTime"] = json!("2026-10-14T11:58:20Z");
    next["body"] = json!({"contentType": "text", "content": "One more thing"});
    msgs["value"].as_array_mut().unwrap().insert(0, next);
    let mut chats: Value = serde_json::from_str(fixture!("chats.json")).unwrap();
    chats["value"][0]["lastMessagePreview"]["id"] = json!("1791979100000");
    http.on("GET", &format!("/chats/{DM}/messages?"), 200, msgs.to_string())
        .on("GET", "/me/chats?", 200, chats.to_string());
    let b = run_poll(&http, &src).unwrap();
    let dm = item(&b, &format!("{DM_ID}/1791979100000"));
    let own: Vec<&str> = dm
        .messages
        .iter()
        .filter(|m| !m.context)
        .map(|m| m.text.as_str())
        .collect();
    assert_eq!(own, ["One more thing"]);
    assert_eq!(
        dm.messages.iter().filter(|m| m.context).count(),
        3,
        "earlier messages as context"
    );
}

#[test]
fn a_muted_chat_is_read_but_never_new() {
    let mut cfg = chats_only();
    cfg["muted"] = json!([GRP_ID]);
    let b = run_poll(&tenant(), &source(cfg, None)).unwrap();
    assert_eq!(b.seen, [format!("{GRP_ID}/1791973740000")]);
}

#[test]
fn a_channel_reply_attaches_to_its_post_and_new_posts_are_a_run() {
    let http = tenant();
    let b = run_poll(&http, &source(with_channel(), None)).unwrap();
    let conv = format!("{TEAM}|{CH}");
    // Both posts are within the lookback: one run of top-level posts.
    let posts = item(&b, &format!("{conv}/{POST_OLD}"));
    assert_eq!(posts.title, "#incidents · Platform");
    assert_eq!(posts.raw_json["conv"], "channel");
    assert!(posts.raw_json.get("mark_ts").is_none(), "channels have no read marker");
    assert_eq!(posts.raw_json["msgs"], json!([POST_OLD, POST_NEW]));
    // The old post changed (a reply today): its thread, attached to the post.
    let thread = item(&b, &format!("{conv}/thread/{POST_OLD}"));
    assert_eq!(thread.attach_to.as_deref(), Some(POST_OLD));
    assert_eq!(thread.raw_json["thread_ts"], POST_OLD);
    assert_eq!(
        thread.title,
        "#incidents · Platform · thread: Alert storm on ingest, looking"
    );
    let own: Vec<&str> = thread
        .messages
        .iter()
        .filter(|m| !m.context)
        .map(|m| m.text.as_str())
        .collect();
    assert_eq!(own, ["Paging Sam", "Root cause is the retry loop"]);
    let state = State::parse(b.cursor.as_deref());
    assert!(state.threads.contains_key(&format!("{conv}/{POST_OLD}")));
}

#[test]
fn a_channel_without_admin_approval_fails_alone() {
    let http = tenant();
    http.on(
        "GET",
        &format!("/teams/{TEAM}/channels/{CH_ENC}/messages?"),
        403,
        r#"{"error": {"code": "Forbidden", "message": "Missing role permissions on the request."}}"#,
    );
    let b = run_poll(&http, &source(with_channel(), None)).unwrap();
    assert_eq!(b.items.len(), 2, "the chats still came in");
    let failed = b.streams.iter().find(|s| s.key == format!("{TEAM}|{CH}")).unwrap();
    let e = failed.error.as_deref().unwrap();
    assert!(e.contains("403") && e.contains("Connect again"), "{e}");
}

#[test]
fn a_bad_channel_entry_says_how_to_write_it() {
    let mut cfg = chats_only();
    cfg["channels"] = json!(["incidents"]);
    let e = run_poll(&tenant(), &source(cfg, None)).unwrap_err().to_string();
    assert!(e.contains("Get link to channel"), "{e}");
    assert_eq!(
        channel_id(&format!("{TEAM}|{CH}")).unwrap(),
        (TEAM.to_owned(), CH.to_owned())
    );
}

fn send_op(http: &FakeHttp, op: &str, payload: Value) -> anyhow::Result<Value> {
    let state = State {
        me: ME.into(),
        tenant: TENANT.into(),
        ..Default::default()
    };
    let src = source(chats_only(), Some(serde_json::to_string(&state).unwrap()));
    ctx_run(http, &src, |ctx| {
        let mut cfg = Config::from_json(&src.config);
        cfg.sleep = |_| {};
        send(ctx, &cfg, op, &payload)
    })
}

fn chat_info(newest: &str, read: &str) -> String {
    json!({
        "id": DM_ID, "chatType": "oneOnOne",
        "viewpoint": { "isHidden": false, "lastMessageReadDateTime": read },
        "lastMessagePreview": { "id": newest, "createdDateTime": "2026-10-14T11:40:00Z" }
    })
    .to_string()
}

#[test]
fn mark_read_marks_the_chat_only_when_you_saw_its_newest_message() {
    let http = tenant();
    http.on(
        "GET",
        &format!("/chats/{DM}?$expand"),
        200,
        chat_info("1791978000000", "2026-10-14T08:00:00Z"),
    )
    .on("POST", &format!("/chats/{DM}/markChatReadForUser"), 204, "");
    let r = send_op(
        &http,
        "teams.mark_read",
        json!({"channel": DM_ID, "ts": "1791978000000"}),
    )
    .unwrap();
    assert_eq!(r, json!({"ok": true}));
    let (_, url, body) = http.requests().last().unwrap().clone();
    assert!(url.ends_with("/markChatReadForUser"));
    assert_eq!(
        serde_json::from_str::<Value>(&body.unwrap()).unwrap(),
        json!({"user": {"id": ME, "tenantId": TENANT}})
    );

    // A message came in after the one you saw: nothing is marked.
    http.on(
        "GET",
        &format!("/chats/{DM}?$expand"),
        200,
        chat_info("1791979100000", "2026-10-14T08:00:00Z"),
    );
    let n = http.requests().len();
    let r = send_op(
        &http,
        "teams.mark_read",
        json!({"channel": DM_ID, "ts": "1791978000000"}),
    )
    .unwrap();
    assert_eq!(r["not_marked"], "newer messages in the chat");
    assert!(http.requests()[n..]
        .iter()
        .all(|r| r.0 != "POST" || r.1.contains("/token")));

    // Already read that far in Teams.
    http.on(
        "GET",
        &format!("/chats/{DM}?$expand"),
        200,
        chat_info("1791978000000", "2026-10-14T11:45:00Z"),
    );
    let r = send_op(
        &http,
        "teams.mark_read",
        json!({"channel": DM_ID, "ts": "1791978000000"}),
    )
    .unwrap();
    assert!(r.get("already").is_some(), "{r}");

    let e = send_op(
        &http,
        "teams.mark_read",
        json!({"channel": format!("{TEAM}|{CH}"), "ts": POST_OLD}),
    )
    .unwrap_err();
    assert!(e.to_string().contains("no read marker for channels"));
}

#[test]
fn a_reply_goes_to_the_chat_or_the_posts_thread() {
    let http = tenant();
    http.on(
        "POST",
        &format!("/chats/{DM}/messages"),
        201,
        r#"{"id": "1791979200000"}"#,
    )
    .on(
        "POST",
        &format!("/teams/{TEAM}/channels/{CH_ENC}/messages/{POST_OLD}/replies"),
        201,
        r#"{"id": "1791979300000"}"#,
    );
    let r = send_op(
        &http,
        "teams.reply",
        json!({"channel": DM_ID, "text": "on it <b>now</b>\nthanks"}),
    )
    .unwrap();
    assert_eq!(r["id"], "1791979200000");
    let body: Value = serde_json::from_str(http.requests().last().unwrap().2.as_deref().unwrap()).unwrap();
    assert_eq!(
        body,
        json!({"body": {"contentType": "html", "content": "on it &lt;b&gt;now&lt;/b&gt;<br>thanks"}})
    );
    let e = send_op(
        &http,
        "teams.reply",
        json!({"channel": DM_ID, "thread_ts": "1791978000000", "text": "x"}),
    )
    .unwrap_err();
    assert!(e.to_string().contains("no threads"));
    send_op(
        &http,
        "teams.reply",
        json!({"channel": format!("{TEAM}|{CH}"), "thread_ts": POST_OLD, "text": "looking"}),
    )
    .unwrap();
    assert!(http
        .requests()
        .last()
        .unwrap()
        .1
        .ends_with(&format!("/messages/{POST_OLD}/replies")));
}

#[test]
fn react_sets_a_reaction_on_a_chat_message_or_a_reply() {
    let http = tenant();
    http.on("POST", "/setReaction", 204, "");
    send_op(
        &http,
        "teams.react",
        json!({"channel": DM_ID, "ts": "1791978000000", "name": "👍"}),
    )
    .unwrap();
    let (_, url, body) = http.requests().last().unwrap().clone();
    assert!(
        url.ends_with(&format!("/chats/{DM}/messages/1791978000000/setReaction")),
        "{url}"
    );
    assert_eq!(
        serde_json::from_str::<Value>(&body.unwrap()).unwrap(),
        json!({"reactionType": "👍"})
    );
    send_op(
        &http,
        "teams.react",
        json!({"channel": format!("{TEAM}|{CH}"), "thread_ts": POST_OLD, "ts": "1791978600000", "name": "👀"}),
    )
    .unwrap();
    assert!(http
        .requests()
        .last()
        .unwrap()
        .1
        .ends_with(&format!("/messages/{POST_OLD}/replies/1791978600000/setReaction")));
}

#[test]
fn teams_markup_becomes_markdown() {
    let t = poll::html_to_markdown(
        r#"<p>Deploy <b>now</b>?</p><ul><li>api</li><li>web</li></ul><pre><code>make ship</code></pre>"#,
        None,
    );
    assert_eq!(t, "Deploy **now**?\n\n*   api\n*   web\n\n```\nmake ship\n```");
    assert_eq!(poll::html_to_markdown("<div><div>plain</div></div>", None), "plain");
}

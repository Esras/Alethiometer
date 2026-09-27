//! Slack Socket Mode: the primary feed; polling is the catch-up.
//!
//! `apps.connections.open` (app-level `xapp-` token) returns a temporary
//! `wss://` URL. Slack pushes `events_api` envelopes over it; each one is
//! ACKed with its `envelope_id` within 3 seconds. A relevant message event
//! *flags* the conversation, thread or mention it belongs to in an in-memory
//! registry and schedules a sync (debounced, so a busy incident channel causes
//! one sync per few seconds). The poller reads the registry (`plan`):
//!
//! - connected and caught up: `Mode::Live`, fetch only what was flagged
//!   (the poller still turns this into a full pass every `catchup_hours`,
//!   because Socket Mode delivery is best-effort: Slack retries an unacked
//!   event a few times and then drops it, so events can be lost without the
//!   connection ever dropping);
//! - connected, not yet caught up since this connection opened (startup, or a
//!   reconnect after a drop): `Mode::CatchUp`, one full pass;
//! - not connected (no app token, or the socket is down): `Mode::Poll`, the
//!   plain polling fallback on `poll_secs`.
//!
//! The registry lives in memory on purpose: after a restart there is no
//! connection yet, so the first sync is a catch-up anyway. All mapping stays
//! in `poll`; this module never builds items.

use super::api::Client;
use super::{Config, State};
use crate::state::App;
use crate::sync::{http::UreqHttp, load_source, sync_source};
use serde_json::{json, Value};
use std::collections::{BTreeSet, HashMap};
use std::sync::{mpsc, Arc, LazyLock, Mutex};
use std::time::Duration;
use tungstenite::{stream::MaybeTlsStream, Message};

/// Wait this long after an event for more before syncing.
const DEBOUNCE: Duration = Duration::from_secs(3);
/// Sync at least this often during a continuous stream of events.
const MAX_DELAY: Duration = Duration::from_secs(15);
/// No frame (Slack pings regularly) for this long means a dead connection.
const READ_TIMEOUT: Duration = Duration::from_secs(120);

// ---------------------------------------------------------------------------
// Registry shared with the poller.

/// What an event asks the poller to look at.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Flag {
    /// New top-level message in a DM, group DM or followed channel.
    Conv(String),
    /// New reply in a thread: (channel, thread_ts).
    Thread(String, String),
    /// A top-level mention of you in a channel you don't follow: (channel, ts).
    Mention(String, String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// No live connection: poll everything (DMs capped) on `poll_secs`.
    Poll,
    /// Connected, but events may have been missed before this connection.
    CatchUp,
    /// Connected and caught up: only flagged conversations.
    Live,
}

/// What one sync should do. Taken at the start of a poll, handed back with
/// `finish` at the end.
#[derive(Debug, Clone)]
pub struct Plan {
    pub mode: Mode,
    pub flags: BTreeSet<Flag>,
    generation: u64,
}

#[derive(Default)]
struct Live {
    connected: bool,
    /// Bumped on every new connection.
    generation: u64,
    /// Generation a catch-up last completed for.
    caught_up: u64,
    flags: BTreeSet<Flag>,
}

static LIVE: LazyLock<Mutex<HashMap<i64, Live>>> = LazyLock::new(Default::default);

fn with<R>(source_id: i64, f: impl FnOnce(&mut Live) -> R) -> R {
    let mut map = LIVE.lock().unwrap_or_else(|e| e.into_inner());
    f(map.entry(source_id).or_default())
}

/// Take this source's flags and decide the mode.
pub fn plan(source_id: i64) -> Plan {
    with(source_id, |l| {
        let mode = match (l.connected, l.caught_up == l.generation) {
            (false, _) => Mode::Poll,
            (true, false) => Mode::CatchUp,
            (true, true) => Mode::Live,
        };
        Plan { mode, flags: std::mem::take(&mut l.flags), generation: l.generation }
    })
}

/// End of a poll. On success a catch-up counts for the connection it started
/// under (a reconnect meanwhile needs another one); on failure the flags go
/// back so the next sync retries them.
pub fn finish(source_id: i64, plan: &Plan, ok: bool) {
    with(source_id, |l| {
        if ok {
            if plan.mode == Mode::CatchUp {
                l.caught_up = plan.generation;
            }
        } else {
            l.flags.extend(plan.flags.iter().cloned());
        }
    })
}

fn set_connected(source_id: i64, connected: bool) {
    with(source_id, |l| {
        if connected && !l.connected {
            l.generation += 1;
        }
        l.connected = connected;
    })
}

fn add_flag(source_id: i64, flag: Flag) {
    with(source_id, |l| {
        l.flags.insert(flag);
    })
}

#[cfg(test)]
pub fn test_set(source_id: i64, connected: bool, caught_up: bool, flags: &[Flag]) {
    with(source_id, |l| {
        l.connected = connected;
        l.generation = 1;
        l.caught_up = if caught_up { 1 } else { 0 };
        l.flags = flags.iter().cloned().collect();
    })
}

// ---------------------------------------------------------------------------
// Connection.

/// Start the live connection for one Slack source in the background.
pub fn start(app: Arc<App>, source_id: i64) {
    let (tx, rx) = mpsc::channel::<()>();
    let sync_app = app.clone();
    std::thread::Builder::new()
        .name(format!("slack-live-sync-{source_id}"))
        .spawn(move || debounced_sync(&sync_app, source_id, rx))
        .ok();
    std::thread::Builder::new()
        .name(format!("slack-socket-{source_id}"))
        .spawn(move || {
            let mut backoff = Duration::from_secs(5);
            loop {
                let exit = connect_and_listen(&app, source_id, &tx);
                // Whatever happened, events may now be missed: next sync polls.
                set_connected(source_id, false);
                match exit {
                    Ok(Exit::Reconnect) => backoff = Duration::from_secs(5),
                    Ok(Exit::Stop(why)) => {
                        eprintln!("slack socket (source {source_id}): {why}; polling only");
                        return;
                    }
                    Err(e) => {
                        eprintln!("slack socket (source {source_id}): {e}; retrying in {}s", backoff.as_secs());
                        std::thread::sleep(backoff);
                        backoff = (backoff * 2).min(Duration::from_secs(300));
                    }
                }
            }
        })
        .ok();
}

enum Exit {
    Reconnect,
    Stop(String),
}

fn connect_and_listen(app: &App, source_id: i64, tx: &mpsc::Sender<()>) -> anyhow::Result<Exit> {
    let source = load_source(&app.db(), source_id)?;
    let cfg = Config::from_json(&source.config);
    let Some(token) = app.platform.secret_get(&cfg.app_token_secret) else {
        return Ok(Exit::Stop(format!("no app-level token in secret '{}'", cfg.app_token_secret)));
    };
    let http = UreqHttp::default();
    let opened = match Client::new(&http, token).post("apps.connections.open", &[]) {
        Ok(v) => v,
        // Wrong or revoked token: retrying won't help.
        Err(e) if ["invalid_auth", "not_authed", "not_allowed_token_type"].iter().any(|c| e.to_string().contains(c)) => {
            return Ok(Exit::Stop(e.to_string()));
        }
        Err(e) => return Err(e),
    };
    let url = opened.get("url").and_then(Value::as_str).ok_or_else(|| anyhow::anyhow!("apps.connections.open: no url"))?;
    let (mut ws, _) = tungstenite::connect(url).map_err(|e| anyhow::anyhow!("socket connect: {e}"))?;
    match ws.get_mut() {
        MaybeTlsStream::Plain(s) => s.set_read_timeout(Some(READ_TIMEOUT))?,
        MaybeTlsStream::Rustls(s) => s.get_mut().set_read_timeout(Some(READ_TIMEOUT))?,
        _ => {}
    }
    // New connection: the next sync is a catch-up for anything missed before it.
    set_connected(source_id, true);
    tx.send(()).ok();
    loop {
        let msg = ws.read().map_err(|e| anyhow::anyhow!("socket read: {e}"))?;
        let text = match msg {
            Message::Text(t) => t.to_string(),
            Message::Close(_) => return Ok(Exit::Reconnect),
            _ => continue, // pings are answered by tungstenite
        };
        let state = State::parse(load_source(&app.db(), source_id)?.cursor.as_deref());
        let action = handle(&text, &state);
        if let Some(ack) = action.ack {
            ws.send(Message::text(ack)).map_err(|e| anyhow::anyhow!("socket ack: {e}"))?;
        }
        if let Some(flag) = action.flag {
            add_flag(source_id, flag);
            tx.send(()).ok();
        }
        if action.reconnect {
            ws.close(None).ok();
            return Ok(Exit::Reconnect);
        }
    }
}

fn debounced_sync(app: &App, source_id: i64, rx: mpsc::Receiver<()>) {
    while rx.recv().is_ok() {
        let first = std::time::Instant::now();
        // Swallow the burst: stop when quiet for DEBOUNCE or after MAX_DELAY.
        while first.elapsed() < MAX_DELAY {
            match rx.recv_timeout(DEBOUNCE) {
                Ok(()) => continue,
                Err(mpsc::RecvTimeoutError::Timeout) => break,
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            }
        }
        if let Err(e) = sync_source(app, source_id) {
            eprintln!("slack live sync (source {source_id}): {e}");
        }
    }
}

#[derive(Debug, Default, PartialEq)]
pub struct Action {
    /// Frame to send back (`{"envelope_id": ...}`).
    pub ack: Option<String>,
    pub flag: Option<Flag>,
    pub reconnect: bool,
}

/// Decide what to do with one frame. Pure, for tests.
pub fn handle(text: &str, state: &State) -> Action {
    let Ok(v) = serde_json::from_str::<Value>(text) else { return Action::default() };
    let kind = v.get("type").and_then(Value::as_str).unwrap_or_default();
    if kind == "disconnect" {
        // Slack asks us to reconnect (refresh_requested, warning, link_disabled).
        return Action { reconnect: true, ..Default::default() };
    }
    let ack = v.get("envelope_id").and_then(Value::as_str).map(|id| json!({ "envelope_id": id }).to_string());
    let flag = if kind == "events_api" { v.pointer("/payload/event").and_then(|e| flag_for(e, state)) } else { None };
    Action { ack, flag, reconnect: false }
}

/// What a message event means for us, if anything.
///
/// - A thread reply matters in a DM or followed channel, in a thread Alethiometer
///   already tracks, when it mentions you, when you wrote it, or when it
///   answers a thread you started (`parent_user_id`). This is what catches
///   replies to old threads live, however old the parent.
/// - A top-level message matters in a DM or followed channel; elsewhere only
///   when it mentions you.
///
/// Edits and deletions (`message_changed`, `message_deleted`) are ignored:
/// items show what arrived, and Slack's own copy stays the source of truth.
fn flag_for(event: &Value, state: &State) -> Option<Flag> {
    let s = |k: &str| event.get(k).and_then(Value::as_str).unwrap_or_default();
    if s("type") != "message" || matches!(s("subtype"), "message_changed" | "message_deleted") {
        return None;
    }
    let (channel, ts, thread_ts) = (s("channel"), s("ts"), s("thread_ts"));
    if channel.is_empty() || ts.is_empty() {
        return None;
    }
    let me = state.me.as_str();
    let dm = matches!(s("channel_type"), "im" | "mpim");
    let followed = state.follow.iter().any(|c| c == channel);
    let mention = !me.is_empty() && s("text").contains(&format!("<@{me}"));
    if !thread_ts.is_empty() && thread_ts != ts {
        let tracked = state.threads.contains_key(&format!("{channel}/{thread_ts}"));
        let mine = !me.is_empty() && (s("user") == me || s("parent_user_id") == me);
        return (dm || followed || tracked || mention || mine).then(|| Flag::Thread(channel.into(), thread_ts.into()));
    }
    if dm || followed {
        return Some(Flag::Conv(channel.into()));
    }
    mention.then(|| Flag::Mention(channel.into(), ts.into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::connectors::slack::poll::ThreadState;

    fn env(event: Value) -> String {
        json!({ "envelope_id": "e-1", "type": "events_api", "accepts_response_payload": false, "payload": { "type": "event_callback", "event": event } }).to_string()
    }

    #[test]
    fn acks_every_envelope_and_flags_only_relevant_events() {
        let mut state = State { me: "U0ME".into(), follow: vec!["C0INC".into()], ..Default::default() };
        state.threads.insert("C0OPS/1700000000.000100".into(), ThreadState::default());
        let flag = |event: Value| handle(&env(event), &state).flag;

        let a = handle(&env(json!({ "type": "message", "channel": "C0INC", "channel_type": "channel", "text": "hi", "ts": "1.1" })), &state);
        assert_eq!(a, Action { ack: Some(r#"{"envelope_id":"e-1"}"#.into()), flag: Some(Flag::Conv("C0INC".into())), reconnect: false });

        let a = handle(&env(json!({ "type": "message", "channel": "C0RANDOM", "channel_type": "channel", "text": "lunch?", "ts": "1.2" })), &state);
        assert_eq!((a.ack.is_some(), a.flag), (true, None));

        assert_eq!(
            flag(json!({ "type": "message", "channel": "C0RANDOM", "channel_type": "channel", "text": "<@U0ME> look", "ts": "1.3" })),
            Some(Flag::Mention("C0RANDOM".into(), "1.3".into()))
        );
        assert_eq!(
            flag(json!({ "type": "message", "channel": "D0PRIYA", "channel_type": "im", "text": "hey", "ts": "1.4" })),
            Some(Flag::Conv("D0PRIYA".into()))
        );
        // Replies: to a months-old thread you started, in a tracked thread, or by you.
        let reply = |ch: &str, extra: Value| {
            let mut e = json!({ "type": "message", "channel": ch, "channel_type": "channel", "text": "ok", "ts": "1790000000.000100", "thread_ts": "1700000000.000100", "user": "U0SAM" });
            e.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
            flag(e)
        };
        let t = |ch: &str| Some(Flag::Thread(ch.into(), "1700000000.000100".into()));
        assert_eq!(reply("C0RANDOM", json!({ "parent_user_id": "U0ME" })), t("C0RANDOM"));
        assert_eq!(reply("C0OPS", json!({})), t("C0OPS"));
        assert_eq!(reply("C0RANDOM", json!({ "user": "U0ME" })), t("C0RANDOM"));
        assert_eq!(reply("C0RANDOM", json!({ "parent_user_id": "U0SAM" })), None);
        // Edits don't count.
        assert_eq!(flag(json!({ "type": "message", "subtype": "message_changed", "channel": "D0PRIYA", "channel_type": "im", "ts": "1.5" })), None);

        assert_eq!(handle(r#"{"type":"hello","num_connections":1}"#, &state), Action::default());
        assert!(handle(r#"{"type":"disconnect","reason":"refresh_requested"}"#, &state).reconnect);
    }

    #[test]
    fn registry_modes_follow_the_connection() {
        let id = 9001;
        assert_eq!(plan(id).mode, Mode::Poll);
        set_connected(id, true);
        add_flag(id, Flag::Conv("D1".into()));
        let p = plan(id);
        assert_eq!((p.mode, p.flags.len()), (Mode::CatchUp, 1));
        // A failed sync puts the flags back.
        finish(id, &p, false);
        let p = plan(id);
        assert_eq!((p.mode, p.flags.len()), (Mode::CatchUp, 1));
        finish(id, &p, true);
        assert_eq!(plan(id).mode, Mode::Live);
        // A drop and reconnect needs a new catch-up.
        set_connected(id, false);
        assert_eq!(plan(id).mode, Mode::Poll);
        set_connected(id, true);
        let p = plan(id);
        assert_eq!(p.mode, Mode::CatchUp);
        // A catch-up that started before another reconnect doesn't count.
        set_connected(id, false);
        set_connected(id, true);
        finish(id, &p, true);
        assert_eq!(plan(id).mode, Mode::CatchUp);
    }
}

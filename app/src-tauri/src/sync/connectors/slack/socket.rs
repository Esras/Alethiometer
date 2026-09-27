//! Slack Socket Mode: a live nudge for the poller, not a second mapper.
//!
//! `apps.connections.open` (app-level `xapp-` token) returns a temporary
//! `wss://` URL. Slack pushes `events_api` envelopes over it; each one is
//! ACKed with its `envelope_id` within 3 seconds. A relevant event (a DM,
//! a message in a followed channel, or a mention) schedules an immediate
//! `sync_source`, debounced so a busy incident channel causes one poll per few
//! seconds rather than one per message. All mapping stays in `poll`.
//!
//! Without an app-level token this thread logs once and exits; polling on
//! `poll_secs` keeps working.

use super::api::Client;
use super::{Config, State};
use crate::state::App;
use crate::sync::{http::UreqHttp, load_source, sync_source};
use serde_json::{json, Value};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;
use tungstenite::{stream::MaybeTlsStream, Message};

/// Wait this long after an event for more before syncing.
const DEBOUNCE: Duration = Duration::from_secs(3);
/// Sync at least this often during a continuous stream of events.
const MAX_DELAY: Duration = Duration::from_secs(15);
/// No frame (Slack pings regularly) for this long means a dead connection.
const READ_TIMEOUT: Duration = Duration::from_secs(120);

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
                match connect_and_listen(&app, source_id, &tx) {
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
    // Sync once on connect to catch anything missed while disconnected.
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
        if action.sync {
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
    pub sync: bool,
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
    let sync = kind == "events_api" && v.pointer("/payload/event").is_some_and(|e| relevant(e, state));
    Action { ack, sync, reconnect: false }
}

/// A DM, a message in a followed channel, or a mention of the user.
fn relevant(event: &Value, state: &State) -> bool {
    let s = |k: &str| event.get(k).and_then(Value::as_str).unwrap_or_default();
    if s("type") != "message" {
        return false;
    }
    if matches!(s("channel_type"), "im" | "mpim") {
        return true;
    }
    if state.follow.iter().any(|c| c == s("channel")) {
        return true;
    }
    !state.me.is_empty() && s("text").contains(&format!("<@{}", state.me))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acks_every_envelope_and_syncs_only_relevant_events() {
        let state = State { me: "U0ME".into(), follow: vec!["C0INC".into()], ..Default::default() };
        let env = |event: Value| json!({ "envelope_id": "e-1", "type": "events_api", "accepts_response_payload": false, "payload": { "type": "event_callback", "event": event } }).to_string();

        let a = handle(&env(json!({ "type": "message", "channel": "C0INC", "channel_type": "channel", "text": "hi" })), &state);
        assert_eq!(a, Action { ack: Some(r#"{"envelope_id":"e-1"}"#.into()), sync: true, reconnect: false });

        let a = handle(&env(json!({ "type": "message", "channel": "C0RANDOM", "channel_type": "channel", "text": "lunch?" })), &state);
        assert_eq!((a.ack.is_some(), a.sync), (true, false));

        let a = handle(&env(json!({ "type": "message", "channel": "C0RANDOM", "channel_type": "channel", "text": "<@U0ME> look" })), &state);
        assert!(a.sync);

        let a = handle(&env(json!({ "type": "message", "channel": "D0PRIYA", "channel_type": "im", "text": "hey" })), &state);
        assert!(a.sync);

        assert_eq!(handle(r#"{"type":"hello","num_connections":1}"#, &state), Action::default());
        assert!(handle(r#"{"type":"disconnect","reason":"refresh_requested"}"#, &state).reconnect);
    }
}

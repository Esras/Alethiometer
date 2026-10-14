//! Slack connector: DMs, group DMs, followed channels, threads you're in and
//! mentions, as one item per unread run (see `poll`). Writeback: mark read,
//! reply, react.
//!
//! Auth is a Slack app the user creates for themselves (see
//! `docs/sources/slack.md`): a user token (`xoxp-`) for the Web API and an
//! app-level token (`xapp-`) for the live Socket Mode connection (`socket`),
//! which is the primary feed. Without it Alethiometer falls back to polling.

pub mod api;
mod poll;
pub mod socket;
#[cfg(test)]
mod tests;

use crate::domain::ports::Reads;
use crate::sync::{Batch, Connector, Ctx};
use api::Client;
use serde_json::{json, Value};

pub use poll::State;

/// A Slack web permalink as a slack:// link for the desktop app, when a Slack
/// source knows that workspace's team id. `message` and `thread_ts` aren't in
/// Slack's documented scheme; without them it still opens the channel.
pub fn deep_link(r: &dyn Reads, url: &str) -> Option<String> {
    let re = regex::Regex::new(r"^(https://[^/]+)/archives/([A-Z0-9]+)/p(\d{7,})(?:\?(.*))?$").expect("static regex");
    let cap = re.captures(url)?;
    let (base, channel, p) = (&cap[1], &cap[2], &cap[3]);
    let team = r
        .sources()
        .of_kind("slack")
        .ok()?
        .into_iter()
        .filter_map(|s| s.cursor)
        .map(|c| State::parse(Some(&c)))
        .find(|s| s.team_url.trim_end_matches('/') == base && !s.team_id.is_empty())?
        .team_id;
    let (secs, micros) = p.split_at(p.len() - 6);
    let mut link = format!("slack://channel?team={team}&id={channel}&message={secs}.{micros}");
    if let Some(t) = cap
        .get(4)
        .and_then(|q| q.as_str().split('&').find_map(|kv| kv.strip_prefix("thread_ts=")))
    {
        link.push_str(&format!("&thread_ts={t}"));
    }
    Some(link)
}

pub struct Slack;

/// Channels you're a member of, as (id, name, private), for Settings → Review….
pub fn member_channels(
    http: &dyn crate::sync::http::Http,
    token: String,
) -> anyhow::Result<Vec<(String, String, bool)>> {
    let client = api::Client::new(http, token);
    let raw = client.get_paged(
        "users.conversations",
        &[
            ("types", "public_channel,private_channel"),
            ("exclude_archived", "true"),
            ("limit", "200"),
        ],
        "channels",
        20,
    )?;
    let mut out: Vec<(String, String, bool)> = raw
        .iter()
        .filter_map(|c| {
            let s = |k: &str| c.get(k).and_then(Value::as_str).map(str::to_owned);
            Some((
                s("id")?,
                s("name")?,
                c.get("is_private").and_then(Value::as_bool).unwrap_or(false),
            ))
        })
        .collect();
    out.sort_by(|a, b| a.1.cmp(&b.1));
    Ok(out)
}

pub use crate::settings::sources::{SLACK_APP_TOKEN as APP_TOKEN, SLACK_USER_TOKEN as USER_TOKEN};

/// `source.config` for kind "slack".
#[derive(Debug, Clone)]
pub struct Config {
    /// Followed channels: ids ("C0123ABCD") or names ("#inc-2291").
    pub channels: Vec<String>,
    /// `"channels": "all"`: every channel you're a member of.
    pub all_channels: bool,
    /// Muted channels, ids/names, or a DM's other user id/an Mpim's name: still read, never new.
    pub muted: Vec<String>,
    /// Poll DMs and group DMs.
    pub dms: bool,
    /// Mentions in channels you don't follow (search; needs `search:read`).
    pub mentions: bool,
    /// How far back the first sync reads, and the most a catch-up reads.
    pub lookback_hours: i64,
    /// DMs and group DMs checked per catch-up, most recently active first.
    /// Without the socket this is every poll; DMs beyond it are missed.
    pub max_dms: usize,
    /// How long a thread you're in is tracked after its last activity.
    pub thread_days: i64,
    /// At most this many tracked threads (most recently active kept).
    pub max_threads: usize,
    /// While the socket stays connected, run a full catch-up this often anyway:
    /// Socket Mode delivery is best-effort (unacked events are retried a few
    /// times, then dropped), so events can be lost without a disconnect.
    pub catchup_hours: i64,
    pub user_token_secret: String,
    pub app_token_secret: String,
    /// Sleep used for Retry-After; replaced in tests.
    pub sleep: fn(std::time::Duration),
}

impl Config {
    pub fn is_muted(&self, id: &str, name: &str) -> bool {
        self.muted.iter().any(|m| m == id || m == name)
    }

    pub fn from_json(v: &Value) -> Config {
        let b = |k: &str, d: bool| v.get(k).and_then(Value::as_bool).unwrap_or(d);
        let i = |k: &str, d: i64| v.get(k).and_then(Value::as_i64).filter(|n| *n > 0).unwrap_or(d);
        let s = |k: &str, d: &str| v.get(k).and_then(Value::as_str).unwrap_or(d).to_owned();
        Config {
            channels: v
                .get("channels")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(Value::as_str).map(str::to_owned).collect())
                .unwrap_or_default(),
            all_channels: v.get("channels").and_then(Value::as_str) == Some("all"),
            muted: v
                .get("muted")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(Value::as_str)
                        .map(|c| c.trim().trim_start_matches('#').to_owned())
                        .collect()
                })
                .unwrap_or_default(),
            dms: b("dms", true),
            mentions: b("mentions", true),
            lookback_hours: i("lookback_hours", 72),
            max_dms: i("max_dms", 50) as usize,
            thread_days: i("thread_days", 90),
            max_threads: i("max_threads", 300) as usize,
            catchup_hours: i("catchup_hours", 3),
            user_token_secret: s("user_token_secret", USER_TOKEN),
            app_token_secret: s("app_token_secret", APP_TOKEN),
            sleep: std::thread::sleep,
        }
    }
}

impl Connector for Slack {
    fn poll(&self, ctx: &Ctx) -> anyhow::Result<Batch> {
        poll::poll(ctx, &Config::from_json(&ctx.source.config))
    }

    fn poll_known(&self, ctx: &Ctx, streams: &mut crate::sync::Polling) -> anyhow::Result<Batch> {
        poll::poll_with(ctx, &Config::from_json(&ctx.source.config), Some(streams))
    }

    fn send(&self, ctx: &Ctx, op: &str, payload: &Value) -> anyhow::Result<Value> {
        send(ctx, &Config::from_json(&ctx.source.config), op, payload)
    }

    fn before_poll(&self, r: &dyn Reads, source: &crate::sync::Source) -> crate::error::Result<Option<String>> {
        super::chat::close_triaged::<State>(r, source)
    }

    fn live(&self, source_id: i64) -> Option<bool> {
        socket::connected(source_id)
    }

    fn deep_link(&self, r: &dyn Reads, url: &str) -> Option<String> {
        deep_link(r, url)
    }
}

/// Outbox ops. Each returns only after Slack answered `ok: true`.
///
/// - `slack.mark_read` `{channel, ts}`: conversations.mark, for an item's
///   `raw_json.channel` / `raw_json.mark_ts` (top-level runs and mentions).
///   Never moves the read marker backwards: `conversations.mark` sets the
///   marker to exactly `ts`, earlier or not, so it first reads the current
///   marker (`conversations.info`) and, when Slack is already read up to `ts`
///   or later, confirms without marking. The check lives here rather than at
///   enqueue time because only Slack knows the marker *now*: the cursor's copy
///   is as old as the last poll, and you may have read on in Slack since, or
///   the op may sit in the outbox for a while. With a `thread_ts` it fails:
///   Slack's public API has no way to mark a thread read (thread items carry no
///   `mark_ts`, so Incoming never queues one).
/// - `slack.reply` `{channel, thread_ts?, text}`: chat.postMessage as the user,
///   in the thread when `thread_ts` is given, else in the conversation itself.
///   `text` is exactly what the user typed; nothing here writes words for them.
/// - `slack.react` `{channel, ts, name}`: reactions.add (`name` without colons).
fn send(ctx: &Ctx, cfg: &Config, op: &str, payload: &Value) -> anyhow::Result<Value> {
    let field = |k: &str| -> anyhow::Result<String> {
        payload
            .get(k)
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty())
            .map(str::to_owned)
            .ok_or_else(|| anyhow::anyhow!("{op}: payload needs '{k}'"))
    };
    let mut client = Client::new(ctx.http, ctx.require_secret(&cfg.user_token_secret)?);
    client.sleep = cfg.sleep;
    match op {
        "slack.mark_read" => {
            if payload
                .get("thread_ts")
                .and_then(Value::as_str)
                .is_some_and(|t| !t.is_empty())
            {
                anyhow::bail!("Slack has no API to mark a thread read");
            }
            let (channel, ts) = (field("channel")?, field("ts")?);
            let info = client.get("conversations.info", &[("channel", &channel)])?;
            if let Some(lr) = info.pointer("/channel/last_read").and_then(Value::as_str) {
                if !api::ts_after(&ts, lr) {
                    // Already read this far in Slack: done, and don't pull it back.
                    return Ok(json!({ "channel": channel, "ts": ts, "already": lr }));
                }
            }
            client.post("conversations.mark", &[("channel", &channel), ("ts", &ts)])?;
            Ok(json!({ "channel": channel, "ts": ts }))
        }
        "slack.reply" => {
            let (channel, text) = (field("channel")?, field("text")?);
            let thread_ts = field("thread_ts").ok();
            let mut form = vec![("channel", channel.as_str())];
            if let Some(t) = &thread_ts {
                form.push(("thread_ts", t.as_str()));
            }
            form.push(("text", text.as_str()));
            let v = client.post("chat.postMessage", &form)?;
            let ts = v.get("ts").and_then(Value::as_str).unwrap_or_default();
            Ok(json!({ "channel": v.get("channel").cloned().unwrap_or(json!(channel)), "ts": ts }))
        }
        "slack.react" => {
            let (channel, ts, name) = (field("channel")?, field("ts")?, field("name")?);
            let name = name.trim_matches(':').to_owned();
            match client.post(
                "reactions.add",
                &[("channel", &channel), ("timestamp", &ts), ("name", &name)],
            ) {
                Ok(_) => Ok(json!({ "channel": channel, "ts": ts, "name": name })),
                // The remote state is what was asked for; confirm rather than fail.
                Err(e) if e.to_string().ends_with(": already_reacted") => {
                    Ok(json!({ "channel": channel, "ts": ts, "name": name, "already": true }))
                }
                Err(e) => Err(e),
            }
        }
        _ => anyhow::bail!("Slack doesn't support '{op}'"),
    }
}

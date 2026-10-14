//! Microsoft Teams connector: your chats and, once an admin has approved
//! `ChannelMessage.Read.All`, the channels you list, through Microsoft Graph,
//! signed in as you (`oauth`; setup in `docs/sources/teams.md`). A chat
//! source like Slack (`docs/chat.md`): one item per unread run, threads in a
//! channel attach to their post. Graph pushes changes only to a public HTTPS
//! endpoint, so this connector polls (user, 2026-10-14; Event Hubs push is a
//! later stage, ISSUES.md). Writeback: mark a chat read, reply, react.

pub mod api;
pub mod oauth;
mod poll;
#[cfg(test)]
mod tests;

use crate::domain::ports::Reads;
use crate::sync::{Batch, Connector, Ctx};
use api::{enc, Client};
use serde_json::{json, Value};

pub use oauth::call as auth_call;
pub use poll::State;

pub struct Teams;

/// `source.config` for kind "teams" (`settings::sources::TeamsConfig` is its schema).
#[derive(Debug, Clone)]
pub struct Config {
    pub client_id: String,
    pub tenant: String,
    pub secret_prefix: String,
    pub chats: bool,
    /// As written in settings: links or "<team id>|<channel id>".
    pub channels: Vec<String>,
    pub muted: Vec<String>,
    pub lookback_hours: i64,
    pub max_chats: usize,
    pub thread_days: i64,
    pub max_threads: usize,
    pub sleep: fn(std::time::Duration),
}

impl Config {
    pub fn from_json(v: &Value) -> Config {
        let d = crate::settings::sources::TeamsConfig::default();
        let b = |k: &str, d: bool| v.get(k).and_then(Value::as_bool).unwrap_or(d);
        let i = |k: &str, d: i64| v.get(k).and_then(Value::as_i64).filter(|n| *n > 0).unwrap_or(d);
        let s = |k: &str, d: &str| {
            v.get(k)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or(d)
                .to_owned()
        };
        let list = |k: &str| -> Vec<String> {
            v.get(k)
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(Value::as_str)
                        .map(|c| c.trim().to_owned())
                        .filter(|c| !c.is_empty())
                        .collect()
                })
                .unwrap_or_default()
        };
        Config {
            client_id: s("client_id", &d.client_id),
            tenant: s("tenant", &d.tenant),
            secret_prefix: s("secret_prefix", &d.secret_prefix),
            chats: b("chats", d.chats),
            channels: list("channels"),
            muted: list("muted"),
            lookback_hours: i("lookback_hours", d.lookback_hours),
            max_chats: i("max_chats", d.max_chats).min(200) as usize,
            thread_days: i("thread_days", d.thread_days),
            max_threads: i("max_threads", d.max_threads) as usize,
            sleep: std::thread::sleep,
        }
    }

    pub fn is_muted(&self, ids: &[&str]) -> bool {
        self.muted.iter().any(|m| ids.contains(&m.as_str()))
    }

    /// The listed channels as (team id, channel id); an entry that's neither
    /// form is an error, so a typo shows instead of reading nothing.
    pub fn channel_ids(&self) -> anyhow::Result<Vec<(String, String)>> {
        self.channels.iter().map(|c| channel_id(c)).collect()
    }
}

/// "<team id>|<channel id>", or a Teams "Get link to channel" link:
/// `https://teams.microsoft.com/l/channel/<channel id, encoded>/<name>?groupId=<team id>&tenantId=…`.
pub fn channel_id(entry: &str) -> anyhow::Result<(String, String)> {
    if let Some((team, channel)) = entry.split_once('|') {
        if !team.is_empty() && !channel.is_empty() {
            return Ok((team.trim().to_owned(), channel.trim().to_owned()));
        }
    }
    let bad = || {
        anyhow::anyhow!(
            "Teams channel '{entry}': paste the channel's link (… › Get link to channel) or write <team id>|<channel id>"
        )
    };
    let (path, query) = entry.split_once('?').ok_or_else(bad)?;
    let encoded = path
        .split("/l/channel/")
        .nth(1)
        .and_then(|p| p.split('/').next())
        .ok_or_else(bad)?;
    let team = query
        .split('&')
        .find_map(|kv| kv.strip_prefix("groupId="))
        .filter(|t| !t.is_empty())
        .ok_or_else(bad)?;
    Ok((super::oauth::pct_decode(team), super::oauth::pct_decode(encoded)))
}

/// A channel conversation's id in items and payloads ("<team>|<channel>");
/// a chat's is its own id.
pub fn split_conv(conv: &str) -> Option<(&str, &str)> {
    conv.split_once('|')
}

impl Connector for Teams {
    fn poll(&self, ctx: &Ctx) -> anyhow::Result<Batch> {
        poll::poll_with(ctx, &Config::from_json(&ctx.source.config), None)
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
}

/// A client with a fresh token; a 401 drops the cached token so the next call refreshes.
fn client<'a>(ctx: &'a Ctx, cfg: &Config) -> anyhow::Result<Client<'a>> {
    let mut c = Client::new(ctx.http, oauth::access_token(ctx, cfg)?);
    c.sleep = cfg.sleep;
    Ok(c)
}

/// Outbox ops (`docs/chat.md`). Each returns once Graph answered 2xx.
fn send(ctx: &Ctx, cfg: &Config, op: &str, payload: &Value) -> anyhow::Result<Value> {
    let p = |k: &str| {
        payload
            .get(k)
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| anyhow::anyhow!("{op} needs '{k}'"))
    };
    let opt = |k: &str| payload.get(k).and_then(Value::as_str).filter(|s| !s.is_empty());
    let c = client(ctx, cfg)?;
    let result = (|| match op {
        "teams.mark_read" => {
            let chat = p("channel")?;
            let ts = p("ts")?;
            if split_conv(chat).is_some() {
                anyhow::bail!("Teams has no read marker for channels");
            }
            let state = State::parse(ctx.source.cursor.as_deref());
            if state.me.is_empty() || state.tenant.is_empty() {
                anyhow::bail!("not synced yet (your user and tenant ids come from the first poll)");
            }
            let info = c.get(&format!("/chats/{}?$expand=lastMessagePreview", enc(chat)))?;
            // markChatReadForUser reads the whole chat: never past a message you haven't seen.
            if let Some(newest) = info.pointer("/lastMessagePreview/id").and_then(Value::as_str) {
                if api::key_after(newest, ts) {
                    return Ok(json!({ "not_marked": "newer messages in the chat", "newest": newest }));
                }
            }
            let read_at = info
                .pointer("/viewpoint/lastMessageReadDateTime")
                .and_then(Value::as_str)
                .and_then(crate::time::Ts::parse);
            let ts_at = ts.parse::<i64>().ok().and_then(crate::time::Ts::from_unix_ms);
            if let (Some(r), Some(t)) = (read_at, ts_at) {
                if r >= t {
                    return Ok(json!({ "already": r.to_db() }));
                }
            }
            c.post(
                &format!("/chats/{}/markChatReadForUser", enc(chat)),
                &json!({ "user": { "id": state.me, "tenantId": state.tenant } }),
            )?;
            Ok(json!({ "ok": true }))
        }
        "teams.reply" => {
            let conv = p("channel")?;
            let body = json!({ "body": { "contentType": "html", "content": html_text(p("text")?) } });
            let made = match (split_conv(conv), opt("thread_ts")) {
                (None, None) => c.post(&format!("/chats/{}/messages", enc(conv)), &body)?,
                (None, Some(_)) => anyhow::bail!("Teams chats have no threads: reply in the chat (⌥T)"),
                (Some((team, channel)), None) => c.post(
                    &format!("/teams/{}/channels/{}/messages", enc(team), enc(channel)),
                    &body,
                )?,
                (Some((team, channel)), Some(root)) => c.post(
                    &format!(
                        "/teams/{}/channels/{}/messages/{}/replies",
                        enc(team),
                        enc(channel),
                        enc(root)
                    ),
                    &body,
                )?,
            };
            Ok(json!({ "id": made.get("id").cloned().unwrap_or(Value::Null) }))
        }
        "teams.react" => {
            let conv = p("channel")?;
            let ts = p("ts")?;
            let name = p("name")?;
            let path = match (split_conv(conv), opt("thread_ts")) {
                (None, _) => format!("/chats/{}/messages/{}", enc(conv), enc(ts)),
                (Some((team, channel)), Some(root)) if root != ts => format!(
                    "/teams/{}/channels/{}/messages/{}/replies/{}",
                    enc(team),
                    enc(channel),
                    enc(root),
                    enc(ts)
                ),
                (Some((team, channel)), _) => {
                    format!("/teams/{}/channels/{}/messages/{}", enc(team), enc(channel), enc(ts))
                }
            };
            c.post(&format!("{path}/setReaction"), &json!({ "reactionType": name }))?;
            Ok(json!({ "ok": true }))
        }
        _ => anyhow::bail!("Teams doesn't support '{op}'"),
    })();
    if let Err(e) = &result {
        if e.downcast_ref::<api::GraphError>().is_some_and(|g| g.is_fatal()) {
            oauth::invalidate(ctx, cfg);
        }
    }
    result
}

/// What you typed, as Teams HTML: escaped, line breaks kept.
pub fn html_text(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\n', "<br>")
}

//! Polling: turn Slack conversations into windows.
//!
//! A *window* is a run of messages in one channel, DM or thread with no gap
//! longer than `gap_min` between consecutive messages. Each window is one item
//! ("#inc-2291 · 42 new · 09:05–10:40"); its stored messages are the window's
//! messages, so expanding it in Incoming shows what happened since the channel
//! last went quiet. When new messages continue an open window, the same item is
//! updated and bumped back to unseen. After a quiet gap a new window (a new
//! item) starts.
//!
//! The item's external id is `<channel>/<window start ts>` for channel and DM
//! windows and `<channel>/<thread ts>/<window start ts>` for thread windows, so
//! it is stable while the window grows.
//!
//! The source cursor is a JSON `State`: per-conversation and per-thread last
//! seen ts plus the open window's start. No message text is kept there.
//!
//! Save only what's needed: the first sync reads `lookback_hours` back, never
//! the full archive, and each item keeps at most `MAX_MSGS` messages.

use super::api::{ts_after, ts_secs, Client};
use super::Config;
use crate::sync::{local_from_unix, Batch, Ctx, Handle, ItemUpsert, Message};
use chrono::TimeZone;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// Messages kept per window item; `msg_count` still counts all of them.
pub const MAX_MSGS: usize = 50;
/// Pages of 200 read per conversation per poll.
const HISTORY_PAGES: usize = 10;
const REPLIES_PAGES: usize = 5;

/// Subtypes that are real content. Everything else (joins, topic changes,
/// pins…) is noise. `thread_broadcast` is left out on purpose: the reply shows
/// up in its thread window.
const CONTENT_SUBTYPES: &[&str] = &["bot_message", "file_share", "me_message"];

/// Stored as the source cursor.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct State {
    /// The token owner's user id and handle (from auth.test).
    #[serde(default)]
    pub me: String,
    #[serde(default)]
    pub me_name: String,
    /// "https://acme.slack.com/", for permalinks.
    #[serde(default)]
    pub team_url: String,
    /// Resolved ids of followed channels (the live socket filters on these).
    #[serde(default)]
    pub follow: Vec<String>,
    /// Key: channel id, or "<channel>/<thread ts>" for a watched thread.
    #[serde(default)]
    pub streams: BTreeMap<String, Stream>,
    /// Newest mention already turned into an item.
    #[serde(default)]
    pub mention_ts: Option<String>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct Stream {
    /// Newest message ts seen.
    pub ts: String,
    /// Start ts of the newest window; it grows while messages keep coming.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run: Option<String>,
    /// Threads only: first words of the parent message, for the title.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

impl State {
    pub fn parse(cursor: Option<&str>) -> State {
        cursor.and_then(|c| serde_json::from_str(c).ok()).unwrap_or_default()
    }
}

#[derive(Debug, Clone)]
enum ConvKind {
    Channel { name: String },
    Im { user: String },
    Mpim { name: String },
}

#[derive(Debug, Clone)]
struct Conv {
    id: String,
    kind: ConvKind,
}

/// One message as read from the API.
#[derive(Debug, Clone)]
struct Msg {
    ts: String,
    user: Option<String>,
    bot_name: Option<String>,
    text: String,
    thread_ts: Option<String>,
    reply_count: i64,
    latest_reply: Option<String>,
}

impl Msg {
    fn parse(v: &Value) -> Option<Msg> {
        let s = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_owned);
        if let Some(sub) = v.get("subtype").and_then(Value::as_str) {
            if !CONTENT_SUBTYPES.contains(&sub) {
                return None;
            }
        }
        let mut text = s("text").unwrap_or_default();
        if text.trim().is_empty() {
            if let Some(files) = v.get("files").and_then(Value::as_array) {
                let names: Vec<&str> = files.iter().filter_map(|f| f.get("name").and_then(Value::as_str)).collect();
                text = format!("[file] {}", names.join(", "));
            }
        }
        Some(Msg {
            ts: s("ts")?,
            user: s("user").filter(|u| !u.is_empty()),
            bot_name: v.pointer("/bot_profile/name").and_then(Value::as_str).map(str::to_owned).or_else(|| s("username")),
            text,
            thread_ts: s("thread_ts"),
            reply_count: v.get("reply_count").and_then(Value::as_i64).unwrap_or(0),
            latest_reply: s("latest_reply"),
        })
    }

    fn is_reply(&self) -> bool {
        matches!(&self.thread_ts, Some(t) if t != &self.ts)
    }
}

pub struct Poller<'a> {
    client: Client<'a>,
    cfg: &'a Config,
    state: State,
    now: f64,
    users: HashMap<String, String>,
    channel_names: HashMap<String, String>,
    items: Vec<ItemUpsert>,
}

pub fn poll(ctx: &Ctx, cfg: &Config) -> anyhow::Result<Batch> {
    let token = ctx.require_secret(&cfg.user_token_secret)?;
    let mut client = Client::new(ctx.http, token);
    client.sleep = cfg.sleep;
    let now = chrono::Local
        .from_local_datetime(&ctx.now)
        .earliest()
        .map(|t| t.timestamp() as f64)
        .unwrap_or_else(|| ctx.now.and_utc().timestamp() as f64);
    let mut p = Poller {
        client,
        cfg,
        state: State::parse(ctx.source.cursor.as_deref()),
        now,
        users: HashMap::new(),
        channel_names: HashMap::new(),
        items: vec![],
    };
    p.run()?;
    Ok(Batch { items: p.items, cursor: Some(serde_json::to_string(&p.state)?), ..Default::default() })
}

impl Poller<'_> {
    fn run(&mut self) -> anyhow::Result<()> {
        if self.state.me.is_empty() || self.state.team_url.is_empty() {
            let v = self.client.get("auth.test", &[])?;
            let s = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or_default().to_owned();
            self.state.me = s("user_id");
            self.state.me_name = s("user");
            self.state.team_url = s("url");
        }
        let convs = self.conversations()?;
        let follow = self.resolve_follow(&convs)?;
        self.state.follow = follow.iter().map(|c| c.id.clone()).collect();

        let mut targets = follow;
        if self.cfg.dms {
            targets.extend(convs.iter().filter(|c| !matches!(c.kind, ConvKind::Channel { .. })).cloned());
        }
        let covered: BTreeSet<String> = targets.iter().map(|c| c.id.clone()).collect();
        for c in &targets {
            self.poll_conv(c)?;
        }
        // Forget watched threads of conversations we no longer follow.
        self.state.streams.retain(|k, _| covered.contains(k.split('/').next().unwrap_or_default()));
        if self.cfg.mentions {
            self.mentions(&covered)?;
        }
        Ok(())
    }

    /// Everything the user is a member of: channels always, DMs when enabled.
    fn conversations(&mut self) -> anyhow::Result<Vec<Conv>> {
        let types = if self.cfg.dms { "public_channel,private_channel,im,mpim" } else { "public_channel,private_channel" };
        let raw = self.client.get_paged(
            "users.conversations",
            &[("types", types), ("exclude_archived", "true"), ("limit", "200")],
            "channels",
            20,
        )?;
        let mut out = vec![];
        for c in raw {
            let s = |k: &str| c.get(k).and_then(Value::as_str).unwrap_or_default().to_owned();
            let b = |k: &str| c.get(k).and_then(Value::as_bool).unwrap_or(false);
            let kind = if b("is_im") {
                if b("is_user_deleted") {
                    continue;
                }
                ConvKind::Im { user: s("user") }
            } else if b("is_mpim") {
                ConvKind::Mpim { name: s("name") }
            } else {
                self.channel_names.insert(s("id"), s("name"));
                ConvKind::Channel { name: s("name") }
            };
            out.push(Conv { id: s("id"), kind });
        }
        Ok(out)
    }

    /// `config.channels` entries are ids ("C0123ABCD") or names ("#inc-2291").
    fn resolve_follow(&mut self, convs: &[Conv]) -> anyhow::Result<Vec<Conv>> {
        let mut out: Vec<Conv> = vec![];
        for entry in &self.cfg.channels {
            let want = entry.trim().trim_start_matches('#');
            let found = convs.iter().find(|c| match &c.kind {
                ConvKind::Channel { name } => c.id == want || name == want,
                _ => false,
            });
            let conv = match found {
                Some(c) => c.clone(),
                None if looks_like_id(want) => {
                    // A public channel you haven't joined: readable, but not listed.
                    let v = self.client.get("conversations.info", &[("channel", want)])?;
                    let name = v.pointer("/channel/name").and_then(Value::as_str).unwrap_or(want).to_owned();
                    self.channel_names.insert(want.to_owned(), name.clone());
                    Conv { id: want.to_owned(), kind: ConvKind::Channel { name } }
                }
                None => anyhow::bail!("Slack: followed channel '{entry}' not found among channels you're in; use its id or join it"),
            };
            if !out.iter().any(|c| c.id == conv.id) {
                out.push(conv);
            }
        }
        Ok(out)
    }

    fn floor(&self) -> f64 {
        self.now - self.cfg.lookback_hours as f64 * 3600.0
    }

    fn gap(&self) -> f64 {
        self.cfg.gap_min as f64 * 60.0
    }

    /// Where to start reading a stream: early enough to rebuild its open
    /// window (if it may still grow) and, for channels, to see thread activity
    /// on recent parents; never before the lookback floor unless the open
    /// window started earlier.
    fn oldest(&self, stream: Option<&Stream>, watch_threads: bool) -> String {
        let floor = self.floor();
        let mut oldest = match stream {
            None => floor,
            Some(s) => ts_secs(&s.ts).max(floor),
        };
        if watch_threads {
            oldest = oldest.min((self.now - self.cfg.thread_hours as f64 * 3600.0).max(floor));
        }
        if let Some(s) = stream {
            if let Some(run) = &s.run {
                if self.now - ts_secs(&s.ts) <= self.gap() {
                    oldest = oldest.min(ts_secs(run));
                }
            }
        }
        // Slack ts format; `inclusive` makes the boundary message count.
        format!("{oldest:.6}")
    }

    fn poll_conv(&mut self, conv: &Conv) -> anyhow::Result<()> {
        let key = conv.id.clone();
        let prev = self.state.streams.get(&key).cloned();
        let oldest = self.oldest(prev.as_ref(), true);
        let raw = self.client.get_paged(
            "conversations.history",
            &[("channel", &conv.id), ("oldest", &oldest), ("inclusive", "true"), ("limit", "200")],
            "messages",
            HISTORY_PAGES,
        )?;
        let mut msgs: Vec<Msg> = raw.iter().filter_map(Msg::parse).filter(|m| !m.is_reply()).collect();
        msgs.sort_by(|a, b| ts_secs(&a.ts).total_cmp(&ts_secs(&b.ts)));

        if let Some((windows, next)) = group(&msgs, prev.as_ref(), self.gap(), self.floor()) {
            for w in windows {
                self.emit(conv, None, &w.msgs, w.has_new_from_others(&self.state.me))?;
            }
            self.state.streams.insert(key.clone(), next);
        } else if prev.is_none() {
            // Nothing recent: start watching from now.
            self.state.streams.insert(key.clone(), Stream { ts: format!("{:.6}", self.now), run: None, title: None });
        }

        // Threads: parents in view with new replies, plus watched threads whose
        // parent is older than what we just read.
        let mut seen_parents = BTreeSet::new();
        for m in msgs.iter().filter(|m| m.reply_count > 0) {
            seen_parents.insert(m.ts.clone());
            let tkey = format!("{}/{}", conv.id, m.ts);
            let tprev = self.state.streams.get(&tkey);
            let last = m.latest_reply.as_deref().unwrap_or(&m.ts);
            let fresh = match tprev {
                Some(t) => ts_after(last, &t.ts),
                None => ts_secs(last) > self.floor(),
            };
            if fresh {
                self.poll_thread(conv, &m.ts, Some(first_words(&clean_plain(&m.text), 6)))?;
            }
        }
        let prefix = format!("{}/", conv.id);
        let watched: Vec<(String, Stream)> = self
            .state
            .streams
            .range(prefix.clone()..)
            .take_while(|(k, _)| k.starts_with(&prefix))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        for (tkey, t) in watched {
            let thread_ts = tkey[prefix.len()..].to_owned();
            if seen_parents.contains(&thread_ts) {
                continue;
            }
            if self.now - ts_secs(&t.ts) > self.cfg.thread_hours as f64 * 3600.0 {
                self.state.streams.remove(&tkey);
                continue;
            }
            self.poll_thread(conv, &thread_ts, None)?;
        }
        Ok(())
    }

    fn poll_thread(&mut self, conv: &Conv, thread_ts: &str, title: Option<String>) -> anyhow::Result<()> {
        let tkey = format!("{}/{}", conv.id, thread_ts);
        let prev = self.state.streams.get(&tkey).cloned();
        let oldest = self.oldest(prev.as_ref(), false);
        let raw = self.client.get_paged(
            "conversations.replies",
            &[("channel", &conv.id), ("ts", thread_ts), ("oldest", &oldest), ("inclusive", "true"), ("limit", "200")],
            "messages",
            REPLIES_PAGES,
        )?;
        // The parent comes back first; it belongs to the channel window.
        let mut msgs: Vec<Msg> = raw.iter().filter_map(Msg::parse).filter(|m| m.ts != thread_ts).collect();
        msgs.sort_by(|a, b| ts_secs(&a.ts).total_cmp(&ts_secs(&b.ts)));
        let title = title.or_else(|| prev.as_ref().and_then(|p| p.title.clone())).unwrap_or_default();
        if let Some((windows, mut next)) = group(&msgs, prev.as_ref(), self.gap(), self.floor()) {
            next.title = Some(title.clone());
            for w in windows {
                self.emit(conv, Some((thread_ts, &title)), &w.msgs, w.has_new_from_others(&self.state.me))?;
            }
            self.state.streams.insert(tkey, next);
        } else if prev.is_none() {
            let ts = msgs.last().map(|m| m.ts.clone()).unwrap_or_else(|| thread_ts.to_owned());
            self.state.streams.insert(tkey, Stream { ts, run: None, title: Some(title) });
        }
        Ok(())
    }

    fn emit(&mut self, conv: &Conv, thread: Option<(&str, &str)>, msgs: &[Msg], bump: bool) -> anyhow::Result<()> {
        let me = self.state.me.clone();
        // A window with only your own messages isn't a signal.
        if msgs.iter().all(|m| m.user.as_deref() == Some(me.as_str())) {
            return Ok(());
        }
        let (first, last) = (&msgs[0], &msgs[msgs.len() - 1]);
        let base = match &conv.kind {
            ConvKind::Channel { name } => format!("#{name}"),
            ConvKind::Im { user } => format!("DM · {}", self.user_name(user)?),
            ConvKind::Mpim { name } => format!("Group DM · {}", mpim_names(name, &self.state.me_name)),
        };
        let title = match thread {
            Some((_, words)) if !words.is_empty() => format!("{base} · thread: {words}"),
            Some(_) => format!("{base} · thread"),
            None => base,
        };
        let mut messages = vec![];
        for m in &msgs[msgs.len().saturating_sub(MAX_MSGS)..] {
            messages.push(Message {
                at: local_from_unix(ts_secs(&m.ts)).unwrap_or_default(),
                author: self.author_name(m)?,
                text: self.clean(&m.text)?,
            });
        }
        let body = messages.iter().map(|m| format!("{}: {}", m.author, m.text)).collect::<Vec<_>>().join("\n");
        let summary = messages.last().map(|m| truncate(&m.text, 140));
        let author = match msgs.iter().rev().find_map(|m| m.user.as_ref().filter(|u| **u != me)) {
            Some(u) => Some(Handle { handle: u.clone(), display_name: self.user_name(u)? }),
            None => None,
        };
        let (external_id, url, raw) = match thread {
            Some((tts, _)) => (
                format!("{}/{}/{}", conv.id, tts, first.ts),
                self.permalink(&conv.id, &last.ts, Some(tts)),
                json!({ "channel": conv.id, "thread_ts": tts, "first_ts": first.ts, "last_ts": last.ts }),
            ),
            None => (
                format!("{}/{}", conv.id, first.ts),
                self.permalink(&conv.id, &last.ts, None),
                // `mark_ts` is what `slack.mark_read` should send for this window.
                // Threads have none: Slack has no API for a thread's read state.
                json!({ "channel": conv.id, "first_ts": first.ts, "last_ts": last.ts, "mark_ts": last.ts }),
            ),
        };
        self.items.push(ItemUpsert {
            external_id,
            kind: if msgs.len() == 1 { "message" } else { "thread" }.into(),
            title,
            body,
            summary,
            url,
            author,
            occurred_at: local_from_unix(ts_secs(&first.ts)).unwrap_or_default(),
            ended_at: local_from_unix(ts_secs(&last.ts)),
            status: "open".into(),
            msg_count: Some(msgs.len() as i64),
            raw_json: raw,
            messages,
            bump,
            ..Default::default()
        });
        Ok(())
    }

    /// Mentions in channels we don't follow, via search (needs `search:read`).
    fn mentions(&mut self, covered: &BTreeSet<String>) -> anyhow::Result<()> {
        let me = self.state.me.clone();
        if me.is_empty() {
            return Ok(());
        }
        let since = self.state.mention_ts.as_deref().map(ts_secs).unwrap_or(0.0).max(self.floor());
        // `after:` is exclusive and by day: ask from the day before.
        let day = chrono::DateTime::from_timestamp(since as i64 - 86_400, 0)
            .map(|t| t.with_timezone(&chrono::Local).format("%Y-%m-%d").to_string())
            .unwrap_or_default();
        let query = format!("<@{me}> after:{day}");
        let v = self.client.get(
            "search.messages",
            &[("query", &query), ("sort", "timestamp"), ("sort_dir", "desc"), ("count", "50")],
        )?;
        let matches = v.pointer("/messages/matches").and_then(Value::as_array).cloned().unwrap_or_default();
        let tag = format!("<@{me}");
        let mut newest = self.state.mention_ts.clone();
        for m in matches {
            let s = |k: &str| m.get(k).and_then(Value::as_str).unwrap_or_default().to_owned();
            let (ts, user, text) = (s("ts"), s("user"), s("text"));
            let ch = m.pointer("/channel/id").and_then(Value::as_str).unwrap_or_default().to_owned();
            let is_dm = m.pointer("/channel/is_im").and_then(Value::as_bool).unwrap_or(false)
                || m.pointer("/channel/is_mpim").and_then(Value::as_bool).unwrap_or(false);
            // Search is fuzzy; keep only real mentions, once, outside covered conversations.
            if ts.is_empty() || ch.is_empty() || covered.contains(&ch) || is_dm || user == me || !text.contains(&tag) {
                continue;
            }
            if ts_secs(&ts) <= since {
                continue;
            }
            if newest.as_deref().is_none_or(|n| ts_after(&ts, n)) {
                newest = Some(ts.clone());
            }
            let chan = m.pointer("/channel/name").and_then(Value::as_str).unwrap_or(&ch).to_owned();
            let name = if user.is_empty() { s("username") } else { self.user_name(&user)? };
            let clean = self.clean(&text)?;
            let at = local_from_unix(ts_secs(&ts)).unwrap_or_default();
            self.items.push(ItemUpsert {
                external_id: format!("{ch}/mention/{ts}"),
                kind: "message".into(),
                title: format!("#{chan} · mention"),
                body: format!("{name}: {clean}"),
                summary: Some(truncate(&clean, 140)),
                url: Some(s("permalink")).filter(|u| !u.is_empty()),
                author: (!user.is_empty()).then(|| Handle { handle: user.clone(), display_name: name.clone() }),
                occurred_at: at.clone(),
                status: "open".into(),
                msg_count: Some(1),
                raw_json: json!({ "channel": ch, "first_ts": ts, "last_ts": ts }),
                messages: vec![Message { at, author: name, text: clean }],
                bump: true,
                ..Default::default()
            });
        }
        self.state.mention_ts = newest;
        Ok(())
    }

    /// Display name for a user id, one `users.info` call per user per poll.
    fn user_name(&mut self, id: &str) -> anyhow::Result<String> {
        if let Some(n) = self.users.get(id) {
            return Ok(n.clone());
        }
        let name = match self.client.get("users.info", &[("user", id)]) {
            Ok(v) => {
                let s = |p: &str| v.pointer(p).and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_owned);
                s("/user/profile/display_name")
                    .or_else(|| s("/user/profile/real_name"))
                    .or_else(|| s("/user/real_name"))
                    .or_else(|| s("/user/name"))
                    .unwrap_or_else(|| id.to_owned())
            }
            // Deleted or foreign users: show the id rather than failing the poll.
            Err(e) if e.to_string().contains("user_not_found") => id.to_owned(),
            Err(e) => return Err(e),
        };
        self.users.insert(id.to_owned(), name.clone());
        Ok(name)
    }

    fn author_name(&mut self, m: &Msg) -> anyhow::Result<String> {
        match (&m.user, &m.bot_name) {
            (Some(u), _) => self.user_name(u),
            (None, Some(b)) => Ok(b.clone()),
            (None, None) => Ok("bot".into()),
        }
    }

    fn permalink(&self, channel: &str, ts: &str, thread_ts: Option<&str>) -> Option<String> {
        if self.state.team_url.is_empty() {
            return None;
        }
        let base = self.state.team_url.trim_end_matches('/');
        let p = ts.replace('.', "");
        Some(match thread_ts {
            Some(t) => format!("{base}/archives/{channel}/p{p}?thread_ts={t}&cid={channel}"),
            None => format!("{base}/archives/{channel}/p{p}"),
        })
    }

    /// Slack mrkdwn to plain text, with user mentions resolved to names.
    fn clean(&mut self, text: &str) -> anyhow::Result<String> {
        let re = regex::Regex::new(r"<([^>]*)>").expect("static regex");
        let mut out = String::new();
        let mut last = 0;
        for cap in re.captures_iter(text) {
            let whole = cap.get(0).expect("match");
            out.push_str(&text[last..whole.start()]);
            last = whole.end();
            let inner = &cap[1];
            let (target, label) = match inner.split_once('|') {
                Some((t, l)) => (t, Some(l)),
                None => (inner, None),
            };
            let rendered = if let Some(uid) = target.strip_prefix('@') {
                format!("@{}", match label {
                    Some(l) => l.to_owned(),
                    None => self.user_name(uid)?,
                })
            } else if let Some(cid) = target.strip_prefix('#') {
                format!("#{}", label.map(str::to_owned).or_else(|| self.channel_names.get(cid).cloned()).unwrap_or_else(|| cid.to_owned()))
            } else if let Some(special) = target.strip_prefix('!') {
                match label {
                    Some(l) => l.to_owned(),
                    None => format!("@{}", special.split('^').next().unwrap_or(special)),
                }
            } else {
                label.unwrap_or(target).to_owned()
            };
            out.push_str(&rendered);
        }
        out.push_str(&text[last..]);
        Ok(unescape(&out))
    }
}

struct Window {
    msgs: Vec<Msg>,
    new_from: usize,
}

impl Window {
    fn has_new_from_others(&self, me: &str) -> bool {
        self.msgs[self.new_from..].iter().any(|m| m.user.as_deref() != Some(me))
    }
}

/// Split a stream's messages (oldest first) into windows that contain new
/// messages, and compute the stream's next state. `None` when nothing is new.
fn group(msgs: &[Msg], prev: Option<&Stream>, gap: f64, floor: f64) -> Option<(Vec<Window>, Stream)> {
    let is_new = |m: &Msg| match prev {
        Some(p) => ts_after(&m.ts, &p.ts),
        None => ts_secs(&m.ts) > floor,
    };
    let first_new = msgs.iter().position(is_new)?;
    // Continue the open window if the first new message is close enough to the last seen one.
    let start = match prev {
        Some(Stream { ts, run: Some(run), .. }) if ts_secs(&msgs[first_new].ts) - ts_secs(ts) <= gap => {
            msgs.iter().position(|m| !ts_after(run, &m.ts)).unwrap_or(first_new).min(first_new)
        }
        _ => first_new,
    };
    let mut windows: Vec<Window> = vec![];
    let mut cur: Vec<Msg> = vec![];
    for m in &msgs[start..] {
        if let Some(prev_m) = cur.last() {
            if ts_secs(&m.ts) - ts_secs(&prev_m.ts) > gap {
                windows.push(Window { msgs: std::mem::take(&mut cur), new_from: 0 });
            }
        }
        cur.push(m.clone());
    }
    windows.push(Window { msgs: cur, new_from: 0 });
    for w in &mut windows {
        w.new_from = w.msgs.iter().position(is_new).unwrap_or(w.msgs.len());
    }
    windows.retain(|w| w.new_from < w.msgs.len());
    let last_w = windows.last()?;
    let next = Stream {
        ts: last_w.msgs.last()?.ts.clone(),
        run: Some(last_w.msgs[0].ts.clone()),
        title: prev.and_then(|p| p.title.clone()),
    };
    Some((windows, next))
}

fn looks_like_id(s: &str) -> bool {
    s.len() >= 9 && (s.starts_with('C') || s.starts_with('G')) && s.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
}

/// "mpdm-alice--bob--me-1" → "alice, bob".
fn mpim_names(name: &str, me: &str) -> String {
    let inner = name.strip_prefix("mpdm-").unwrap_or(name);
    let inner = inner.rsplit_once('-').map(|(a, _)| a).unwrap_or(inner);
    inner.split("--").filter(|h| *h != me).collect::<Vec<_>>().join(", ")
}

/// Plain text without resolving names (for thread titles).
fn clean_plain(text: &str) -> String {
    let re = regex::Regex::new(r"<[^|>]*\|([^>]*)>|<[@#!]?([^>]*)>").expect("static regex");
    unescape(&re.replace_all(text, "$1$2"))
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&amp;", "&")
}

fn first_words(s: &str, n: usize) -> String {
    let words: Vec<&str> = s.split_whitespace().collect();
    let mut out = words.iter().take(n).copied().collect::<Vec<_>>().join(" ");
    if words.len() > n {
        out.push('…');
    }
    out
}

fn truncate(s: &str, n: usize) -> String {
    let one_line = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if one_line.chars().count() <= n {
        one_line
    } else {
        format!("{}…", one_line.chars().take(n - 1).collect::<String>())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(ts: &str, user: &str) -> Msg {
        Msg {
            ts: ts.into(),
            user: Some(user.into()),
            bot_name: None,
            text: "x".into(),
            thread_ts: None,
            reply_count: 0,
            latest_reply: None,
        }
    }

    #[test]
    fn groups_by_gap_and_continues_open_window() {
        let gap = 7200.0;
        let msgs = vec![m("1000.000100", "U1"), m("2000.000100", "U2"), m("20000.000100", "U1")];
        let (w, next) = group(&msgs, None, gap, 0.0).unwrap();
        assert_eq!(w.len(), 2);
        assert_eq!(w[0].msgs.len(), 2);
        assert_eq!(next, Stream { ts: "20000.000100".into(), run: Some("20000.000100".into()), title: None });

        // A new message within the gap continues the window from its start.
        let mut more = msgs.clone();
        more.push(m("21000.000100", "U2"));
        let (w, next) = group(&more, Some(&next), gap, 0.0).unwrap();
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].msgs[0].ts, "20000.000100");
        assert_eq!(w[0].msgs.len(), 2);
        assert_eq!(w[0].new_from, 1);
        assert_eq!(next.run.as_deref(), Some("20000.000100"));

        // Nothing new: nothing to do.
        assert!(group(&more, Some(&next), gap, 0.0).is_none());
    }

    #[test]
    fn cleans_mrkdwn() {
        assert_eq!(clean_plain("Ping <@U1|sam> in <#C1|inc> see <https://x.io|doc> &amp; <!here>"), "Ping sam in inc see doc & here");
        assert_eq!(mpim_names("mpdm-alice--bob--me-1", "me"), "alice, bob");
        assert_eq!(first_words("one two three", 2), "one two…");
        assert_eq!(truncate("abcdef", 4), "abc…");
        assert!(looks_like_id("C0123ABCD") && !looks_like_id("inc-2291"));
    }
}

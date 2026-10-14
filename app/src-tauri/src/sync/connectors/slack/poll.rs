//! Turning Slack into signals. Every message is captured, yours included,
//! whatever Slack's read marker says; only triage in Alethiometer closes a signal.
//!
//! **Units.** A conversation (channel, DM, group DM) and each thread in it are
//! separate *streams*. A stream's *run* is its messages since the last triaged
//! one (`State::closed`), within the lookback. One item holds one stream's run:
//!
//! - top level: `<channel>/<first ts>`, a new item per run. A DM or channel mixes
//!   topics, so the AI assigns topics per message (a split) with the earlier
//!   messages and their topics as context;
//! - thread: `<channel>/thread/<thread ts>`, one item per thread for good: a
//!   thread is one topic, and new replies bring it back (under its topic);
//! - a top-level mention in a channel you don't follow: `<channel>/mention/<ts>`.
//!
//! Until the item is triaged every new message in the stream joins it and bumps
//! it back to new. Triage (the item is seen) closes the run before the next
//! poll (`close_triaged`); the next message starts a new one. Each item also
//! carries context messages, never counted or classified: the few before a
//! top-level run; for a thread, its first message and every earlier reply.
//!
//! **A thread attaches to its parent.** When Alethiometer holds the parent as a
//! message of some item (a top-level run, triaged or not, or the split part it
//! went to), the thread's messages are stored there as the parent's replies and
//! new ones bring that item back (`ItemUpsert::attach_to`, applied in `sync`).
//! The thread item above exists only when the parent was never captured (older
//! than the lookback, a channel you don't follow), or from before this rule.
//!
//! **Per-message ids.** `raw_json.msgs` lists each stored message's ts in the
//! same order as the item's messages, so a later "split by topic" can move a
//! subset of them to another item without re-reading Slack.
//!
//! **What is fetched** depends on `socket::plan` (see `socket`): a live sync
//! reads only what events flagged, plus followed channels you haven't joined
//! (no events for those) and, every `READ_CHECK_SECS`, the streams of open
//! items. A catch-up (startup, reconnect, every `catchup_hours` while
//! connected, or every poll without the socket)
//! checks followed channels, DMs (capped to `max_dms` without the socket),
//! tracked threads and two searches. DMs are only read (`conversations.history`)
//! when `conversations.info` says there is something new.
//!
//! **Threads you're in** are tracked in the cursor for `thread_days` after
//! their last activity (at most `max_threads`), so replies to an old thread are
//! caught however old the parent: live by events, on catch-up by
//! `conversations.replies` from the tracked list, and newly discovered through
//! `search.messages` (mentions of you and your own replies).
//!
//! The cursor (`State`) holds timestamps and titles, never message text.

use super::api::{ts_after, ts_secs, ApiError, Client};
use super::socket::{self, Flag, Mode, Plan};
use super::Config;
use crate::clock::Clock;
use crate::sync::{Batch, Ctx, Handle, ItemUpsert, Message, Polling, Profile, StreamOutcome};
use crate::time::Ts;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// Earlier replies sent as a thread's context. A run's own messages are all
/// sent and stored; views show the latest (`store::incoming`).
pub const MAX_MSGS: usize = 50;
const CONTEXT_MSGS: usize = 3;
/// Pages of 200 read per conversation per poll.
const HISTORY_PAGES: usize = 10;
const REPLIES_PAGES: usize = 5;
/// Search lags behind messages: each search reads back this far before the
/// newest hit it handled. A mention read again carries the same revision
/// (its ts), so applying it again changes nothing.
const SEARCH_OVERLAP_SECS: f64 = 3600.0;
/// A conversation or thread Slack says isn't there this many polls in a row
/// is forgotten (its items stay as they are).
const DROP_AFTER: i64 = 3;
/// Live mode: how often open streams are re-read, in case an event was lost.
const READ_CHECK_SECS: f64 = 120.0;
/// Without the socket, threads active within a day are checked every poll,
/// older ones at most hourly.
const HOT_THREAD_SECS: f64 = 86_400.0;
const COLD_THREAD_CHECK_SECS: f64 = 3_600.0;
/// Followed channels are read at least this far back, so that a new thread
/// on a recent parent you already read is found.
const RECENT_PARENTS_SECS: f64 = 86_400.0;

/// Subtypes that are real content. Everything else (joins, topic changes,
/// pins…) is noise. `thread_broadcast` is left out on purpose: the reply shows
/// up in its thread.
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
    /// "T0123ABCD", for slack:// links.
    #[serde(default)]
    pub team_id: String,
    /// Resolved ids of followed channels (the live socket filters on these).
    #[serde(default)]
    pub follow: Vec<String>,
    /// Followed channels you aren't a member of: no events, no read marker.
    #[serde(default)]
    pub unjoined: Vec<String>,
    /// Per conversation id.
    #[serde(default)]
    pub convs: BTreeMap<String, ConvState>,
    /// Tracked threads, key "<channel>/<thread ts>".
    #[serde(default)]
    pub threads: BTreeMap<String, ThreadState>,
    /// Items not triaged yet. Key: the stream ("<channel>",
    /// "<channel>/<thread ts>") or, for mentions, the item's external id.
    #[serde(default)]
    pub open: BTreeMap<String, Open>,
    #[serde(default)]
    pub closed: BTreeMap<String, String>,
    /// Newest mention / own thread reply already handled from search.
    #[serde(default)]
    pub mention_ts: Option<String>,
    #[serde(default)]
    pub reply_ts: Option<String>,
    /// When open items' read markers were last checked (Unix seconds).
    #[serde(default)]
    pub read_checked: f64,
    /// When the last full catch-up completed (Unix seconds).
    #[serde(default)]
    pub caught_up: f64,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct ConvState {
    /// Newest top-level message ts read from the API.
    pub seen: String,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct ThreadState {
    /// Newest reply ts read from the API.
    pub seen: String,
    /// Title of the thread item ("#inc-2291 · thread: first words…").
    #[serde(default)]
    pub title: String,
    /// Last `conversations.replies` check (Unix seconds).
    #[serde(default)]
    pub checked: f64,
}

pub use super::super::chat::Open;

impl State {
    pub fn parse(cursor: Option<&str>) -> State {
        cursor.and_then(|c| serde_json::from_str(c).ok()).unwrap_or_default()
    }
}

impl super::super::chat::Runs for State {
    fn runs(&mut self) -> (&mut BTreeMap<String, Open>, &mut BTreeMap<String, String>) {
        (&mut self.open, &mut self.closed)
    }
}

#[derive(Debug, Clone)]
enum ConvKind {
    Channel { name: String },
    Im { user: String },
    Mpim { name: String },
}

/// A conversation as `users.conversations` / `conversations.info` describe it.
#[derive(Debug, Clone)]
struct Conv {
    id: String,
    kind: ConvKind,
    member: bool,
    /// Your read marker (members only).
    last_read: Option<String>,
    /// Newest message ts, when Slack says (DMs do).
    latest: Option<String>,
    created: i64,
}

impl Conv {
    fn parse(c: &Value) -> Option<Conv> {
        let s = |k: &str| c.get(k).and_then(Value::as_str).unwrap_or_default().to_owned();
        let b = |k: &str| c.get(k).and_then(Value::as_bool);
        let kind = if b("is_im") == Some(true) {
            if b("is_user_deleted") == Some(true) {
                return None;
            }
            ConvKind::Im { user: s("user") }
        } else if b("is_mpim") == Some(true) {
            ConvKind::Mpim { name: s("name") }
        } else {
            ConvKind::Channel { name: s("name") }
        };
        let latest = match c.get("latest") {
            Some(Value::Object(m)) => m.get("ts").and_then(Value::as_str).map(str::to_owned),
            Some(Value::String(t)) => Some(t.clone()),
            _ => None,
        };
        Some(Conv {
            id: s("id"),
            member: !matches!(kind, ConvKind::Channel { .. }) || b("is_member").unwrap_or(true),
            kind,
            last_read: c.get("last_read").and_then(Value::as_str).map(str::to_owned),
            latest,
            created: c.get("created").and_then(Value::as_i64).unwrap_or(0),
        })
    }

    fn is_dm(&self) -> bool {
        !matches!(self.kind, ConvKind::Channel { .. })
    }
}

/// One message as read from the API.
#[derive(Debug, Clone)]
struct Msg {
    ts: String,
    user: Option<String>,
    bot_name: Option<String>,
    bot_id: Option<String>,
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
            text = rich_text(v);
        }
        for f in v.get("files").and_then(Value::as_array).into_iter().flatten() {
            let image = f
                .get("mimetype")
                .and_then(Value::as_str)
                .is_some_and(|m| m.starts_with("image/"));
            let name = f.get("name").and_then(Value::as_str).unwrap_or("");
            let line = format!("[{}] {name}", if image { "image" } else { "file" });
            text = if text.trim().is_empty() {
                line
            } else {
                format!("{text}\n{line}")
            };
        }
        Some(Msg {
            ts: s("ts")?,
            user: s("user").filter(|u| !u.is_empty()),
            bot_name: v
                .pointer("/bot_profile/name")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .or_else(|| s("username")),
            bot_id: s("bot_id").filter(|b| !b.is_empty()),
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

fn rich_text(v: &Value) -> String {
    let s = |v: &Value, k: &str| {
        v.get(k)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(str::to_owned)
    };
    let mut out: Vec<String> = vec![];
    for a in v.get("attachments").and_then(Value::as_array).into_iter().flatten() {
        let before = out.len();
        out.extend(["pretext", "title", "text"].iter().filter_map(|k| s(a, k)));
        for f in a.get("fields").and_then(Value::as_array).into_iter().flatten() {
            match (s(f, "title"), s(f, "value")) {
                (Some(t), Some(v)) => out.push(format!("{t}: {v}")),
                (t, v) => out.extend(t.or(v)),
            }
        }
        out.extend(a.get("blocks").map(blocks_text).filter(|t| !t.is_empty()));
        if out.len() == before {
            out.extend(s(a, "fallback"));
        }
    }
    out.extend(v.get("blocks").map(blocks_text).filter(|t| !t.is_empty()));
    out.join("\n")
}

fn blocks_text(blocks: &Value) -> String {
    fn walk(v: &Value, out: &mut String) {
        match v {
            Value::Array(a) => a.iter().for_each(|x| walk(x, out)),
            Value::Object(o) => {
                match o.get("type").and_then(Value::as_str) {
                    Some("user") => {
                        return out.push_str(&format!(
                            "<@{}>",
                            o.get("user_id").and_then(Value::as_str).unwrap_or("")
                        ))
                    }
                    Some("link") if o.get("text").is_none() => {
                        return out.push_str(o.get("url").and_then(Value::as_str).unwrap_or(""))
                    }
                    Some("image") => return,
                    _ => {}
                }
                match o.get("text") {
                    Some(Value::String(t)) => out.push_str(t),
                    Some(t) => walk(t, out),
                    None => {}
                }
                for k in ["elements", "fields"] {
                    if let Some(x) = o.get(k) {
                        walk(x, out);
                    }
                }
            }
            _ => {}
        }
    }
    let mut parts = vec![];
    for b in blocks.as_array().into_iter().flatten() {
        let mut t = String::new();
        walk(b, &mut t);
        if !t.trim().is_empty() {
            parts.push(t.trim().to_owned());
        }
    }
    parts.join("\n")
}

/// Which messages of a stream belong to its run: with an open run, everything
/// from its start; otherwise what's newer than anything read before (`after`)
/// and than the last triaged run (`closed`), within the lookback floor.
struct Marker {
    from: Option<String>,
    after: Option<String>,
    closed: Option<String>,
    floor: f64,
}

impl Marker {
    fn unread(&self, ts: &str) -> bool {
        if self.closed.as_deref().is_some_and(|c| !ts_after(ts, c)) {
            return false;
        }
        if let Some(from) = &self.from {
            return !ts_after(from, ts);
        }
        ts_secs(ts) > self.floor && self.after.as_deref().is_none_or(|a| ts_after(ts, a))
    }

    /// Lower bound for `oldest` when reading history.
    fn oldest(&self) -> f64 {
        if let Some(from) = &self.from {
            return ts_secs(from);
        }
        let t = |o: &Option<String>| o.as_deref().map(ts_secs).unwrap_or(0.0);
        t(&self.after).max(t(&self.closed)).max(self.floor)
    }
}

pub struct Poller<'a, 'r, 's> {
    client: Client<'a>,
    cfg: &'a Config,
    state: State,
    now: f64,
    /// For `search.messages`' `after:`, which is by calendar day.
    clock: Clock,
    plan: Plan,
    users: HashMap<String, String>,
    channel_names: HashMap<String, String>,
    infos: HashMap<String, Option<Conv>>,
    done_convs: BTreeSet<String>,
    done_threads: BTreeSet<String>,
    items: Vec<ItemUpsert>,
    seen: Vec<String>,
    profiles: Vec<Profile>,
    /// How each stream went; a stream is recorded once per poll.
    outcomes: Vec<StreamOutcome>,
    recorded: BTreeSet<String>,
    /// Failures in a row per stream before this poll.
    failures: BTreeMap<String, i64>,
    /// Writes what's been read so far (`Polling::commit`).
    streams: Option<&'r mut Polling<'s>>,
}

pub fn poll(ctx: &Ctx, cfg: &Config) -> anyhow::Result<Batch> {
    poll_with(ctx, cfg, None)
}

/// A poll that commits stream by stream when given `streams`.
pub fn poll_with<'r, 's>(ctx: &Ctx, cfg: &Config, streams: Option<&'r mut Polling<'s>>) -> anyhow::Result<Batch> {
    let token = ctx.require_secret(&cfg.user_token_secret)?;
    let mut client = Client::new(ctx.http, token);
    client.sleep = cfg.sleep;
    let now = ctx.now.unix_ms() as f64 / 1000.0;
    let plan = socket::plan(ctx.source.id);
    let mut p = Poller {
        client,
        cfg,
        state: State::parse(ctx.source.cursor.as_deref()),
        now,
        clock: ctx.clock.clone(),
        plan: plan.clone(),
        users: HashMap::new(),
        channel_names: HashMap::new(),
        infos: HashMap::new(),
        done_convs: BTreeSet::new(),
        done_threads: BTreeSet::new(),
        items: vec![],
        seen: vec![],
        profiles: vec![],
        outcomes: vec![],
        recorded: BTreeSet::new(),
        failures: streams.as_ref().map(|s| s.failures.clone()).unwrap_or_default(),
        streams,
    };
    let result = p.run();
    socket::finish(ctx.source.id, &plan, result.is_ok());
    result?;
    p.take_batch()
}

impl Poller<'_, '_, '_> {
    /// What's been read so far, with the cursor as it stands.
    fn take_batch(&mut self) -> anyhow::Result<Batch> {
        Ok(Batch {
            items: std::mem::take(&mut self.items),
            seen: std::mem::take(&mut self.seen),
            profiles: std::mem::take(&mut self.profiles),
            streams: std::mem::take(&mut self.outcomes),
            cursor: Some(serde_json::to_string(&self.state)?),
            ..Default::default()
        })
    }

    /// Read one stream. A Slack error about this stream alone (not found,
    /// not in channel, a missing scope) is recorded and the poll goes on;
    /// one about the token or account, or the network, ends the poll. What
    /// the stream produced is committed straight away.
    fn stream<T>(&mut self, key: &str, f: impl FnOnce(&mut Self) -> anyhow::Result<T>) -> anyhow::Result<Option<T>> {
        let (value, error) = match f(self) {
            Ok(v) => (Some(v), None),
            Err(e) => match e.downcast_ref::<ApiError>() {
                Some(api) if !api.is_fatal() => {
                    if api.is_not_found() && self.failures.get(key).copied().unwrap_or(0) + 1 >= DROP_AFTER {
                        self.state.threads.remove(key);
                        self.state.convs.remove(key);
                        self.state.open.remove(key);
                    }
                    (None, Some(e.to_string()))
                }
                _ => return Err(e),
            },
        };
        if self.recorded.insert(key.to_owned()) || error.is_some() {
            self.outcomes.retain(|o| o.key != key);
            self.outcomes.push(StreamOutcome {
                key: key.to_owned(),
                error,
            });
        }
        if !self.items.is_empty() {
            if let Some(streams) = self.streams.as_mut() {
                let b = Batch {
                    items: std::mem::take(&mut self.items),
                    seen: std::mem::take(&mut self.seen),
                    profiles: std::mem::take(&mut self.profiles),
                    streams: std::mem::take(&mut self.outcomes),
                    cursor: Some(serde_json::to_string(&self.state)?),
                    ..Default::default()
                };
                (streams.commit)(b)?;
            }
        }
        Ok(value)
    }

    fn run(&mut self) -> anyhow::Result<()> {
        if self.state.me.is_empty() || self.state.team_url.is_empty() || self.state.team_id.is_empty() {
            let v = self.client.get("auth.test", &[])?;
            let s = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or_default().to_owned();
            self.state.me = s("user_id");
            self.state.me_name = s("user");
            self.state.team_url = s("url");
            self.state.team_id = s("team_id");
        }
        // Live, but the last full pass is `catchup_hours` old: do one now.
        // Socket Mode is best-effort, so events can be lost while connected.
        if self.plan.mode == Mode::Live && self.now - self.state.caught_up >= self.cfg.catchup_hours as f64 * 3600.0 {
            self.plan.mode = Mode::CatchUp;
        }
        match self.plan.mode {
            Mode::Live => self.live()?,
            Mode::CatchUp | Mode::Poll => self.catch_up()?,
        }
        self.prune();
        Ok(())
    }

    fn floor(&self) -> f64 {
        self.now - self.cfg.lookback_hours as f64 * 3600.0
    }

    /// Only what events flagged, unjoined followed channels, and now and
    /// then the open streams.
    fn live(&mut self) -> anyhow::Result<()> {
        let mut convs: Vec<String> = self.state.unjoined.clone();
        let mut threads: Vec<(String, String)> = vec![];
        let mut mentions = vec![];
        for f in self.plan.flags.clone() {
            match f {
                Flag::Conv(c) => convs.push(c),
                Flag::Thread(c, t) => threads.push((c, t)),
                Flag::Mention(c, ts) => mentions.push((c, ts)),
            }
        }
        if self.now - self.state.read_checked >= READ_CHECK_SECS {
            self.state.read_checked = self.now;
            let (c, t) = self.open_streams();
            convs.extend(c);
            threads.extend(t);
        }
        for c in convs {
            self.stream(&c, |p| p.check_conv(&c))?;
        }
        for (c, t) in threads {
            self.stream(&format!("{c}/{t}"), |p| p.check_thread(&c, &t))?;
        }
        for (c, ts) in mentions {
            self.stream(&format!("{c}/mention/{ts}"), |p| p.fetch_mention(&c, &ts))?;
        }
        Ok(())
    }

    /// Full pass: followed channels, DMs, open items, flagged ones, tracked
    /// threads and the searches. Runs on connect/reconnect, every poll without
    /// the socket, and every `catchup_hours` while the socket stays up.
    fn catch_up(&mut self) -> anyhow::Result<()> {
        let listing = self.conversations()?;
        let follow = self.resolve_follow(&listing)?;
        self.state.follow = follow.iter().map(|c| c.id.clone()).collect();
        self.state.unjoined = follow.iter().filter(|c| !c.member).map(|c| c.id.clone()).collect();

        let mut convs: Vec<String> = self.state.follow.clone();
        if self.cfg.dms {
            convs.extend(self.pick_dms(&listing));
        }
        let (open_convs, open_threads) = self.open_streams();
        convs.extend(open_convs);
        let mut threads: Vec<(String, String)> = open_threads;
        let mut mentions = vec![];
        for f in self.plan.flags.clone() {
            match f {
                Flag::Conv(c) => convs.push(c),
                Flag::Thread(c, t) => threads.push((c, t)),
                Flag::Mention(c, ts) => mentions.push((c, ts)),
            }
        }
        for c in convs {
            self.stream(&c, |p| p.check_conv(&c))?;
        }
        if self.cfg.mentions {
            threads.extend(
                self.stream("search:mentions", |p| p.search_mentions())?
                    .unwrap_or_default(),
            );
            threads.extend(
                self.stream("search:replies", |p| p.search_my_replies())?
                    .unwrap_or_default(),
            );
        }
        // Tracked threads: all of them on a socket catch-up (it's rare);
        // without the socket, busy ones every poll and quiet ones hourly.
        let all = self.plan.mode == Mode::CatchUp;
        let tracked: Vec<(String, String)> = self
            .state
            .threads
            .iter()
            .filter(|(_, t)| {
                all || self.now - ts_secs(&t.seen) < HOT_THREAD_SECS || self.now - t.checked >= COLD_THREAD_CHECK_SECS
            })
            .filter_map(|(k, _)| k.split_once('/').map(|(c, t)| (c.to_owned(), t.to_owned())))
            .collect();
        threads.extend(tracked);
        for (c, t) in threads {
            self.stream(&format!("{c}/{t}"), |p| p.check_thread(&c, &t))?;
        }
        for (c, ts) in mentions {
            self.stream(&format!("{c}/mention/{ts}"), |p| p.fetch_mention(&c, &ts))?;
        }
        self.state.read_checked = self.now;
        self.state.caught_up = self.now;
        Ok(())
    }

    /// Conversations and threads that have an open (untriaged) item.
    fn open_streams(&self) -> (Vec<String>, Vec<(String, String)>) {
        let mut convs = vec![];
        let mut threads = vec![];
        for key in self.state.open.keys() {
            match key.split('/').collect::<Vec<_>>()[..] {
                [c] => convs.push(c.to_owned()),
                [c, t] => threads.push((c.to_owned(), t.to_owned())),
                _ => {}
            }
        }
        (convs, threads)
    }

    /// Everything the user is a member of: channels always, DMs when enabled.
    fn conversations(&mut self) -> anyhow::Result<Vec<Conv>> {
        let types = if self.cfg.dms {
            "public_channel,private_channel,im,mpim"
        } else {
            "public_channel,private_channel"
        };
        let raw = self.client.get_paged(
            "users.conversations",
            &[("types", types), ("exclude_archived", "true"), ("limit", "200")],
            "channels",
            20,
        )?;
        let mut out = vec![];
        for c in raw.iter().filter_map(Conv::parse) {
            if let ConvKind::Channel { name } = &c.kind {
                self.channel_names.insert(c.id.clone(), name.clone());
            }
            // The listing rarely carries read markers; when it does, save the info call.
            if c.last_read.is_some() {
                self.infos.insert(c.id.clone(), Some(c.clone()));
            }
            out.push(c);
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
                // A public channel you haven't joined: readable, but not listed.
                None if looks_like_id(want) => match self.info(want)? {
                    Some(mut c) => {
                        c.member = false;
                        self.infos.insert(c.id.clone(), Some(c.clone()));
                        c
                    }
                    None => anyhow::bail!("Slack: followed channel '{entry}' not found"),
                },
                None => anyhow::bail!(
                    "Slack: followed channel '{entry}' not found among channels you're in; use its id or join it"
                ),
            };
            if !out.iter().any(|c| c.id == conv.id) {
                out.push(conv);
            }
        }
        if self.cfg.all_channels {
            for c in convs
                .iter()
                .filter(|c| matches!(c.kind, ConvKind::Channel { .. }) && c.member)
            {
                if !out.iter().any(|o| o.id == c.id) {
                    out.push(c.clone());
                }
            }
        }
        Ok(out)
    }

    /// The `max_dms` most recently active DMs and group DMs: activity Alethiometer has
    /// seen first, then the conversation's creation time (Slack lists no
    /// last-activity time for DMs).
    fn pick_dms(&self, listing: &[Conv]) -> Vec<String> {
        let mut dms: Vec<&Conv> = listing.iter().filter(|c| c.is_dm()).collect();
        let active = |c: &Conv| self.state.convs.get(&c.id).map(|s| ts_secs(&s.seen)).unwrap_or(0.0);
        dms.sort_by(|a, b| active(b).total_cmp(&active(a)).then(b.created.cmp(&a.created)));
        dms.iter().take(self.cfg.max_dms).map(|c| c.id.clone()).collect()
    }

    /// `conversations.info`, once per conversation per poll. `None` for a DM
    /// with a deleted user.
    fn info(&mut self, id: &str) -> anyhow::Result<Option<Conv>> {
        if let Some(c) = self.infos.get(id) {
            return Ok(c.clone());
        }
        let v = self.client.get("conversations.info", &[("channel", id)])?;
        let c = v.get("channel").and_then(Conv::parse);
        if let Some(Conv {
            kind: ConvKind::Channel { name },
            ..
        }) = &c
        {
            self.channel_names.insert(id.to_owned(), name.clone());
        }
        self.infos.insert(id.to_owned(), c.clone());
        Ok(c)
    }

    /// Top-level messages of one conversation.
    fn check_conv(&mut self, id: &str) -> anyhow::Result<()> {
        if !self.done_convs.insert(id.to_owned()) {
            return Ok(());
        }
        let Some(conv) = self.info(id)? else { return Ok(()) };
        let followed = self.state.follow.iter().any(|c| c == id);
        let prev = self.state.convs.get(id).cloned();
        let open = self.state.open.get(id).cloned();
        let marker = Marker {
            from: open.as_ref().map(|o| o.first.clone()),
            after: prev.as_ref().map(|p| p.seen.clone()),
            closed: self.state.closed.get(id).cloned(),
            floor: self.floor(),
        };
        // DMs: skip history when Slack's summary says nothing changed.
        if !followed {
            if let Some(latest) = &conv.latest {
                let nothing_new = prev.as_ref().is_some_and(|p| !ts_after(latest, &p.seen));
                if nothing_new || !marker.unread(latest) {
                    return Ok(());
                }
            }
        }
        let mut oldest = marker.oldest();
        if followed {
            oldest = oldest.min((self.now - RECENT_PARENTS_SECS).max(self.floor()));
        }
        let oldest = format!("{oldest:.6}");
        let raw = self.client.get_paged(
            "conversations.history",
            &[
                ("channel", id),
                ("oldest", &oldest),
                ("inclusive", "true"),
                ("limit", "200"),
            ],
            "messages",
            HISTORY_PAGES,
        )?;
        let mut msgs: Vec<Msg> = raw.iter().filter_map(Msg::parse).filter(|m| !m.is_reply()).collect();
        msgs.sort_by(|a, b| ts_secs(&a.ts).total_cmp(&ts_secs(&b.ts)));
        let prev_seen = prev.map(|p| p.seen);
        if let Some(newest) = msgs.last() {
            if prev_seen.as_deref().is_none_or(|s| ts_after(&newest.ts, s)) {
                self.state.convs.insert(
                    id.to_owned(),
                    ConvState {
                        seen: newest.ts.clone(),
                    },
                );
            }
        }
        // Parents with replies we haven't read yet.
        let mut threads = vec![];
        for m in msgs.iter().filter(|m| m.reply_count > 0) {
            let last = m.latest_reply.as_deref().unwrap_or(&m.ts);
            let fresh = match self.state.threads.get(&format!("{id}/{}", m.ts)) {
                Some(t) => ts_after(last, &t.seen),
                None => ts_secs(last) > self.floor(),
            };
            if fresh {
                threads.push(m.ts.clone());
            }
        }
        let unread: Vec<Msg> = msgs.into_iter().filter(|m| marker.unread(&m.ts)).collect();
        self.emit(&conv, None, id, &unread, prev_seen.as_deref(), None)?;
        for t in threads {
            self.stream(&format!("{id}/{t}"), |p| p.check_thread(id, &t))?;
        }
        Ok(())
    }

    /// One thread: its unread replies become the thread's item. Tracked while
    /// you're in it (or it's in a DM or followed channel).
    fn check_thread(&mut self, channel: &str, thread_ts: &str) -> anyhow::Result<()> {
        let key = format!("{channel}/{thread_ts}");
        if !self.done_threads.insert(key.clone()) {
            return Ok(());
        }
        let Some(conv) = self.info(channel)? else { return Ok(()) };
        // No `oldest`: the parent (first in the answer) carries the thread's
        // read marker and title, and threads are rarely longer than a page.
        // Parent deleted: `stream` stops tracking it after a few tries; the item stays.
        let raw = self.client.get_paged(
            "conversations.replies",
            &[("channel", channel), ("ts", thread_ts), ("limit", "200")],
            "messages",
            REPLIES_PAGES,
        )?;
        let parent = raw
            .iter()
            .find(|m| m.get("ts").and_then(Value::as_str) == Some(thread_ts));
        let mut msgs: Vec<Msg> = raw
            .iter()
            .filter_map(Msg::parse)
            .filter(|m| m.ts != thread_ts)
            .collect();
        msgs.sort_by(|a, b| ts_secs(&a.ts).total_cmp(&ts_secs(&b.ts)));
        let me = self.state.me.clone();
        let pstr = |k: &str| parent.and_then(|p| p.get(k)).and_then(Value::as_str);
        let in_it = pstr("user") == Some(me.as_str())
            || parent.and_then(|p| p.get("subscribed")).and_then(Value::as_bool) == Some(true)
            || parent
                .and_then(|p| p.get("reply_users"))
                .and_then(Value::as_array)
                .is_some_and(|u| u.iter().any(|u| u.as_str() == Some(me.as_str())))
            || msgs
                .iter()
                .any(|m| m.user.as_deref() == Some(me.as_str()) || m.text.contains(&format!("<@{me}")));
        let followed = self.state.follow.iter().any(|c| c == channel);
        let prev = self.state.threads.get(&key).cloned();
        if !(in_it || followed || conv.is_dm()) {
            self.state.threads.remove(&key);
            return Ok(());
        }
        let title = match prev.as_ref().map(|t| t.title.clone()).filter(|t| !t.is_empty()) {
            Some(t) => t,
            None => {
                let cleaned = self.clean(pstr("text").unwrap_or_default())?;
                let words = first_words(&cleaned, 6);
                let base = self.base_title(&conv)?;
                if words.is_empty() {
                    format!("{base} · thread")
                } else {
                    format!("{base} · thread: {words}")
                }
            }
        };
        let open = self.state.open.get(&key).cloned();
        let prev_seen = prev.as_ref().map(|t| t.seen.clone());
        let marker = Marker {
            from: open.map(|o| o.first),
            after: prev_seen.clone(),
            closed: self.state.closed.get(&key).cloned(),
            // The floor bounds the first read of a thread, not its age.
            floor: if prev.is_some() { 0.0 } else { self.floor() },
        };
        let newest = msgs
            .last()
            .map(|m| m.ts.clone())
            .unwrap_or_else(|| thread_ts.to_owned());
        let seen = match &prev_seen {
            Some(s) if !ts_after(&newest, s) => s.clone(),
            _ => newest,
        };
        self.state.threads.insert(
            key.clone(),
            ThreadState {
                seen,
                title: title.clone(),
                checked: self.now,
            },
        );
        let start = msgs.iter().position(|m| marker.unread(&m.ts)).unwrap_or(msgs.len());
        let mut context: Vec<Msg> = parent.and_then(Msg::parse).into_iter().collect();
        context.extend(msgs[..start].iter().skip(start.saturating_sub(MAX_MSGS)).cloned());
        let unread: Vec<Msg> = msgs.into_iter().filter(|m| marker.unread(&m.ts)).collect();
        self.emit(
            &conv,
            Some((thread_ts, &title)),
            &key,
            &unread,
            prev_seen.as_deref(),
            Some(context),
        )
    }

    /// Write, update or close the item for one stream's unread run.
    fn emit(
        &mut self,
        conv: &Conv,
        thread: Option<(&str, &str)>,
        key: &str,
        unread: &[Msg],
        prev_seen: Option<&str>,
        context: Option<Vec<Msg>>,
    ) -> anyhow::Result<()> {
        let me = self.state.me.clone();
        let open = self.state.open.get(key).cloned();
        if unread.is_empty() {
            return Ok(());
        }
        let (first, last) = (&unread[0], &unread[unread.len() - 1]);
        let bump = unread.iter().any(|m| prev_seen.is_none_or(|s| ts_after(&m.ts, s)));
        let unchanged = open.as_ref().is_some_and(|o| o.first == first.ts && o.last == last.ts);
        if !bump && unchanged {
            return Ok(());
        }
        let context = match context {
            Some(c) => c,
            None => self.context_before(&conv.id, &first.ts)?,
        };
        let id = match (&open, thread) {
            (Some(o), _) => o.id.clone(),
            (None, Some((tts, _))) => format!("{}/thread/{tts}", conv.id),
            (None, None) => format!("{}/{}", conv.id, first.ts),
        };
        let title = match thread {
            Some((_, t)) => t.to_owned(),
            None => self.base_title(conv)?,
        };
        let kept = unread;
        let mut messages = vec![];
        for (m, is_context) in context.iter().map(|m| (m, true)).chain(kept.iter().map(|m| (m, false))) {
            messages.push(Message {
                at: Ts::from_unix_secs(ts_secs(&m.ts)).unwrap_or_default(),
                author: self.author_name(m)?,
                text: self.clean(&m.text)?,
                author_id: m.user.clone(),
                context: is_context,
            });
        }
        let run = messages.iter().filter(|m| !m.context);
        let body = run
            .clone()
            .map(|m| format!("{}: {}", m.author, m.text))
            .collect::<Vec<_>>()
            .join("\n");
        let summary = run.clone().next_back().map(|m| truncate(&m.text, 140));
        let author = match unread.iter().rev().find_map(|m| m.user.as_ref().filter(|u| **u != me)) {
            Some(u) => Some(Handle {
                handle: u.clone(),
                display_name: self.user_name(u)?,
            }),
            None => None,
        };
        let mut raw = json!({
            "channel": conv.id,
            "first_ts": first.ts,
            "last_ts": last.ts,
            // One ts per stored message, in the same order.
            "msgs": context.iter().chain(kept).map(|m| m.ts.as_str()).collect::<Vec<_>>(),
        });
        match thread {
            Some((tts, _)) => raw["thread_ts"] = json!(tts),
            // What `slack.mark_read` sends: conversations.mark up to here. Only
            // top-level runs in conversations you're in; Slack has no API to
            // mark a thread read.
            None if conv.member => raw["mark_ts"] = json!(last.ts),
            None => {}
        }
        self.items.push(ItemUpsert {
            external_id: id.clone(),
            kind: if thread.is_some() || unread.len() > 1 {
                "thread"
            } else {
                "message"
            }
            .into(),
            title,
            body,
            summary,
            url: self.permalink(&conv.id, &last.ts, thread.map(|t| t.0)),
            author,
            occurred_at: Ts::from_unix_secs(ts_secs(&first.ts)).unwrap_or_default(),
            ended_at: Ts::from_unix_secs(ts_secs(&last.ts)),
            status: "open".into(),
            msg_count: Some(unread.len() as i64),
            raw_json: raw,
            messages,
            bump,
            attach_to: thread.map(|t| t.0.to_owned()),
            ..Default::default()
        });
        let muted = match &conv.kind {
            ConvKind::Channel { name } => self.cfg.is_muted(&conv.id, name),
            ConvKind::Im { user } => self.cfg.is_muted(&conv.id, user),
            ConvKind::Mpim { name } => self.cfg.is_muted(&conv.id, name),
        };
        if muted {
            self.seen.push(id.clone());
        }
        self.state.open.insert(
            key.to_owned(),
            Open {
                id,
                first: first.ts.clone(),
                last: last.ts.clone(),
            },
        );
        Ok(())
    }

    fn context_before(&mut self, channel: &str, ts: &str) -> anyhow::Result<Vec<Msg>> {
        let limit = (CONTEXT_MSGS * 4).to_string();
        let v = self.client.get(
            "conversations.history",
            &[
                ("channel", channel),
                ("latest", ts),
                ("inclusive", "false"),
                ("limit", &limit),
            ],
        )?;
        let mut msgs: Vec<Msg> = v
            .get("messages")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Msg::parse)
                    .filter(|m| !m.is_reply() && ts_after(ts, &m.ts))
                    .collect()
            })
            .unwrap_or_default();
        msgs.sort_by(|a, b| ts_secs(&a.ts).total_cmp(&ts_secs(&b.ts)));
        Ok(msgs.split_off(msgs.len().saturating_sub(CONTEXT_MSGS)))
    }

    fn base_title(&mut self, conv: &Conv) -> anyhow::Result<String> {
        Ok(match &conv.kind {
            ConvKind::Channel { name } => format!("#{name}"),
            ConvKind::Im { user } => format!("DM · {}", self.user_name(user)?),
            ConvKind::Mpim { name } => format!("Group DM · {}", mpim_names(name, &self.state.me_name)),
        })
    }

    /// `search.messages`, newest first. `after:` is by day and exclusive, so
    /// ask from the day before `since`.
    fn search(&mut self, query: &str, since: f64) -> anyhow::Result<Vec<Value>> {
        let day = Ts::from_unix_secs(since - 86_400.0)
            .map(|t| self.clock.day_of(t).to_string())
            .unwrap_or_default();
        let q = format!("{query} after:{day}");
        let v = self.client.get(
            "search.messages",
            &[
                ("query", &q),
                ("sort", "timestamp"),
                ("sort_dir", "desc"),
                ("count", "100"),
            ],
        )?;
        Ok(v.pointer("/messages/matches")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default())
    }

    /// Mentions of you since the last search. In a thread: check the thread
    /// (and so start tracking it). In a DM or followed channel: that
    /// conversation's check covers it (this also reaches DMs beyond
    /// `max_dms`). Elsewhere: a mention item.
    fn search_mentions(&mut self) -> anyhow::Result<Vec<(String, String)>> {
        let me = self.state.me.clone();
        let since =
            (self.state.mention_ts.as_deref().map(ts_secs).unwrap_or(0.0) - SEARCH_OVERLAP_SECS).max(self.floor());
        let tag = format!("<@{me}");
        let mut newest = self.state.mention_ts.clone();
        let mut threads = vec![];
        for m in self.search(&format!("<@{me}>"), since)? {
            let hit = Hit::parse(&m);
            // Search is fuzzy; keep only real mentions by others, once.
            if hit.ts.is_empty()
                || hit.channel.is_empty()
                || hit.user == me
                || !hit.text.contains(&tag)
                || ts_secs(&hit.ts) <= since
            {
                continue;
            }
            if newest.as_deref().is_none_or(|n| ts_after(&hit.ts, n)) {
                newest = Some(hit.ts.clone());
            }
            if let Some(tts) = hit.thread_ts.filter(|t| *t != hit.ts) {
                threads.push((hit.channel, tts));
            } else if hit.dm || self.state.follow.contains(&hit.channel) {
                let c = hit.channel.clone();
                self.stream(&c, |p| p.check_conv(&c))?;
            } else {
                let msg = Msg {
                    ts: hit.ts,
                    user: Some(hit.user).filter(|u| !u.is_empty()),
                    bot_name: Some(hit.username),
                    bot_id: None,
                    text: hit.text,
                    thread_ts: None,
                    reply_count: 0,
                    latest_reply: None,
                };
                self.mention_item(&hit.channel, hit.channel_name, &msg, hit.permalink)?;
            }
        }
        self.state.mention_ts = newest;
        Ok(threads)
    }

    /// Your own thread replies since the last search: threads you're in that
    /// Alethiometer may not know yet (you replied from your phone while it was off).
    fn search_my_replies(&mut self) -> anyhow::Result<Vec<(String, String)>> {
        let me = self.state.me.clone();
        let since =
            (self.state.reply_ts.as_deref().map(ts_secs).unwrap_or(0.0) - SEARCH_OVERLAP_SECS).max(self.floor());
        let mut newest = self.state.reply_ts.clone();
        let mut threads = vec![];
        for m in self.search(&format!("from:<@{me}>"), since)? {
            let hit = Hit::parse(&m);
            if hit.user != me || ts_secs(&hit.ts) <= since {
                continue;
            }
            if newest.as_deref().is_none_or(|n| ts_after(&hit.ts, n)) {
                newest = Some(hit.ts.clone());
            }
            if let Some(tts) = hit.thread_ts.filter(|t| *t != hit.ts) {
                threads.push((hit.channel, tts));
            }
        }
        self.state.reply_ts = newest;
        Ok(threads)
    }

    /// A live mention event: read that one message.
    fn fetch_mention(&mut self, channel: &str, ts: &str) -> anyhow::Result<()> {
        if self.state.mention_ts.as_deref().is_some_and(|n| !ts_after(ts, n)) {
            return Ok(());
        }
        let v = self.client.get(
            "conversations.history",
            &[
                ("channel", channel),
                ("oldest", ts),
                ("latest", ts),
                ("inclusive", "true"),
                ("limit", "1"),
            ],
        )?;
        let Some(msg) = v
            .get("messages")
            .and_then(Value::as_array)
            .and_then(|a| a.first())
            .and_then(Msg::parse)
        else {
            return Ok(());
        };
        let name = match self.info(channel)? {
            Some(Conv {
                kind: ConvKind::Channel { name },
                ..
            }) => name,
            _ => channel.to_owned(),
        };
        self.mention_item(channel, name, &msg, None)?;
        self.state.mention_ts = Some(ts.to_owned());
        Ok(())
    }

    /// One mention in a channel you don't follow. When you're a member it
    /// carries `mark_ts` = the mention, so confirming it marks the channel read
    /// up to there in Slack. That also marks what came before it read: by the
    /// user's choice, the mention is the unread part that matters there.
    /// (`slack.mark_read` never moves the marker backwards.)
    fn mention_item(
        &mut self,
        channel: &str,
        channel_name: String,
        m: &Msg,
        permalink: Option<String>,
    ) -> anyhow::Result<()> {
        let member = self.info(channel).ok().flatten().is_some_and(|c| c.member);
        let name = self.author_name(m)?;
        let clean = self.clean(&m.text)?;
        let at = Ts::from_unix_secs(ts_secs(&m.ts)).unwrap_or_default();
        let id = format!("{channel}/mention/{}", m.ts);
        self.items.push(ItemUpsert {
            external_id: id.clone(),
            kind: "message".into(),
            title: format!("#{channel_name} · mention"),
            body: format!("{name}: {clean}"),
            summary: Some(truncate(&clean, 140)),
            url: permalink.or_else(|| self.permalink(channel, &m.ts, None)),
            author: m.user.as_ref().map(|u| Handle {
                handle: u.clone(),
                display_name: name.clone(),
            }),
            occurred_at: at.clone(),
            status: "open".into(),
            msg_count: Some(1),
            raw_json: {
                let mut raw = json!({ "channel": channel, "first_ts": m.ts, "last_ts": m.ts, "msgs": [m.ts] });
                if member {
                    raw["mark_ts"] = json!(m.ts);
                }
                raw
            },
            messages: vec![Message {
                at,
                author: name,
                text: clean,
                author_id: m.user.clone(),
                context: false,
            }],
            bump: true,
            remote_rev: Some(m.ts.clone()),
            ..Default::default()
        });
        if self.cfg.is_muted(channel, &channel_name) {
            self.seen.push(id.clone());
        }
        self.state.open.insert(
            id.clone(),
            Open {
                id,
                first: m.ts.clone(),
                last: m.ts.clone(),
            },
        );
        Ok(())
    }

    /// Keep the cursor bounded: threads and open runs quiet for `thread_days`
    /// are forgotten (their items stay as they are), at most `max_threads`.
    fn prune(&mut self) {
        let cutoff = self.now - self.cfg.thread_days as f64 * 86_400.0;
        self.state.open.retain(|_, o| ts_secs(&o.last) >= cutoff);
        self.state.closed.retain(|_, last| ts_secs(last) >= cutoff);
        let open = &self.state.open;
        self.state
            .threads
            .retain(|k, t| ts_secs(&t.seen) >= cutoff || open.contains_key(k));
        if self.state.threads.len() > self.cfg.max_threads {
            let mut by_age: Vec<(f64, String)> = self
                .state
                .threads
                .iter()
                .map(|(k, t)| (ts_secs(&t.seen), k.clone()))
                .collect();
            by_age.sort_by(|a, b| a.0.total_cmp(&b.0));
            let extra = self.state.threads.len() - self.cfg.max_threads;
            for (_, k) in by_age.into_iter().filter(|(_, k)| !open.contains_key(k)).take(extra) {
                self.state.threads.remove(&k);
            }
        }
        let follow = &self.state.follow;
        self.state
            .convs
            .retain(|k, c| ts_secs(&c.seen) >= cutoff || follow.contains(k));
    }

    /// Display name for a user id, one `users.info` call per user per poll.
    fn user_name(&mut self, id: &str) -> anyhow::Result<String> {
        if let Some(n) = self.users.get(id) {
            return Ok(n.clone());
        }
        let name = match self.client.get("users.info", &[("user", id)]) {
            Ok(v) => {
                let s = |p: &str| {
                    v.pointer(p)
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_owned)
                };
                // Real name and email (`users:read.email`) for People (queries::people).
                let real_name = s("/user/profile/real_name").or_else(|| s("/user/real_name"));
                let email = s("/user/profile/email");
                let is_bot = id == "USLACKBOT" || v.pointer("/user/is_bot").and_then(Value::as_bool) == Some(true);
                if real_name.is_some() || email.is_some() || is_bot {
                    self.profiles.push(Profile {
                        kind: "slack".into(),
                        account: id.to_owned(),
                        real_name: real_name.clone(),
                        email,
                        is_bot,
                    });
                }
                s("/user/profile/display_name")
                    .or(real_name)
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
            (None, Some(b)) if !b.is_empty() => Ok(b.clone()),
            _ => match m.bot_id.clone() {
                Some(id) => self.bot_name(&id),
                None => Ok("bot".into()),
            },
        }
    }

    fn bot_name(&mut self, id: &str) -> anyhow::Result<String> {
        if let Some(n) = self.users.get(id) {
            return Ok(n.clone());
        }
        let name = match self.client.get("bots.info", &[("bot", id)]) {
            Ok(v) => v
                .pointer("/bot/name")
                .and_then(Value::as_str)
                .filter(|n| !n.is_empty())
                .unwrap_or("bot")
                .to_owned(),
            Err(e) if e.to_string().contains("bot_not_found") => "bot".into(),
            Err(e) => return Err(e),
        };
        self.users.insert(id.to_owned(), name.clone());
        Ok(name)
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
                format!(
                    "@{}",
                    match label {
                        Some(l) => l.to_owned(),
                        None => self.user_name(uid)?,
                    }
                )
            } else if let Some(cid) = target.strip_prefix('#') {
                let name = match label {
                    Some(l) => l.to_owned(),
                    None => {
                        if !self.channel_names.contains_key(cid) && self.info(cid).is_err() {
                            // Private or deleted: keep the id, and don't ask again this poll.
                            self.channel_names.insert(cid.to_owned(), cid.to_owned());
                        }
                        self.channel_names.get(cid).cloned().unwrap_or_else(|| cid.to_owned())
                    }
                };
                format!("#{name}")
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
        Ok(slack_markup(&unescape(&out)))
    }
}

/// One `search.messages` match.
struct Hit {
    ts: String,
    user: String,
    username: String,
    text: String,
    channel: String,
    channel_name: String,
    dm: bool,
    /// Search matches don't carry `thread_ts`; a reply's permalink does.
    thread_ts: Option<String>,
    permalink: Option<String>,
}

impl Hit {
    fn parse(m: &Value) -> Hit {
        let s = |k: &str| m.get(k).and_then(Value::as_str).unwrap_or_default().to_owned();
        let permalink = s("permalink");
        let channel = m
            .pointer("/channel/id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let thread_ts = m
            .get("thread_ts")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .or_else(|| {
                permalink
                    .split("thread_ts=")
                    .nth(1)
                    .map(|t| t.split('&').next().unwrap_or(t).to_owned())
            });
        let b = |k: &str| {
            m.pointer(&format!("/channel/{k}"))
                .and_then(Value::as_bool)
                .unwrap_or(false)
        };
        Hit {
            ts: s("ts"),
            user: s("user"),
            username: s("username"),
            text: s("text"),
            channel_name: m
                .pointer("/channel/name")
                .and_then(Value::as_str)
                .unwrap_or(&channel)
                .to_owned(),
            dm: b("is_im") || b("is_mpim"),
            channel,
            thread_ts,
            permalink: Some(permalink).filter(|p| !p.is_empty()),
        }
    }
}

fn looks_like_id(s: &str) -> bool {
    s.len() >= 9
        && (s.starts_with('C') || s.starts_with('G'))
        && s.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
}

/// "mpdm-alice--bob--me-1" → "alice, bob".
fn mpim_names(name: &str, me: &str) -> String {
    let inner = name.strip_prefix("mpdm-").unwrap_or(name);
    let inner = inner.rsplit_once('-').map(|(a, _)| a).unwrap_or(inner);
    inner.split("--").filter(|h| *h != me).collect::<Vec<_>>().join(", ")
}

#[cfg(test)]
fn clean_plain(text: &str) -> String {
    let re = regex::Regex::new(r"<[^|>]*\|([^>]*)>|<[@#!]?([^>]*)>").expect("static regex");
    unescape(&re.replace_all(text, "$1$2"))
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&amp;", "&")
}

/// Slack's single-star bold and single-tilde strike to GFM's doubled form.
fn slack_markup(s: &str) -> String {
    let bold = regex::Regex::new(r"\*([^*\n]+)\*").expect("static regex");
    let strike = regex::Regex::new(r"~([^~\n]+)~").expect("static regex");
    strike
        .replace_all(&bold.replace_all(s, "**$1**"), "~~$1~~")
        .into_owned()
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

    #[test]
    fn files_are_always_listed() {
        let m = Msg::parse(&json!({ "ts": "1.0", "text": "look", "files": [{ "name": "image.png", "mimetype": "image/png" }, { "name": "q3.pdf", "mimetype": "application/pdf" }] })).unwrap();
        assert_eq!(m.text, "look\n[image] image.png\n[file] q3.pdf");
    }

    #[test]
    fn empty_text_falls_back_to_attachments_and_blocks() {
        let dd = json!({ "ts": "1.0", "bot_id": "B0DD", "text": "", "attachments": [{
            "title": "Triggered: Query Monitor Disabled on uams",
            "text": "* Validate the query works",
            "fields": [{ "title": "Tags", "value": "@stack_name:uams" }],
            "fallback": "ignored"
        }]});
        let m = Msg::parse(&dd).unwrap();
        assert_eq!(
            m.text,
            "Triggered: Query Monitor Disabled on uams\n* Validate the query works\nTags: @stack_name:uams"
        );
        assert_eq!(m.bot_id.as_deref(), Some("B0DD"));

        let blocks = json!({ "ts": "2.0", "text": "", "blocks": [
            { "type": "header", "text": { "type": "plain_text", "text": "Deploy" } },
            { "type": "rich_text", "elements": [{ "type": "rich_text_section", "elements": [
                { "type": "text", "text": "done by " }, { "type": "user", "user_id": "U0ME" }
            ]}]}
        ]});
        assert_eq!(Msg::parse(&blocks).unwrap().text, "Deploy\ndone by <@U0ME>");
    }

    #[test]
    fn marker_ignores_slacks_read_state() {
        let m = Marker {
            from: None,
            after: Some("90.0".into()),
            closed: None,
            floor: 10.0,
        };
        assert!(!m.unread("90.0") && m.unread("91.0"));
        assert_eq!(m.oldest(), 90.0);
        // An open run keeps everything from its start, older than the floor or not.
        let m = Marker {
            from: Some("5.000000".into()),
            after: Some("90.0".into()),
            closed: None,
            floor: 10.0,
        };
        assert!(m.unread("5.000000") && m.unread("50.0") && !m.unread("4.9"));
        assert_eq!(m.oldest(), 5.0);
        // A triaged run's messages never come back.
        let m = Marker {
            from: None,
            after: Some("20.0".into()),
            closed: Some("60.0".into()),
            floor: 10.0,
        };
        assert!(!m.unread("60.0") && m.unread("60.1"));
        assert_eq!(m.oldest(), 60.0);
        // Nothing read yet: the lookback floor bounds it.
        let m = Marker {
            from: None,
            after: None,
            closed: None,
            floor: 10.0,
        };
        assert!(!m.unread("9.0") && m.unread("11.0"));
        assert_eq!(m.oldest(), 10.0);
    }

    #[test]
    fn cleans_mrkdwn() {
        assert_eq!(
            clean_plain("Ping <@U1|sam> in <#C1|inc> see <https://x.io|doc> &amp; <!here>"),
            "Ping sam in inc see doc & here"
        );
        assert_eq!(mpim_names("mpdm-alice--bob--me-1", "me"), "alice, bob");
        assert_eq!(first_words("one two three", 2), "one two…");
        assert_eq!(truncate("abcdef", 4), "abc…");
        assert!(looks_like_id("C0123ABCD") && !looks_like_id("inc-2291"));
        assert_eq!(
            slack_markup("*bold* and ~gone~ and _italic_ stays"),
            "**bold** and ~~gone~~ and _italic_ stays"
        );
    }

    #[test]
    fn search_hits_find_their_thread_in_the_permalink() {
        let h = Hit::parse(
            &json!({ "ts": "2.0", "permalink": "https://a.slack.com/archives/C1/p2000000?thread_ts=1.000100&cid=C1", "channel": { "id": "C1", "name": "ops" } }),
        );
        assert_eq!((h.channel.as_str(), h.thread_ts.as_deref()), ("C1", Some("1.000100")));
        let h = Hit::parse(
            &json!({ "ts": "2.0", "permalink": "https://a.slack.com/archives/C1/p2000000", "channel": { "id": "C1" } }),
        );
        assert_eq!(h.thread_ts, None);
    }
}

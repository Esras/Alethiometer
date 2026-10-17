//! Turning Teams into signals, by the chat model (`docs/chat.md`): every
//! message is captured, yours included; Teams' read state plays no part; only
//! triage here closes a signal.
//!
//! **Streams.** A chat (1:1, group, meeting) is one stream; Teams chats have
//! no threads. A listed channel is one stream for its posts, and each post
//! with replies is a thread stream, like a Slack thread: its replies attach to
//! the post when an item holds it (`ItemUpsert::attach_to`), else they get a
//! thread item `<team>|<channel>/thread/<post id>` with the post as context.
//!
//! **Keys** are Teams' message ids, which are their creation times in
//! milliseconds (`api::key_after`). Items: `<chat>/<first id>` and
//! `<team>|<channel>/<first id>` for runs of top-level messages.
//!
//! **What is fetched** each poll: your `max_chats` most recently active chats
//! (one call), then the messages of those with something new (by
//! `lastModifiedDateTime`, so edits show too); per listed channel its newest
//! posts, and the replies of posts that changed or of tracked threads (those
//! active in the last day every poll, older ones hourly).
//!
//! The cursor (`State`) holds ids, times and titles, never message text.

use super::super::chat::{Open, Runs};
use super::api::{enc, key_after, Client, GraphError};
use super::Config;
use crate::sync::{Batch, Ctx, Handle, ItemUpsert, Message, Polling, Profile, StreamOutcome};
use crate::time::Ts;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// At most this many messages in one item; older ones of a long run stay stored.
const MAX_MSGS: usize = 50;
/// Messages before a run, shown as context.
const CONTEXT_MSGS: usize = 3;
/// How far before a run to look for its context.
const CONTEXT_HOURS: i64 = 24;
/// Pages of 50 per chat, channel or thread per poll.
const PAGES: usize = 10;
/// A stream not found this many polls in a row is dropped.
const DROP_AFTER: i64 = 3;
/// Threads active more recently than this are checked every poll; others hourly.
const HOT_THREAD_MS: i64 = 24 * 3600 * 1000;
const COLD_THREAD_CHECK_MS: i64 = 3600 * 1000;
/// Profiles already sent, remembered so a poll doesn't look everyone up again.
const MAX_PROFILED: usize = 2000;

/// Stored as the source cursor.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct State {
    /// Your user id and name (`/me`) and your tenant's id (`/organization`):
    /// mark read needs both.
    #[serde(default)]
    pub me: String,
    #[serde(default)]
    pub me_name: String,
    #[serde(default)]
    pub tenant: String,
    #[serde(default)]
    pub chats: BTreeMap<String, ConvState>,
    /// Listed channels, by "<team>|<channel>".
    #[serde(default)]
    pub channels: BTreeMap<String, ConvState>,
    /// Channel threads, by "<team>|<channel>/<post id>".
    #[serde(default)]
    pub threads: BTreeMap<String, ThreadState>,
    #[serde(default)]
    pub open: BTreeMap<String, Open>,
    #[serde(default)]
    pub closed: BTreeMap<String, String>,
    /// Users whose profile was sent (`Batch::profiles`).
    #[serde(default)]
    pub profiled: Vec<String>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct ConvState {
    /// Newest message id read.
    #[serde(default)]
    pub seen: String,
    /// Newest `lastModifiedDateTime` read (a channel: of its posts).
    #[serde(default)]
    pub modified: String,
    #[serde(default)]
    pub title: String,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct ThreadState {
    /// Newest reply id read.
    pub seen: String,
    #[serde(default)]
    pub title: String,
    /// Last reply (or the post) time, Unix ms: how long it stays tracked.
    #[serde(default)]
    pub active: i64,
    /// Last check, Unix ms.
    #[serde(default)]
    pub checked: i64,
}

impl State {
    pub fn parse(cursor: Option<&str>) -> State {
        cursor.and_then(|c| serde_json::from_str(c).ok()).unwrap_or_default()
    }
}

impl Runs for State {
    fn runs(&mut self) -> (&mut BTreeMap<String, Open>, &mut BTreeMap<String, String>) {
        (&mut self.open, &mut self.closed)
    }
}

/// One message, as far as signals care.
#[derive(Debug, Clone)]
pub(super) struct Msg {
    pub id: String,
    pub at: Ts,
    pub modified: String,
    pub user: Option<String>,
    pub name: String,
    pub bot: bool,
    pub text: String,
    pub url: Option<String>,
}

impl Msg {
    /// A chatMessage, or None for system events and deleted messages.
    pub fn parse(v: &Value) -> Option<Msg> {
        let s = |p: &str| v.pointer(p).and_then(Value::as_str);
        if s("/messageType") != Some("message") || !v.get("deletedDateTime").is_none_or(Value::is_null) {
            return None;
        }
        let (user, name, bot) = match (v.pointer("/from/user"), v.pointer("/from/application")) {
            (Some(u), _) if u.is_object() => (
                u.get("id").and_then(Value::as_str).map(str::to_owned),
                u.get("displayName")
                    .and_then(Value::as_str)
                    .unwrap_or("someone")
                    .to_owned(),
                false,
            ),
            (_, Some(a)) if a.is_object() => (
                a.get("id").and_then(Value::as_str).map(str::to_owned),
                a.get("displayName")
                    .and_then(Value::as_str)
                    .unwrap_or("an app")
                    .to_owned(),
                true,
            ),
            _ => (None, "someone".to_owned(), false),
        };
        let body = s("/body/content").unwrap_or_default();
        let mut text = if s("/body/contentType") == Some("html") {
            html_to_markdown(body, v.get("attachments"))
        } else {
            body.trim().to_owned()
        };
        if text.is_empty() {
            text = attachment_names(v.get("attachments")).join(" ");
        }
        Some(Msg {
            id: s("/id")?.to_owned(),
            at: Ts::parse(s("/createdDateTime")?)?,
            modified: s("/lastModifiedDateTime").unwrap_or_default().to_owned(),
            user,
            name,
            bot,
            text,
            url: s("/webUrl").map(str::to_owned),
        })
    }
}

fn attachment_names(attachments: Option<&Value>) -> Vec<String> {
    attachments
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|a| a.get("contentType").and_then(Value::as_str) != Some("messageReference"))
        .map(
            |a| match a.get("name").and_then(Value::as_str).filter(|n| !n.is_empty()) {
                Some(n) => format!("[file: {n}]"),
                None => "[attachment]".to_owned(),
            },
        )
        .collect()
}

/// Teams' message HTML as Markdown (htmd). Its own elements: `<at>` (a
/// mention) keeps its text, `<emoji>` becomes its `alt`, `<attachment>` the
/// file's name.
pub(super) fn html_to_markdown(html: &str, attachments: Option<&Value>) -> String {
    use htmd::element_handler::Handlers;
    use htmd::Element;
    let names: HashMap<String, String> = attachments
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|a| {
            let id = a.get("id").and_then(Value::as_str)?;
            let name = a.get("name").and_then(Value::as_str).unwrap_or_default();
            Some((id.to_owned(), name.to_owned()))
        })
        .collect();
    let attr = |e: &Element, k: &str| {
        e.attrs
            .iter()
            .find(|a| a.name.local.as_ref() == k)
            .map(|a| a.value.to_string())
    };
    let converter = htmd::HtmlToMarkdown::builder()
        .add_handler(vec!["emoji"], move |_: &dyn Handlers, e: Element| {
            Some(attr(&e, "alt").unwrap_or_default().into())
        })
        .add_handler(vec!["attachment"], move |_: &dyn Handlers, e: Element| {
            let name = attr(&e, "id")
                .and_then(|id| names.get(&id).cloned())
                .filter(|n| !n.is_empty());
            Some(match name {
                Some(n) => format!("[file: {n}]").into(),
                None => "[attachment]".to_string().into(),
            })
        })
        .build();
    converter.convert(html).unwrap_or_default().trim().to_owned()
}

/// What makes a message part of the run: after the last triaged run
/// (`closed`), and from the open run's first message, else after what was
/// read (`after`) and within the lookback (`floor`).
struct Marker {
    from: Option<String>,
    after: Option<String>,
    closed: Option<String>,
    floor: Ts,
}

impl Marker {
    fn unread(&self, m: &Msg) -> bool {
        if self.closed.as_deref().is_some_and(|c| !key_after(&m.id, c)) {
            return false;
        }
        if let Some(from) = &self.from {
            return !key_after(from, &m.id);
        }
        m.at > self.floor && self.after.as_deref().is_none_or(|a| key_after(&m.id, a))
    }

    /// From when to read: the open run's start, else what was read, else the floor.
    fn since(&self) -> Ts {
        let at = |k: &Option<String>| {
            k.as_deref()
                .and_then(|k| k.parse::<i64>().ok())
                .and_then(Ts::from_unix_ms)
        };
        if let Some(t) = at(&self.from) {
            return t;
        }
        [at(&self.after), at(&self.closed), Some(self.floor)]
            .into_iter()
            .flatten()
            .max()
            .unwrap_or(self.floor)
    }
}

/// A chat or channel, as items name it.
struct Conv {
    /// The chat id, or "<team>|<channel>".
    id: String,
    title: String,
    /// dm | group | channel (`docs/chat.md`).
    kind: &'static str,
    url: Option<String>,
}

struct Poller<'a, 'r, 's> {
    client: Client<'a>,
    cfg: &'a Config,
    state: State,
    now: Ts,
    items: Vec<ItemUpsert>,
    seen: Vec<String>,
    profiles: Vec<Profile>,
    outcomes: Vec<StreamOutcome>,
    failures: BTreeMap<String, i64>,
    /// Threads read this poll.
    done_threads: BTreeSet<String>,
    streams: Option<&'r mut Polling<'s>>,
}

pub fn poll_with(ctx: &Ctx, cfg: &Config, streams: Option<&mut Polling>) -> anyhow::Result<Batch> {
    let mut client = Client::new(ctx.http, super::oauth::access_token(ctx, cfg)?);
    client.sleep = cfg.sleep;
    let mut p = Poller {
        client,
        cfg,
        state: State::parse(ctx.source.cursor.as_deref()),
        now: ctx.now,
        items: vec![],
        seen: vec![],
        profiles: vec![],
        outcomes: vec![],
        failures: streams.as_ref().map(|s| s.failures.clone()).unwrap_or_default(),
        done_threads: BTreeSet::new(),
        streams,
    };
    let result = p.run();
    if let Err(e) = &result {
        if e.downcast_ref::<GraphError>().is_some_and(GraphError::is_fatal) {
            super::oauth::invalidate(ctx, cfg);
        }
    }
    result?;
    p.batch()
}

impl Poller<'_, '_, '_> {
    fn batch(&mut self) -> anyhow::Result<Batch> {
        Ok(Batch {
            items: std::mem::take(&mut self.items),
            seen: std::mem::take(&mut self.seen),
            profiles: std::mem::take(&mut self.profiles),
            streams: std::mem::take(&mut self.outcomes),
            cursor: Some(serde_json::to_string(&self.state)?),
            ..Default::default()
        })
    }

    fn floor(&self) -> Ts {
        self.now - jiff::SignedDuration::from_hours(self.cfg.lookback_hours)
    }

    /// Read one stream. A Graph error about it alone (not found, forbidden: a
    /// channel without admin approval) is recorded and the poll goes on; a bad
    /// sign-in or the network ends the poll. What it produced is committed.
    fn stream(&mut self, key: &str, f: impl FnOnce(&mut Self) -> anyhow::Result<()>) -> anyhow::Result<()> {
        let error = match f(self) {
            Ok(()) => None,
            Err(e) => match e.downcast_ref::<GraphError>() {
                Some(g) if !g.is_fatal() => {
                    if g.is_not_found() && self.failures.get(key).copied().unwrap_or(0) + 1 >= DROP_AFTER {
                        self.state.chats.remove(key);
                        self.state.channels.remove(key);
                        self.state.threads.remove(key);
                        self.state.open.remove(key);
                    }
                    Some(e.to_string())
                }
                _ => return Err(e),
            },
        };
        self.outcomes.retain(|o| o.key != key);
        self.outcomes.push(StreamOutcome {
            key: key.to_owned(),
            error,
        });
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
        Ok(())
    }

    fn run(&mut self) -> anyhow::Result<()> {
        if self.state.me.is_empty() {
            let me = self.client.get("/me?$select=id,displayName,mail,userPrincipalName")?;
            let s = |k: &str| me.get(k).and_then(Value::as_str).unwrap_or_default().to_owned();
            self.state.me = s("id");
            self.state.me_name = s("displayName");
            let email = Some(s("mail"))
                .filter(|m| !m.is_empty())
                .or_else(|| Some(s("userPrincipalName")).filter(|m| m.contains('@')));
            self.profiles.push(Profile {
                kind: "teams".into(),
                account: self.state.me.clone(),
                real_name: Some(self.state.me_name.clone()).filter(|n| !n.is_empty()),
                email: email.map(|e| e.to_lowercase()),
                is_bot: false,
            });
            self.state.profiled.push(self.state.me.clone());
        }
        if self.state.tenant.is_empty() {
            let org = self.client.get("/organization?$select=id")?;
            self.state.tenant = org
                .pointer("/value/0/id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
        }
        if self.cfg.chats {
            self.chats()?;
        }
        for (team, channel) in self.cfg.channel_ids()? {
            let key = format!("{team}|{channel}");
            self.stream(&key, |p| p.channel(&team, &channel))?;
        }
        self.threads()?;
        self.prune();
        Ok(())
    }

    // ---------------------------------------------------------------- chats

    fn chats(&mut self) -> anyhow::Result<()> {
        let max = self.cfg.max_chats;
        let mut listed = 0;
        let chats = self.client.get_paged(
            "/me/chats?$expand=lastMessagePreview&$orderby=lastMessagePreview/createdDateTime%20desc&$top=50",
            max.div_ceil(50).max(1),
            |page| {
                listed += page.len();
                listed >= max
            },
        )?;
        for chat in chats.into_iter().take(max) {
            let Some(id) = chat.get("id").and_then(Value::as_str).map(str::to_owned) else {
                continue;
            };
            let Some(newest) = chat.pointer("/lastMessagePreview/id").and_then(Value::as_str) else {
                continue;
            };
            let at = chat
                .pointer("/lastMessagePreview/createdDateTime")
                .and_then(Value::as_str)
                .and_then(Ts::parse);
            let prev = self.state.chats.get(&id);
            let open = self.state.open.contains_key(&id);
            let fresh = prev.is_none_or(|p| key_after(newest, &p.seen));
            let recent = at.is_some_and(|t| t > self.floor());
            // Nothing new since the last read, and nothing open whose messages could have changed.
            if !open && !(fresh && recent) {
                continue;
            }
            let newest = newest.to_owned();
            self.stream(&id.clone(), |p| p.chat(&id, &chat, &newest))?;
        }
        Ok(())
    }

    fn chat(&mut self, id: &str, chat: &Value, newest: &str) -> anyhow::Result<()> {
        let conv = self.chat_conv(id, chat)?;
        let prev = self.state.chats.get(id).cloned().unwrap_or_default();
        let marker = Marker {
            from: self.state.open.get(id).map(|o| o.first.clone()),
            after: Some(prev.seen.clone()).filter(|s| !s.is_empty()),
            closed: self.state.closed.get(id).cloned(),
            floor: self.floor(),
        };
        let since = marker.since() - jiff::SignedDuration::from_hours(CONTEXT_HOURS);
        let filter = since.timestamp().strftime("%Y-%m-%dT%H:%M:%S.000Z").to_string();
        let raw = self.client.get_paged(
            &format!(
                "/chats/{}/messages?$top=50&$orderby=lastModifiedDateTime%20desc&$filter=lastModifiedDateTime%20gt%20{filter}",
                enc(id)
            ),
            PAGES,
            |_| false,
        )?;
        let mut msgs: Vec<Msg> = raw.iter().filter_map(Msg::parse).filter(|m| m.at > since).collect();
        msgs.sort_by(|a, b| key_cmp(&a.id, &b.id));
        msgs.dedup_by(|a, b| a.id == b.id);
        let mut state = ConvState {
            title: conv.title.clone(),
            ..prev
        };
        if key_after(newest, &state.seen) {
            state.seen = newest.to_owned();
        }
        self.state.chats.insert(id.to_owned(), state);
        let start = msgs.iter().position(|m| marker.unread(m)).unwrap_or(msgs.len());
        let context: Vec<Msg> = msgs[..start].iter().rev().take(CONTEXT_MSGS).rev().cloned().collect();
        let unread: Vec<Msg> = msgs.into_iter().filter(|m| marker.unread(m)).collect();
        // New signal is judged against what was read before this poll (`marker.after`).
        self.emit(&conv, None, id, unread, marker.after.as_deref(), context, true)
    }

    /// A chat's title, kind and link; members are read once for its title.
    fn chat_conv(&mut self, id: &str, chat: &Value) -> anyhow::Result<Conv> {
        let s = |k: &str| chat.get(k).and_then(Value::as_str).filter(|s| !s.is_empty());
        let (kind, label) = match s("chatType") {
            Some("oneOnOne") => ("dm", "Chat"),
            Some("meeting") => ("group", "Meeting chat"),
            _ => ("group", "Group chat"),
        };
        let title = match (s("topic"), self.state.chats.get(id).map(|c| c.title.clone())) {
            (Some(t), _) => format!("{label} · {t}"),
            (None, Some(t)) if !t.is_empty() => t,
            (None, _) => {
                let members = self.client.get(&format!("/chats/{}/members", enc(id)))?;
                let others: Vec<String> = members
                    .get("value")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter(|m| m.get("userId").and_then(Value::as_str) != Some(self.state.me.as_str()))
                    .filter_map(|m| m.get("displayName").and_then(Value::as_str).map(str::to_owned))
                    .collect();
                if others.is_empty() {
                    label.to_owned()
                } else {
                    format!("{label} · {}", others.join(", "))
                }
            }
        };
        Ok(Conv {
            id: id.to_owned(),
            title,
            kind,
            url: s("webUrl").map(str::to_owned),
        })
    }

    // ---------------------------------------------------------------- channels

    /// A listed channel's new posts as one run, and the threads of posts that changed.
    fn channel(&mut self, team: &str, channel: &str) -> anyhow::Result<()> {
        let key = format!("{team}|{channel}");
        let prev = self.state.channels.get(&key).cloned().unwrap_or_default();
        let title = if prev.title.is_empty() {
            let t = self.client.get(&format!("/teams/{}?$select=displayName", enc(team)))?;
            let c = self.client.get(&format!(
                "/teams/{}/channels/{}?$select=displayName",
                enc(team),
                enc(channel)
            ))?;
            let name = |v: &Value| v.get("displayName").and_then(Value::as_str).unwrap_or("?").to_owned();
            format!("#{} · {}", name(&c), name(&t))
        } else {
            prev.title.clone()
        };
        let conv = Conv {
            id: key.clone(),
            title,
            kind: "channel",
            url: None,
        };
        let marker = Marker {
            from: self.state.open.get(&key).map(|o| o.first.clone()),
            after: Some(prev.seen.clone()).filter(|s| !s.is_empty()),
            closed: self.state.closed.get(&key).cloned(),
            floor: self.floor(),
        };
        let since = marker.since() - jiff::SignedDuration::from_hours(CONTEXT_HOURS);
        let modified_before = Ts::parse(&prev.modified);
        let modified = |m: &Value| {
            m.get("lastModifiedDateTime")
                .and_then(Value::as_str)
                .and_then(Ts::parse)
        };
        // Newest posts first; stop at a page that's all older than what's been read.
        let raw = self.client.get_paged(
            &format!("/teams/{}/channels/{}/messages?$top=50", enc(team), enc(channel)),
            PAGES,
            |page| {
                page.iter().all(|m| {
                    let at = m.get("createdDateTime").and_then(Value::as_str).and_then(Ts::parse);
                    modified(m) <= modified_before && at.is_none_or(|t| t <= since)
                })
            },
        )?;
        let mut posts: Vec<Msg> = raw.iter().filter_map(Msg::parse).collect();
        posts.sort_by(|a, b| key_cmp(&a.id, &b.id));
        posts.dedup_by(|a, b| a.id == b.id);
        // Posts whose replies may have changed: modified since the last read, within the thread window.
        let window = self.now - jiff::SignedDuration::from_hours(24 * self.cfg.thread_days);
        let changed: Vec<Msg> = posts
            .iter()
            .filter(|m| Ts::parse(&m.modified) > modified_before && m.at > window.max(self.floor()))
            .cloned()
            .collect();
        let mut state = ConvState {
            title: conv.title.clone(),
            ..prev
        };
        if let Some(newest) = posts.last() {
            if key_after(&newest.id, &state.seen) {
                state.seen = newest.id.clone();
            }
        }
        if let Some(m) = posts.iter().filter_map(|m| Ts::parse(&m.modified)).max() {
            if Some(m) > modified_before {
                state.modified = m.to_db();
            }
        }
        let posts: Vec<Msg> = posts.into_iter().filter(|m| m.at > since).collect();
        let start = posts.iter().position(|m| marker.unread(m)).unwrap_or(posts.len());
        let context: Vec<Msg> = posts[..start].iter().rev().take(CONTEXT_MSGS).rev().cloned().collect();
        let unread: Vec<Msg> = posts.into_iter().filter(|m| marker.unread(m)).collect();
        self.state.channels.insert(key.clone(), state);
        self.emit(&conv, None, &key, unread, marker.after.as_deref(), context, false)?;
        for post in changed {
            let tkey = format!("{key}/{}", post.id);
            self.stream(&tkey, |p| p.thread(team, channel, &post.id, Some(post.clone())))?;
        }
        Ok(())
    }

    /// Tracked threads due a check: active in the last day every poll, older ones hourly.
    fn threads(&mut self) -> anyhow::Result<()> {
        let listed: BTreeSet<String> = self
            .cfg
            .channel_ids()?
            .into_iter()
            .map(|(t, c)| format!("{t}|{c}"))
            .collect();
        let now = self.now.unix_ms();
        let due: Vec<String> = self
            .state
            .threads
            .iter()
            .filter(|(k, t)| {
                let hot = now - t.active < HOT_THREAD_MS;
                !self.done_threads.contains(*k) && (hot || now - t.checked >= COLD_THREAD_CHECK_MS)
            })
            .map(|(k, _)| k.clone())
            .collect();
        for key in due {
            let Some((conv, post)) = key.split_once('/') else {
                continue;
            };
            let Some((team, channel)) = super::split_conv(conv) else {
                continue;
            };
            if !listed.contains(conv) {
                continue;
            }
            let (team, channel, post) = (team.to_owned(), channel.to_owned(), post.to_owned());
            self.stream(&key, |p| p.thread(&team, &channel, &post, None))?;
        }
        Ok(())
    }

    /// One channel thread: its unread replies, attached to the post.
    fn thread(&mut self, team: &str, channel: &str, post_id: &str, post: Option<Msg>) -> anyhow::Result<()> {
        let conv_id = format!("{team}|{channel}");
        let key = format!("{conv_id}/{post_id}");
        if !self.done_threads.insert(key.clone()) {
            return Ok(());
        }
        let base = format!(
            "/teams/{}/channels/{}/messages/{}",
            enc(team),
            enc(channel),
            enc(post_id)
        );
        let post = match post {
            Some(p) => Some(p),
            None => Msg::parse(&self.client.get(&base)?),
        };
        let prev = self.state.threads.get(&key).cloned();
        let raw = self
            .client
            .get_paged(&format!("{base}/replies?$top=50"), PAGES, |_| false)?;
        let mut replies: Vec<Msg> = raw.iter().filter_map(Msg::parse).collect();
        replies.sort_by(|a, b| key_cmp(&a.id, &b.id));
        replies.dedup_by(|a, b| a.id == b.id);
        let now = self.now.unix_ms();
        let Some(newest) = replies.last().cloned() else {
            // No replies yet: track the post so its first reply is caught.
            if let Some(p) = &post {
                self.state.threads.entry(key).or_insert(ThreadState {
                    seen: p.id.clone(),
                    title: String::new(),
                    active: p.at.unix_ms(),
                    checked: now,
                });
            }
            return Ok(());
        };
        let channel_title = self
            .state
            .channels
            .get(&conv_id)
            .map(|c| c.title.clone())
            .unwrap_or_else(|| "Teams channel".into());
        let title = match prev.as_ref().map(|t| t.title.clone()).filter(|t| !t.is_empty()) {
            Some(t) => t,
            None => {
                let words = post.as_ref().map(|p| first_words(&p.text, 6)).unwrap_or_default();
                if words.is_empty() {
                    format!("{channel_title} · thread")
                } else {
                    format!("{channel_title} · thread: {words}")
                }
            }
        };
        let prev_seen = prev.as_ref().map(|t| t.seen.clone()).filter(|s| !s.is_empty());
        let marker = Marker {
            from: self.state.open.get(&key).map(|o| o.first.clone()),
            after: prev_seen.clone(),
            closed: self.state.closed.get(&key).cloned(),
            // The floor bounds the first read of a thread, not its age.
            floor: if prev.is_some() { Ts::default() } else { self.floor() },
        };
        self.state.threads.insert(
            key.clone(),
            ThreadState {
                seen: newest.id.clone(),
                title: title.clone(),
                active: newest.at.unix_ms(),
                checked: now,
            },
        );
        let start = replies.iter().position(|m| marker.unread(m)).unwrap_or(replies.len());
        let mut context: Vec<Msg> = post.into_iter().collect();
        context.extend(replies[..start].iter().skip(start.saturating_sub(MAX_MSGS)).cloned());
        let unread: Vec<Msg> = replies.into_iter().filter(|m| marker.unread(m)).collect();
        let conv = Conv {
            id: conv_id,
            title: channel_title,
            kind: "channel",
            url: None,
        };
        self.emit(
            &conv,
            Some((post_id, &title)),
            &key,
            unread,
            prev_seen.as_deref(),
            context,
            false,
        )
    }

    /// Stop tracking threads quiet for `thread_days`, then all but the `max_threads` most recent.
    fn prune(&mut self) {
        let cutoff = self.now.unix_ms() - self.cfg.thread_days * 24 * 3600 * 1000;
        self.state.threads.retain(|_, t| t.active >= cutoff);
        if self.state.threads.len() > self.cfg.max_threads {
            let mut by_age: Vec<(i64, String)> =
                self.state.threads.iter().map(|(k, t)| (t.active, k.clone())).collect();
            by_age.sort();
            let drop = by_age.len() - self.cfg.max_threads;
            for (_, k) in by_age.into_iter().take(drop) {
                self.state.threads.remove(&k);
            }
        }
        if self.state.profiled.len() > MAX_PROFILED {
            let extra = self.state.profiled.len() - MAX_PROFILED;
            self.state.profiled.drain(..extra);
        }
    }

    // ---------------------------------------------------------------- items

    /// Write or update the item for one stream's unread run.
    #[allow(clippy::too_many_arguments)]
    fn emit(
        &mut self,
        conv: &Conv,
        thread: Option<(&str, &str)>,
        key: &str,
        unread: Vec<Msg>,
        read_before: Option<&str>,
        context: Vec<Msg>,
        markable: bool,
    ) -> anyhow::Result<()> {
        if unread.is_empty() {
            return Ok(());
        }
        let open = self.state.open.get(key).cloned();
        let unread: Vec<Msg> = unread.into_iter().rev().take(MAX_MSGS).rev().collect();
        let (first, last) = (&unread[0], &unread[unread.len() - 1]);
        let bump = unread.iter().any(|m| read_before.is_none_or(|s| key_after(&m.id, s)));
        let unchanged = open.as_ref().is_some_and(|o| o.first == first.id && o.last == last.id);
        if !bump && unchanged {
            return Ok(());
        }
        let id = match (&open, thread) {
            (Some(o), _) => o.id.clone(),
            (None, Some((post, _))) => format!("{}/thread/{post}", conv.id),
            (None, None) => format!("{}/{}", conv.id, first.id),
        };
        self.profile(context.iter().chain(&unread))?;
        let messages: Vec<Message> = context
            .iter()
            .map(|m| (m, true))
            .chain(unread.iter().map(|m| (m, false)))
            .map(|(m, is_context)| Message {
                at: m.at,
                author: m.name.clone(),
                text: m.text.clone(),
                author_id: m.user.clone(),
                context: is_context,
            })
            .collect();
        let body = unread
            .iter()
            .map(|m| format!("{}: {}", m.name, m.text))
            .collect::<Vec<_>>()
            .join("\n");
        let author = unread
            .iter()
            .rev()
            .find(|m| m.user.as_deref().is_some_and(|u| u != self.state.me))
            .map(|m| Handle {
                handle: m.user.clone().unwrap_or_default(),
                display_name: m.name.clone(),
            });
        let mut raw = json!({
            "channel": conv.id,
            "conv": conv.kind,
            // How ignore suggestions name it (`queries::ignore`).
            "name": conv.title,
            "first_ts": first.id,
            "last_ts": last.id,
            // One id per stored message, in the same order.
            "msgs": context.iter().chain(&unread).map(|m| m.id.as_str()).collect::<Vec<_>>(),
        });
        match thread {
            Some((post, _)) => raw["thread_ts"] = json!(post),
            // What `teams.mark_read` sends; Graph has no read marker for channels.
            None if markable => raw["mark_ts"] = json!(last.id),
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
            title: thread.map(|t| t.1.to_owned()).unwrap_or_else(|| conv.title.clone()),
            body,
            summary: Some(truncate(&last.text, 140)),
            url: last.url.clone().or_else(|| conv.url.clone()),
            author,
            occurred_at: first.at,
            ended_at: Some(last.at),
            status: "open".into(),
            msg_count: Some(unread.len() as i64),
            raw_json: raw,
            messages,
            bump,
            attach_to: thread.map(|t| t.0.to_owned()),
            ..Default::default()
        });
        let (conv_id, channel_id) = (conv.id.as_str(), super::split_conv(&conv.id).map(|c| c.1).unwrap_or(""));
        if self.cfg.is_muted(&[conv_id, channel_id]) {
            self.seen.push(id.clone());
        }
        self.state.open.insert(
            key.to_owned(),
            Open {
                id,
                first: first.id.clone(),
                last: last.id.clone(),
            },
        );
        Ok(())
    }

    /// Profiles (name, email) of authors not sent before: People notes match by them.
    fn profile<'m>(&mut self, msgs: impl Iterator<Item = &'m Msg>) -> anyhow::Result<()> {
        let mut todo: Vec<(String, String, bool)> = vec![];
        for m in msgs {
            if let Some(u) = &m.user {
                if !self.state.profiled.contains(u) && !todo.iter().any(|t| &t.0 == u) {
                    todo.push((u.clone(), m.name.clone(), m.bot));
                }
            }
        }
        for (id, name, bot) in todo {
            let email = if bot {
                None
            } else {
                match self.client.get(&format!(
                    "/users/{}?$select=id,displayName,mail,userPrincipalName",
                    enc(&id)
                )) {
                    Ok(u) => {
                        let s = |k: &str| u.get(k).and_then(Value::as_str).filter(|s| s.contains('@'));
                        s("mail").or(s("userPrincipalName")).map(str::to_lowercase)
                    }
                    // A guest or another tenant's user: name only.
                    Err(e) if e.downcast_ref::<GraphError>().is_some_and(|g| !g.is_fatal()) => None,
                    Err(e) => return Err(e),
                }
            };
            self.profiles.push(Profile {
                kind: "teams".into(),
                account: id.clone(),
                real_name: Some(name),
                email,
                is_bot: bot,
            });
            self.state.profiled.push(id);
        }
        Ok(())
    }
}

/// Keys in time order (numbers; text when one isn't).
fn key_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    match (a.parse::<u64>(), b.parse::<u64>()) {
        (Ok(x), Ok(y)) => x.cmp(&y),
        _ => a.cmp(b),
    }
}

fn first_words(text: &str, n: usize) -> String {
    let words: Vec<&str> = text.split_whitespace().take(n + 1).collect();
    if words.len() > n {
        format!("{}…", words[..n].join(" "))
    } else {
        words.join(" ")
    }
}

fn truncate(s: &str, n: usize) -> String {
    let line = s.lines().next().unwrap_or_default();
    if line.chars().count() <= n {
        line.to_owned()
    } else {
        format!("{}…", line.chars().take(n - 1).collect::<String>())
    }
}

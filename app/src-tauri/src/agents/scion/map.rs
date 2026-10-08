//! What the supervisor fetched from a Hub (`State`) → the Agents tab's model.
//! `build` is pure: groups, sessions, thread rows and asks, ready to publish.
//!
//! - A group thread (conversation `kind: group`) is a Group with a Room; its
//!   agent participants are the group's sessions. An agent in several threads
//!   goes under the most recently active one. Agents in no thread go in one
//!   group per project, `…:agents`.
//! - Rows are one per message. A send to several agents is stored by the Hub
//!   as one row per recipient (the primary, then `mention` copies with the
//!   same sender, body and time); those fold into one row whose delivery
//!   counts the recipients it reached.
//! - A session's own thread is its DM with you, the room messages it sent or
//!   received, its agent ↔ agent messages, and state changes seen while
//!   polling. The Hub doesn't expose the harness transcript (tool calls,
//!   output) over the API; `assistant-reply` (the Stop-hook mirror of each
//!   turn's final text) is the closest thing and shows as its Output.

use crate::agents::explain::short_model;
use crate::agents::model::{Choice, Delivery, Fact, Group, Kind, Request, Room, Row, Session, Stats, Status, Who};
use crate::time::Ts;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};

/// The Hub's mention loop cap (`pkg/hub/mention_pair_limiter.go`): a constant
/// there, not readable over the API.
pub const LOOP_CAP: &str = "loop cap 10 / 10m";

#[derive(Default)]
pub struct State {
    pub me_id: String,
    pub me_email: String,
    pub projects: Vec<Value>,
    pub agents: Vec<Value>,
    pub convs: BTreeMap<String, Conv>,
    /// Agent id → its agent ↔ agent messages.
    pub interagent: HashMap<String, Vec<Value>>,
    /// Agent id → (Hub time, "running → completed"), seen between polls.
    pub transitions: HashMap<String, Vec<(String, String)>>,
    /// Your sends not yet listed by the Hub.
    pub pending: Vec<Pending>,
    /// Row exts you discarded.
    pub hidden: HashSet<String>,
    /// Asks you answered (message ids).
    pub answered: HashSet<String>,
}

#[derive(Default, Clone)]
pub struct Conv {
    pub meta: Value,
    pub participants: Vec<Value>,
    /// Oldest first.
    pub messages: Vec<Value>,
    /// `lastActivityAt` when the messages were read.
    pub read_at: String,
}

#[derive(Clone, Debug)]
pub struct Pending {
    pub key: String,
    /// `ext` is `local:<n>` until the Hub answers with the message id.
    pub row: Row,
}

pub struct Cx<'a> {
    pub source: &'a str,
    /// Web UI base, no trailing slash.
    pub web: &'a str,
    pub live: bool,
}

#[derive(Default)]
pub struct Snapshot {
    pub groups: Vec<Group>,
    pub sessions: Vec<Session>,
    pub threads: Vec<(String, Vec<Row>)>,
    pub requests: Vec<Request>,
}

pub fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v.get(k).and_then(Value::as_str).unwrap_or("")
}

pub fn session_id(source: &str, agent: &str) -> String {
    format!("scion:{source}:{agent}")
}

pub fn ask_id(source: &str, message: &str) -> String {
    format!("scion:{source}:ask:{message}")
}

/// Milliseconds since the epoch; 0 for empty or Go's zero time.
pub fn ms(t: &str) -> i64 {
    Ts::parse(t).map(Ts::unix_ms).filter(|m| *m > 0).unwrap_or(0)
}

/// Hub time (RFC 3339) → the store's form (`Ts::to_db`); empty for empty or Go's zero time.
pub fn local(t: &str) -> String {
    Ts::parse(t)
        .filter(|d| d.unix_ms() > 0)
        .map(Ts::to_db)
        .unwrap_or_default()
}

/// "claude" → "Claude Code".
pub fn harness_label(h: &str) -> String {
    match h {
        "claude" => "Claude Code".into(),
        "gemini" | "gemini-cli" => "Gemini CLI".into(),
        "codex" => "Codex".into(),
        "opencode" => "OpenCode".into(),
        "pi" => "Pi".into(),
        "copilot" => "Copilot".into(),
        "generic" => "Generic".into(),
        other => {
            let mut c = other.chars();
            c.next()
                .map(|f| f.to_uppercase().chain(c).collect())
                .unwrap_or_default()
        }
    }
}

/// The status mark. Failed: phase `error`, or activity limits_exceeded,
/// stalled or crashed. Waiting: an open ask, or waiting_for_input / blocked.
/// Running: starting up, or working / thinking / executing. Done: activity
/// `completed`, running or stopped. Idle: everything else (idle, offline,
/// stopped, suspended).
pub fn status(agent: &Value, ask: bool) -> Status {
    let (phase, activity) = (s(agent, "phase"), s(agent, "activity"));
    if phase == "error" || matches!(activity, "limits_exceeded" | "stalled" | "crashed") {
        return Status::Failed;
    }
    if ask || (phase == "running" && matches!(activity, "waiting_for_input" | "blocked")) {
        return Status::Waiting;
    }
    match (phase, activity) {
        ("created" | "provisioning" | "cloning" | "starting", _) => Status::Running,
        ("running", "working" | "thinking" | "executing") => Status::Running,
        ("running" | "stopped", "completed") => Status::Done,
        _ => Status::Idle,
    }
}

/// Short model name: "sonnet" for Claude ids; other vendors' ids as they are.
pub fn model_label(m: &str) -> String {
    if m.contains("claude") || m.contains("anthropic") {
        short_model(m)
    } else {
        m.to_owned()
    }
}

fn busy(agent: &Value) -> bool {
    s(agent, "phase") == "running" && matches!(s(agent, "activity"), "working" | "thinking" | "executing")
}

/// Choices read from an input-needed message: up to three `1)`, `1.`, `-`
/// or `*` lines, each a title with an optional detail after a colon or dash.
/// The lines before them are the question.
pub fn parse_choices(text: &str) -> (String, Vec<Choice>) {
    let mut question = vec![];
    let mut choices = vec![];
    for line in text.lines() {
        let t = line.trim();
        if let Some(body) = option_body(t) {
            if choices.len() < 3 {
                let (title, detail) = split_detail(body);
                choices.push(Choice {
                    key: (choices.len() + 1).to_string(),
                    title,
                    detail,
                    ..Default::default()
                });
            }
        } else if choices.is_empty() && !t.is_empty() {
            question.push(t);
        }
    }
    if choices.is_empty() {
        return (text.trim().to_owned(), vec![]);
    }
    (question.join(" "), choices)
}

fn option_body(t: &str) -> Option<&str> {
    for p in ["- ", "* ", "• "] {
        if let Some(r) = t.strip_prefix(p) {
            return Some(r.trim());
        }
    }
    let digits = t.chars().take_while(char::is_ascii_digit).count();
    if digits == 0 || digits > 2 {
        return None;
    }
    let rest = &t[digits..];
    rest.strip_prefix(')')
        .or_else(|| rest.strip_prefix('.'))
        .filter(|r| r.starts_with(' '))
        .map(str::trim)
}

fn split_detail(body: &str) -> (String, String) {
    for sep in [": ", " — ", " – ", " - "] {
        if let Some((a, b)) = body.split_once(sep) {
            return (a.trim().to_owned(), b.trim().to_owned());
        }
    }
    (body.to_owned(), String::new())
}

/// Title and subtitle from a task: its first sentence, then the rest.
pub fn title_from(task: &str) -> (String, String) {
    let t = task.trim();
    let first = t.lines().next().unwrap_or("");
    let cut = first.find(". ").map(|i| i + 1).unwrap_or(first.len());
    let rest = t[cut..].trim().replace('\n', " ");
    (
        crate::ai::context::clip(first[..cut].trim(), 90),
        crate::ai::context::clip(&rest, 140),
    )
}

#[derive(Clone, PartialEq)]
enum Party {
    Me,
    User(String),
    /// id (empty when unknown), name, slug.
    Agent(String, String, String),
    Hub,
    Thread,
}

/// One send: the Hub's rows for it, primary first.
struct Post<'a> {
    main: &'a Value,
    all: Vec<&'a Value>,
    conv: Option<&'a Conv>,
}

#[derive(Clone, Copy)]
enum View<'a> {
    Room,
    /// An agent's own thread (agent id).
    Own(&'a str),
}

struct Ix<'a> {
    cx: &'a Cx<'a>,
    st: &'a State,
    agents: HashMap<&'a str, &'a Value>,
    slugs: HashMap<&'a str, &'a Value>,
    /// Agent id → time of its latest message anywhere.
    last_sent: HashMap<String, i64>,
}

impl<'a> Ix<'a> {
    fn new(cx: &'a Cx<'a>, st: &'a State) -> Ix<'a> {
        let agents: HashMap<&str, &Value> = st.agents.iter().map(|a| (s(a, "id"), a)).collect();
        let slugs = st.agents.iter().map(|a| (s(a, "slug"), a)).collect();
        let mut last_sent: HashMap<String, i64> = HashMap::new();
        let all = st
            .convs
            .values()
            .flat_map(|c| c.messages.iter())
            .chain(st.interagent.values().flatten());
        for m in all {
            if s(m, "sender").starts_with("agent:") && !s(m, "senderId").is_empty() {
                let e = last_sent.entry(s(m, "senderId").to_owned()).or_default();
                *e = (*e).max(ms(s(m, "createdAt")));
            }
        }
        Ix {
            cx,
            st,
            agents,
            slugs,
            last_sent,
        }
    }

    fn sid(&self, agent: &str) -> String {
        session_id(self.cx.source, agent)
    }

    fn party(&self, addr: &str, id: &str) -> Party {
        let (kind, rest) = addr.split_once(':').unwrap_or((addr, ""));
        match kind {
            "user" if (!id.is_empty() && id == self.st.me_id) || rest == self.st.me_email || rest == self.st.me_id => {
                Party::Me
            }
            "user" => Party::User(rest.to_owned()),
            "agent" => {
                let a = self.agents.get(id).or_else(|| self.slugs.get(rest));
                match a {
                    Some(a) => Party::Agent(s(a, "id").into(), name_of(a).into(), s(a, "slug").into()),
                    None => Party::Agent(String::new(), rest.into(), rest.into()),
                }
            }
            "thread" => Party::Thread,
            _ => Party::Hub,
        }
    }

    fn who(&self, p: &Party) -> Option<Who> {
        match p {
            Party::Me => Some(Who::you()),
            Party::User(u) => Some(Who {
                name: u.split('@').next().unwrap_or(u).to_owned(),
                session: None,
                kind: "user".into(),
            }),
            Party::Agent(id, name, _) => Some(Who::agent(name, (!id.is_empty()).then(|| self.sid(id)))),
            Party::Hub => Some(Who {
                name: "Hub".into(),
                session: None,
                kind: "hub".into(),
            }),
            Party::Thread => None,
        }
    }

    fn sender(&self, m: &Value) -> Party {
        if s(m, "type") == "system" {
            return Party::Hub;
        }
        self.party(s(m, "sender"), s(m, "senderId"))
    }

    fn recipient(&self, m: &Value) -> Party {
        self.party(s(m, "recipient"), s(m, "recipientId"))
    }

    /// Fold each send's per-recipient rows into one post. `msgs` oldest first.
    fn posts(&self, msgs: Vec<(&'a Value, Option<&'a Conv>)>) -> Vec<Post<'a>> {
        let mut out: Vec<Post> = vec![];
        for (m, conv) in msgs {
            let same = out.last().is_some_and(|p| {
                ["sender", "createdAt", "msg", "conversationId"]
                    .iter()
                    .all(|k| s(p.main, k) == s(m, k))
                    && !s(m, "msg").is_empty()
            });
            if same {
                let p = out.last_mut().unwrap();
                p.all.push(m);
                if s(p.main, "type") == "mention" && s(m, "type") != "mention" {
                    p.main = m;
                }
            } else {
                out.push(Post {
                    main: m,
                    all: vec![m],
                    conv,
                });
            }
        }
        out
    }

    fn delivery(&self, p: &Post, from_me: bool) -> Option<Delivery> {
        if !p.all.iter().any(|m| !s(m, "dispatchState").is_empty()) {
            return None;
        }
        let failed: Vec<&&Value> = p.all.iter().filter(|m| s(m, "dispatchState") == "failed").collect();
        let reason = failed
            .iter()
            .map(|m| s(m, "dispatchFailureReason"))
            .find(|r| !r.is_empty())
            .unwrap_or("")
            .to_owned();
        let delivered: Vec<&&Value> = p.all.iter().filter(|m| s(m, "dispatchState") == "dispatched").collect();
        let names = |rows: &[&&Value]| -> Vec<String> {
            rows.iter()
                .filter_map(|m| match self.recipient(m) {
                    Party::Agent(_, n, _) => Some(n),
                    Party::User(u) => Some(u),
                    _ => None,
                })
                .collect()
        };
        let mut d = Delivery {
            count: delivered.len(),
            at: local(s(p.main, "createdAt")),
            delivered_at: delivered
                .first()
                .map(|m| local(s(m, "dispatchedAt")))
                .unwrap_or_default(),
            seen_by: names(
                &p.all
                    .iter()
                    .filter(|m| m["read"].as_bool() == Some(true))
                    .collect::<Vec<_>>(),
            ),
            ..Default::default()
        };
        let has = |st: &str| p.all.iter().any(|m| s(m, "dispatchState") == st);
        if failed
            .iter()
            .any(|m| s(m, "dispatchFailureReason").starts_with("Agent unreachable"))
        {
            d.state = "unreachable".into();
            d.reason = reason;
        } else if !failed.is_empty() {
            d.state = "failed".into();
            d.reason = reason;
        } else if has("deferred") {
            d.state = "deferred".into();
            d.reason = format!(
                "{} is reincarnating",
                names(
                    &p.all
                        .iter()
                        .filter(|m| s(m, "dispatchState") == "deferred")
                        .collect::<Vec<_>>()
                )
                .join(", ")
            );
        } else if has("pending") {
            d.state = "pending".into();
        } else {
            d.state = "delivered".into();
            // The broker buffers a message for a busy harness; until it speaks
            // again, call it deferred (the Hub has no state for this).
            let at = ms(s(p.main, "createdAt"));
            let holding: Vec<String> = p
                .all
                .iter()
                .filter_map(|m| match self.recipient(m) {
                    Party::Agent(id, n, _)
                        if from_me
                            && self.agents.get(id.as_str()).is_some_and(|a| busy(a))
                            && self.last_sent.get(&id).is_none_or(|t| *t <= at) =>
                    {
                        Some(n)
                    }
                    _ => None,
                })
                .collect();
            if !holding.is_empty() {
                d.state = "deferred".into();
                d.reason = format!("{} was mid-turn", holding.join(", "));
            }
        }
        if d.state != "delivered" {
            d.delivered_at.clear();
        }
        Some(d)
    }

    fn row(&self, p: &Post, view: View) -> Row {
        let m = p.main;
        let ty = s(m, "type");
        let sender = self.sender(m);
        let rcpts: Vec<Party> = p.all.iter().map(|m| self.recipient(m)).collect();
        let text = s(m, "msg");
        let room = p
            .conv
            .filter(|c| s(&c.meta, "kind") == "group")
            .map(|c| Who::room(s(&c.meta, "displayName"), s(&c.meta, "id")));
        let mentioned = match rcpts.as_slice() {
            [Party::Agent(id, name, slug)] if !slug.is_empty() && text.contains(&format!("@{slug}")) => {
                Some(Party::Agent(id.clone(), name.clone(), slug.clone()))
            }
            _ => None,
        };
        let own = |pt: &Party| matches!((view, pt), (View::Own(a), Party::Agent(id, _, _)) if id == a);
        let mut kind = match ty {
            "state-change" => Kind::State,
            "system" => Kind::Hub,
            "input-needed" => Kind::Ask,
            _ if sender == Party::Me => Kind::You,
            "assistant-reply" if own(&sender) => Kind::Output,
            _ => Kind::Message,
        };
        let to = match kind {
            Kind::State | Kind::Hub | Kind::Output => None,
            _ if matches!(rcpts.as_slice(), [Party::Me]) => Some(Who::you()),
            _ => match (&mentioned, &room) {
                (Some(a), _) if !own(a) => self.who(a),
                (_, Some(r)) => Some(r.clone()),
                // A DM in its own thread is to the session itself.
                _ if matches!(view, View::Own(_)) && !own(&sender) => None,
                _ => rcpts.first().and_then(|r| self.who(r)),
            },
        };
        let delivery = self.delivery(p, sender == Party::Me);
        if delivery.as_ref().is_some_and(|d| d.state == "failed") {
            kind = Kind::Failed;
        }
        let label = match ty {
            "assistant-reply" if kind == Kind::Output => "",
            "assistant-reply" => "reply",
            "chat" | "" => "message",
            t => t,
        };
        let note = match (view, &room) {
            (View::Own(_), Some(r)) => format!("in {}", r.name),
            _ => String::new(),
        };
        let open = kind == Kind::Ask && p.conv.is_some_and(|c| self.ask_open(m, c));
        Row {
            at: local(s(m, "createdAt")),
            kind,
            who: if own(&sender) { None } else { self.who(&sender) },
            to,
            label: label.into(),
            text: text.into(),
            note,
            delivery,
            request: open.then(|| ask_id(self.cx.source, s(m, "id"))),
            ext: s(m, "id").into(),
            ..Default::default()
        }
    }

    /// An ask is open until the asking agent posts again in the same
    /// conversation, you send it something there, you answer it here, or the
    /// agent stops.
    fn ask_open(&self, m: &Value, conv: &Conv) -> bool {
        if self.st.answered.contains(s(m, "id")) {
            return false;
        }
        let at = ms(s(m, "createdAt"));
        let asker = s(m, "senderId");
        let slug = self.agents.get(asker).map(|a| format!("@{}", s(a, "slug")));
        let alive = self
            .agents
            .get(asker)
            .is_none_or(|a| !matches!(s(a, "phase"), "stopped" | "stopping" | "error" | "suspended"));
        let answers = |x: &Value| {
            let to_asker =
                s(x, "recipientId") == asker || slug.as_ref().is_some_and(|sl| s(x, "msg").contains(sl.as_str()));
            (self.party(s(x, "sender"), s(x, "senderId")) == Party::Me && to_asker)
                || (!asker.is_empty() && s(x, "senderId") == asker)
        };
        alive && !conv.messages.iter().any(|x| ms(s(x, "createdAt")) > at && answers(x))
    }

    fn request(&self, m: &Value, thread: String) -> Request {
        let (question, choices) = parse_choices(s(m, "msg"));
        let (session, name) = match self.sender(m) {
            Party::Agent(id, n, _) => (if id.is_empty() { String::new() } else { self.sid(&id) }, n),
            _ => (String::new(), "An agent".into()),
        };
        Request {
            id: ask_id(self.cx.source, s(m, "id")),
            session_id: session,
            thread,
            kind: "ask".into(),
            at: local(s(m, "createdAt")),
            title: format!("{name} is waiting for you"),
            tag: "question".into(),
            question,
            choices,
            via: "input-needed".into(),
            ..Default::default()
        }
    }

    fn involves(&self, m: &Value, a: &Value) -> bool {
        let (id, slug) = (s(a, "id"), format!("agent:{}", s(a, "slug")));
        s(m, "senderId") == id || s(m, "recipientId") == id || s(m, "sender") == slug || s(m, "recipient") == slug
    }

    fn own_rows(&self, a: &Value) -> Vec<Row> {
        let id = s(a, "id");
        let mut seen = HashSet::new();
        let mut msgs: Vec<(&Value, Option<&Conv>)> = vec![];
        for c in self.st.convs.values() {
            let direct = s(&c.meta, "kind") == "direct";
            if direct && !s(&c.meta, "externalRef").contains(id) {
                continue;
            }
            for m in &c.messages {
                if (direct || self.involves(m, a)) && seen.insert(s(m, "id")) {
                    msgs.push((m, Some(c)));
                }
            }
        }
        for m in self.st.interagent.get(id).into_iter().flatten() {
            if seen.insert(s(m, "id")) {
                msgs.push((m, self.st.convs.get(s(m, "conversationId"))));
            }
        }
        msgs.sort_by(|(a, _), (b, _)| (ms(s(a, "createdAt")), s(a, "id")).cmp(&(ms(s(b, "createdAt")), s(b, "id"))));
        let mut rows: Vec<(i64, Row)> = self
            .posts(msgs)
            .iter()
            .map(|p| (ms(s(p.main, "createdAt")), self.row(p, View::Own(id))))
            .collect();
        let broker = s(a, "runtimeBrokerName");
        let started = [s(a, "template"), broker]
            .iter()
            .filter(|x| !x.is_empty())
            .copied()
            .collect::<Vec<_>>()
            .join(" · ");
        let created = s(a, "created");
        rows.push((
            ms(created),
            Row {
                at: local(created),
                kind: Kind::State,
                text: format!("started · {started}").trim_end_matches(" · ").into(),
                ext: "created".into(),
                ..Default::default()
            },
        ));
        // The task it was started with isn't a message on the Hub; it opens the thread as yours, unless the Hub also sent it as one.
        let task = s(&a["appliedConfig"], "task");
        if !task.is_empty()
            && !rows
                .iter()
                .any(|(_, r)| r.kind == Kind::You && r.text.trim() == task.trim())
        {
            rows.push((
                ms(created),
                Row {
                    at: local(created),
                    kind: Kind::You,
                    label: "task".into(),
                    text: task.into(),
                    ext: "task".into(),
                    ..Default::default()
                },
            ));
        }
        for (at, text) in self.st.transitions.get(id).into_iter().flatten() {
            rows.push((
                ms(at),
                Row {
                    at: local(at),
                    kind: Kind::State,
                    text: text.clone(),
                    ext: format!("st:{at}"),
                    ..Default::default()
                },
            ));
        }
        rows.sort_by_key(|(t, _)| *t);
        rows.into_iter().map(|(_, r)| r).collect()
    }

    fn room_rows(&self, c: &'a Conv) -> Vec<Row> {
        let msgs = c.messages.iter().map(|m| (m, Some(c))).collect();
        let mut rows: Vec<(i64, Row)> = self
            .posts(msgs)
            .iter()
            .map(|p| (ms(s(p.main, "createdAt")), self.row(p, View::Room)))
            .collect();
        for p in c
            .participants
            .iter()
            .filter(|p| s(p, "principalKind") == "agent" && p["leftAt"].is_null())
        {
            let Some(a) = self.agents.get(s(p, "principalId")) else {
                continue;
            };
            let at = s(p, "joinedAt");
            rows.push((
                ms(at),
                Row {
                    at: local(at),
                    kind: Kind::State,
                    who: self.who(&Party::Agent(s(a, "id").into(), name_of(a).into(), s(a, "slug").into())),
                    text: "joined".into(),
                    ext: format!("join:{}", s(a, "id")),
                    ..Default::default()
                },
            ));
        }
        rows.sort_by_key(|(t, _)| *t);
        rows.into_iter().map(|(_, r)| r).collect()
    }

    /// Agent ids that belong to a group thread: its agent participants, and
    /// any agent that posted or was addressed in it.
    fn members(&self, c: &Conv) -> Vec<String> {
        let mut out: Vec<String> = c
            .participants
            .iter()
            .filter(|p| s(p, "principalKind") == "agent" && p["leftAt"].is_null())
            .map(|p| s(p, "principalId").to_owned())
            .collect();
        for m in &c.messages {
            for p in [self.party(s(m, "sender"), s(m, "senderId")), self.recipient(m)] {
                if let Party::Agent(id, _, _) = p {
                    if !id.is_empty() && !out.contains(&id) {
                        out.push(id);
                    }
                }
            }
        }
        out.retain(|id| self.agents.contains_key(id.as_str()));
        out
    }
}

fn name_of(a: &Value) -> &str {
    let n = s(a, "name");
    if n.is_empty() {
        s(a, "slug")
    } else {
        n
    }
}

fn group_kind(c: &Conv) -> bool {
    s(&c.meta, "kind") == "group" && c.meta["deletedAt"].is_null() && c.meta["archivedAt"].is_null()
}

/// `thread:<project>:<topic>` → the native chat topic a group thread posts to.
pub fn topic_of(conv: &Value) -> Option<&str> {
    let r = s(conv, "externalRef").strip_prefix("thread:")?;
    r.split_once(':').map(|(_, t)| t).filter(|t| !t.is_empty())
}

/// Publishable state for one Hub. `color(group, name)` picks a palette index.
pub fn build(cx: &Cx, st: &State, color: &mut dyn FnMut(&str, &str) -> usize) -> Snapshot {
    let ix = Ix::new(cx, st);
    let mut snap = Snapshot::default();
    let projects: HashMap<&str, &Value> = st.projects.iter().map(|p| (s(p, "id"), p)).collect();
    let rooms: Vec<&Conv> = st
        .convs
        .values()
        .filter(|c| group_kind(c) && projects.contains_key(s(&c.meta, "projectId")))
        .collect();

    // Each agent's room: the most recently active thread it's in.
    let mut home: HashMap<String, &Conv> = HashMap::new();
    for c in &rooms {
        for id in ix.members(c) {
            let newer = home
                .get(&id)
                .is_none_or(|h| ms(s(&h.meta, "lastActivityAt")) < ms(s(&c.meta, "lastActivityAt")));
            if newer {
                home.insert(id, *c);
            }
        }
    }
    let gid = |pid: &str, tail: &str| format!("scion:{}:{pid}:{tail}", cx.source);
    let broker_of = |ids: &[&Value]| {
        let mut n: BTreeMap<&str, usize> = BTreeMap::new();
        for a in ids {
            *n.entry(s(a, "runtimeBrokerName")).or_default() += 1;
        }
        n.into_iter()
            .filter(|(b, _)| !b.is_empty())
            .max_by_key(|(_, c)| *c)
            .map(|(b, _)| b.to_owned())
            .unwrap_or_else(|| cx.source.to_owned())
    };

    for c in &rooms {
        let (pid, cid) = (s(&c.meta, "projectId"), s(&c.meta, "id"));
        let members: Vec<&Value> = ix
            .members(c)
            .iter()
            .filter_map(|id| ix.agents.get(id.as_str()).copied())
            .collect();
        let broker = broker_of(&members);
        let mode = s(&c.meta, "defaultAgentId");
        let mode = match ix.agents.get(mode) {
            Some(a) => format!("routes to @{}", s(a, "slug")),
            None => "mentions only".into(),
        };
        let url = match topic_of(&c.meta) {
            Some(t) => format!("{}/chat/space/{pid}/thread/{t}", cx.web),
            None => format!("{}/chat", cx.web),
        };
        let rows = ix.room_rows(c);
        snap.groups.push(Group {
            id: gid(pid, cid),
            source: "scion".into(),
            project: name_of(projects[pid]).into(),
            name: s(&c.meta, "displayName").into(),
            goal: String::new(),
            host: format!("Scion · {broker}"),
            room: Some(Room {
                id: cid.into(),
                name: s(&c.meta, "displayName").into(),
                members: members.len(),
                unread: 0,
                active_at: local(s(&c.meta, "lastActivityAt")),
                hub: format!("Scion Hub · {broker}"),
                thread_ref: format!("group thread {}", &cid[..cid.len().min(8)]),
                mode,
                loop_cap: LOOP_CAP.into(),
                messages: rows.iter().filter(|r| !r.ext.starts_with("join:")).count(),
                live: cx.live,
                url,
            }),
            created_at: local(s(&c.meta, "createdAt")),
        });
        snap.threads.push((format!("r:{cid}"), rows));
        for m in c
            .messages
            .iter()
            .filter(|m| s(m, "type") == "input-needed" && ix.ask_open(m, c))
        {
            snap.requests.push(ix.request(m, format!("r:{cid}")));
        }
    }

    // DM asks are pinned in the agent's own thread.
    for c in st.convs.values().filter(|c| s(&c.meta, "kind") == "direct") {
        for m in c
            .messages
            .iter()
            .filter(|m| s(m, "type") == "input-needed" && ix.ask_open(m, c))
        {
            let thread = match ix.sender(m) {
                Party::Agent(id, _, _) if !id.is_empty() => format!("s:{}", ix.sid(&id)),
                _ => continue,
            };
            snap.requests.push(ix.request(m, thread));
        }
    }

    let mut unsorted: BTreeMap<&str, Vec<&Value>> = BTreeMap::new();
    for a in &st.agents {
        if !home.contains_key(s(a, "id")) && projects.contains_key(s(a, "projectId")) {
            unsorted.entry(s(a, "projectId")).or_default().push(a);
        }
    }
    // A project with no room and no agent still gets its group, or nothing could start an agent in it.
    for p in &st.projects {
        let pid = s(p, "id");
        if !rooms.iter().any(|c| s(&c.meta, "projectId") == pid) {
            unsorted.entry(pid).or_default();
        }
    }
    for (pid, agents) in &unsorted {
        let p = projects[pid];
        snap.groups.push(Group {
            id: gid(pid, "agents"),
            source: "scion".into(),
            project: name_of(p).into(),
            name: name_of(p).into(),
            goal: String::new(),
            host: format!("Scion · {}", broker_of(agents)),
            room: None,
            created_at: local(s(p, "created")),
        });
    }

    for a in st.agents.iter().filter(|a| projects.contains_key(s(a, "projectId"))) {
        let id = s(a, "id");
        let sid = ix.sid(id);
        let room = home.get(id);
        let group = match room {
            Some(c) => gid(s(&c.meta, "projectId"), s(&c.meta, "id")),
            None => gid(s(a, "projectId"), "agents"),
        };
        let ask = snap.requests.iter().any(|r| r.session_id == sid);
        let cfg = &a["appliedConfig"];
        let task = if s(cfg, "task").is_empty() {
            s(a, "taskSummary")
        } else {
            s(cfg, "task")
        };
        let (title, rest) = title_from(task);
        let running = s(a, "phase") == "running";
        let detail = s(a, "message");
        let project = projects[s(a, "projectId")];
        let branch = if s(cfg, "branch").is_empty() {
            s(a, "slug")
        } else {
            s(cfg, "branch")
        };
        let parent = a["ancestry"]
            .as_array()
            .and_then(|v| v.last())
            .and_then(Value::as_str)
            .filter(|p| ix.agents.contains_key(p));
        let problem = match (s(a, "phase"), s(a, "activity")) {
            ("error", _) => format!(
                "error{}",
                Some(s(a, "exitReason"))
                    .filter(|r| !r.is_empty())
                    .map(|r| format!(" · {r}"))
                    .unwrap_or_default()
            ),
            (_, "offline") => "offline: no heartbeat from the agent".into(),
            (_, "limits_exceeded") => "limits exceeded".into(),
            (_, "stalled") => "stalled".into(),
            _ => String::new(),
        };
        let mut context = format!("Scion Hub · project {}", s(project, "slug"));
        if let Some(c) = room {
            context.push_str(&format!(" · #{}", s(&c.meta, "displayName")));
        }
        let harness = if s(a, "resolvedHarness").is_empty() {
            s(a, "harnessConfig")
        } else {
            s(a, "resolvedHarness")
        };
        let model = s(cfg, "model");
        snap.sessions.push(Session {
            id: sid.clone(),
            source: "scion".into(),
            color: color(&group, name_of(a)),
            group_id: Some(group),
            parent_id: parent.map(|p| ix.sid(p)),
            name: name_of(a).into(),
            template: s(a, "template").into(),
            harness: harness_label(harness),
            model: model_label(model),
            host: format!(
                "Scion · {}",
                if s(a, "runtimeBrokerName").is_empty() {
                    cx.source
                } else {
                    s(a, "runtimeBrokerName")
                }
            ),
            cwd: String::new(),
            status: status(a, ask),
            title: if title.is_empty() { name_of(a).into() } else { title },
            subtitle: if running && !detail.is_empty() && detail != "Ready" {
                detail.into()
            } else {
                rest
            },
            context,
            created_at: local(s(a, "created")),
            active_at: [s(a, "lastActivityEvent"), s(a, "updated")]
                .iter()
                .map(|t| local(t))
                .find(|t| !t.is_empty())
                .unwrap_or_default(),
            stats: Stats {
                turns: a["currentTurns"].as_u64().unwrap_or(0),
                started_at: local(s(a, "startedAt")),
                ..Default::default()
            },
            facts: crate::agents::model::Facts {
                branch: Some(Fact {
                    label: branch.into(),
                    ..Default::default()
                }),
                ..Default::default()
            },
            live: cx.live && running,
            problem,
            url: format!("{}/agents/{id}", cx.web),
            ext: id.into(),
            ..Default::default()
        });
        snap.threads.push((format!("s:{sid}"), ix.own_rows(a)));
    }

    // Your sends the Hub hasn't listed yet, and nothing you discarded.
    for (key, rows) in &mut snap.threads {
        let have: HashSet<String> = rows.iter().map(|r| r.ext.clone()).collect();
        rows.extend(
            st.pending
                .iter()
                .filter(|p| p.key == *key && !have.contains(&p.row.ext))
                .map(|p| p.row.clone()),
        );
        rows.retain(|r| !st.hidden.contains(&r.ext));
    }
    snap
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub fn fixture(name: &str) -> Value {
        let path = format!("{}/tests/fixtures/scion/{name}", env!("CARGO_MANIFEST_DIR"));
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap()
    }

    pub const ROOM: &str = "c1000000-0000-4000-8000-0000000000aa";
    pub const DM: &str = "c2000000-0000-4000-8000-0000000000cc";
    pub const PLANNER: &str = "a1000000-0000-4000-8000-000000000001";
    pub const DIFF: &str = "a1000000-0000-4000-8000-000000000002";

    fn oldest_first(v: Value) -> Vec<Value> {
        let mut items = v["items"].as_array().cloned().unwrap();
        items.reverse();
        items
    }

    /// The fixtures as the supervisor would hold them.
    pub fn state() -> State {
        let me = fixture("me.json");
        let mut st = State {
            me_id: s(&me, "id").into(),
            me_email: s(&me, "email").into(),
            projects: fixture("projects.json")["projects"].as_array().cloned().unwrap()[..1].to_vec(),
            agents: fixture("agents.json")["agents"].as_array().cloned().unwrap(),
            ..Default::default()
        };
        let room = fixture("conversation.json");
        st.convs.insert(
            ROOM.into(),
            Conv {
                participants: room["participants"].as_array().cloned().unwrap(),
                meta: room,
                messages: oldest_first(fixture("room_messages.json")),
                read_at: String::new(),
            },
        );
        let dm = fixture("conversations.json")["conversations"][1].clone();
        st.convs.insert(
            DM.into(),
            Conv {
                meta: dm,
                messages: oldest_first(fixture("dm_messages.json")),
                ..Default::default()
            },
        );
        st.interagent.insert(
            DIFF.into(),
            fixture("interagent.json")["messages"].as_array().cloned().unwrap(),
        );
        st
    }

    pub fn snapshot(st: &State) -> Snapshot {
        let cx = Cx {
            source: "work-hub",
            web: "https://hub.example.com",
            live: true,
        };
        let mut n = 0;
        build(&cx, st, &mut |_, _| {
            n += 1;
            n - 1
        })
    }

    fn rows<'a>(snap: &'a Snapshot, key: &str) -> &'a [Row] {
        &snap.threads.iter().find(|(k, _)| k == key).unwrap().1
    }

    fn by_ext<'a>(rows: &'a [Row], ext: &str) -> &'a Row {
        rows.iter()
            .find(|r| r.ext == ext)
            .unwrap_or_else(|| panic!("no row {ext}"))
    }

    #[test]
    fn a_project_with_no_room_or_agent_still_has_a_group() {
        let mut st = state();
        let mut p = st.projects[0].clone();
        p["id"] = "7d9a0c2e-0000-4c3a-8e7d-000000000099".into();
        p["name"] = "fresh".into();
        st.projects.push(p);
        let snap = snapshot(&st);
        let g = snap
            .groups
            .iter()
            .find(|g| g.id.ends_with("000000000099:agents"))
            .expect("an empty project is listed");
        assert_eq!((g.source.as_str(), g.room.is_none()), ("scion", true));
    }

    #[test]
    fn projects_agents_and_threads_become_groups_and_sessions() {
        let snap = snapshot(&state());
        let ids: Vec<&str> = snap.groups.iter().map(|g| g.id.as_str()).collect();
        let pid = "7d9a0c2e-5b1f-4c3a-8e7d-2f1a3b4c5d01";
        assert_eq!(
            ids,
            [
                format!("scion:work-hub:{pid}:{ROOM}"),
                format!("scion:work-hub:{pid}:agents")
            ]
        );
        let g = &snap.groups[0];
        assert_eq!(
            (g.project.as_str(), g.name.as_str(), g.host.as_str()),
            ("ingest-svc", "migration", "Scion · gcp-1")
        );
        let room = g.room.as_ref().unwrap();
        assert_eq!(room.members, 3);
        assert_eq!(room.thread_ref, "group thread c1000000");
        assert_eq!(room.mode, "routes to @planner");
        assert_eq!(room.loop_cap, LOOP_CAP);
        assert_eq!(
            room.url,
            format!("https://hub.example.com/chat/space/{pid}/thread/b1000000-0000-4000-8000-0000000000bb")
        );

        let s = |name: &str| snap.sessions.iter().find(|s| s.name == name).unwrap();
        let planner = s("planner");
        assert_eq!(planner.id, format!("scion:work-hub:{PLANNER}"));
        assert_eq!(planner.group_id.as_deref(), Some(ids[0]));
        assert_eq!(
            (planner.harness.as_str(), planner.model.as_str(), planner.host.as_str()),
            ("Claude Code", "sonnet", "Scion · gcp-1")
        );
        assert_eq!(planner.status, Status::Running);
        assert_eq!(planner.title, "Plan the schema migration.");
        assert_eq!(planner.facts.branch.as_ref().unwrap().label, "planner");
        assert_eq!(planner.url, format!("https://hub.example.com/agents/{PLANNER}"));
        assert!(planner.live);
        let diff = s("schema-diff");
        assert_eq!(diff.status, Status::Waiting);
        assert_eq!(
            diff.parent_id.as_deref(),
            Some(planner.id.as_str()),
            "ancestry's last agent is the parent"
        );
        assert_eq!(diff.harness, "Gemini CLI");
        assert_eq!(s("reviewer").status, Status::Done);
        let loner = s("loner");
        assert_eq!(loner.status, Status::Failed);
        assert_eq!(
            loner.group_id.as_deref(),
            Some(ids[1]),
            "not in a thread: the project's agents group"
        );
        assert_eq!(loner.problem, "error · crashed");
        assert_eq!(
            loner.facts.branch.as_ref().unwrap().label,
            "loner",
            "the branch defaults to the slug"
        );
    }

    #[test]
    fn room_messages_map_by_type() {
        let snap = snapshot(&state());
        let r = rows(&snap, &format!("r:{ROOM}"));
        let post = by_ext(r, "f0000000-0000-4000-8000-000000000001");
        assert_eq!(post.kind, Kind::You);
        assert_eq!(post.who, Some(Who::you()));
        assert_eq!(post.to.as_ref().unwrap().kind, "room", "two recipients: to the room");
        assert!(
            r.iter().all(|x| x.ext != "f0000000-0000-4000-8000-000000000002"),
            "the mention copy folds into its post"
        );
        let reply = by_ext(r, "f0000000-0000-4000-8000-000000000003");
        assert_eq!((reply.kind, reply.label.as_str()), (Kind::Message, "instruction"));
        assert_eq!(
            reply.who.as_ref().unwrap().session.as_deref(),
            Some(format!("scion:work-hub:{DIFF}").as_str())
        );
        assert_eq!(
            reply.to.as_ref().unwrap().name,
            "planner",
            "@planner addresses one member"
        );
        assert_eq!(by_ext(r, "f0000000-0000-4000-8000-000000000004").kind, Kind::State);
        let ask = by_ext(r, "f0000000-0000-4000-8000-000000000005");
        assert_eq!((ask.kind, ask.label.as_str()), (Kind::Ask, "input-needed"));
        assert!(
            ask.request.is_some(),
            "a later post from you would close it; there's none"
        );
        let hub = by_ext(r, "f0000000-0000-4000-8000-000000000006");
        assert_eq!((hub.kind, hub.who.as_ref().unwrap().kind.as_str()), (Kind::Hub, "hub"));
        assert!(r
            .iter()
            .any(|x| x.ext == format!("join:{DIFF}") && x.kind == Kind::State));
    }

    #[test]
    fn delivery_states() {
        let snap = snapshot(&state());
        let r = rows(&snap, &format!("r:{ROOM}"));
        let d = |ext: &str| by_ext(r, ext).delivery.clone().unwrap();
        let both = d("f0000000-0000-4000-8000-000000000001");
        assert_eq!((both.state.as_str(), both.count), ("delivered", 2));
        assert_eq!(both.seen_by, ["schema-diff"]);
        assert!(!both.delivered_at.is_empty());
        let gone = d("f0000000-0000-4000-8000-000000000007");
        assert_eq!(
            (gone.state.as_str(), gone.reason.as_str()),
            ("unreachable", "Agent unreachable (stopped)")
        );
        let failed = by_ext(r, "f0000000-0000-4000-8000-000000000008");
        assert_eq!(failed.kind, Kind::Failed);
        let fd = failed.delivery.clone().unwrap();
        assert_eq!(
            (fd.state.as_str(), fd.reason.as_str(), fd.retry_at.as_str()),
            ("failed", "tmux exec error: session not found", "")
        );
        // planner is working and hasn't posted since: held by the broker.
        let later = d("f0000000-0000-4000-8000-000000000009");
        assert_eq!(later.state, "pending");
        let mut st = state();
        st.convs.get_mut(ROOM).unwrap().messages.iter_mut().for_each(|m| {
            if s(m, "id").ends_with("09") {
                m["dispatchState"] = "dispatched".into();
            }
        });
        let snap = snapshot(&st);
        let held = by_ext(
            rows(&snap, &format!("r:{ROOM}")),
            "f0000000-0000-4000-8000-000000000009",
        )
        .delivery
        .clone()
        .unwrap();
        assert_eq!(
            (held.state.as_str(), held.reason.as_str()),
            ("deferred", "planner was mid-turn")
        );
    }

    #[test]
    fn input_needed_becomes_an_ask_with_choices() {
        let snap = snapshot(&state());
        assert_eq!(snap.requests.len(), 1);
        let q = &snap.requests[0];
        assert_eq!(
            (q.kind.as_str(), q.thread.as_str(), q.via.as_str()),
            ("ask", format!("r:{ROOM}").as_str(), "input-needed")
        );
        assert_eq!(q.session_id, format!("scion:work-hub:{DIFF}"));
        assert_eq!(q.question, "Which environment should I diff against?");
        let c: Vec<(&str, &str, &str)> = q
            .choices
            .iter()
            .map(|c| (c.key.as_str(), c.title.as_str(), c.detail.as_str()))
            .collect();
        assert_eq!(
            c,
            [
                ("1", "staging", "the default"),
                ("2", "prod", "read-only replica"),
                ("3", "skip", "do nothing")
            ]
        );
        let mut st = state();
        st.answered.insert("f0000000-0000-4000-8000-000000000005".into());
        assert!(snapshot(&st).requests.is_empty());
    }

    #[test]
    fn choices_parse_loosely() {
        let (q, c) = parse_choices("Pick one:\n1. Keep it\n2) Drop it — breaks prod\n3. Ask Sam\n4. Too many");
        assert_eq!(q, "Pick one:");
        assert_eq!(c.len(), 3);
        assert_eq!((c[1].title.as_str(), c[1].detail.as_str()), ("Drop it", "breaks prod"));
        let (q, c) = parse_choices("Should I push?");
        assert_eq!((q.as_str(), c.len()), ("Should I push?", 0));
    }

    #[test]
    fn an_agents_own_thread_has_its_dms_room_messages_and_agent_mail() {
        let snap = snapshot(&state());
        let r = rows(&snap, &format!("s:scion:work-hub:{DIFF}"));
        let dm = by_ext(r, "d0000000-0000-4000-8000-000000000001");
        assert_eq!(
            (dm.kind, dm.to.clone()),
            (Kind::You, None),
            "a DM is to the session itself"
        );
        let out = by_ext(r, "d0000000-0000-4000-8000-000000000002");
        assert_eq!(
            (out.kind, out.who.clone()),
            (Kind::Output, None),
            "the turn's reply is its own output"
        );
        let from = by_ext(r, "90000000-0000-4000-8000-000000000001");
        assert_eq!(
            (from.kind, from.who.as_ref().unwrap().name.as_str(), from.to.clone()),
            (Kind::Message, "planner", None)
        );
        let sent = by_ext(r, "f0000000-0000-4000-8000-000000000003");
        assert_eq!(
            (
                sent.who.clone(),
                sent.to.as_ref().unwrap().name.as_str(),
                sent.note.as_str()
            ),
            (None, "planner", "in #migration")
        );
        assert_eq!(
            r.iter()
                .filter(|x| x.ext == "f0000000-0000-4000-8000-000000000003")
                .count(),
            1,
            "room and agent mail overlap once"
        );
        assert_eq!(r[0].ext, "created");
        let task = r
            .iter()
            .position(|x| x.ext == "task")
            .expect("the start task is in the thread");
        assert_eq!(
            (task, r[task].kind, r[task].text.as_str()),
            (1, Kind::You, "Diff the staging schema against prod")
        );
        assert!(
            r.iter().any(|x| x.kind == Kind::Ask),
            "its ask in the room is in its transcript too"
        );
    }

    #[test]
    fn pending_and_hidden_rows() {
        let mut st = state();
        let key = format!("r:{ROOM}");
        st.pending.push(Pending {
            key: key.clone(),
            row: Row {
                kind: Kind::You,
                ext: "local:1".into(),
                text: "hi".into(),
                ..Default::default()
            },
        });
        st.pending.push(Pending {
            key: key.clone(),
            row: Row {
                kind: Kind::You,
                ext: "f0000000-0000-4000-8000-000000000001".into(),
                ..Default::default()
            },
        });
        st.hidden.insert("f0000000-0000-4000-8000-000000000008".into());
        let snap = snapshot(&st);
        let r = rows(&snap, &key);
        assert!(r.iter().any(|x| x.ext == "local:1"));
        assert_eq!(
            r.iter()
                .filter(|x| x.ext == "f0000000-0000-4000-8000-000000000001")
                .count(),
            1,
            "the Hub's copy wins once listed"
        );
        assert!(r.iter().all(|x| x.ext != "f0000000-0000-4000-8000-000000000008"));
    }

    #[test]
    fn statuses() {
        let a = |phase: &str, activity: &str| serde_json::json!({ "phase": phase, "activity": activity });
        assert_eq!(status(&a("running", "thinking"), false), Status::Running);
        assert_eq!(status(&a("starting", ""), false), Status::Running);
        assert_eq!(status(&a("running", "blocked"), false), Status::Waiting);
        assert_eq!(status(&a("running", "working"), true), Status::Waiting);
        assert_eq!(status(&a("running", "completed"), false), Status::Done);
        assert_eq!(status(&a("suspended", ""), false), Status::Idle);
        assert_eq!(status(&a("running", "offline"), false), Status::Idle);
        assert_eq!(status(&a("running", "stalled"), false), Status::Failed);
        assert_eq!(status(&a("error", ""), true), Status::Failed);
    }
}

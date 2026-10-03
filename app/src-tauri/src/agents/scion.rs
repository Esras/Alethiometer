//! Scion Hub client. Alethiometer is a Hub client like the CLI or the web UI:
//! rooms (group threads), DMs and delivery states all come from the Hub; there
//! is no messaging layer here. `map` turns what's fetched into the model.
//!
//! One thread per enabled `scion` source. It polls rather than streams: the
//! Hub's message stream is per agent (`/agents/{id}/messages/stream`), and
//! one conversation list per project covers every room and DM.
//! - Every 3 s: the conversation list per project, then the messages of any
//!   conversation whose `lastActivityAt` moved or that holds a send still
//!   settling.
//! - Every 15 s (or right after you act): projects, agents, room
//!   participants, and the agent ↔ agent mail of agents that changed.
//!
//! The supervisor re-reads settings every 3 s and restarts a source's thread
//! when its config changes; a removed source's groups and sessions go away.

mod api;
mod map;

use super::model::{Delivery, Kind, Row, Session, Who};
use super::now;
use crate::error::{msg, Result};
use crate::settings::sources::ScionConfig;
use crate::state::App;
use crate::sync::http::{Http, UreqHttp};
use api::{hub_error, Client};
use map::{s, Pending};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

const TICK: Duration = Duration::from_secs(3);
const FULL: Duration = Duration::from_secs(15);

#[derive(Default)]
pub struct Hubs {
    hubs: Mutex<HashMap<String, Arc<Hub>>>,
}

impl Hubs {
    fn get(&self) -> MutexGuard<'_, HashMap<String, Arc<Hub>>> {
        self.hubs.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// One Hub source.
pub(crate) struct Hub {
    source: String,
    cfg: ScionConfig,
    /// The source's settings as last read, to notice a change.
    sig: String,
    http: Arc<dyn Http>,
    state: Mutex<map::State>,
    link: Mutex<Link>,
    /// Template version and broker names → Hub ids, from the last catalog.
    ids: Mutex<HashMap<String, String>>,
    stop: AtomicBool,
    /// Refresh everything on the next tick.
    kick: AtomicBool,
    next: AtomicU64,
}

#[derive(Default)]
struct Link {
    state: &'static str,
    error: String,
    full_at: Option<Instant>,
    /// Agent id → (phase, activity, lastActivityEvent) at the last refresh.
    seen: HashMap<String, (String, String, String)>,
    /// Thread keys published last time.
    keys: HashSet<String>,
    /// Native chat is off on this Hub (interagent 404s).
    no_chat: bool,
}

impl Hub {
    pub(crate) fn new(source: &str, cfg: ScionConfig, sig: String, http: Arc<dyn Http>) -> Hub {
        Hub {
            source: source.into(),
            cfg,
            sig,
            http,
            state: Default::default(),
            link: Mutex::new(Link { state: "connecting", ..Default::default() }),
            ids: Default::default(),
            stop: AtomicBool::new(false),
            kick: AtomicBool::new(false),
            next: AtomicU64::new(1),
        }
    }

    fn st(&self) -> MutexGuard<'_, map::State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn link(&self) -> MutexGuard<'_, Link> {
        self.link.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn client(&self, app: &App) -> anyhow::Result<Client<'_>> {
        let token = app.platform.secret_get(&self.cfg.token_secret).map(|t| t.trim().to_owned()).filter(|t| !t.is_empty());
        let token = token.ok_or_else(|| anyhow::anyhow!("no access token: add a scion_pat_… token as {}", self.cfg.token_secret))?;
        Ok(Client { http: &*self.http, base: self.cfg.hub_url.clone(), token })
    }

    fn web(&self) -> String {
        let w = if self.cfg.web_url.is_empty() { &self.cfg.hub_url } else { &self.cfg.web_url };
        w.trim_end_matches('/').to_owned()
    }

    fn agent(&self, id: &str) -> Option<Value> {
        self.st().agents.iter().find(|a| s(a, "id") == id).cloned()
    }
}

/// `scion:<source>:<rest>` → (source, rest).
fn split_id(id: &str) -> Option<(&str, &str)> {
    id.strip_prefix("scion:")?.split_once(':')
}

fn hub(app: &App, source: &str) -> Result<Arc<Hub>> {
    app.agents.scion.get().get(source).cloned().ok_or_else(|| msg(format!("Scion Hub '{source}' isn't connected")))
}

/// The hub holding a conversation.
fn hub_with_conv(app: &App, conv: &str) -> Result<Arc<Hub>> {
    let hubs: Vec<Arc<Hub>> = app.agents.scion.get().values().cloned().collect();
    hubs.into_iter().find(|h| h.st().convs.contains_key(conv)).ok_or_else(|| msg("No connected Scion Hub has this room"))
}

fn err(e: anyhow::Error) -> crate::error::Error {
    msg(e.to_string())
}

pub fn start(app: Arc<App>) {
    std::thread::Builder::new()
        .name("scion-hubs".into())
        .spawn(move || loop {
            reconcile(&app);
            std::thread::sleep(TICK);
        })
        .ok();
}

/// Start, restart or stop one thread per enabled scion source.
fn reconcile(app: &Arc<App>) {
    let wanted: Vec<(String, ScionConfig, String)> = app
        .settings()
        .sources
        .iter()
        .filter(|s| s.kind == "scion" && s.enabled)
        .filter_map(|s| {
            let cfg = ScionConfig::from_json(&s.config).ok().filter(|c| !c.hub_url.is_empty())?;
            Some((s.name.clone(), cfg, format!("{}{}", s.name, s.config)))
        })
        .collect();
    let mut hubs = app.agents.scion.get();
    let gone: Vec<String> = hubs.iter().filter(|(n, h)| !wanted.iter().any(|(wn, _, sig)| wn == *n && *sig == h.sig)).map(|(n, _)| n.clone()).collect();
    for name in gone {
        if let Some(h) = hubs.remove(&name) {
            h.stop.store(true, Ordering::SeqCst);
            clear(app, &name);
        }
    }
    for (name, cfg, sig) in wanted {
        if hubs.contains_key(&name) {
            continue;
        }
        let http = Arc::new(UreqHttp::with_timeout(cfg.timeout_secs.clamp(1, 60) as u64));
        let h = Arc::new(Hub::new(&name, cfg, sig, http));
        hubs.insert(name.clone(), h.clone());
        let a = app.clone();
        std::thread::Builder::new().name(format!("scion-{name}")).spawn(move || run(&a, &h)).ok();
    }
}

fn run(app: &App, hub: &Hub) {
    while !hub.stop.load(Ordering::SeqCst) {
        poll(app, hub);
        let until = Instant::now() + TICK;
        while Instant::now() < until && !hub.stop.load(Ordering::SeqCst) && !hub.kick.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(250));
        }
    }
}

/// One tick, then publish; an error marks the Hub (logged once per message).
pub(crate) fn poll(app: &App, hub: &Hub) {
    let r = tick(app, hub);
    {
        let mut l = hub.link();
        match r {
            Ok(()) => {
                l.state = "connected";
                l.error.clear();
            }
            Err(e) => {
                let e = e.to_string();
                if l.error != e {
                    eprintln!("scion {}: {e}", hub.source);
                }
                l.state = "error";
                l.error = e;
            }
        }
    }
    publish(app, hub);
}

fn tick(app: &App, hub: &Hub) -> anyhow::Result<()> {
    let c = hub.client(app)?;
    let full = hub.kick.swap(false, Ordering::SeqCst) || hub.link().full_at.is_none_or(|t| t.elapsed() >= FULL);
    if full {
        refresh(&c, hub)?;
    }
    let (projects, agents, me) = {
        let st = hub.st();
        let p: Vec<String> = st.projects.iter().map(|p| s(p, "id").to_owned()).collect();
        let a: HashSet<String> = st.agents.iter().map(|a| s(a, "id").to_owned()).collect();
        (p, a, st.me_id.clone())
    };
    let mut listed: BTreeMap<String, Value> = BTreeMap::new();
    for p in &projects {
        for conv in c.conversations(p)? {
            let keep = match s(&conv, "kind") {
                "group" => projects.iter().any(|p| p == s(&conv, "projectId")),
                "direct" => {
                    let r = s(&conv, "externalRef");
                    r.contains(&me) && r.strip_prefix("dm:agent:").and_then(|x| x.split(':').next()).is_some_and(|a| agents.contains(a))
                }
                _ => false,
            };
            if keep {
                listed.insert(s(&conv, "id").to_owned(), conv);
            }
        }
    }
    let max = hub.cfg.max_messages.clamp(20, 1000);
    for (id, conv) in &listed {
        let (fresh, moved, unsettled) = {
            let st = hub.st();
            match st.convs.get(id) {
                None => (true, true, false),
                Some(old) => (false, old.read_at != s(conv, "lastActivityAt"), unsettled(&st, id, old)),
            }
        };
        let group = s(conv, "kind") == "group";
        let participants = if group && (fresh || full) { Some(c.conversation(id)?["participants"].as_array().cloned().unwrap_or_default()) } else { None };
        let messages = if fresh || moved || unsettled {
            let mut m = c.messages(id, max)?;
            m.reverse();
            Some(m)
        } else {
            None
        };
        let mut st = hub.st();
        let e = st.convs.entry(id.clone()).or_default();
        e.meta = conv.clone();
        if let Some(p) = participants {
            e.participants = p;
        }
        if let Some(m) = messages {
            e.messages = m;
            e.read_at = s(conv, "lastActivityAt").to_owned();
        }
    }
    hub.st().convs.retain(|id, _| listed.contains_key(id));
    if full {
        interagent(&c, hub, &me)?;
        hub.link().full_at = Some(Instant::now());
    }
    Ok(())
}

/// A send of yours that may still change: pending, or dispatched in the last
/// two minutes (a broker can still report it failed).
fn unsettled(st: &map::State, id: &str, conv: &map::Conv) -> bool {
    let now = jiff::Timestamp::now().as_millisecond();
    let recent = |t: i64| t > now - 120_000 && t <= now + 60_000;
    let agents_dm = s(&conv.meta, "externalRef");
    st.pending.iter().any(|p| p.key == format!("r:{id}") || p.key.rsplit(':').next().is_some_and(|a| !a.is_empty() && agents_dm.contains(a)))
        || conv.messages.iter().any(|m| s(m, "dispatchState") == "pending" || (s(m, "dispatchState") == "dispatched" && recent(map::ms(s(m, "createdAt")))))
}

/// Projects, agents and you; notes agents' state changes between refreshes.
fn refresh(c: &Client, hub: &Hub) -> anyhow::Result<()> {
    if hub.st().me_id.is_empty() {
        let me = c.me()?;
        let mut st = hub.st();
        st.me_id = s(&me, "id").into();
        st.me_email = s(&me, "email").into();
    }
    let want = &hub.cfg.projects;
    let projects: Vec<Value> = c
        .projects()?
        .into_iter()
        .filter(|p| want.is_empty() || want.iter().any(|w| [s(p, "id"), s(p, "slug"), s(p, "name")].contains(&w.as_str())))
        .collect();
    let mut agents = vec![];
    for p in &projects {
        agents.extend(c.agents(s(p, "id"))?);
    }
    let mut l = hub.link();
    let mut st = hub.st();
    for a in &agents {
        let id = s(a, "id").to_owned();
        let now = (s(a, "phase").to_owned(), s(a, "activity").to_owned(), s(a, "lastActivityEvent").to_owned());
        if let Some(old) = l.seen.get(&id) {
            let text = if old.0 != now.0 {
                Some(format!("{} → {}", old.0, now.0))
            } else if old.1 != now.1 && !now.1.is_empty() {
                Some(format!("{} → {}", old.1, now.1).replace('_', " "))
            } else {
                None
            };
            if let Some(text) = text {
                let at = if s(a, "updated").is_empty() { crate::time::Ts::new(jiff::Timestamp::now()).to_db() } else { s(a, "updated").to_owned() };
                let t = st.transitions.entry(id.clone()).or_default();
                t.push((at, text.trim_start_matches(" → ").to_owned()));
                if t.len() > 50 {
                    t.remove(0);
                }
            }
        }
        l.seen.insert(id, now);
    }
    st.projects = projects;
    st.agents = agents;
    Ok(())
}

/// Agent ↔ agent mail, for agents whose activity moved. Needs native chat
/// and attach on the agent; without either the thread shows rooms and DMs.
fn interagent(c: &Client, hub: &Hub, me: &str) -> anyhow::Result<()> {
    if hub.link().no_chat || me.is_empty() {
        return Ok(());
    }
    let agents: Vec<(String, String)> = hub.st().agents.iter().map(|a| (s(a, "id").to_owned(), s(a, "lastActivityEvent").to_owned())).collect();
    for (id, at) in agents {
        let key = format!("ia:{id}");
        if hub.link().seen.get(&key).is_some_and(|v| v.2 == at) {
            continue;
        }
        match c.interagent(&format!("dm:agent:{id}:user:{me}"), 100) {
            Ok(m) => {
                hub.st().interagent.insert(id.clone(), m);
            }
            Err(e) if hub_error(&e).is_some_and(|h| h.status == 404 || h.status == 501) => {
                hub.link().no_chat = true;
                return Ok(());
            }
            Err(e) if hub_error(&e).is_some_and(|h| h.status == 403) => {}
            Err(e) => return Err(e),
        }
        hub.link().seen.insert(key, (String::new(), String::new(), at));
    }
    Ok(())
}

static ATTACH: Mutex<BTreeMap<String, String>> = Mutex::new(BTreeMap::new());

/// Put this Hub's snapshot into the store, replacing what it put there before.
pub(crate) fn publish(app: &App, hub: &Hub) {
    let live = hub.link().state == "connected";
    let web = hub.web();
    let palette = app.settings().agents.names.palette.len();
    let snap = {
        let st = hub.st();
        let cx = map::Cx { source: &hub.source, web: &web, live };
        map::build(&cx, &st, &mut |g, n| super::store::color_for(&app.db(), g, n, palette).unwrap_or(0))
    };
    let listed: HashSet<&str> = snap.threads.iter().flat_map(|(_, rows)| rows.iter().map(|r| r.ext.as_str())).filter(|e| !e.starts_with("local:")).collect();
    hub.st().pending.retain(|p| p.row.ext.starts_with("local:") || !listed.contains(p.row.ext.as_str()));
    {
        let st = hub.st();
        let mut attach = ATTACH.lock().unwrap_or_else(|e| e.into_inner());
        for a in &st.agents {
            let project = st.projects.iter().find(|p| s(p, "id") == s(a, "projectId")).map(|p| s(p, "slug")).unwrap_or("");
            attach.insert(map::session_id(&hub.source, s(a, "id")), format!("scion attach {} --hub {} -g {project}", s(a, "slug"), hub.cfg.hub_url));
        }
    }
    let prefix = format!("scion:{}:", hub.source);
    let mut i = app.agents.store.lock();
    if hub.stop.load(Ordering::SeqCst) {
        return;
    }
    let groups: HashSet<String> = snap.groups.iter().map(|g| g.id.clone()).collect();
    let sessions: HashSet<String> = snap.sessions.iter().map(|s| s.id.clone()).collect();
    let requests: HashSet<String> = snap.requests.iter().map(|r| r.id.clone()).collect();
    for g in snap.groups {
        i.put_group(g);
    }
    for s in snap.sessions {
        i.put_session(s);
    }
    for r in snap.requests {
        i.put_request(r);
    }
    let stale = |ids: Vec<&String>, keep: &HashSet<String>| ids.into_iter().filter(|id| id.starts_with(&prefix) && !keep.contains(*id)).cloned().collect::<Vec<_>>();
    for id in stale(i.groups.keys().collect(), &groups) {
        i.remove_group(&id);
    }
    for id in stale(i.sessions.keys().collect(), &sessions) {
        i.remove_session(&id);
    }
    for id in stale(i.requests.keys().collect(), &requests) {
        i.remove_request(&id);
    }
    let keys: HashSet<String> = snap.threads.iter().map(|(k, _)| k.clone()).collect();
    for (key, rows) in snap.threads {
        let same = i.threads.get(&key).is_some_and(|t| {
            t.rows.len() == rows.len() && t.rows.iter().zip(&rows).all(|(a, b)| Row { id: 0, rev: 0, ..a.clone() } == *b)
        });
        if !same {
            i.set_rows(&key, rows);
        }
    }
    let mut l = hub.link();
    for k in l.keys.difference(&keys) {
        i.threads.remove(k);
    }
    l.keys = keys;
}

/// Drop a removed source's groups, sessions, asks and rooms.
fn clear(app: &App, source: &str) {
    let prefix = format!("scion:{source}:");
    let mut i = app.agents.store.lock();
    let sessions: Vec<String> = i.sessions.keys().filter(|k| k.starts_with(&prefix)).cloned().collect();
    for id in sessions {
        i.remove_session(&id);
    }
    let groups: Vec<(String, Option<String>)> = i.groups.values().filter(|g| g.id.starts_with(&prefix)).map(|g| (g.id.clone(), g.room.as_ref().map(|r| format!("r:{}", r.id)))).collect();
    for (id, room) in groups {
        i.remove_group(&id);
        if let Some(k) = room {
            i.threads.remove(&k);
        }
    }
    let reqs: Vec<String> = i.requests.keys().filter(|k| k.starts_with(&prefix)).cloned().collect();
    for id in reqs {
        i.remove_request(&id);
    }
}

/// `[{source, name, url, state, error}]`, state connected | connecting | error | off.
pub fn hubs(app: &App) -> Value {
    let running = app.agents.scion.get();
    let list: Vec<Value> = app
        .settings()
        .sources
        .iter()
        .filter(|s| s.kind == "scion")
        .map(|src| {
            let cfg = ScionConfig::from_json(&src.config).unwrap_or_default();
            let name = cfg.hub_url.split("://").nth(1).unwrap_or(&cfg.hub_url).trim_end_matches('/').to_owned();
            let (state, error) = match running.get(&src.name) {
                Some(h) if src.enabled => {
                    let l = h.link();
                    (l.state, l.error.clone())
                }
                _ => ("off", String::new()),
            };
            json!({ "source": src.name, "name": name, "url": cfg.hub_url, "state": state, "error": error })
        })
        .collect();
    json!(list)
}

/// Where a send goes.
#[derive(Clone)]
enum Target {
    /// A group thread's native chat topic.
    Room { topic: String },
    /// A DM to an agent (id).
    Dm { agent: String, wake: bool },
}

/// Resolve a thread key (and `@` member) to a Hub, a target and the text to send.
fn target(app: &App, key: &str, text: &str, to: Option<&str>) -> Result<(Arc<Hub>, Target, String, Option<Who>)> {
    if let Some(conv) = key.strip_prefix("r:") {
        let hub = hub_with_conv(app, conv)?;
        let (meta, member) = {
            let st = hub.st();
            let meta = st.convs[conv].meta.clone();
            let member = to.map(|n| st.agents.iter().find(|a| s(a, "name") == n || s(a, "slug") == n).cloned().ok_or_else(|| msg(format!("No agent named {n} on this Hub"))));
            (meta, member.transpose()?)
        };
        let topic = map::topic_of(&meta).ok_or_else(|| msg(format!("#{} lives on {}; post to it there", s(&meta, "displayName"), s(&meta, "surface"))))?.to_owned();
        let (content, who) = match &member {
            Some(a) => {
                let at = format!("@{}", s(a, "slug"));
                let content = if text.trim_start().starts_with(&at) { text.trim().to_owned() } else { format!("{at} {}", text.trim()) };
                (content, Some(Who::agent(s(a, "name"), Some(map::session_id(&hub.source, s(a, "id"))))))
            }
            None => (text.trim().to_owned(), Some(Who::room(s(&meta, "displayName"), conv))),
        };
        return Ok((hub, Target::Room { topic }, content, who));
    }
    let id = key.strip_prefix("s:").unwrap_or(key);
    let (source, agent) = split_id(id).ok_or_else(|| msg("not a Scion thread"))?;
    let hub = hub(app, source)?;
    if hub.agent(agent).is_none() {
        return Err(msg("This agent is gone from the Hub"));
    }
    Ok((hub, Target::Dm { agent: agent.into(), wake: false }, text.trim().to_owned(), None))
}

/// The optimistic You row, until the Hub lists the message.
fn queue(app: &App, hub: &Hub, key: &str, text: &str, to: Option<Who>) -> (u64, String) {
    let ext = format!("local:{}", hub.next.fetch_add(1, Ordering::SeqCst));
    let ts = now(app);
    let row = Row {
        at: ts.clone(),
        kind: Kind::You,
        who: Some(Who::you()),
        to,
        label: "message".into(),
        text: text.into(),
        delivery: Some(Delivery { state: "pending".into(), at: ts, ..Default::default() }),
        ext: ext.clone(),
        ..Default::default()
    };
    let id = app.agents.store.lock().push_row(key, row.clone());
    hub.st().pending.push(Pending { key: key.into(), row });
    (id, ext)
}

/// A queued send.
struct Out {
    key: String,
    row: u64,
    ext: String,
    target: Target,
    content: String,
    reply_to: Option<String>,
    kind: &'static str,
}

/// Post it, then set the row's delivery (and Hub id) from the answer.
fn dispatch(app: &App, hub: &Hub, o: &Out) {
    let (key, row, ext) = (o.key.as_str(), o.row, o.ext.as_str());
    let r = hub.client(app).and_then(|c| match &o.target {
        Target::Room { topic } => c.post_thread(topic, &o.content, o.reply_to.as_deref(), ext),
        Target::Dm { agent, wake } => c.message_agent(agent, &o.content, o.kind, *wake),
    });
    let ts = now(app);
    let (id, d) = match r {
        Ok(v) => {
            let id = [s(&v, "id"), s(&v, "message_id")].into_iter().find(|x| !x.is_empty()).unwrap_or(ext).to_owned();
            let st = [s(&v, "dispatchState"), s(&v, "status")].into_iter().find(|x| !x.is_empty()).unwrap_or("delivered");
            let reason = s(&v, "dispatchFailureReason").to_owned();
            let state = match st {
                "failed" if s(&v, "dispatchFailureCode") == "agent_unreachable" || reason.starts_with("Agent unreachable") => "unreachable",
                "failed" | "ambiguous" => "failed",
                "deferred" => "deferred",
                "pending" => "pending",
                _ => "delivered",
            };
            let mentioned = v["mentions"].as_array().map_or(0, |m| m.iter().filter(|m| s(m, "status") == "delivered").count());
            let count = if state == "delivered" { mentioned.max(1) } else { 0 };
            (id, Delivery { state: state.into(), count, at: ts.clone(), delivered_at: if state == "delivered" { ts } else { String::new() }, reason, ..Default::default() })
        }
        Err(e) => {
            let state = if hub_error(&e).is_some_and(|h| h.code == "agent_not_running" || h.code == "agent_unreachable") { "unreachable" } else { "failed" };
            (ext.to_owned(), Delivery { state: state.into(), at: ts, reason: e.to_string(), ..Default::default() })
        }
    };
    let failed = d.state == "failed";
    {
        let mut st = hub.st();
        if let Some(p) = st.pending.iter_mut().find(|p| p.row.ext == ext) {
            p.row.ext = id.clone();
            p.row.delivery = Some(d.clone());
            if failed {
                p.row.kind = Kind::Failed;
            }
        }
    }
    app.agents.store.lock().update_row(key, row, |r| {
        r.ext = id;
        r.delivery = Some(d);
        if failed {
            r.kind = Kind::Failed;
        }
    });
    hub.kick.store(true, Ordering::SeqCst);
}

/// In the background when the app is shared (always, outside tests).
fn later(app: &App, f: impl FnOnce(&App) + Send + 'static) {
    match app.arc() {
        Some(a) => {
            std::thread::spawn(move || f(&a));
        }
        None => f(app),
    }
}

fn send_as(app: &App, key: &str, text: &str, to: Option<&str>, reply_to: Option<String>, kind: &'static str) -> Result<()> {
    let (hub, t, content, who) = target(app, key, text, to)?;
    if content.is_empty() {
        return Err(msg("Write a message first"));
    }
    let (row, ext) = queue(app, &hub, key, &content, who);
    let o = Out { key: key.to_owned(), row, ext, target: t, content, reply_to, kind };
    later(app, move |app| dispatch(app, &hub, &o));
    Ok(())
}

/// `r:<room>` posts to the group thread (`to` addresses one member with an
/// @mention); `s:scion:…` DMs the agent.
pub fn send(app: &App, key: &str, text: &str, to: Option<&str>) -> Result<()> {
    send_as(app, key, text, to, None, "instruction")
}

fn agent_of(app: &App, id: &str) -> Result<(Arc<Hub>, Value)> {
    let (source, agent) = split_id(id).ok_or_else(|| msg("not a Scion agent"))?;
    let hub = hub(app, source)?;
    let a = hub.agent(agent).ok_or_else(|| msg("This agent is gone from the Hub"))?;
    Ok((hub, a))
}

/// The harness's own interrupt key (Escape for Claude Code, else Ctrl-C), as
/// terminal input. Needs `agent:attach`.
pub fn interrupt(app: &App, id: &str) -> Result<()> {
    let (hub, a) = agent_of(app, id)?;
    let key = if matches!(s(&a, "resolvedHarness"), "claude" | "muse-code") { "Escape" } else { "C-c" };
    let c = hub.client(app).map_err(err)?;
    c.keys(s(&a, "id"), key).map_err(|e| match hub_error(&e) {
        Some(h) if h.code == "agent_not_running" => msg("It isn't running"),
        _ => err(e),
    })?;
    hub.kick.store(true, Ordering::SeqCst);
    Ok(())
}

/// Reply to an input-needed ask: in a room, as a reply to the ask (the Hub
/// routes it to the asker); in a DM, as a `reply` message.
pub fn answer(app: &App, request: &str, text: &str) -> Result<()> {
    let req = app.agents.store.lock().requests.get(request).cloned().ok_or_else(|| msg("That question was already answered"))?;
    let (source, rest) = split_id(request).ok_or_else(|| msg("not a Scion ask"))?;
    let message = rest.strip_prefix("ask:").ok_or_else(|| msg("not a Scion ask"))?.to_owned();
    let hub = hub(app, source)?;
    if req.thread.starts_with("r:") {
        send_as(app, &req.thread, text, None, Some(message.clone()), "reply")?;
    } else {
        send_as(app, &format!("s:{}", req.session_id), text, None, None, "reply")?;
    }
    hub.st().answered.insert(message);
    app.agents.store.lock().remove_request(request);
    Ok(())
}

fn slugify(s: &str) -> String {
    let mut out = String::new();
    for c in s.trim().to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-').to_owned()
}

/// Start an agent through the Hub, then (if asked) add it to a room. The
/// task is its first instruction. Returns the session id.
pub fn start_agent(app: &App, args: &Value) -> Result<String> {
    let arg = |k: &str| args.get(k).and_then(Value::as_str).map(str::trim).filter(|v| !v.is_empty());
    let group = arg("group").ok_or_else(|| msg("Pick a Scion group"))?;
    let (source, rest) = split_id(group).ok_or_else(|| msg("not a Scion group"))?;
    let (project, tail) = rest.split_once(':').ok_or_else(|| msg("not a Scion group"))?;
    let hub = hub(app, source)?;
    let name = arg("name").ok_or_else(|| msg("Name the agent"))?;
    let slug = slugify(name);
    if hub.st().agents.iter().any(|a| s(a, "projectId") == project && (s(a, "slug") == slug || s(a, "name") == name)) {
        return Err(msg(format!("There's already an agent named {name} in this project")));
    }
    let template = arg("template").ok_or_else(|| msg("Pick a template"))?;
    let ids = hub.ids.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let template = arg("template_version").and_then(|v| ids.get(&format!("t:{template}:{v}"))).map(String::as_str).unwrap_or(template);
    let mut body = json!({ "name": name, "projectId": project, "template": template });
    if let Some(h) = arg("harness_config") {
        body["harnessConfig"] = json!(h);
    }
    if let Some(b) = arg("broker") {
        let known = hub.st().agents.iter().find(|a| s(a, "runtimeBrokerName") == b).map(|a| s(a, "runtimeBrokerId").to_owned());
        body["runtimeBrokerId"] = json!(ids.get(&format!("b:{b}")).cloned().or(known).unwrap_or_else(|| b.to_owned()));
    }
    for (k, v) in [("branch", arg("branch")), ("task", arg("task"))] {
        if let Some(v) = v {
            body[k] = json!(v);
        }
    }
    let c = hub.client(app).map_err(err)?;
    let v = c.create_agent(&body).map_err(err)?;
    if let Some(need) = v["envGather"]["needs"].as_array().filter(|n| !n.is_empty()) {
        let need: Vec<&str> = need.iter().filter_map(Value::as_str).collect();
        return Err(msg(format!("The Hub needs {} before this agent can start; start it with `scion start` in a terminal", need.join(", "))));
    }
    let agent = v["agent"].clone();
    let id = s(&agent, "id").to_owned();
    if id.is_empty() {
        return Err(msg("The Hub didn't return the new agent"));
    }
    let room = arg("room").map(|r| r.trim_start_matches("r:").to_owned()).or_else(|| (tail != "agents").then(|| tail.to_owned()));
    if let Some(room) = room {
        if let Err(e) = c.add_participant(&room, &id) {
            // Only participants may add one; a mention also makes it one.
            let topic = hub.st().convs.get(&room).and_then(|c| map::topic_of(&c.meta).map(str::to_owned));
            let joined = topic.ok_or(e).and_then(|t| c.post_thread(&t, &format!("@{} joined this thread", s(&agent, "slug")), None, &format!("join:{id}")));
            if let Err(e) = joined {
                let at = crate::time::Ts::new(jiff::Timestamp::now()).to_db();
                hub.st().transitions.entry(id.clone()).or_default().push((at, format!("couldn't add to the room: {e}")));
            }
        }
    }
    hub.st().agents.push(agent);
    hub.kick.store(true, Ordering::SeqCst);
    publish(app, &hub);
    Ok(map::session_id(source, &id))
}

/// The new-agent form's choices for a group's project.
pub fn catalog(app: &App, group: &str) -> Result<Value> {
    let (source, rest) = split_id(group).ok_or_else(|| msg("not a Scion group"))?;
    let project = rest.split_once(':').map(|(p, _)| p).unwrap_or(rest);
    let hub = hub(app, source)?;
    let c = hub.client(app).map_err(err)?;
    let templates = c.templates().map_err(err)?;
    let harness = c.harness_configs().map_err(err)?;
    let providers = c.providers(project).map_err(err)?;
    let (slug, default_broker) = {
        let st = hub.st();
        let p = st.projects.iter().find(|p| s(p, "id") == project);
        (p.map(|p| s(p, "slug").to_owned()).unwrap_or_default(), p.map(|p| s(p, "defaultRuntimeBrokerId").to_owned()).unwrap_or_default())
    };
    let mut ids = HashMap::new();
    // Same-named templates in several scopes are its versions; the Hub
    // resolves a name project first, then global.
    let rank = |t: &Value| match s(t, "scope") {
        "project" => 0,
        "user" => 1,
        _ => 2,
    };
    let mut names: Vec<&str> = vec![];
    for t in &templates {
        if !names.contains(&s(t, "name")) {
            names.push(s(t, "name"));
        }
    }
    let mut out = vec![];
    for (n, name) in names.iter().enumerate() {
        let mut versions: Vec<&Value> = templates.iter().filter(|t| s(t, "name") == *name && s(t, "status") != "archived").collect();
        versions.sort_by_key(|t| rank(t));
        let Some(first) = versions.first() else { continue };
        let mut labels = vec![];
        for t in &versions {
            let hash: String = s(t, "contentHash").chars().take(7).collect();
            let mut label = s(t, "scope").to_owned();
            if labels.contains(&label) || versions.iter().filter(|v| s(v, "scope") == s(t, "scope")).count() > 1 {
                label = format!("{label} · {hash}");
            }
            ids.insert(format!("t:{name}:{label}"), s(t, "id").to_owned());
            labels.push(label);
        }
        let files: Vec<&str> = first["files"].as_array().map(|f| f.iter().map(|f| s(f, "path")).collect()).unwrap_or_default();
        let instructions = if n < 12 && (files.is_empty() || files.contains(&"agents.md")) {
            c.template_file(s(first, "id"), "agents.md").map(|t| crate::ai::context::clip(&t, 4000)).unwrap_or_default()
        } else {
            String::new()
        };
        let mut skills: Vec<&str> = vec![];
        for f in &files {
            let parts: Vec<&str> = f.split('/').collect();
            if let Some(i) = parts.iter().position(|p| *p == "skills") {
                if let Some(sk) = parts.get(i + 1).filter(|_| parts.len() > i + 2) {
                    if !skills.contains(sk) {
                        skills.push(*sk);
                    }
                }
            }
        }
        out.push(json!({ "name": name, "versions": labels, "instructions": instructions, "skills": skills, "description": s(first, "description") }));
    }
    let harness_configs: Vec<Value> = harness
        .iter()
        .filter(|h| s(h, "status") != "archived")
        .map(|h| {
            let cfg = &h["config"];
            let model = s(cfg, "model");
            json!({
                "name": s(h, "name"),
                "harness": map::harness_label(if s(cfg, "harness").is_empty() { s(h, "harness") } else { s(cfg, "harness") }),
                "model": map::model_label(model),
                "auth": s(cfg, "authSelectedType"),
            })
        })
        .collect();
    let brokers: Vec<Value> = providers
        .iter()
        .map(|p| {
            ids.insert(format!("b:{}", s(p, "brokerName")), s(p, "brokerId").to_owned());
            let mut note = vec![];
            if s(p, "brokerId") == default_broker {
                note.push("default".to_owned());
            }
            if let (Some(n), Some(max)) = (p["agentCount"].as_u64(), p["agentLimit"].as_u64()) {
                note.push(format!("{n}/{max} agents"));
            }
            json!({ "name": s(p, "brokerName"), "kind": s(p, "status"), "note": note.join(" · ") })
        })
        .collect();
    *hub.ids.lock().unwrap_or_else(|e| e.into_inner()) = ids;
    Ok(json!({
        "templates": out,
        "harness_configs": harness_configs,
        "brokers": brokers,
        "branch": "",
        "project": slug,
        "command_template": "scion start {name} -t {template} -b {branch} \"{task}\"",
    }))
}

pub fn stop(app: &App, id: &str) -> Result<()> {
    let (hub, a) = agent_of(app, id)?;
    hub.client(app).and_then(|c| c.action(s(&a, "id"), "stop")).map_err(err)?;
    hub.kick.store(true, Ordering::SeqCst);
    Ok(())
}

/// Screen 08's actions on an undelivered send: retry | edit (returns the
/// text) | cancel | discard | start_and_send | forward. The old row is
/// hidden here; nothing is deleted on the Hub.
pub fn delivery(app: &App, thread: &str, row: u64, action: &str, to: Option<&str>) -> Result<Value> {
    let r = app.agents.store.lock().threads.get(thread).and_then(|t| t.rows.iter().find(|r| r.id == row).cloned()).ok_or_else(|| msg("That message is gone"))?;
    let hub = match thread.strip_prefix("r:") {
        Some(conv) => hub_with_conv(app, conv)?,
        None => hub(app, split_id(thread.trim_start_matches("s:")).map(|(s, _)| s).ok_or_else(|| msg("not a Scion thread"))?)?,
    };
    let hide = || {
        let mut st = hub.st();
        st.hidden.insert(r.ext.clone());
        st.pending.retain(|p| p.row.ext != r.ext);
    };
    let out = match action {
        "edit" => {
            hide();
            json!({ "ok": true, "text": r.text })
        }
        "cancel" | "discard" => {
            hide();
            json!({ "ok": true })
        }
        "retry" => {
            hide();
            send(app, thread, &r.text, None)?;
            json!({ "ok": true })
        }
        "forward" => {
            let to = to.ok_or_else(|| msg("Forward to whom?"))?;
            let text = strip_mention(&r.text);
            if thread.starts_with("r:") {
                send(app, thread, text, Some(to))?;
            } else {
                let id = hub.st().agents.iter().find(|a| s(a, "name") == to || s(a, "slug") == to).map(|a| s(a, "id").to_owned()).ok_or_else(|| msg(format!("No agent named {to} on this Hub")))?;
                send(app, &format!("s:{}", map::session_id(&hub.source, &id)), text, None)?;
            }
            hide();
            json!({ "ok": true })
        }
        "start_and_send" => {
            let agent = match thread.strip_prefix("r:") {
                Some(_) => r.to.as_ref().and_then(|w| w.session.clone()).filter(|s| s.starts_with("scion:")),
                None => Some(thread.trim_start_matches("s:").to_owned()),
            };
            let agent = agent.and_then(|s| split_id(&s).map(|(_, a)| a.to_owned())).ok_or_else(|| msg("Start which agent? Forward it instead"))?;
            hide();
            start_then_send(app, hub.clone(), thread, &agent, &r.text);
            json!({ "ok": true })
        }
        _ => return Err(msg(format!("unknown delivery action '{action}'"))),
    };
    publish(app, &hub);
    Ok(out)
}

fn strip_mention(text: &str) -> &str {
    match text.strip_prefix('@') {
        Some(rest) => rest.split_once(' ').map(|(_, t)| t.trim_start()).unwrap_or(text),
        None => text,
    }
}

/// Start (or resume) the agent, wait for it to run, then send.
fn start_then_send(app: &App, hub: Arc<Hub>, thread: &str, agent: &str, text: &str) {
    let Ok((_, t, content, who)) = target(app, thread, text, None) else { return };
    let (row, ext) = queue(app, &hub, thread, &content, who);
    let o = Out { key: thread.to_owned(), row, ext, target: t, content, reply_to: None, kind: "instruction" };
    let agent = agent.to_owned();
    later(app, move |app| {
        let started = hub.client(app).and_then(|c| {
            if let Err(e) = c.action(&agent, "start") {
                if !hub_error(&e).is_some_and(|h| h.status == 409) {
                    return Err(e);
                }
            }
            for _ in 0..40 {
                if s(&c.agent(&agent)?, "phase") == "running" {
                    return Ok(());
                }
                std::thread::sleep(Duration::from_secs(3));
            }
            Err(anyhow::anyhow!("it didn't reach running in two minutes"))
        });
        match started {
            Ok(()) => dispatch(app, &hub, &o),
            Err(e) => {
                let d = Delivery { state: "unreachable".into(), at: now(app), reason: format!("couldn't start it: {e}"), ..Default::default() };
                if let Some(p) = hub.st().pending.iter_mut().find(|p| p.row.ext == o.ext) {
                    p.row.delivery = Some(d.clone());
                }
                app.agents.store.lock().update_row(&o.key, o.row, |r| r.delivery = Some(d));
            }
        }
    });
}

/// The terminal button: `scion attach <slug> --hub <url> -g <project>`.
pub fn attach_command(s: &Session) -> String {
    ATTACH.lock().unwrap_or_else(|e| e.into_inner()).get(&s.id).cloned().unwrap_or_else(|| format!("scion attach {}", s.name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::model::Status;
    use crate::ai::context::tests::app;
    use crate::sync::http::FakeHttp;
    use map::tests::{fixture, DIFF, PLANNER, ROOM};

    const PID: &str = "7d9a0c2e-5b1f-4c3a-8e7d-2f1a3b4c5d01";
    const TOPIC: &str = "b1000000-0000-4000-8000-0000000000bb";

    fn text(name: &str) -> String {
        fixture(name).to_string()
    }

    fn hub_http() -> Arc<FakeHttp> {
        let http = Arc::new(FakeHttp::default());
        http.on("GET", "/api/v1/auth/me", 200, text("me.json"));
        http.on("GET", "/api/v1/projects?", 200, text("projects.json"));
        http.on("GET", "/agents?limit=500", 200, r#"{"agents":[],"totalCount":0}"#);
        http.on("GET", &format!("/api/v1/projects/{PID}/agents"), 200, text("agents.json"));
        http.on("GET", "/api/v1/conversations?include_project_groups", 200, r#"{"conversations":[]}"#);
        http.on("GET", &format!("/api/v1/conversations?include_project_groups={PID}"), 200, text("conversations.json"));
        http.on("GET", &format!("/api/v1/conversations/{ROOM}"), 200, text("conversation.json"));
        http.on("GET", &format!("/api/v1/conversations/{ROOM}/messages"), 200, text("room_messages.json"));
        http.on("GET", "/api/v1/conversations/c2000000-0000-4000-8000-0000000000cc/messages", 200, text("dm_messages.json"));
        http.on("GET", "/interagent", 200, r#"{"messages":[]}"#);
        http
    }

    /// An app with one connected Hub source, polled once.
    fn connected(http: &Arc<FakeHttp>) -> (App, tempfile::TempDir, Arc<Hub>) {
        let (app, dir) = app();
        app.platform.secret_set("scion.token", "scion_pat_test").unwrap();
        let src: crate::settings::SourceConfig = serde_json::from_value(json!({ "kind": "scion", "name": "work-hub", "tag": "work", "config": { "hub_url": "https://hub.example.com", "projects": ["ingest-svc"] } })).unwrap();
        app.update_settings(|s| s.sources.push(src.clone()));
        let cfg = ScionConfig::from_json(&src.config).unwrap();
        let h = Arc::new(Hub::new("work-hub", cfg, String::new(), http.clone()));
        app.agents.scion.get().insert("work-hub".into(), h.clone());
        poll(&app, &h);
        (app, dir, h)
    }

    fn posted(http: &FakeHttp, part: &str) -> Value {
        let (_, _, body) = http.requests().into_iter().rev().find(|(m, u, _)| m == "POST" && u.contains(part)).unwrap_or_else(|| panic!("no POST to {part}"));
        serde_json::from_str(&body.unwrap()).unwrap()
    }

    #[test]
    fn a_poll_fills_the_tree() {
        let http = hub_http();
        let (app, _d, _h) = connected(&http);
        let st = &hubs(&app)[0];
        assert_eq!((st["state"].as_str(), st["name"].as_str()), (Some("connected"), Some("hub.example.com")), "{st}");
        let i = app.agents.store.lock();
        assert_eq!(i.groups.len(), 2);
        assert_eq!(i.sessions.len(), 4);
        assert_eq!(i.sessions[&format!("scion:work-hub:{DIFF}")].status, Status::Waiting);
        assert!(i.threads[&format!("r:{ROOM}")].rows.len() >= 8);
        assert_eq!(i.requests.len(), 1);
        drop(i);
        let s = Session { id: format!("scion:work-hub:{PLANNER}"), name: "planner".into(), ..Default::default() };
        assert_eq!(attach_command(&s), "scion attach planner --hub https://hub.example.com -g ingest-svc");
        assert!(http.requests().iter().any(|(_, u, _)| u.contains("/interagent")), "agent mail is read on a full refresh");
    }

    #[test]
    fn a_second_poll_rereads_only_what_moved() {
        let http = hub_http();
        let (app, _d, h) = connected(&http);
        let before = http.requests().len();
        let rev = app.agents.store.rev();
        poll(&app, &h);
        let urls: Vec<String> = http.requests()[before..].iter().map(|(_, u, _)| u.clone()).collect();
        assert_eq!(urls.len(), 2, "the conversation list, and the room with a send still pending: {urls:?}");
        assert!(urls[1].contains(&format!("{ROOM}/messages")), "{urls:?}");
        assert_eq!(app.agents.store.rev(), rev, "nothing changed, nothing bumped");
    }

    #[test]
    fn sending_to_a_room_member_posts_a_mention() {
        let http = hub_http();
        http.on("POST", "/api/v1/chat/conversations/", 201, text("chat_send.json"));
        let (app, _d, _h) = connected(&http);
        let key = format!("r:{ROOM}");
        send(&app, &key, "ship it", Some("planner")).unwrap();
        let body = posted(&http, &format!("/api/v1/chat/conversations/{TOPIC}/messages"));
        assert_eq!(body["content"], "@planner ship it");
        assert!(body["idempotency_key"].as_str().unwrap().starts_with("local:"));
        let i = app.agents.store.lock();
        let r = i.threads[&key].rows.iter().find(|r| r.text == "@planner ship it").unwrap();
        assert_eq!(r.ext, "f0000000-0000-4000-8000-000000000010", "the row takes the Hub's id");
        let d = r.delivery.as_ref().unwrap();
        assert_eq!((d.state.as_str(), d.count), ("delivered", 1));
        assert_eq!(r.to.as_ref().unwrap().name, "planner");
    }

    #[test]
    fn a_dm_to_a_stopped_agent_is_unreachable() {
        let http = hub_http();
        http.on("POST", "/message", 409, text("agent_not_running.json"));
        let (app, _d, _h) = connected(&http);
        let key = "s:scion:work-hub:a1000000-0000-4000-8000-000000000003";
        send(&app, key, "take another look", None).unwrap();
        let body = posted(&http, "/api/v1/agents/a1000000-0000-4000-8000-000000000003/message");
        assert_eq!(body["structured_message"]["msg"], "take another look");
        let i = app.agents.store.lock();
        let r = i.threads[key].rows.iter().find(|r| r.kind == Kind::You && r.text == "take another look").unwrap();
        assert_eq!(r.delivery.as_ref().unwrap().state, "unreachable");
        drop(i);
        // The next publish keeps it: the Hub never stored it.
        publish(&app, &hub(&app, "work-hub").unwrap());
        assert!(app.agents.store.lock().threads[key].rows.iter().any(|r| r.text == "take another look"));
        let row = app.agents.store.lock().threads[key].rows.iter().find(|r| r.text == "take another look").unwrap().id;
        let v = delivery(&app, key, row, "edit", None).unwrap();
        assert_eq!(v["text"], "take another look");
        assert!(!app.agents.store.lock().threads[key].rows.iter().any(|r| r.text == "take another look"), "edit takes it back");
    }

    #[test]
    fn answering_an_ask_replies_to_it_and_unpins_it() {
        let http = hub_http();
        http.on("POST", "/api/v1/chat/conversations/", 201, text("chat_send.json"));
        let (app, _d, h) = connected(&http);
        let req = app.agents.store.lock().requests.keys().next().cloned().unwrap();
        answer(&app, &req, "staging").unwrap();
        let body = posted(&http, "/messages");
        assert_eq!((body["content"].as_str(), body["reply_to_id"].as_str()), (Some("staging"), Some("f0000000-0000-4000-8000-000000000005")));
        assert!(app.agents.store.lock().requests.is_empty());
        publish(&app, &h);
        assert!(app.agents.store.lock().requests.is_empty(), "it stays answered");
    }

    #[test]
    fn starting_an_agent_creates_it_and_adds_it_to_the_room() {
        let http = hub_http();
        http.on("POST", "/api/v1/agents", 201, text("create_agent.json"));
        http.on("POST", "/participants", 201, text("participant.json"));
        let (app, _d, _h) = connected(&http);
        let group = format!("scion:work-hub:{PID}:{ROOM}");
        let args = json!({ "group": group, "name": "migrator", "template": "coder", "harness_config": "claude-web", "broker": "gcp-1", "branch": "migrator-users", "task": "Write the migration for the users table" });
        assert!(start_agent(&app, &json!({ "group": group, "name": "planner", "template": "coder" })).is_err(), "names are unique in a project");
        let id = start_agent(&app, &args).unwrap();
        assert_eq!(id, "scion:work-hub:a1000000-0000-4000-8000-000000000005");
        let body = posted(&http, "/api/v1/agents");
        assert_eq!(body, json!({ "name": "migrator", "projectId": PID, "template": "coder", "harnessConfig": "claude-web", "runtimeBrokerId": "e1000000-0000-4000-8000-000000000001", "branch": "migrator-users", "task": "Write the migration for the users table" }));
        let p = posted(&http, "/participants");
        assert_eq!(p, json!({ "principalKind": "agent", "principalId": "a1000000-0000-4000-8000-000000000005" }));
        assert!(app.agents.store.lock().sessions.contains_key(&id));
    }

    #[test]
    fn the_catalog_lists_templates_harness_configs_and_brokers() {
        let http = hub_http();
        http.on("GET", "/api/v1/templates?", 200, text("templates.json"));
        http.on("GET", "/files/agents.md", 200, text("template_file.json"));
        http.on("GET", "/api/v1/harness-configs?", 200, text("harness_configs.json"));
        http.on("GET", "/providers", 200, text("providers.json"));
        http.on("POST", "/api/v1/agents", 201, text("create_agent.json"));
        let (app, _d, _h) = connected(&http);
        let group = format!("scion:work-hub:{PID}:agents");
        let c = catalog(&app, &group).unwrap();
        let coder = &c["templates"][0];
        assert_eq!(coder["name"], "coder");
        assert_eq!(coder["versions"], json!(["project", "global"]));
        assert!(coder["instructions"].as_str().unwrap().starts_with("# Coder"));
        assert_eq!(coder["skills"], json!(["sql-diff", "pr-notes"]));
        assert_eq!(c["templates"][1]["name"], "planner");
        assert_eq!(c["harness_configs"][0], json!({ "name": "claude-web", "harness": "Claude Code", "model": "sonnet", "auth": "oauth-token" }));
        assert_eq!(c["brokers"][0], json!({ "name": "gcp-1", "kind": "online", "note": "default · 3/10 agents" }));
        assert_eq!(c["project"], "ingest-svc");
        assert!(c["command_template"].as_str().unwrap().starts_with("scion start {name} -t {template} -b {branch}"));
        // The form's chips map back to Hub ids.
        start_agent(&app, &json!({ "group": group, "name": "migrator", "template": "coder", "template_version": "global", "broker": "gcp-1" })).unwrap();
        let body = posted(&http, "/api/v1/agents");
        assert_eq!((body["template"].as_str(), body["runtimeBrokerId"].as_str()), (Some("70000000-0000-4000-8000-000000000002"), Some("e1000000-0000-4000-8000-000000000001")));
    }

    #[test]
    fn a_removed_source_takes_its_agents_with_it() {
        let http = hub_http();
        let (app, _d, _h) = connected(&http);
        clear(&app, "work-hub");
        let i = app.agents.store.lock();
        assert!(i.groups.is_empty() && i.sessions.is_empty() && i.requests.is_empty());
        assert!(!i.threads.contains_key(&format!("r:{ROOM}")));
    }

    #[test]
    fn errors_show_on_the_hub() {
        let http = Arc::new(FakeHttp::default());
        http.on("GET", "/api/v1/auth/me", 401, r#"{"error":{"code":"unauthorized","message":"invalid access token"}}"#);
        let (app, _d, _h) = connected(&http);
        let st = &hubs(&app)[0];
        assert_eq!(st["state"], "error");
        assert!(st["error"].as_str().unwrap().contains("rejected the access token"), "{st}");
    }

    #[test]
    fn slugs_and_mentions() {
        assert_eq!(slugify(" Schema Diff! "), "schema-diff");
        assert_eq!(strip_mention("@planner ship it"), "ship it");
        assert_eq!(strip_mention("ship it"), "ship it");
    }
}

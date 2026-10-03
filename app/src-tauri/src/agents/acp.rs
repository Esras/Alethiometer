//! Local agents over ACP v1 (JSON-RPC 2.0, one message per line).
//! Alethiometer is the client: the harness's adapter (`agents.harnesses`,
//! e.g. claude-agent-acp, pi-acp) signs in its own way. No fs/terminal
//! capabilities are offered, so agents use their own tools.
//!
//! The adapters run in the agent host (`daemon`, `app/agentd`), not in the
//! app: a quit, crash or rebuild of the app leaves them running, mid-turn or
//! waiting on an approval. Their stdout comes back as numbered events
//! (`on_event`); everything needed to pick up after a restart (open rows,
//! tool calls, prompts and approvals in flight) is the `Turn`, stored with
//! how far the log has been read (`store::HostState`).
//!
//! `session/update` notifications become rows (`apply`); a
//! `session/request_permission` becomes a pinned approval answered with one of
//! the agent's own options. "Always" is whatever the agent offers: the agent
//! keeps that rule, not Alethiometer.
//!
//! An agent that exited (its host stopped, the machine restarted) is started
//! again when you message it, and its ACP session resumed (`session/resume`,
//! else `session/load` with the replay dropped: the transcript is stored).

use super::model::{Delivery, Kind, PermOption, Request, Row, RowImage, Session, Status, Tool, Who};
use super::store::HostState;
use super::{daemon, now, session_thread};
use crate::error::{msg, Result};
use crate::state::App;
use agentd::proto::{self as dproto, Event, HostInfo};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex, OnceLock};
use std::time::Duration;

const PROTOCOL_VERSION: u64 = 1;
/// npx may download the adapter on first run.
const INIT_TIMEOUT: Duration = Duration::from_secs(240);
const CALL_TIMEOUT: Duration = Duration::from_secs(120);

fn now_ms() -> u64 {
    dproto::now_ms()
}

#[derive(Default)]
pub struct Host {
    conns: Mutex<HashMap<String, Arc<Conn>>>,
    /// Sessions being started or reattached right now.
    starting: Mutex<std::collections::HashSet<String>>,
}

impl Host {
    fn get(&self, id: &str) -> Option<Arc<Conn>> {
        self.conns.lock().unwrap_or_else(|e| e.into_inner()).get(id).filter(|c| c.alive.load(Ordering::SeqCst)).cloned()
    }

    fn any(&self, id: &str) -> Option<Arc<Conn>> {
        self.conns.lock().unwrap_or_else(|e| e.into_inner()).get(id).cloned()
    }

    fn all(&self) -> Vec<Arc<Conn>> {
        self.conns.lock().unwrap_or_else(|e| e.into_inner()).values().cloned().collect()
    }
}

type Reply = std::result::Result<Value, Value>;

/// One agent in the agent host, and the ACP session it runs.
pub struct Conn {
    session: String,
    app: std::sync::Weak<App>,
    /// JSON-RPC ids: unique across app restarts (a prompt's answer can arrive in the next run).
    next: AtomicU64,
    /// Requests this run is waiting on (initialize, session/new …).
    pending: Mutex<HashMap<u64, mpsc::Sender<Reply>>>,
    alive: AtomicBool,
    /// Stopped on purpose: its exit isn't a failure.
    stopping: AtomicBool,
    /// While `session/load` replays history, its updates are dropped (the transcript is stored).
    replaying: AtomicBool,
    turn: Mutex<Turn>,
    stderr: Mutex<Vec<String>>,
}

/// An approval waiting on you: the agent's request id and what it asked.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct Waiting {
    rpc: Value,
    params: Value,
}

/// Where streamed updates go, and what's in flight. Stored after every
/// event, so another run of the app can carry on from it.
#[derive(Debug, Default, Serialize, Deserialize)]
pub(crate) struct Turn {
    /// Open output / thinking row per thread (a subagent's go to its own thread), keyed by `open_key`.
    #[serde(default)]
    open: HashMap<String, u64>,
    /// toolCallId → (thread, row id, started ms).
    #[serde(default)]
    tools: HashMap<String, (String, u64, u64)>,
    /// Subagent Task toolCallId → child session id.
    #[serde(default)]
    subagents: HashMap<String, String>,
    /// Permission requests waiting on you, by request id.
    #[serde(default)]
    waiting: HashMap<String, Waiting>,
    /// Prompts sent and not yet answered: JSON-RPC id → your row.
    #[serde(default)]
    prompts: BTreeMap<u64, u64>,
    #[serde(default)]
    caps: Value,
    /// The ACP sessionId.
    #[serde(default)]
    sid: String,
}

fn open_key(thread: &str, k: Kind) -> String {
    format!("{thread}\u{1f}{k:?}")
}

impl Conn {
    fn new(app: &App, session: &str, turn: Turn, alive: bool) -> Conn {
        Conn {
            session: session.to_owned(),
            app: app.arc().map(|a| Arc::downgrade(&a)).unwrap_or_default(),
            next: AtomicU64::new(now_ms() * 1000),
            pending: Mutex::new(HashMap::new()),
            alive: AtomicBool::new(alive),
            stopping: AtomicBool::new(false),
            replaying: AtomicBool::new(false),
            turn: Mutex::new(turn),
            stderr: Mutex::new(vec![]),
        }
    }

    fn write(&self, v: &Value) -> Result<()> {
        let app = self.app.upgrade().ok_or_else(|| msg("shutting down"))?;
        let line = serde_json::to_string(v)?;
        daemon::call(&app, &dproto::Request::Write { id: self.session.clone(), line }).map(|_| ())
    }

    fn request(&self, method: &str, params: Value, timeout: Option<Duration>) -> Result<Value> {
        let id = self.next.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = mpsc::channel();
        self.pending.lock().unwrap_or_else(|e| e.into_inner()).insert(id, tx);
        self.write(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }))?;
        let got = match timeout {
            Some(t) => rx.recv_timeout(t).map_err(|_| msg(format!("{method}: no answer from the agent"))),
            None => rx.recv().map_err(|_| msg(format!("{method}: the agent exited"))),
        };
        self.pending.lock().unwrap_or_else(|e| e.into_inner()).remove(&id);
        match got? {
            Ok(v) => Ok(v),
            Err(e) => Err(msg(rpc_error(&e))),
        }
    }

    fn notify(&self, method: &str, params: Value) -> Result<()> {
        self.write(&json!({ "jsonrpc": "2.0", "method": method, "params": params }))
    }

    fn respond(&self, id: &Value, result: Value) -> Result<()> {
        self.write(&json!({ "jsonrpc": "2.0", "id": id, "result": result }))
    }

    /// Stop the agent in the host; its exit comes back as an event.
    fn kill(&self) {
        if let Some(app) = self.app.upgrade() {
            if let Err(e) = daemon::call(&app, &dproto::Request::Kill { id: self.session.clone() }) {
                eprintln!("agents: couldn't stop {}: {e}", self.session);
            }
        }
    }

    fn fail_pending(&self) {
        for (_, tx) in self.pending.lock().unwrap_or_else(|e| e.into_inner()).drain() {
            let _ = tx.send(Err(json!({ "message": "the agent exited" })));
        }
    }

    fn sid(&self) -> String {
        self.turn.lock().unwrap_or_else(|e| e.into_inner()).sid.clone()
    }

    fn stderr_tail(&self) -> String {
        let e = self.stderr.lock().unwrap_or_else(|e| e.into_inner());
        e.iter().rev().take(3).rev().cloned().collect::<Vec<_>>().join(" · ")
    }
}

pub fn rpc_error(e: &Value) -> String {
    let m = e.get("message").and_then(Value::as_str).unwrap_or("error");
    let detail = e.get("data").and_then(|d| d.get("details").or(d.get("message"))).and_then(Value::as_str);
    match detail {
        Some(d) if d != m => format!("{m}: {d}"),
        _ => m.to_owned(),
    }
}

/// This machine's short name, for host tags ("laptop").
pub fn host_label() -> String {
    static H: OnceLock<String> = OnceLock::new();
    H.get_or_init(|| {
        let out = Command::new("hostname").arg("-s").output().ok().filter(|o| o.status.success());
        out.map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
            .filter(|s| !s.is_empty())
            .or_else(|| std::env::var("HOSTNAME").ok())
            .unwrap_or_else(|| "local".into())
    })
    .clone()
}

/// The login shell's PATH: apps started from the Dock get a minimal one, and
/// npx (or the adapter) usually lives somewhere the shell's profile adds.
fn login_path() -> String {
    static P: OnceLock<String> = OnceLock::new();
    P.get_or_init(|| {
        let current = std::env::var("PATH").unwrap_or_default();
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let out = Command::new(shell).args(["-ilc", "printf '__P__%s__P__' \"$PATH\""]).stdin(Stdio::null()).stderr(Stdio::null()).output();
            let _ = tx.send(out);
        });
        let found = rx
            .recv_timeout(Duration::from_secs(5))
            .ok()
            .and_then(|o| o.ok())
            .and_then(|o| {
                let s = String::from_utf8_lossy(&o.stdout).into_owned();
                let start = s.find("__P__")? + 5;
                let end = s[start..].find("__P__")? + start;
                Some(s[start..end].to_owned())
            })
            .unwrap_or_default();
        let mut parts: Vec<&str> = found.split(':').filter(|p| !p.is_empty()).collect();
        for p in current.split(':') {
            if !p.is_empty() && !parts.contains(&p) {
                parts.push(p);
            }
        }
        parts.join(":")
    })
    .clone()
}

/// Where `cmd` is on the login PATH, or None.
fn which(cmd: &str, path: &str) -> Option<std::path::PathBuf> {
    if cmd.contains('/') {
        let p = std::path::PathBuf::from(cmd);
        return p.exists().then_some(p);
    }
    path.split(':').map(|d| std::path::Path::new(d).join(cmd)).find(|p| p.is_file())
}

pub struct StartArgs {
    pub harness: String,
    pub cwd: String,
    pub name: Option<String>,
    pub group_id: Option<String>,
    pub task: String,
}

fn expand_home(p: &str) -> String {
    match (p.strip_prefix('~'), dirs::home_dir()) {
        (Some(rest), Some(h)) => format!("{}{rest}", h.display()),
        _ => p.to_owned(),
    }
}

fn tilde(p: &str) -> String {
    match dirs::home_dir().map(|h| h.display().to_string()) {
        Some(h) if p.starts_with(&h) => format!("~{}", &p[h.len()..]),
        _ => p.to_owned(),
    }
}

/// Title and subtitle from the first prompt: its first sentence, then the rest.
fn title_from(task: &str) -> (String, String) {
    let t = task.trim();
    let first_line = t.lines().next().unwrap_or("");
    let cut = first_line.find(". ").map(|i| i + 1).unwrap_or(first_line.len());
    let title = crate::ai::context::clip(first_line[..cut].trim(), 90);
    let rest = t[cut.min(t.len())..].trim().replace('\n', " ");
    (title, crate::ai::context::clip(&rest, 140))
}

/// Start a new local session; the first prompt (if any) is sent once it's up.
pub fn start_session(app: &App, a: &StartArgs) -> Result<String> {
    let settings = app.settings();
    let h = settings.agents.harnesses.iter().find(|h| h.key == a.harness).ok_or_else(|| msg(format!("no harness '{}' in agents.harnesses", a.harness)))?;
    let cwd = expand_home(a.cwd.trim());
    if !std::path::Path::new(&cwd).is_dir() {
        return Err(msg(format!("{} isn't a directory", a.cwd)));
    }
    let id = format!("acp-{:08x}", rand_u32());
    let name = a.name.clone().filter(|n| !n.trim().is_empty()).map(|n| n.trim().to_owned()).unwrap_or_else(|| format!("{}-{}", h.key, &id[4..8]));
    let group_key = a.group_id.clone().unwrap_or_else(|| "unsorted".into());
    let color = super::store::color_for(&app.db(), &group_key, &id, settings.agents.names.palette.len())?;
    let (title, subtitle) = title_from(&a.task);
    let ts = now(app);
    let s = Session {
        id: id.clone(),
        source: "acp".into(),
        group_id: a.group_id.clone(),
        name,
        template: h.key.clone(),
        harness: h.label.clone(),
        host: host_label(),
        cwd: cwd.clone(),
        status: Status::Running,
        title,
        subtitle,
        context: format!("local · {}", tilde(&cwd)),
        created_at: ts.clone(),
        active_at: ts.clone(),
        color,
        stats: super::model::Stats { started_at: ts.clone(), ..Default::default() },
        ..Default::default()
    };
    {
        let mut i = app.agents.store.lock();
        i.put_session(s);
        i.push_row(&session_thread(&id), state_row(&ts, &format!("Session started · {} · {} · {}", h.label, host_label(), tilde(&cwd))));
    }
    let task = a.task.trim().to_owned();
    let Some(arc) = app.arc() else { return Ok(id) };
    let sid = id.clone();
    std::thread::spawn(move || {
        let queued = (!task.is_empty()).then(|| push_you(&arc, &sid, &task, true, vec![]));
        match connect(&arc, &sid, None) {
            Ok(conn) => {
                if let Some(row) = queued {
                    prompt(&arc, &conn, row, task, vec![]);
                } else {
                    set_status(&arc, &sid, Status::Idle);
                }
            }
            Err(e) => fail(&arc, &sid, &e.to_string()),
        }
        super::facts::refresh(&arc, &sid);
        super::guess::suggest(&arc, &sid);
    });
    Ok(id)
}

fn rand_u32() -> u32 {
    let mut b = [0u8; 4];
    ring::rand::SecureRandom::fill(&ring::rand::SystemRandom::new(), &mut b).ok();
    u32::from_le_bytes(b) ^ (std::process::id() << 8)
}

fn state_row(at: &str, text: &str) -> Row {
    Row { at: at.into(), kind: Kind::State, text: text.into(), ..Default::default() }
}

fn set_status(app: &App, id: &str, st: Status) {
    let ts = now(app);
    app.agents.store.lock().update_session(id, |s| {
        s.status = st;
        s.active_at = ts;
    });
}

fn fail(app: &App, id: &str, why: &str) {
    let ts = now(app);
    let mut i = app.agents.store.lock();
    i.update_session(id, |s| {
        s.status = Status::Failed;
        s.live = false;
        s.problem = why.to_owned();
    });
    i.push_row(&session_thread(id), Row { at: ts, kind: Kind::Failed, text: why.to_owned(), label: "couldn't start".into(), ..Default::default() });
}

/// Start the adapter in the agent host, initialize, and open (or resume) the ACP session.
fn connect(app: &Arc<App>, id: &str, resume: Option<&str>) -> Result<Arc<Conn>> {
    let s = app.agents.store.lock().sessions.get(id).cloned().ok_or_else(|| msg("no such session"))?;
    let settings = app.settings();
    let h = settings.agents.harnesses.iter().find(|h| h.key == s.template).ok_or_else(|| msg(format!("the {} harness is gone from settings", s.template)))?;
    let path = login_path();
    let exe = which(&h.command, &path).ok_or_else(|| msg(format!("{} isn't on your PATH (agents.harnesses.{}.command)", h.command, h.key)))?;
    // A new process starts its turn over; its events keep counting from where the log was read.
    let after = app.agents.store.lock().hosts.get(id).map(|h| h.seq).unwrap_or(0);
    app.agents.store.lock().set_host(id, HostState { seq: after, turn: Value::Null });
    let conn = Arc::new(Conn::new(app, id, Turn::default(), true));
    app.agents.acp.conns.lock().unwrap_or_else(|e| e.into_inner()).insert(id.to_owned(), conn.clone());
    let mut env: BTreeMap<String, String> = h.env.clone().into_iter().collect();
    env.insert("PATH".into(), path);
    let spawned = daemon::call(
        app,
        &dproto::Request::Spawn { id: id.to_owned(), cmd: exe.display().to_string(), args: h.args.clone(), env, cwd: s.cwd.clone(), after },
    );
    if let Err(e) = spawned {
        conn.alive.store(false, Ordering::SeqCst);
        return Err(e);
    }
    daemon::kick(app);

    let init = conn
        .request(
            "initialize",
            json!({
                "protocolVersion": PROTOCOL_VERSION,
                "clientCapabilities": { "fs": { "readTextFile": false, "writeTextFile": false }, "terminal": false },
                "clientInfo": { "name": "alethiometer", "version": env!("CARGO_PKG_VERSION") },
            }),
            Some(INIT_TIMEOUT),
        )
        .map_err(|e| {
            let tail = conn.stderr_tail();
            conn.kill();
            msg(if tail.is_empty() { e.to_string() } else { format!("{e} · {tail}") })
        })?;
    let caps = init.get("agentCapabilities").cloned().unwrap_or(Value::Null);
    conn.turn.lock().unwrap_or_else(|e| e.into_inner()).caps = caps.clone();

    let cwd = s.cwd.clone();
    let opened = match resume {
        Some(sid) if caps.pointer("/sessionCapabilities/resume").is_some() => {
            conn.request("session/resume", json!({ "sessionId": sid, "cwd": cwd, "mcpServers": [] }), Some(CALL_TIMEOUT)).map(|v| (sid.to_owned(), v))
        }
        Some(sid) if caps.get("loadSession").and_then(Value::as_bool) == Some(true) => {
            conn.replaying.store(true, Ordering::SeqCst);
            let r = conn.request("session/load", json!({ "sessionId": sid, "cwd": cwd, "mcpServers": [] }), Some(CALL_TIMEOUT));
            conn.replaying.store(false, Ordering::SeqCst);
            r.map(|v| (sid.to_owned(), v))
        }
        Some(_) => Err(msg("this agent can't resume a session; start a new one")),
        None => conn.request("session/new", json!({ "cwd": cwd, "mcpServers": [] }), Some(CALL_TIMEOUT)).and_then(|v| {
            let sid = v.get("sessionId").and_then(Value::as_str).ok_or_else(|| msg("session/new: no sessionId"))?.to_owned();
            Ok((sid, v))
        }),
    };
    let (sid, opened) = opened.map_err(|e| {
        conn.kill();
        e
    })?;
    conn.turn.lock().unwrap_or_else(|e| e.into_inner()).sid = sid.clone();
    // In-project edits are the agent's call to let through (agents.accept_edits).
    if settings.agents.accept_edits {
        if let Some(mode) = accept_edits_mode(&opened) {
            if let Err(e) = conn.request("session/set_mode", json!({ "sessionId": sid, "modeId": mode }), Some(CALL_TIMEOUT)) {
                eprintln!("agents: couldn't switch {id} to {mode}: {e}");
            }
        }
    }
    let model = model_of(&opened);
    app.agents.store.lock().update_session(id, |s| {
        s.ext = sid;
        s.live = true;
        s.problem.clear();
        if let Some(m) = model {
            s.model = m;
        }
    });
    save_turn(app, &conn, None);
    Ok(conn)
}

/// Store the turn (and, after an event, how far the log has been read).
fn save_turn(app: &App, conn: &Conn, seq: Option<u64>) {
    let turn = serde_json::to_value(&*conn.turn.lock().unwrap_or_else(|e| e.into_inner())).unwrap_or(Value::Null);
    let mut i = app.agents.store.lock();
    let seq = seq.unwrap_or_else(|| i.hosts.get(&conn.session).map(|h| h.seq).unwrap_or(0));
    i.set_host(&conn.session, HostState { seq, turn });
}

/// An event's time on the app's clock (which `fake_now` may shift).
fn ts_of(app: &App, at_ms: u64) -> String {
    let ms = at_ms as i64 + app.clock.offset_ms();
    chrono::DateTime::from_timestamp_millis(ms)
        .map(|t| t.with_timezone(&chrono::Local).naive_local().format("%Y-%m-%dT%H:%M:%S").to_string())
        .unwrap_or_else(|| now(app))
}

/// The Conn for a session's agent, made from its stored turn when this run hasn't seen it yet.
fn conn_for(app: &App, id: &str) -> Option<Arc<Conn>> {
    if let Some(c) = app.agents.acp.any(id) {
        return Some(c);
    }
    let exists = app.agents.store.lock().sessions.get(id).is_some_and(|s| s.source == "acp" && s.parent_id.is_none());
    if !exists {
        return None;
    }
    let turn: Turn = app.agents.store.lock().hosts.get(id).and_then(|h| serde_json::from_value(h.turn.clone()).ok()).unwrap_or_default();
    let c = Arc::new(Conn::new(app, id, turn, false));
    app.agents.acp.conns.lock().unwrap_or_else(|e| e.into_inner()).insert(id.to_owned(), c.clone());
    Some(c)
}

/// One event from the agent host, in order. Already-applied ones are skipped,
/// so a resubscribe that overlaps is harmless.
pub fn on_event(app: &Arc<App>, ev: Event) {
    let Some(conn) = conn_for(app, &ev.id) else {
        if ev.kind == dproto::Kind::Exit {
            let _ = daemon::call(app, &dproto::Request::Forget { id: ev.id });
        }
        return;
    };
    // Rows and the log position move together (see `Store::gate`).
    let _gate = app.agents.store.gate();
    let seen = app.agents.store.lock().hosts.get(&ev.id).map(|h| h.seq).unwrap_or(0);
    if ev.seq <= seen {
        return;
    }
    let _ = app.agents.store.ensure_loaded(&app.db(), &session_thread(&ev.id));
    let ts = ts_of(app, ev.at_ms);
    match ev.kind {
        dproto::Kind::Out => {
            // Adapters and wrappers sometimes print non-JSON lines; skip them.
            if let Some(v) = ev.line.as_deref().and_then(|l| serde_json::from_str::<Value>(l.trim()).ok()) {
                handle(app, &conn, v, &ts);
            }
        }
        dproto::Kind::Err => {
            let mut e = conn.stderr.lock().unwrap_or_else(|e| e.into_inner());
            e.push(ev.line.unwrap_or_default());
            if e.len() > 50 {
                e.remove(0);
            }
        }
        dproto::Kind::Exit => {
            exited(app, &conn, ev.line.as_deref(), &ts);
            save_turn(app, &conn, Some(ev.seq));
            let _ = daemon::call(app, &dproto::Request::Forget { id: ev.id });
            return;
        }
    }
    save_turn(app, &conn, Some(ev.seq));
}

/// The agent's process ended: close what was open, say why if it was busy.
fn exited(app: &App, conn: &Conn, note: Option<&str>, ts: &str) {
    let was_alive = conn.alive.swap(false, Ordering::SeqCst);
    conn.fail_pending();
    let busy = {
        let mut t = conn.turn.lock().unwrap_or_else(|e| e.into_inner());
        let busy = !t.prompts.is_empty();
        let open: Vec<(String, u64)> = t.open.drain().map(|(k, id)| (k.split('\u{1f}').next().unwrap_or("").to_owned(), id)).collect();
        t.prompts.clear();
        t.waiting.clear();
        let mut i = app.agents.store.lock();
        for (k, id) in open {
            i.update_row(&k, id, |r| r.open = false);
        }
        busy
    };
    let tail = note.map(str::to_owned).unwrap_or_else(|| conn.stderr_tail());
    let mut i = app.agents.store.lock();
    let reqs: Vec<String> = i.requests.values().filter(|r| r.session_id == conn.session).map(|r| r.id.clone()).collect();
    for r in reqs {
        i.remove_request(&r);
    }
    let failed = !conn.stopping.swap(false, Ordering::SeqCst) && (busy || i.sessions.get(&conn.session).is_some_and(|s| s.status == Status::Waiting));
    i.update_session(&conn.session, |s| {
        s.live = false;
        if failed {
            s.status = Status::Failed;
            s.problem = if tail.is_empty() { "the agent exited".into() } else { tail.clone() };
        } else if s.status != Status::Failed {
            s.status = Status::Idle;
        }
    });
    let children: Vec<String> = i.sessions.values().filter(|c| c.parent_id.as_deref() == Some(conn.session.as_str()) && c.status == Status::Running).map(|c| c.id.clone()).collect();
    for c in children {
        i.update_session(&c, |c| {
            c.status = Status::Idle;
            c.live = false;
        });
    }
    if failed && (was_alive || note.is_some()) {
        i.push_row(&session_thread(&conn.session), Row { at: ts.into(), kind: Kind::Failed, label: "exited".into(), text: if tail.is_empty() { "The agent exited.".into() } else { tail }, ..Default::default() });
    }
}

/// After (re)connecting to the agent host: attach the agents still running,
/// with the approvals they were waiting on; settle the ones that aren't.
pub fn restore(app: &Arc<App>, hosts: &[HostInfo]) {
    let alive: HashMap<&str, bool> = hosts.iter().map(|h| (h.id.as_str(), h.alive)).collect();
    let local: Vec<Session> = app.agents.store.lock().sessions.values().filter(|s| s.source == "acp" && s.parent_id.is_none()).cloned().collect();
    for s in local {
        match alive.get(s.id.as_str()) {
            Some(true) => {
                let _ = app.agents.store.ensure_loaded(&app.db(), &session_thread(&s.id));
                let Some(conn) = conn_for(app, &s.id) else { continue };
                conn.alive.store(true, Ordering::SeqCst);
                let waiting: Vec<Waiting> = conn.turn.lock().unwrap_or_else(|e| e.into_inner()).waiting.values().cloned().collect();
                app.agents.store.lock().update_session(&s.id, |x| x.live = true);
                for w in waiting {
                    let req = format!("{}:{}", s.id, w.rpc);
                    if !app.agents.store.lock().requests.contains_key(&req) {
                        permission(app, &conn, w.rpc.clone(), &w.params);
                    }
                }
            }
            // Exited while we were away: its events (ending in the exit) are still to come.
            Some(false) => {}
            None => {
                // Nothing in the host: whatever it was doing is over.
                if matches!(s.status, Status::Running | Status::Waiting) {
                    app.agents.store.lock().update_session(&s.id, |x| {
                        x.status = Status::Idle;
                        x.live = false;
                    });
                }
            }
        }
    }
    // Logs of agents this app no longer knows.
    for h in hosts.iter().filter(|h| !h.alive) {
        if app.agents.store.lock().sessions.get(&h.id).is_none() {
            let _ = daemon::call(app, &dproto::Request::Forget { id: h.id.clone() });
        }
    }
}

/// The connection to the agent host broke: nothing is attached until it's back.
pub fn host_gone(app: &App) {
    for c in app.agents.acp.all() {
        c.alive.store(false, Ordering::SeqCst);
        c.fail_pending();
        app.agents.store.lock().update_session(&c.session, |s| s.live = false);
    }
}

/// Anything local that needs the agent host.
pub fn any_local(app: &App) -> bool {
    let i = app.agents.store.lock();
    !app.agents.acp.starting.lock().unwrap_or_else(|e| e.into_inner()).is_empty()
        || i.sessions.values().any(|s| s.source == "acp" && s.parent_id.is_none() && (matches!(s.status, Status::Running | Status::Waiting) || s.live))
}

/// The agent's own mode that applies edits without asking, if it has one.
pub fn accept_edits_mode(v: &Value) -> Option<String> {
    let modes = v.pointer("/modes/availableModes")?.as_array()?;
    let current = v.pointer("/modes/currentModeId").and_then(Value::as_str);
    let m = modes.iter().filter_map(|m| m.get("id").and_then(Value::as_str)).find(|id| *id == "acceptEdits" || *id == "accept_edits")?;
    (current != Some(m)).then(|| m.to_owned())
}

/// The current model's short name from a session/new (or resume) answer.
pub fn model_of(v: &Value) -> Option<String> {
    let from_config = v.get("configOptions").and_then(Value::as_array).and_then(|opts| {
        let o = opts.iter().find(|o| o.get("id").and_then(Value::as_str) == Some("model"))?;
        o.get("currentValue").and_then(Value::as_str).map(str::to_owned)
    });
    let from_models = v.pointer("/models/currentModelId").and_then(Value::as_str).map(str::to_owned);
    from_config.or(from_models).filter(|m| !m.is_empty()).map(|m| super::explain::short_model(&m))
}

fn handle(app: &Arc<App>, conn: &Arc<Conn>, v: Value, ts: &str) {
    let method = v.get("method").and_then(Value::as_str);
    match (method, v.get("id")) {
        (Some(m), Some(id)) => {
            let params = v.get("params").cloned().unwrap_or(Value::Null);
            match m {
                "session/request_permission" => permission(app, conn, id.clone(), &params),
                _ => {
                    let _ = conn.write(&json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32601, "message": format!("{m} isn't supported by this client") } }));
                }
            }
        }
        (Some("session/update"), None) => {
            if conn.replaying.load(Ordering::SeqCst) {
                return;
            }
            if let Some(u) = v.pointer("/params/update") {
                let mut turn = conn.turn.lock().unwrap_or_else(|e| e.into_inner());
                apply(app, &conn.session, &mut turn, u, ts);
            }
        }
        (Some(_), None) => {}
        (None, Some(id)) => {
            let Some(id) = id.as_u64() else { return };
            let r = match v.get("error") {
                Some(e) => Err(e.clone()),
                None => Ok(v.get("result").cloned().unwrap_or(Value::Null)),
            };
            if let Some(tx) = conn.pending.lock().unwrap_or_else(|e| e.into_inner()).remove(&id) {
                let _ = tx.send(r);
            } else if conn.turn.lock().unwrap_or_else(|e| e.into_inner()).prompts.contains_key(&id) {
                finish_prompt(app, conn, id, r, ts);
            }
        }
        _ => {}
    }
}

fn meta<'a>(u: &'a Value, path: &str) -> Option<&'a Value> {
    u.get("_meta").and_then(|m| m.pointer(path))
}

/// The thread an update belongs to: a subagent's inner updates go to its own.
fn target(turn: &Turn, session: &str, u: &Value) -> (String, Option<String>) {
    let parent = meta(u, "/claudeCode/parentToolUseId").and_then(Value::as_str);
    match parent.and_then(|p| turn.subagents.get(p)) {
        Some(child) => (session_thread(child), Some(child.clone())),
        None => (session_thread(session), None),
    }
}

/// Map one `session/update` onto the store.
pub(crate) fn apply(app: &App, session: &str, turn: &mut Turn, u: &Value, ts: &str) {
    let kind = u.get("sessionUpdate").and_then(Value::as_str).unwrap_or("");
    let store = &app.agents.store;
    match kind {
        "agent_message_chunk" | "agent_thought_chunk" => {
            let text = u.pointer("/content/text").and_then(Value::as_str).unwrap_or("");
            if text.is_empty() {
                return;
            }
            let (key, _) = target(turn, session, u);
            let k = if kind == "agent_message_chunk" { Kind::Output } else { Kind::Thinking };
            let other = if k == Kind::Output { Kind::Thinking } else { Kind::Output };
            let mut i = store.lock();
            // A new kind of text closes the other open row.
            if let Some(id) = turn.open.remove(&open_key(&key, other)) {
                i.update_row(&key, id, |r| r.open = false);
            }
            let last_is_open = turn.open.get(&open_key(&key, k)).is_some_and(|id| i.threads.get(&key).and_then(|t| t.rows.last()).is_some_and(|r| r.id == *id));
            if last_is_open {
                let id = turn.open[&open_key(&key, k)];
                i.update_row(&key, id, |r| {
                    r.text.push_str(text);
                    if k == Kind::Thinking {
                        r.tokens = Some(super::model::rough_tokens(&r.text));
                    }
                });
            } else {
                if let Some(id) = turn.open.remove(&open_key(&key, k)) {
                    i.update_row(&key, id, |r| r.open = false);
                }
                let row = Row {
                    at: ts.into(),
                    kind: k,
                    label: if k == Kind::Output { "output".into() } else { "thinking".into() },
                    text: text.into(),
                    tokens: (k == Kind::Thinking).then(|| super::model::rough_tokens(text)),
                    open: true,
                    ..Default::default()
                };
                let id = i.push_row(&key, row);
                turn.open.insert(open_key(&key, k), id);
            }
        }
        "tool_call" | "tool_call_update" => tool_update(app, session, turn, u, ts),
        "plan" => {
            let entries = u.get("entries").and_then(Value::as_array).cloned().unwrap_or_default();
            let done = entries.iter().filter(|e| e.get("status").and_then(Value::as_str) == Some("completed")).count();
            let current = entries
                .iter()
                .find(|e| e.get("status").and_then(Value::as_str) == Some("in_progress"))
                .and_then(|e| e.get("content").and_then(Value::as_str))
                .map(|c| format!(" · now: {c}"))
                .unwrap_or_default();
            let text = format!("plan · {done} of {} done{current}", entries.len());
            let key = session_thread(session);
            let mut i = store.lock();
            let existing = i.find_row(&key, |r| r.ext == "plan").map(|r| r.id);
            match existing {
                Some(id) => {
                    i.update_row(&key, id, |r| r.text = text);
                }
                None => {
                    i.push_row(&key, Row { at: ts.into(), kind: Kind::State, text, ext: "plan".into(), ..Default::default() });
                }
            }
        }
        "usage_update" => {
            let used = u.get("used").and_then(Value::as_u64);
            let size = u.get("size").and_then(Value::as_u64);
            let cost = u.pointer("/cost/amount").and_then(Value::as_f64);
            let model = meta(u, "/_claude~1model").and_then(Value::as_str).map(super::explain::short_model);
            store.lock().update_session(session, |s| {
                if let Some(x) = used {
                    s.stats.ctx_used = x;
                }
                if let Some(x) = size {
                    s.stats.ctx_size = x;
                }
                // claude-agent-acp reports the session's running total.
                if let Some(c) = cost {
                    s.stats.cost_usd = Some(c);
                }
                if let Some(m) = model {
                    s.model = m;
                }
            });
        }
        "session_info_update" => {
            if let Some(t) = u.get("title").and_then(Value::as_str).filter(|t| !t.trim().is_empty()) {
                let t = t.trim().to_owned();
                store.lock().update_session(session, |s| {
                    if s.title.is_empty() {
                        s.title = t;
                    }
                });
            }
        }
        _ => {}
    }
}

fn strip_fence(s: &str) -> String {
    let t = s.trim();
    if let Some(rest) = t.strip_prefix("```") {
        let body = rest.split_once('\n').map(|(_, b)| b).unwrap_or("");
        return body.trim_end().trim_end_matches("```").trim_end().to_owned();
    }
    t.to_owned()
}

fn rel(path: &str, cwd: &str) -> String {
    let p = path.strip_prefix(cwd).map(|r| r.trim_start_matches('/')).filter(|r| !r.is_empty()).unwrap_or(path);
    tilde(p)
}

/// Lines added and removed between two texts (LCS on lines; big files fall back to counts).
pub fn diff_stat(old: &str, new: &str) -> (u64, u64) {
    let a: Vec<&str> = old.lines().collect();
    let b: Vec<&str> = new.lines().collect();
    if a.len() * b.len() > 4_000_000 {
        let mut m: HashMap<&str, i64> = HashMap::new();
        for l in &a {
            *m.entry(l).or_default() += 1;
        }
        let mut added = 0;
        for l in &b {
            let e = m.entry(l).or_default();
            if *e > 0 {
                *e -= 1;
            } else {
                added += 1;
            }
        }
        let removed: i64 = m.values().filter(|v| **v > 0).sum();
        return (added, removed as u64);
    }
    let mut dp = vec![vec![0u32; b.len() + 1]; a.len() + 1];
    for i in (0..a.len()).rev() {
        for j in (0..b.len()).rev() {
            dp[i][j] = if a[i] == b[j] { dp[i + 1][j + 1] + 1 } else { dp[i + 1][j].max(dp[i][j + 1]) };
        }
    }
    let common = dp[0][0] as u64;
    (b.len() as u64 - common, a.len() as u64 - common)
}

/// Short verb and what it acts on, from the tool's name, kind, title and input.
pub fn tool_label(u: &Value, cwd: &str) -> (String, String) {
    let name = meta(u, "/claudeCode/toolName").and_then(Value::as_str).or(u.get("name").and_then(Value::as_str)).unwrap_or("");
    let kind = u.get("kind").and_then(Value::as_str).unwrap_or("");
    let title = u.get("title").and_then(Value::as_str).unwrap_or("").trim();
    let input = u.get("rawInput").cloned().unwrap_or(Value::Null);
    let s = |k: &str| input.get(k).and_then(Value::as_str).map(str::to_owned);
    let path = s("file_path").or(s("path")).or_else(|| u.pointer("/locations/0/path").and_then(Value::as_str).map(str::to_owned)).map(|p| rel(&p, cwd));
    let strip = |verb: &str| title.strip_prefix(verb).map(|t| t.trim().to_owned()).unwrap_or_else(|| title.to_owned());
    let (n, d) = match name {
        "Read" => ("read", path.unwrap_or_else(|| strip("Read"))),
        "Edit" | "MultiEdit" => ("edit", path.unwrap_or_else(|| strip("Edit"))),
        "Write" => ("write", path.unwrap_or_else(|| strip("Write"))),
        "Bash" => ("bash", s("command").unwrap_or_else(|| if title == "Terminal" { String::new() } else { title.to_owned() })),
        "Grep" => ("grep", strip("grep")),
        "Glob" => ("glob", s("pattern").unwrap_or_else(|| title.to_owned())),
        "WebFetch" => ("fetch", s("url").unwrap_or_else(|| title.to_owned())),
        "WebSearch" => ("search", s("query").unwrap_or_else(|| title.to_owned())),
        "TodoWrite" => ("todo", title.to_owned()),
        _ => match kind {
            // pi-acp: bash's title is the command; other tools are bare names.
            "execute" => ("bash", if title.is_empty() { s("command").unwrap_or_default() } else { title.to_owned() }),
            "read" => ("read", path.unwrap_or_else(|| title.to_owned())),
            "edit" => (if title == "write" { "write" } else { "edit" }, path.unwrap_or_else(|| title.to_owned())),
            "delete" => ("delete", path.unwrap_or_else(|| title.to_owned())),
            "move" => ("move", path.unwrap_or_else(|| title.to_owned())),
            "search" => ("search", title.to_owned()),
            "fetch" => ("fetch", title.to_owned()),
            _ => {
                let n = if name.is_empty() { title.split_whitespace().next().unwrap_or("tool").to_lowercase() } else { name.to_lowercase() };
                let d = if title.to_lowercase() == n { String::new() } else { title.to_owned() };
                return (n, d);
            }
        },
    };
    (n.to_owned(), d)
}

fn tool_update(app: &App, session: &str, turn: &mut Turn, u: &Value, ts: &str) {
    let Some(call) = u.get("toolCallId").and_then(Value::as_str) else { return };
    let store = &app.agents.store;
    let cwd = store.lock().sessions.get(session).map(|s| s.cwd.clone()).unwrap_or_default();
    let is_subagent = meta(u, "/claudeCode/subagent").and_then(Value::as_bool) == Some(true) || turn.subagents.contains_key(call);
    if is_subagent {
        return subagent_update(app, session, turn, call, u, ts);
    }
    let (key, child) = target(turn, session, u);
    let status = u.get("status").and_then(Value::as_str);
    let mut i = store.lock();
    // Text streamed before a tool call is done.
    for k in [Kind::Output, Kind::Thinking] {
        if let Some(id) = turn.open.remove(&open_key(&key, k)) {
            i.update_row(&key, id, |r| r.open = false);
        }
    }
    let row_id = match turn.tools.get(call) {
        Some((_, id, _)) => *id,
        None => {
            let (name, detail) = tool_label(u, &cwd);
            let row = Row {
                at: ts.into(),
                kind: Kind::Tool,
                tool: Some(Tool { name, detail, status: status.unwrap_or("pending").into(), ..Default::default() }),
                open: true,
                ext: call.into(),
                ..Default::default()
            };
            let id = i.push_row(&key, row);
            turn.tools.insert(call.to_owned(), (key.clone(), id, now_ms()));
            id
        }
    };
    let started = turn.tools.get(call).map(|t| t.2).unwrap_or_else(now_ms);
    let has_label = u.get("title").is_some() || u.get("rawInput").is_some_and(|v| v.as_object().is_some_and(|o| !o.is_empty()));
    let (name, detail) = tool_label(u, &cwd);
    let content = u.get("content").and_then(Value::as_array).cloned();
    let raw_out = u.get("rawOutput").and_then(Value::as_str).map(str::to_owned);
    let term_out = meta(u, "/terminal_output/data").and_then(Value::as_str).map(str::to_owned);
    let exit = meta(u, "/terminal_exit/exit_code").and_then(Value::as_i64);
    let done = matches!(status, Some("completed") | Some("failed"));
    i.update_row(&key, row_id, |r| {
        let t = r.tool.get_or_insert_with(Tool::default);
        if has_label {
            if !name.is_empty() {
                t.name = name;
            }
            if !detail.is_empty() {
                t.detail = detail;
            }
        }
        if let Some(s) = status {
            t.status = s.into();
        }
        if let Some(out) = term_out {
            t.result.push_str(&out);
        }
        if let Some(blocks) = &content {
            let mut text = vec![];
            let (mut add, mut del, mut diffs) = (0, 0, false);
            for b in blocks {
                match b.get("type").and_then(Value::as_str) {
                    Some("content") => {
                        if let Some(s) = b.pointer("/content/text").and_then(Value::as_str) {
                            text.push(strip_fence(s));
                        }
                    }
                    Some("diff") => {
                        diffs = true;
                        let (a, d) = diff_stat(b.get("oldText").and_then(Value::as_str).unwrap_or(""), b.get("newText").and_then(Value::as_str).unwrap_or(""));
                        add += a;
                        del += d;
                    }
                    _ => {}
                }
            }
            if diffs {
                t.added = Some(add);
                t.removed = Some(del);
            }
            // Only a finished call's content is its result (before, it's the description).
            if done && !text.is_empty() {
                t.result = text.join("\n");
            }
        }
        if done {
            r.open = false;
            t.ms = Some(now_ms().saturating_sub(started));
            if t.result.is_empty() {
                if let Some(o) = &raw_out {
                    t.result = strip_fence(o);
                }
            }
            if t.name == "read" && status == Some("completed") {
                let n = t.result.lines().count();
                t.result = format!("{n} lines");
            }
            if matches!(t.name.as_str(), "edit" | "write") && status == Some("completed") && t.added.is_some() {
                t.result = "applied".into();
            }
            if let Some(code) = exit {
                if code != 0 {
                    t.status = "failed".into();
                }
            }
        }
    });
    drop(i);
    // A subagent's inner call: count it in the folded summary.
    if let Some(child) = child {
        if u.get("sessionUpdate").and_then(Value::as_str) == Some("tool_call") {
            refresh_subagent_summary(app, turn, &child);
        }
    }
}

fn subagent_update(app: &App, session: &str, turn: &mut Turn, call: &str, u: &Value, ts: &str) {
    let store = &app.agents.store;
    let key = session_thread(session);
    let input = u.get("rawInput").cloned().unwrap_or(Value::Null);
    let s = |k: &str| input.get(k).and_then(Value::as_str).map(str::to_owned);
    let status = u.get("status").and_then(Value::as_str);
    let child = match turn.subagents.get(call) {
        Some(c) => c.clone(),
        None => {
            let parent = store.lock().sessions.get(session).cloned().unwrap_or_default();
            let child = format!("{session}~{}", call.trim_start_matches("toolu_").chars().take(8).collect::<String>());
            let group = parent.group_id.clone().unwrap_or_else(|| "unsorted".into());
            let color = super::store::color_for(&app.db(), &group, &child, app.settings().agents.names.palette.len()).unwrap_or(0);
            let mut i = store.lock();
            i.put_session(Session {
                id: child.clone(),
                source: "acp".into(),
                group_id: parent.group_id.clone(),
                parent_id: Some(session.to_owned()),
                name: "subagent".into(),
                template: parent.template.clone(),
                harness: parent.harness.clone(),
                model: parent.model.clone(),
                host: parent.host.clone(),
                cwd: parent.cwd.clone(),
                status: Status::Running,
                context: format!("subagent of {}", parent.name),
                created_at: ts.into(),
                active_at: ts.into(),
                color,
                live: true,
                ..Default::default()
            });
            let row = Row {
                at: ts.into(),
                kind: Kind::Message,
                to: Some(Who::agent("subagent", Some(child.clone()))),
                label: "task".into(),
                note: format!("subagent · {}", parent.model),
                children: vec![
                    Row { kind: Kind::Tool, tool: Some(Tool { name: "subagent".into(), summary: "starting".into(), ..Default::default() }), ext: "summary".into(), ..Default::default() },
                ],
                ext: call.into(),
                open: true,
                ..Default::default()
            };
            let id = i.push_row(&key, row);
            turn.tools.insert(call.to_owned(), (key.clone(), id, now_ms()));
            turn.subagents.insert(call.to_owned(), child.clone());
            child
        }
    };
    let name = s("subagent_type").filter(|t| t != "general-purpose").map(|t| t.to_lowercase()).or_else(|| s("description").map(|d| slug(&d)));
    let task = s("prompt").or(s("description"));
    let Some((_, row_id, started)) = turn.tools.get(call).cloned() else { return };
    let done = matches!(status, Some("completed") | Some("failed"));
    let result = u
        .get("content")
        .and_then(Value::as_array)
        .map(|b| b.iter().filter_map(|b| b.pointer("/content/text").and_then(Value::as_str)).map(strip_fence).collect::<Vec<_>>().join("\n"))
        .filter(|t| !t.is_empty());
    let total_ms = meta(u, "/claudeCode/toolResponse/totalDurationMs").and_then(Value::as_u64);
    let mut i = store.lock();
    if let Some(n) = &name {
        i.update_session(&child, |c| c.name = n.clone());
    }
    if let Some(t) = &task {
        i.update_session(&child, |c| {
            if c.title.is_empty() || done {
                c.title = crate::ai::context::clip(t.lines().next().unwrap_or(""), 90);
            }
        });
    }
    let child_name = i.sessions.get(&child).map(|c| c.name.clone()).unwrap_or_default();
    i.update_row(&key, row_id, |r| {
        r.to = Some(Who::agent(&child_name, Some(child.clone())));
        if let Some(t) = &task {
            r.text = t.clone();
        }
        if let Some(summary) = r.children.iter_mut().find(|c| c.ext == "summary") {
            if let Some(t) = summary.tool.as_mut() {
                t.name = child_name.clone();
            }
        }
        if done {
            r.open = false;
            let ms = total_ms.unwrap_or(now_ms().saturating_sub(started));
            if let Some(summary) = r.children.iter_mut().find(|c| c.ext == "summary").and_then(|c| c.tool.as_mut()) {
                summary.ms = Some(ms);
                summary.status = status.unwrap_or("completed").into();
            }
            if let Some(res) = &result {
                r.children.retain(|c| c.ext != "result");
                r.children.push(Row {
                    at: ts.into(),
                    kind: Kind::Message,
                    who: Some(Who::agent(&child_name, Some(child.clone()))),
                    label: "result".into(),
                    text: res.clone(),
                    ext: "result".into(),
                    ..Default::default()
                });
            }
        }
    });
    if done {
        i.update_session(&child, |c| {
            c.status = if status == Some("failed") { Status::Failed } else { Status::Done };
            c.live = false;
            c.active_at = ts.into();
        });
        if let Some(res) = result {
            let k = session_thread(&child);
            i.push_row(&k, Row { at: ts.into(), kind: Kind::Output, label: "output".into(), text: res, ..Default::default() });
        }
    }
    drop(i);
    if done {
        refresh_subagent_summary(app, turn, &child);
    }
}

fn slug(s: &str) -> String {
    let words: Vec<String> = s.split_whitespace().take(3).map(|w| w.chars().filter(|c| c.is_alphanumeric()).collect::<String>().to_lowercase()).filter(|w| !w.is_empty()).collect();
    if words.is_empty() {
        "subagent".into()
    } else {
        words.join("-")
    }
}

/// "5 tool calls · read ×3, grep ×2 · 4.8s" from the subagent's own thread.
fn refresh_subagent_summary(app: &App, turn: &Turn, child: &str) {
    let Some(call) = turn.subagents.iter().find(|(_, c)| c.as_str() == child).map(|(k, _)| k.clone()) else { return };
    let Some((key, row_id, _)) = turn.tools.get(&call).cloned() else { return };
    let mut i = app.agents.store.lock();
    let mut counts: Vec<(String, usize)> = vec![];
    let mut n = 0;
    if let Some(t) = i.threads.get(&session_thread(child)) {
        for r in t.rows.iter().filter(|r| r.kind == Kind::Tool) {
            n += 1;
            let name = r.tool.as_ref().map(|t| t.name.clone()).unwrap_or_default();
            match counts.iter_mut().find(|(k, _)| *k == name) {
                Some((_, c)) => *c += 1,
                None => counts.push((name, 1)),
            }
        }
    }
    let parts = counts.iter().map(|(k, c)| if *c > 1 { format!("{k} ×{c}") } else { k.clone() }).collect::<Vec<_>>().join(", ");
    i.update_row(&key, row_id, |r| {
        if let Some(t) = r.children.iter_mut().find(|c| c.ext == "summary").and_then(|c| c.tool.as_mut()) {
            let secs = t.ms.map(|ms| format!(" · {:.1}s", ms as f64 / 1000.0)).unwrap_or_default();
            let calls = if n == 1 { "1 tool call".to_string() } else { format!("{n} tool calls") };
            t.summary = if n == 0 { format!("no tool calls{secs}") } else { format!("{calls} · {parts}{secs}") };
        }
    });
}

/// `session/request_permission`: pin an approval with the agent's own options.
fn permission(app: &Arc<App>, conn: &Arc<Conn>, rpc_id: Value, p: &Value) {
    let session = conn.session.clone();
    let tc = p.get("toolCall").cloned().unwrap_or(Value::Null);
    let call = tc.get("toolCallId").and_then(Value::as_str).unwrap_or("").to_owned();
    let ts = now(app);
    let s = app.agents.store.lock().sessions.get(&session).cloned().unwrap_or_default();
    let (name, detail) = tool_label(&tc, &s.cwd);
    let kind = tc.get("kind").and_then(Value::as_str).unwrap_or("");
    let command = tc.pointer("/rawInput/command").and_then(Value::as_str).map(str::to_owned).unwrap_or_else(|| edit_preview(&tc).unwrap_or_else(|| detail.clone()));
    let req_id = format!("{session}:{}", rpc_id);
    let options: Vec<PermOption> = p
        .get("options")
        .and_then(Value::as_array)
        .map(|opts| {
            opts.iter()
                .enumerate()
                .map(|(i, o)| {
                    let kind = o.get("kind").and_then(Value::as_str).unwrap_or("").to_owned();
                    let name = o.get("name").and_then(Value::as_str).unwrap_or("").to_owned();
                    PermOption { id: o.get("optionId").and_then(Value::as_str).unwrap_or("").into(), name, kind, key: (i + 1).to_string() }
                })
                .collect()
        })
        .unwrap_or_default();
    let parsed = (kind == "execute" || name == "bash").then(|| super::shell::parse(&command, &s.cwd, &local_bins(&s.cwd)));
    let outside = |p: &str| p.starts_with('/') && !p.starts_with(&s.cwd) || p.starts_with('~');
    let tag = match &parsed {
        Some(p) => match p.verdict.as_str() {
            "read-only" => "low",
            "writes" => "writes",
            "network" => "network",
            _ => "unknown",
        }
        .to_owned(),
        None if outside(&detail) => "outside repo".into(),
        None if matches!(kind, "edit" | "delete" | "move") => "writes".into(),
        None if kind == "read" || kind == "search" => "low".into(),
        None => kind.to_owned(),
    };
    let title = p.pointer("/_meta/permission/title").and_then(Value::as_str).map(str::to_owned);
    let plain = match kind {
        "execute" => "Run a shell command".to_owned(),
        "edit" => format!("Edit {detail}"),
        "read" => format!("Read {detail}"),
        "delete" => format!("Delete {detail}"),
        "fetch" => format!("Fetch {detail}"),
        "switch_mode" => title.clone().unwrap_or_else(|| "Switch mode".into()),
        _ => title.clone().unwrap_or_else(|| format!("Use {name}")),
    };
    let req = Request {
        id: req_id.clone(),
        session_id: session.clone(),
        thread: session_thread(&session),
        kind: "approval".into(),
        at: ts.clone(),
        title: plain,
        tool: format!("{name} · {}", tilde(&s.cwd)),
        cwd: s.cwd.clone(),
        command: command.clone(),
        options,
        parsed,
        explained: None,
        tag,
        ..Default::default()
    };
    let key = {
        let turn = conn.turn.lock().unwrap_or_else(|e| e.into_inner());
        target(&turn, &session, p.get("toolCall").unwrap_or(&Value::Null)).0
    };
    let _ = target;
    {
        let mut turn = conn.turn.lock().unwrap_or_else(|e| e.into_inner());
        turn.waiting.insert(req_id.clone(), Waiting { rpc: rpc_id, params: p.clone() });
        let mut i = app.agents.store.lock();
        let existing = turn.tools.get(&call).map(|(_, id, _)| *id);
        let row_id = match existing {
            Some(id) => id,
            None => {
                let id = i.push_row(&key, Row { at: ts.clone(), kind: Kind::Tool, tool: Some(Tool { name: name.clone(), detail: detail.clone(), status: "pending".into(), ..Default::default() }), ext: call.clone(), open: true, ..Default::default() });
                if !call.is_empty() {
                    turn.tools.insert(call.clone(), (key.clone(), id, now_ms()));
                }
                id
            }
        };
        i.update_row(&key, row_id, |r| {
            r.kind = Kind::Approval;
            r.label = "needs approval".into();
            r.request = Some(req_id.clone());
            r.text = if kind == "execute" || name == "bash" { command.clone() } else { detail.clone() };
        });
        i.put_request(req);
        i.update_session(&session, |s| s.status = Status::Waiting);
    }
    super::explain::request(app, &req_id);
}

/// An edit's diff, shortened, for the approval card: the path, then -/+ lines.
fn edit_preview(tc: &Value) -> Option<String> {
    let d = tc.get("content")?.as_array()?.iter().find(|b| b.get("type").and_then(Value::as_str) == Some("diff"))?;
    let old = d.get("oldText").and_then(Value::as_str).unwrap_or("");
    let new = d.get("newText").and_then(Value::as_str).unwrap_or("");
    let a: Vec<&str> = old.lines().collect();
    let b: Vec<&str> = new.lines().collect();
    let mut out = vec![];
    for l in &a {
        if !b.contains(l) {
            out.push(format!("- {l}"));
        }
    }
    for l in &b {
        if !a.contains(l) {
            out.push(format!("+ {l}"));
        }
    }
    if out.is_empty() {
        return None;
    }
    let more = out.len().saturating_sub(30);
    out.truncate(30);
    if more > 0 {
        out.push(format!("… {more} more lines"));
    }
    Some(out.join("\n"))
}

fn local_bins(cwd: &str) -> Vec<String> {
    std::fs::read_dir(std::path::Path::new(cwd).join("node_modules/.bin"))
        .map(|d| d.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).collect())
        .unwrap_or_default()
}

/// Answer an approval with one of the agent's options. A reason on a deny is
/// sent as your next message.
pub fn respond(app: &App, req_id: &str, option: &str, reason: Option<&str>) -> Result<()> {
    let req = app.agents.store.lock().requests.get(req_id).cloned().ok_or_else(|| msg("that request is gone"))?;
    let conn = app.agents.acp.get(&req.session_id).ok_or_else(|| msg("the agent isn't running anymore"))?;
    let opt = req.options.iter().find(|o| o.id == option || (!o.key.is_empty() && o.key == option)).cloned().ok_or_else(|| msg("not one of the agent's options"))?;
    let rpc_id = {
        let mut t = conn.turn.lock().unwrap_or_else(|e| e.into_inner());
        // The tool's time starts when it's allowed, not while it waited on you.
        let call = app.agents.store.lock().find_row(&req.thread, |r| r.request.as_deref() == Some(req_id)).map(|r| r.ext.clone());
        if let Some(entry) = call.and_then(|c| t.tools.get_mut(&c)) {
            entry.2 = now_ms();
        }
        t.waiting.remove(req_id).ok_or_else(|| msg("already answered"))?.rpc
    };
    conn.respond(&rpc_id, json!({ "outcome": { "outcome": "selected", "optionId": opt.id } }))?;
    let ts = now(app);
    let hm = ts.get(11..16).unwrap_or("").to_owned();
    let verdict = match opt.kind.as_str() {
        "allow_once" => format!("allowed once at {hm}"),
        "allow_always" => format!("{} at {hm}", opt.name.to_lowercase()),
        _ => format!("denied at {hm}"),
    };
    {
        let mut i = app.agents.store.lock();
        i.remove_request(req_id);
        let key = req.thread.clone();
        if let Some(id) = i.find_row(&key, |r| r.request.as_deref() == Some(req_id)).map(|r| r.id) {
            i.update_row(&key, id, |r| {
                r.kind = Kind::Tool;
                r.label.clear();
                r.request = None;
                r.note = verdict.clone();
                if opt.kind.starts_with("reject") {
                    if let Some(t) = r.tool.as_mut() {
                        t.status = "failed".into();
                    }
                }
            });
        }
        let still_waiting = i.requests.values().any(|r| r.session_id == req.session_id);
        i.update_session(&req.session_id, |s| s.status = if still_waiting { Status::Waiting } else { Status::Running });
    }
    if let Some(r) = reason.filter(|r| !r.trim().is_empty()) {
        send(app, &req.session_id, r, vec![])?;
    }
    Ok(())
}

fn push_you(app: &App, id: &str, text: &str, mid_turn: bool, images: Vec<RowImage>) -> u64 {
    let ts = now(app);
    let mut i = app.agents.store.lock();
    let first = i.find_row(&session_thread(id), |r| r.kind == Kind::You).is_none();
    let name = i.sessions.get(id).map(|s| s.name.clone()).unwrap_or_default();
    let delivery = if mid_turn && !first {
        Delivery { state: "deferred".into(), at: ts.clone(), reason: format!("{name} was mid-turn"), ..Default::default() }
    } else {
        Delivery { state: "pending".into(), at: ts.clone(), ..Default::default() }
    };
    let row = Row { at: ts, kind: Kind::You, who: Some(Who::you()), label: if first { "prompt".into() } else { "message".into() }, text: text.into(), images, delivery: Some(delivery), ..Default::default() };
    i.push_row(&session_thread(id), row)
}

/// Send `session/prompt`. Its answer may come in a later run of the app, so
/// it's tracked in the turn (`prompts`) and finished in `finish_prompt`.
fn prompt(app: &Arc<App>, conn: &Arc<Conn>, row: u64, text: String, images: Vec<RowImage>) {
    let id = conn.session.clone();
    let key = session_thread(&id);
    let rpc = conn.next.fetch_add(1, Ordering::SeqCst);
    // The prompt is tracked before anything else is stored, so a crash can't keep one without the other.
    let gate = app.agents.store.gate();
    let first = {
        let mut t = conn.turn.lock().unwrap_or_else(|e| e.into_inner());
        t.prompts.insert(rpc, row);
        t.prompts.len() == 1
    };
    {
        let ts = now(app);
        let mut i = app.agents.store.lock();
        if first {
            i.update_row(&key, row, |r| {
                if let Some(d) = r.delivery.as_mut() {
                    d.state = "delivered".into();
                    d.delivered_at = ts.clone();
                    d.count = 1;
                }
            });
        } else {
            i.update_session(&id, |s| s.queued.push(row));
        }
        i.update_session(&id, |s| {
            s.status = Status::Running;
            s.active_at = ts;
        });
    }
    save_turn(app, conn, None);
    drop(gate);
    let sid = conn.sid();
    let mut blocks = vec![json!({ "type": "text", "text": text })];
    blocks.extend(images.iter().map(|im| json!({ "type": "image", "data": im.data, "mimeType": im.mime })));
    let sent = conn.write(&json!({ "jsonrpc": "2.0", "id": rpc, "method": "session/prompt", "params": { "sessionId": sid, "prompt": blocks } }));
    if let Err(e) = sent {
        finish_prompt(app, conn, rpc, Err(json!({ "message": e.to_string() })), &now(app));
    }
}

/// A prompt's answer: the turn is over (or failed).
fn finish_prompt(app: &App, conn: &Conn, rpc: u64, res: Reply, ts: &str) {
    let id = conn.session.clone();
    let key = session_thread(&id);
    let mut t = conn.turn.lock().unwrap_or_else(|e| e.into_inner());
    let Some(row) = t.prompts.remove(&rpc) else { return };
    let idle = t.prompts.is_empty();
    let mut i = app.agents.store.lock();
    for (k, rid) in t.open.drain() {
        let thread = k.split('\u{1f}').next().unwrap_or("").to_owned();
        i.update_row(&thread, rid, |r| r.open = false);
    }
    // The next queued message is the one the agent takes now.
    let next = i.sessions.get(&id).and_then(|s| s.queued.first().copied());
    if let Some(n) = next {
        i.update_row(&key, n, |r| {
            if let Some(d) = r.delivery.as_mut() {
                d.state = "delivered".into();
                d.delivered_at = ts.to_owned();
                d.count = 1;
            }
        });
    }
    let waiting = i.requests.values().any(|r| r.session_id == id);
    let mut problem = None;
    match &res {
        Ok(v) => {
            let stop = v.get("stopReason").and_then(Value::as_str).unwrap_or("end_turn");
            if stop == "cancelled" {
                i.push_row(&key, state_row(ts, "interrupted"));
            } else if stop != "end_turn" {
                i.push_row(&key, state_row(ts, &format!("turn ended · {}", stop.replace('_', " "))));
            }
        }
        Err(e) => {
            let e = rpc_error(e);
            let auth = e.contains("uthentication") || e.contains("login") || e.contains("log in");
            let text = if auth { format!("{e} · sign in to the harness in a terminal, then send again") } else { e.clone() };
            i.push_row(&key, Row { at: ts.into(), kind: Kind::Failed, label: "prompt failed".into(), text, ..Default::default() });
            i.update_row(&key, row, |r| {
                if let Some(d) = r.delivery.as_mut() {
                    if d.state != "delivered" {
                        d.state = "failed".into();
                        d.reason = e.clone();
                    }
                }
            });
            problem = Some(e);
        }
    }
    i.update_session(&id, |s| {
        if next.is_some() {
            s.queued.remove(0);
        }
        s.stats.turns += 1;
        s.active_at = ts.to_owned();
        if let Some(p) = &problem {
            s.problem = p.clone();
        }
        if idle && !waiting {
            s.status = if problem.is_some() { Status::Failed } else { Status::Idle };
        }
    });
    drop(i);
    drop(t);
    if idle {
        super::facts::refresh(app, &id);
    }
}

/// Message a local session. One from an earlier run is started again first.
pub fn send(app: &App, id: &str, text: &str, images: Vec<RowImage>) -> Result<()> {
    if id.contains('~') {
        return Err(msg("Subagents take messages through their parent"));
    }
    let arc = app.arc().ok_or_else(|| msg("not ready"))?;
    if let Some(conn) = app.agents.acp.get(id) {
        let mid = !conn.turn.lock().unwrap_or_else(|e| e.into_inner()).prompts.is_empty();
        let row = push_you(app, id, text, mid, images.clone());
        prompt(&arc, &conn, row, text.to_owned(), images);
        return Ok(());
    }
    let s = app.agents.store.lock().sessions.get(id).cloned().ok_or_else(|| msg("no such session"))?;
    if !app.agents.acp.starting.lock().unwrap_or_else(|e| e.into_inner()).insert(id.to_owned()) {
        return Err(msg("Still starting; send again in a moment"));
    }
    let row = push_you(app, id, text, true, images.clone());
    let ts = now(app);
    {
        let mut i = app.agents.store.lock();
        i.push_row(&session_thread(id), state_row(&ts, &format!("reattaching · {}", s.harness)));
        i.update_session(id, |s| {
            s.status = Status::Running;
            s.problem.clear();
        });
    }
    let (id, text) = (id.to_owned(), text.to_owned());
    std::thread::spawn(move || {
        let resume = (!s.ext.is_empty()).then_some(s.ext.as_str());
        let r = connect(&arc, &id, resume);
        arc.agents.acp.starting.lock().unwrap_or_else(|e| e.into_inner()).remove(&id);
        match r {
            Ok(conn) => prompt(&arc, &conn, row, text, images),
            Err(e) => {
                fail(&arc, &id, &e.to_string());
                let key = session_thread(&id);
                arc.agents.store.lock().update_row(&key, row, |r| {
                    if let Some(d) = r.delivery.as_mut() {
                        d.state = "unreachable".into();
                        d.reason = e.to_string();
                    }
                });
            }
        }
    });
    Ok(())
}

/// ⌘.: cancel the turn (and any queued ones); open approvals are cancelled too.
pub fn interrupt(app: &App, id: &str) -> Result<()> {
    let id = id.split('~').next().unwrap_or(id);
    let conn = app.agents.acp.get(id).ok_or_else(|| msg("It isn't running"))?;
    let waiting: Vec<(String, Value)> = conn.turn.lock().unwrap_or_else(|e| e.into_inner()).waiting.drain().map(|(k, w)| (k, w.rpc)).collect();
    for (req, rpc) in waiting {
        let _ = conn.respond(&rpc, json!({ "outcome": { "outcome": "cancelled" } }));
        app.agents.store.lock().remove_request(&req);
    }
    conn.notify("session/cancel", json!({ "sessionId": conn.sid() }))
}

/// Stop the agent (its process in the host). The transcript stays; sending starts it again.
pub fn stop(app: &App, id: &str) {
    if let Some(c) = app.agents.acp.any(id) {
        c.stopping.store(true, Ordering::SeqCst);
        c.kill();
    }
    let mut i = app.agents.store.lock();
    let reqs: Vec<String> = i.requests.values().filter(|r| r.session_id == id).map(|r| r.id.clone()).collect();
    for r in reqs {
        i.remove_request(&r);
    }
    i.update_session(id, |s| {
        s.live = false;
        if s.status != Status::Failed {
            s.status = Status::Idle;
        }
    });
}

/// Stop every local agent.
pub fn stop_all(app: &App) -> usize {
    let live: Vec<String> = app.agents.acp.all().into_iter().filter(|c| c.alive.load(Ordering::SeqCst)).map(|c| c.session.clone()).collect();
    for id in &live {
        stop(app, id);
    }
    live.len()
}

/// Open in Zed: the session's directory.
pub fn open_in_editor(app: &App, cwd: &str) -> Result<()> {
    app.platform.open_in_app("Zed", cwd).map_err(|e| msg(format!("couldn't open Zed: {e}")))
}

/// Directories of earlier sessions, newest first, for the new-session form.
pub fn recent_dirs(app: &App) -> Vec<String> {
    let i = app.agents.store.lock();
    let mut s: Vec<&Session> = i.sessions.values().filter(|s| s.source == "acp" && s.parent_id.is_none()).collect();
    s.sort_by(|a, b| b.active_at.cmp(&a.active_at));
    let mut out: Vec<String> = vec![];
    for x in s {
        let d = tilde(&x.cwd);
        if !d.is_empty() && !out.contains(&d) {
            out.push(d);
        }
    }
    out
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::ai::context::tests::app;

    /// Replay a recorded claude-agent-acp scenario through `apply`.
    pub(crate) fn replay(app: &App, session: &str, file: &str) -> Turn {
        let text = std::fs::read_to_string(format!("{}/tests/fixtures/acp/{file}", env!("CARGO_MANIFEST_DIR"))).unwrap();
        let mut turn = Turn::default();
        for line in text.lines() {
            let v: Value = serde_json::from_str(line).unwrap();
            if v["kind"] == "sessionUpdate" {
                apply(app, session, &mut turn, &v["payload"]["update"], "2026-10-03T10:41:00");
            }
        }
        turn
    }

    fn session(app: &App) -> String {
        let id = "acp-0000test".to_string();
        app.agents.store.lock().put_session(Session { id: id.clone(), source: "acp".into(), name: "header-fix".into(), cwd: "<cwd>".into(), ..Default::default() });
        id
    }

    fn rows(app: &App, key: &str) -> Vec<Row> {
        app.agents.store.lock().threads.get(key).map(|t| t.rows.clone()).unwrap_or_default()
    }

    #[test]
    fn bash_call_becomes_one_tool_row_with_its_output() {
        let (app, _d) = app();
        let id = session(&app);
        replay(&app, &id, "claude-bash-foreground.jsonl");
        let r = rows(&app, &session_thread(&id));
        let tool = r.iter().find(|r| r.kind == Kind::Tool).unwrap();
        let t = tool.tool.as_ref().unwrap();
        assert_eq!((t.name.as_str(), t.detail.as_str(), t.status.as_str()), ("bash", "ls -la", "completed"));
        assert_eq!(t.result, "a.ts\nb.ts");
        let out = r.iter().find(|r| r.kind == Kind::Output).unwrap();
        assert_eq!(out.text, "Done.", "chunks join into one output row");
        let s = &app.agents.store.lock().sessions[&id];
        assert_eq!((s.model.as_str(), s.stats.ctx_size), ("sonnet", 200000));
    }

    #[test]
    fn read_shows_a_line_count_and_edits_count_lines() {
        let (app, _d) = app();
        let id = session(&app);
        replay(&app, &id, "claude-read.jsonl");
        replay(&app, &id, "claude-edit-with-permission.jsonl");
        let r = rows(&app, &session_thread(&id));
        let tools: Vec<&Tool> = r.iter().filter_map(|r| r.tool.as_ref()).collect();
        let read = tools.iter().find(|t| t.name == "read").unwrap();
        assert!(read.result.ends_with(" lines"), "{}", read.result);
        let edit = tools.iter().find(|t| t.name == "edit").unwrap();
        assert_eq!(edit.detail, "src/app.ts", "paths are relative to the session's directory");
        assert_eq!((edit.added, edit.removed), (Some(1), Some(1)));
        assert_eq!(edit.result, "applied");
    }

    #[test]
    fn thinking_and_text_are_separate_rows() {
        let (app, _d) = app();
        let id = session(&app);
        replay(&app, &id, "claude-text-and-thinking.jsonl");
        let r = rows(&app, &session_thread(&id));
        let kinds: Vec<Kind> = r.iter().map(|r| r.kind).collect();
        assert!(kinds.contains(&Kind::Thinking) && kinds.contains(&Kind::Output), "{kinds:?}");
        let th = r.iter().find(|r| r.kind == Kind::Thinking).unwrap();
        assert!(th.tokens.unwrap_or(0) > 0);
    }

    #[test]
    fn a_subagent_is_a_message_box_and_its_own_session() {
        let (app, _d) = app();
        let id = session(&app);
        replay(&app, &id, "claude-subagent-task-legacy.jsonl");
        let r = rows(&app, &session_thread(&id));
        let task = r.iter().find(|r| r.kind == Kind::Message && r.label == "task").unwrap();
        assert_eq!(task.text, "Find the parser");
        let child = task.to.as_ref().unwrap().session.clone().unwrap();
        let summary = task.children.iter().find(|c| c.ext == "summary").unwrap().tool.clone().unwrap();
        assert!(summary.summary.contains("tool call"), "{}", summary.summary);
        assert!(task.children.iter().any(|c| c.label == "result"), "the result is the box's last row");
        let i = app.agents.store.lock();
        let c = &i.sessions[&child];
        assert_eq!(c.parent_id.as_deref(), Some(id.as_str()));
        assert_eq!(c.status, Status::Done);
        assert!(i.threads[&session_thread(&child)].rows.iter().any(|r| r.kind == Kind::Tool), "inner calls go to the subagent's thread");
    }

    #[test]
    fn tool_labels_for_both_adapters() {
        let pi_bash = json!({ "kind": "execute", "title": "npm run check" });
        assert_eq!(tool_label(&pi_bash, "/r"), ("bash".into(), "npm run check".into()));
        let pi_read = json!({ "kind": "read", "title": "read", "locations": [{ "path": "/r/src/a.ts" }] });
        assert_eq!(tool_label(&pi_read, "/r"), ("read".into(), "src/a.ts".into()));
        let grep = json!({ "_meta": { "claudeCode": { "toolName": "Grep" } }, "title": "grep -n \"x\" src" });
        assert_eq!(tool_label(&grep, "/r"), ("grep".into(), "-n \"x\" src".into()));
    }

    #[test]
    fn diff_stats() {
        assert_eq!(diff_stat("a\nb\nc", "a\nB\nc\nd"), (2, 1));
        assert_eq!(diff_stat("", "x\ny"), (2, 0));
    }

    #[test]
    fn titles_from_the_first_prompt() {
        let (t, s) = title_from("Fix the Settings header overlap. Title and close button collide under 720px.");
        assert_eq!(t, "Fix the Settings header overlap.");
        assert_eq!(s, "Title and close button collide under 720px.");
    }

    #[test]
    fn accept_edits_mode_only_when_offered() {
        let claude = json!({ "modes": { "currentModeId": "default", "availableModes": [{ "id": "default" }, { "id": "acceptEdits" }, { "id": "plan" }] } });
        assert_eq!(accept_edits_mode(&claude).as_deref(), Some("acceptEdits"));
        let already = json!({ "modes": { "currentModeId": "acceptEdits", "availableModes": [{ "id": "acceptEdits" }] } });
        assert_eq!(accept_edits_mode(&already), None);
        let pi = json!({ "modes": { "currentModeId": "medium", "availableModes": [{ "id": "low" }, { "id": "medium" }] } });
        assert_eq!(accept_edits_mode(&pi), None, "pi's modes are thinking levels");
    }

    #[test]
    fn model_from_session_new() {
        let claude = json!({ "configOptions": [{ "id": "model", "currentValue": "claude-sonnet-4-6" }] });
        assert_eq!(model_of(&claude).as_deref(), Some("sonnet"));
        let pi = json!({ "models": { "currentModelId": "anthropic/claude-haiku-4-5" } });
        assert_eq!(model_of(&pi).as_deref(), Some("haiku"));
    }
}

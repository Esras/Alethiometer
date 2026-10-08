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

use super::model::{Choice, Delivery, Kind, PermOption, QueuedMessage, Request, Row, RowImage, Session, Status, Tool, Who};
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

pub(super) fn now_ms_pub() -> u64 {
    now_ms()
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

/// An approval or question waiting on you: the agent's request id and what it asked.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct Waiting {
    rpc: Value,
    params: Value,
    /// `session/request_permission` (empty in turns stored before asks) or `elicitation/create`.
    #[serde(default)]
    method: String,
}

impl Waiting {
    fn is_ask(&self) -> bool {
        self.method == ELICIT
    }

    /// What settles it without an answer.
    fn cancelled(&self) -> Value {
        if self.is_ask() {
            json!({ "action": "cancel" })
        } else {
            json!({ "outcome": { "outcome": "cancelled" } })
        }
    }
}

const ELICIT: &str = "elicitation/create";

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
    /// What each open tool call has said about itself (kind, title, name,
    /// input): a permission request may repeat only what changed.
    #[serde(default)]
    calls: HashMap<String, Value>,
    /// Prompts sent and not yet answered: JSON-RPC id → your row.
    #[serde(default)]
    prompts: BTreeMap<u64, u64>,
    #[serde(default)]
    caps: Value,
    /// The reason you typed with a deny, and whether the option you picked asks for one ("No, provide reason"):
    /// then the agent's follow-up question gets it as its answer. Otherwise it goes out as your next message when the turn ends.
    /// Also the words typed with an ask's free choice ("Type something."), which always asks.
    #[serde(default)]
    deny_reason: Option<(String, bool)>,
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
        let run = |cmd: &str, args: &[&str]| {
            let out = Command::new(cmd).args(args).output().ok().filter(|o| o.status.success())?;
            Some(String::from_utf8_lossy(&out.stdout).trim().to_owned()).filter(|s| !s.is_empty())
        };
        let mac = if cfg!(target_os = "macos") { run("scutil", &["--get", "LocalHostName"]) } else { None };
        mac.or_else(|| run("hostname", &["-s"]))
            .or_else(|| std::env::var("HOSTNAME").ok())
            .unwrap_or_else(|| "local".into())
    })
    .clone()
}

/// The login shell's PATH: apps started from the Dock get a minimal one, and
/// npx (or the adapter) usually lives somewhere the shell's profile adds.
pub(super) fn login_path() -> String {
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

/// The project's direnv environment (its `.envrc`, a Nix devshell say), as Zed
/// loads one for the agents it starts: `direnv export json` in `cwd`, from the
/// app's own environment. The app may itself run inside a devshell (`tauri
/// dev`), perhaps an older one or another project's: direnv unloads that first
/// (its `DIRENV_DIFF`), so none of it leaks into PATH. Variables it unsets
/// aren't carried: the agent host only sets. No direnv, a blocked `.envrc` or
/// none: nothing. A devshell that's still building after two minutes is given
/// up on, and the agent starts without it.
fn direnv_env(cwd: &str, path: &str) -> BTreeMap<String, String> {
    let Some(direnv) = which("direnv", path).filter(|_| !cwd.is_empty()) else { return BTreeMap::new() };
    let mut c = Command::new(direnv);
    c.args(["export", "json"]).current_dir(cwd).env("PATH", path).stdin(Stdio::null()).stderr(Stdio::null());
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(c.output());
    });
    let Some(out) = rx.recv_timeout(Duration::from_secs(120)).ok().and_then(|o| o.ok()).filter(|o| o.status.success()) else {
        return BTreeMap::new();
    };
    direnv_vars(&out.stdout, std::env::var("DIRENV_WATCHES").ok())
}

/// `direnv export json`'s output as variables to set. Nothing to change (the
/// app already has this directory's environment, up to date) leaves out
/// `DIRENV_WATCHES`; the app's own is the one that applies.
fn direnv_vars(out: &[u8], own_watches: Option<String>) -> BTreeMap<String, String> {
    let vars: BTreeMap<String, Value> = serde_json::from_slice(out).unwrap_or_default();
    let mut env: BTreeMap<String, String> = vars.iter().filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_owned()))).collect();
    if !vars.contains_key("DIRENV_WATCHES") {
        if let Some(w) = own_watches {
            env.insert("DIRENV_WATCHES".into(), w);
        }
    }
    env
}

/// What direnv watches for an environment it exported: `DIRENV_WATCHES`, zlib-compressed JSON in base64url.
fn direnv_watches(env: &BTreeMap<String, String>) -> Vec<super::model::EnvWatch> {
    use base64::Engine;
    use std::io::Read;
    let Some(raw) = env.get("DIRENV_WATCHES") else { return vec![] };
    let Ok(zipped) = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(raw.trim_end_matches('=')) else { return vec![] };
    let mut json = String::new();
    if flate2::read::ZlibDecoder::new(zipped.as_slice()).read_to_string(&mut json).is_err() {
        return vec![];
    }
    serde_json::from_str(&json).unwrap_or_default()
}

/// One of the watched files changed (direnv's own test: existence, then mtime in seconds).
pub fn env_changed(watches: &[super::model::EnvWatch]) -> bool {
    watches.iter().any(|w| match std::fs::metadata(&w.path).and_then(|m| m.modified()) {
        Ok(t) => !w.exists || t.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0) != w.modtime,
        Err(_) => w.exists,
    })
}

/// Where `cmd` is on the login PATH, or None.
pub(super) fn which(cmd: &str, path: &str) -> Option<std::path::PathBuf> {
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
    pub model: Option<String>,
    pub thinking: Option<String>,
    /// Run it in an OpenShell sandbox instead of on this machine.
    pub sandbox: Option<SandboxStart>,
}

/// Where in OpenShell: a new sandbox made from the harness's settings with
/// `cwd` uploaded into it, or an existing one with `workdir` inside it.
pub struct SandboxStart {
    pub gateway: String,
    /// An existing sandbox; None makes one.
    pub existing: Option<String>,
    pub workdir: String,
    /// The new sandbox's image; empty is the harness's `openshell.from`.
    pub from: String,
}

pub(super) fn expand_home(p: &str) -> String {
    match (p.strip_prefix('~'), dirs::home_dir()) {
        (Some(rest), Some(h)) => format!("{}{rest}", h.display()),
        _ => p.to_owned(),
    }
}

pub(crate) fn tilde(p: &str) -> String {
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
    let existing = a.sandbox.as_ref().and_then(|b| b.existing.clone());
    if existing.is_none() && !std::path::Path::new(&cwd).is_dir() {
        return Err(msg(format!("{} isn't a directory", a.cwd)));
    }
    if existing.is_some() && !a.sandbox.as_ref().is_some_and(|b| b.workdir.trim().starts_with('/')) {
        return Err(msg("Give its directory inside the sandbox, from / (e.g. /sandbox/project)"));
    }
    if a.sandbox.is_some() && super::openshell::cli(&settings.agents.openshell.command).is_none() {
        return Err(msg(format!("{} isn't on your PATH (agents.openshell.command)", settings.agents.openshell.command)));
    }
    let id = format!("acp-{:08x}", rand_u32());
    // An existing sandbox runs in the directory given inside it; a new one, in the upload (known once it's made).
    let sandbox = a.sandbox.as_ref().map(|b| super::model::Sandbox {
        gateway: b.gateway.trim().to_owned(),
        name: existing.clone().unwrap_or_else(|| super::openshell::sandbox_name(&id)),
        upload: if existing.is_some() { String::new() } else { cwd.clone() },
        allow_all: false,
        synced_turns: 0,
    });
    let cwd = match (&existing, a.sandbox.as_ref()) {
        (Some(_), Some(b)) => b.workdir.trim().to_owned(),
        _ => cwd,
    };
    let place = match &sandbox {
        Some(b) => format!("openshell · {} · {}", if b.gateway.is_empty() { "active gateway" } else { &b.gateway }, b.name),
        None => format!("local · {}", tilde(&cwd)),
    };
    let name = a.name.clone().filter(|n| !n.trim().is_empty()).map(|n| n.trim().to_owned()).unwrap_or_else(|| format!("{}-{}", h.key, &id[4..8]));
    let group_key = a.group_id.clone().unwrap_or_else(|| "unsorted".into());
    let color = super::store::color_for(app.store(), &group_key, &id, settings.agents.names.palette.len())?;
    let (title, subtitle) = title_from(&a.task);
    let ts = now(app);
    let s = Session {
        id: id.clone(),
        source: "acp".into(),
        group_id: a.group_id.clone(),
        name,
        template: h.key.clone(),
        harness: h.label.clone(),
        host: sandbox.as_ref().map(|b| if b.gateway.is_empty() { "openshell".to_owned() } else { b.gateway.clone() }).unwrap_or_else(host_label),
        cwd: cwd.clone(),
        status: Status::Running,
        title,
        subtitle,
        context: place.clone(),
        sandbox: sandbox.clone(),
        created_at: ts.clone(),
        active_at: ts.clone(),
        color,
        stats: super::model::Stats { started_at: ts.clone(), ..Default::default() },
        ..Default::default()
    };
    {
        let mut i = app.agents.store.lock();
        i.put_session(s);
        let at = if sandbox.is_some() { place.clone() } else { format!("{} · {}", host_label(), tilde(&cwd)) };
        i.push_row(&session_thread(&id), state_row(&ts, &format!("Session started · {} · {at}", h.label)));
    }
    let task = a.task.trim().to_owned();
    let model = a.model.clone();
    let thinking = a.thinking.clone();
    // A new sandbox: made (and the directory uploaded) before the adapter starts in it.
    let make = match (&sandbox, existing) {
        (Some(b), None) => {
            let o = h.openshell.clone().unwrap_or_default();
            let from = a.sandbox.as_ref().map(|s| s.from.trim().to_owned()).filter(|f| !f.is_empty()).unwrap_or(o.from);
            Some(super::openshell::NewSandbox { gateway: b.gateway.clone(), name: b.name.clone(), from, providers: o.providers, policy: o.policy, upload: b.upload.clone() })
        }
        _ => None,
    };
    let Some(arc) = app.arc() else { return Ok(id) };
    // Running but not in the agent host yet (a sandbox can take minutes to make): a reattach must leave it be.
    app.agents.acp.starting.lock().unwrap_or_else(|e| e.into_inner()).insert(id.clone());
    let sid = id.clone();
    std::thread::spawn(move || {
        let queued = (!task.is_empty()).then(|| push_you(&arc, &sid, &task, true, vec![]));
        let made = match make {
            Some(n) => make_sandbox(&arc, &sid, &n).map_err(|e| format!("couldn't create sandbox {} · {e}", n.name)),
            None => Ok(()),
        };
        let connected = made.and_then(|_| connect(&arc, &sid, None).map_err(|e| e.to_string()));
        arc.agents.acp.starting.lock().unwrap_or_else(|e| e.into_inner()).remove(&sid);
        match connected {
            Ok(conn) => {
                if let Some(m) = model {
                    set_model(&arc, &conn, &sid, &m);
                }
                if let Some(t) = thinking {
                    if let Err(e) = set_thinking_on(&arc, &conn, &sid, &t) {
                        eprintln!("agents: couldn't set {sid}'s thinking to {t}: {e}");
                    }
                }
                if let Some(row) = queued {
                    prompt(&arc, &conn, row, task, vec![]);
                } else {
                    set_status(&arc, &sid, Status::Idle);
                }
            }
            Err(e) => fail(&arc, &sid, &e),
        }
        super::facts::refresh(&arc, &sid);
        super::guess::suggest(&arc, &sid);
    });
    Ok(id)
}

/// Create the session's sandbox and upload its directory; the session then runs where the upload landed.
fn make_sandbox(app: &App, id: &str, n: &super::openshell::NewSandbox) -> Result<()> {
    let from = if n.from.is_empty() { "the gateway's default image".to_owned() } else { n.from.clone() };
    let ts = now(app);
    app.agents.store.lock().push_row(&session_thread(id), state_row(&ts, &format!("Creating sandbox {} from {from} · uploading {}", n.name, tilde(&n.upload))));
    let workdir = super::openshell::create(&app.settings().agents.openshell.command, n)?;
    // What went in, for bringing changes back (`openshell::sync`).
    let sb = super::model::Sandbox { gateway: n.gateway.clone(), name: n.name.clone(), upload: n.upload.clone(), ..Default::default() };
    if let Err(e) = super::openshell::record_upload(app, &sb) {
        eprintln!("agents: couldn't record what went into sandbox {}: {e}", n.name);
    }
    let ts = now(app);
    let mut i = app.agents.store.lock();
    i.update_session(id, |s| s.cwd = workdir.clone());
    i.push_row(&session_thread(id), state_row(&ts, &format!("Sandbox {} ready · {} → {workdir}", n.name, tilde(&n.upload))));
    Ok(())
}

fn set_model(app: &App, conn: &Conn, id: &str, model: &str) {
    let sid = conn.sid();
    let by_model = conn.request("session/set_model", json!({ "sessionId": sid, "modelId": model }), Some(CALL_TIMEOUT));
    let done = by_model.or_else(|_| conn.request("session/set_config_option", json!({ "sessionId": sid, "configId": "model", "value": model }), Some(CALL_TIMEOUT)));
    match done {
        Ok(_) => {
            app.agents.store.lock().update_session(id, |s| s.model = super::explain::short_model(model));
        }
        Err(e) => eprintln!("agents: couldn't switch {id} to model {model}: {e}"),
    }
}

/// The models a harness offers: the adapter is started briefly, asked for a session, and stopped. Cached per harness.
pub fn models(app: &App, harness: &str, cwd: &str) -> Result<Value> {
    static CACHE: OnceLock<Mutex<HashMap<String, Value>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    if let Some(v) = cache.lock().unwrap_or_else(|e| e.into_inner()).get(harness) {
        return Ok(v.clone());
    }
    let settings = app.settings();
    let h = settings.agents.harnesses.iter().find(|h| h.key == harness).ok_or_else(|| msg(format!("no harness '{harness}'")))?;
    let path = login_path();
    let exe = which(&h.command, &path).ok_or_else(|| msg(format!("{} isn't on your PATH", h.command)))?;
    let cwd = expand_home(cwd.trim());
    let cwd = if std::path::Path::new(&cwd).is_dir() { cwd } else { dirs::home_dir().map(|d| d.display().to_string()).unwrap_or_else(|| ".".into()) };
    let mut child = Command::new(exe)
        .args(&h.args)
        .envs(&h.env)
        .env("PATH", path)
        .current_dir(&cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| msg(e.to_string()))?;
    let mut stdin = child.stdin.take().ok_or_else(|| msg("no stdin"))?;
    let stdout = child.stdout.take().ok_or_else(|| msg("no stdout"))?;
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        use std::io::BufRead;
        for line in std::io::BufReader::new(stdout).lines().map_while(|l| l.ok()) {
            if let Ok(v) = serde_json::from_str::<Value>(&line) {
                if tx.send(v).is_err() {
                    break;
                }
            }
        }
    });
    let init = json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "protocolVersion": PROTOCOL_VERSION, "clientCapabilities": {}, "clientInfo": { "name": "alethiometer", "version": env!("CARGO_PKG_VERSION") } } });
    let new = json!({ "jsonrpc": "2.0", "id": 2, "method": "session/new", "params": { "cwd": cwd, "mcpServers": [] } });
    let sent = {
        use std::io::Write;
        writeln!(stdin, "{init}").and_then(|_| writeln!(stdin, "{new}")).and_then(|_| stdin.flush())
    };
    let deadline = std::time::Instant::now() + INIT_TIMEOUT;
    let mut reply = Err(msg("the agent didn't answer"));
    if sent.is_ok() {
        while let Some(left) = deadline.checked_duration_since(std::time::Instant::now()) {
            let Ok(v) = rx.recv_timeout(left) else { break };
            if v.get("id") == Some(&json!(2)) {
                reply = match v.get("error") {
                    Some(e) => Err(msg(rpc_error(e))),
                    None => Ok(v.get("result").cloned().unwrap_or(Value::Null)),
                };
                break;
            }
        }
    }
    let _ = child.kill();
    let _ = child.wait();
    let r = reply?;
    let list = |items: Option<&Vec<Value>>, id: &str| -> Vec<Value> {
        items.map(|a| a.iter().filter_map(|m| Some(json!({ "id": m.get(id)?.as_str()?, "name": m.get("name").and_then(Value::as_str).unwrap_or_default() }))).collect()).unwrap_or_default()
    };
    let option = r.get("configOptions").and_then(Value::as_array).and_then(|o| o.iter().find(|o| o.get("id").and_then(Value::as_str) == Some("model")));
    let mut models = list(r.pointer("/models/availableModels").and_then(Value::as_array), "modelId");
    if models.is_empty() {
        models = list(option.and_then(|o| o.get("options")).and_then(Value::as_array), "value");
    }
    let current = r.pointer("/models/currentModelId").and_then(Value::as_str).or_else(|| option.and_then(|o| o.get("currentValue")).and_then(Value::as_str)).unwrap_or_default();
    let out = json!({ "models": models, "current": current, "thinking": thinking_of(&r) });
    cache.lock().unwrap_or_else(|e| e.into_inner()).insert(harness.to_owned(), out.clone());
    Ok(out)
}

fn rand_u32() -> u32 {
    let mut b = [0u8; 4];
    ring::rand::SecureRandom::fill(&ring::rand::SystemRandom::new(), &mut b).ok();
    u32::from_le_bytes(b) ^ (std::process::id() << 8)
}

fn state_row(at: &str, text: &str) -> Row {
    Row { at: at.into(), kind: Kind::State, text: text.into(), ..Default::default() }
}

/// A "N queued" marker stands in for a held-back message until it's actually sent.
fn queue_message(app: &App, id: &str, text: &str, images: Vec<RowImage>) {
    let ts = now(app);
    let key = session_thread(id);
    let mut i = app.agents.store.lock();
    let marker = i.sessions.get(id).and_then(|s| s.queue_marker);
    let n = i.sessions.get(id).map(|s| s.queue_peak + 1).unwrap_or(1);
    let note = format!("{n} message{} queued", if n == 1 { "" } else { "s" });
    let marker = match marker {
        Some(rid) => {
            i.update_row(&key, rid, |r| r.text = note);
            rid
        }
        None => i.push_row(&key, Row { ext: "queue-marker".into(), ..state_row(&ts, &note) }),
    };
    i.update_session(id, |s| {
        s.queue_marker = Some(marker);
        s.queue_peak = n;
        s.queued.push(QueuedMessage { text: text.to_owned(), images });
    });
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
    // In a sandbox, the agent host runs the OpenShell CLI, which runs the adapter there.
    let (exe, args, spawn_cwd) = match &s.sandbox {
        Some(sb) => {
            let cli = &settings.agents.openshell.command;
            let exe = which(cli, &path).ok_or_else(|| msg(format!("{cli} isn't on your PATH (agents.openshell.command)")))?;
            super::openshell::ensure_started(cli, sb).map_err(|e| msg(format!("couldn't start sandbox {} · {e}", sb.name)))?;
            let local = Some(sb.upload.clone()).filter(|d| std::path::Path::new(d).is_dir()).or_else(|| dirs::home_dir().map(|d| d.display().to_string())).unwrap_or_else(|| "/".into());
            (exe, super::openshell::exec_args(sb, &s.cwd, h), local)
        }
        None => {
            let exe = which(&h.command, &path).ok_or_else(|| msg(format!("{} isn't on your PATH (agents.harnesses.{}.command)", h.command, h.key)))?;
            (exe, h.args.clone(), s.cwd.clone())
        }
    };
    // A new process starts its turn over; its events keep counting from where the log was read.
    let after = app.agents.store.lock().hosts.get(id).map(|h| h.seq).unwrap_or(0);
    app.agents.store.lock().set_host(id, HostState { seq: after, turn: Value::Null });
    let conn = Arc::new(Conn::new(app, id, Turn::default(), true));
    app.agents.acp.conns.lock().unwrap_or_else(|e| e.into_inner()).insert(id.to_owned(), conn.clone());
    // The project's devshell is this machine's; a sandbox has its own environment (the harness's `openshell.env`).
    let mut env = if s.sandbox.is_some() { BTreeMap::new() } else { direnv_env(&s.cwd, &path) };
    let watches = direnv_watches(&env);
    app.agents.store.lock().update_session(id, |s| {
        s.env_watches = watches;
        s.env_changed = false;
    });
    env.entry("PATH".into()).or_insert(path);
    if s.sandbox.is_none() {
        env.extend(h.env.clone());
    }
    let spawned = daemon::call(app, &dproto::Request::Spawn { id: id.to_owned(), cmd: exe.display().to_string(), args, env, cwd: spawn_cwd, after });
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
                "clientCapabilities": { "fs": { "readTextFile": false, "writeTextFile": false }, "terminal": false, "elicitation": { "form": {} } },
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
    // In a sandbox, Claude Code decides in its auto mode and other agents ask nothing (`openshell::mode`).
    // Otherwise in-project edits are the agent's call to let through (agents.accept_edits).
    let allow_all = s.sandbox.is_some() && !super::openshell::is_claude(&init);
    let mode = if s.sandbox.is_some() { super::openshell::mode(&init, &opened) } else { settings.agents.accept_edits.then(|| accept_edits_mode(&opened)).flatten() };
    if let Some(mode) = mode {
        if let Err(e) = conn.request("session/set_mode", json!({ "sessionId": sid, "modeId": mode }), Some(CALL_TIMEOUT)) {
            eprintln!("agents: couldn't switch {id} to {mode}: {e}");
        }
    }
    let model = model_of(&opened);
    let thinking = thinking_of(&opened);
    app.agents.store.lock().update_session(id, |s| {
        s.thinking = thinking;
        s.ext = sid;
        s.live = true;
        if let Some(b) = s.sandbox.as_mut() {
            b.allow_all = allow_all;
        }
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
    crate::time::Ts::from_unix_ms(at_ms as i64 + app.clock.offset_ms()).map(crate::time::Ts::to_db).unwrap_or_else(|| now(app))
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

/// Work an agent-host event leads to that runs after the gate is released:
/// sending a held-back message takes the gate itself, and the rest talks to
/// the agent host or reads the project. Nothing that takes the gate may run
/// while it's held (it isn't re-entrant).
pub(crate) enum After {
    /// The turn is over and a message was held back: send it now.
    Send(QueuedMessage),
    /// The turn is over and nothing is waiting: refresh the facts box.
    Facts,
    /// The agent exited and its events are applied: the host can drop its log.
    Forget,
}

fn run_after(app: &App, id: &str, after: After) {
    match after {
        After::Send(q) => {
            if let (Some(a), Some(c)) = (app.arc(), app.agents.acp.get(id)) {
                let row = push_you(app, id, &q.text, false, q.images.clone());
                prompt(&a, &c, row, q.text, q.images);
            }
        }
        After::Facts => super::facts::refresh(app, id),
        After::Forget => {
            let _ = daemon::call(app, &dproto::Request::Forget { id: id.to_owned() });
        }
    }
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
    let id = ev.id.clone();
    if let Some(after) = apply_event(app, &conn, ev) {
        run_after(app, &id, after);
    }
}

/// Apply one event under the gate: rows and the log position move together
/// (see `Store::gate`).
fn apply_event(app: &Arc<App>, conn: &Arc<Conn>, ev: Event) -> Option<After> {
    let _gate = app.agents.store.gate();
    let seen = app.agents.store.lock().hosts.get(&ev.id).map(|h| h.seq).unwrap_or(0);
    if ev.seq <= seen {
        return None;
    }
    let _ = app.agents.store.ensure_loaded(app.store(), &session_thread(&ev.id));
    let ts = ts_of(app, ev.at_ms);
    let after = match ev.kind {
        dproto::Kind::Out => {
            // Adapters and wrappers sometimes print non-JSON lines; skip them.
            ev.line.as_deref().and_then(|l| serde_json::from_str::<Value>(l.trim()).ok()).and_then(|v| handle(app, conn, v, &ts))
        }
        dproto::Kind::Err => {
            let mut e = conn.stderr.lock().unwrap_or_else(|e| e.into_inner());
            e.push(ev.line.unwrap_or_default());
            if e.len() > 50 {
                e.remove(0);
            }
            None
        }
        dproto::Kind::Exit => {
            exited(app, conn, ev.line.as_deref(), &ts);
            Some(After::Forget)
        }
    };
    save_turn(app, conn, Some(ev.seq));
    after
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
                let _ = app.agents.store.ensure_loaded(app.store(), &session_thread(&s.id));
                let Some(conn) = conn_for(app, &s.id) else { continue };
                conn.alive.store(true, Ordering::SeqCst);
                let waiting: Vec<Waiting> = conn.turn.lock().unwrap_or_else(|e| e.into_inner()).waiting.values().cloned().collect();
                app.agents.store.lock().update_session(&s.id, |x| x.live = true);
                for w in waiting {
                    let req = format!("{}:{}", s.id, w.rpc);
                    if !app.agents.store.lock().requests.contains_key(&req) {
                        if w.is_ask() {
                            elicit(app, &conn, w.rpc.clone(), &w.params);
                        } else {
                            permission(app, &conn, w.rpc.clone(), &w.params);
                        }
                    }
                }
            }
            // Exited while we were away: its events (ending in the exit) are still to come.
            Some(false) => {}
            // Being started or reattached right now: not in the host yet.
            None if app.agents.acp.starting.lock().unwrap_or_else(|e| e.into_inner()).contains(&s.id) => {}
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
    matches!(current, None | Some("default")).then(|| m.to_owned())
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

/// The agent's thinking option from a session/new, set_config_option or config_option_update.
pub fn thinking_of(v: &Value) -> Option<super::model::ConfigChoice> {
    let o = v.get("configOptions")?.as_array()?.iter().find(|o| o.get("category").and_then(Value::as_str) == Some("thought_level"))?;
    let str_of = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).unwrap_or_default().to_owned();
    let options: Vec<super::model::ChoiceOption> = o
        .get("options")?
        .as_array()?
        .iter()
        .map(|x| super::model::ChoiceOption { value: str_of(x, "value"), name: str_of(x, "name") })
        .filter(|x| !x.value.is_empty())
        .collect();
    let id = str_of(o, "id");
    (!id.is_empty() && !options.is_empty()).then(|| super::model::ConfigChoice { id, current: str_of(o, "currentValue"), options })
}

fn set_thinking_on(app: &App, conn: &Conn, id: &str, value: &str) -> Result<()> {
    let opt = app.agents.store.lock().sessions.get(id).and_then(|s| s.thinking.clone()).ok_or_else(|| msg("This agent has no thinking setting"))?;
    if !opt.options.iter().any(|o| o.value == value) {
        return Err(msg(format!("'{value}' isn't one of this agent's thinking levels")));
    }
    let r = conn.request("session/set_config_option", json!({ "sessionId": conn.sid(), "configId": opt.id, "value": value }), Some(CALL_TIMEOUT))?;
    let next = thinking_of(&r).unwrap_or(super::model::ConfigChoice { current: value.to_owned(), ..opt });
    app.agents.store.lock().update_session(id, |s| s.thinking = Some(next));
    Ok(())
}

/// Change a running session's thinking level.
pub fn set_thinking(app: &App, id: &str, value: &str) -> Result<()> {
    let conn = app.agents.acp.get(id).ok_or_else(|| msg("It isn't running; send it a message first"))?;
    set_thinking_on(app, &conn, id, value)
}

fn handle(app: &Arc<App>, conn: &Arc<Conn>, v: Value, ts: &str) -> Option<After> {
    let method = v.get("method").and_then(Value::as_str);
    match (method, v.get("id")) {
        (Some(m), Some(id)) => {
            let params = v.get("params").cloned().unwrap_or(Value::Null);
            match m {
                "session/request_permission" => permission(app, conn, id.clone(), &params),
                ELICIT => elicit(app, conn, id.clone(), &params),
                _ => {
                    let _ = conn.write(&json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32601, "message": format!("{m} isn't supported by this client") } }));
                }
            }
        }
        (Some("session/update"), None) => {
            if conn.replaying.load(Ordering::SeqCst) {
                return None;
            }
            if let Some(u) = v.pointer("/params/update") {
                let mut turn = conn.turn.lock().unwrap_or_else(|e| e.into_inner());
                apply(app, &conn.session, &mut turn, u, ts);
            }
        }
        (Some(_), None) => {}
        (None, Some(id)) => {
            let id = id.as_u64()?;
            let r = match v.get("error") {
                Some(e) => Err(e.clone()),
                None => Ok(v.get("result").cloned().unwrap_or(Value::Null)),
            };
            if let Some(tx) = conn.pending.lock().unwrap_or_else(|e| e.into_inner()).remove(&id) {
                let _ = tx.send(r);
            } else if conn.turn.lock().unwrap_or_else(|e| e.into_inner()).prompts.contains_key(&id) {
                return finish_prompt(app, conn, id, r, ts);
            }
        }
        _ => {}
    }
    None
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
        "config_option_update" => {
            let thinking = thinking_of(u);
            store.lock().update_session(session, |s| s.thinking = thinking);
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

/// Fold a tool call update into what's known about the call. Not its
/// content: that can be large, and a permission request's own content is its message.
fn remember(turn: &mut Turn, call: &str, u: &Value) {
    let e = turn.calls.entry(call.to_owned()).or_insert_with(|| json!({}));
    // The status tells a call waiting to run (where pi's permission gates ask) from one running.
    for k in ["kind", "title", "name", "status"] {
        if let Some(v) = u.get(k).filter(|v| !v.is_null()) {
            e[k] = v.clone();
        }
    }
    if let Some(n) = meta(u, "/claudeCode/toolName") {
        e["name"] = n.clone();
    }
    if let Some(input) = u.get("rawInput").and_then(Value::as_object).filter(|o| !o.is_empty()) {
        // A Write's whole file isn't needed to label or parse it.
        let small: serde_json::Map<String, Value> = input.iter().filter(|(_, v)| v.to_string().len() <= 8192).map(|(k, v)| (k.clone(), v.clone())).collect();
        e["rawInput"] = Value::Object(small);
    }
    if matches!(u.get("status").and_then(Value::as_str), Some("completed" | "failed")) {
        turn.calls.remove(call);
    }
}

fn tool_update(app: &App, session: &str, turn: &mut Turn, u: &Value, ts: &str) {
    let Some(call) = u.get("toolCallId").and_then(Value::as_str) else { return };
    remember(turn, call, u);
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
            let color = super::store::color_for(app.store(), &group, &child, app.settings().agents.names.palette.len()).unwrap_or(0);
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
                sandbox: parent.sandbox.clone(),
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
    let asked = p.get("toolCall").cloned().unwrap_or(Value::Null);
    let call = asked.get("toolCallId").and_then(Value::as_str).unwrap_or("").to_owned();
    // The request's toolCall is an update: what it leaves out is what the call already said.
    let mut tc = conn.turn.lock().unwrap_or_else(|e| e.into_inner()).calls.get(&call).cloned().unwrap_or_else(|| json!({}));
    if let Some(o) = asked.as_object() {
        for (k, v) in o.iter().filter(|(_, v)| !v.is_null()) {
            tc[k] = v.clone();
        }
    }
    // Text the agent (or a gate in it) sent with the request: shown, never explained.
    let message = asked
        .get("content")
        .and_then(Value::as_array)
        .map(|blocks| blocks.iter().filter(|b| b.get("type").and_then(Value::as_str) == Some("content")).filter_map(|b| b.pointer("/content/text").and_then(Value::as_str)).map(|t| strip_fence(t).trim().to_owned()).filter(|t| !t.is_empty()).collect::<Vec<_>>().join("\n\n"))
        .unwrap_or_default();
    let ts = now(app);
    let s = app.agents.store.lock().sessions.get(&session).cloned().unwrap_or_default();
    let known = conn.turn.lock().unwrap_or_else(|e| e.into_inner()).tools.get(&call).cloned();
    let known = known.and_then(|(k, id, _)| app.agents.store.lock().find_row(&k, |r| r.id == id).and_then(|r| r.tool.clone()));
    let (name, detail) = match known {
        Some(t) if tc.get("kind").is_none() => (t.name, t.detail),
        _ => tool_label(&tc, &s.cwd),
    };
    let kind = tc.get("kind").and_then(Value::as_str).unwrap_or(match name.as_str() {
        "bash" => "execute",
        "edit" | "write" => "edit",
        "read" | "delete" | "move" | "fetch" | "search" => name.as_str(),
        _ => "",
    });
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
    // A request about no call the agent announced, while none waits to run, is
    // a prompt of its own (pi-acp sends an extension's select or confirm this
    // way): a question, shown with its own words, never answered for you.
    let (known_call, call_pending) = {
        let turn = conn.turn.lock().unwrap_or_else(|e| e.into_inner());
        (!call.is_empty() && (turn.tools.contains_key(&call) || turn.calls.contains_key(&call)), turn.calls.iter().any(|(id, c)| *id != call && c.get("status").and_then(Value::as_str) == Some("pending")))
    };
    let question = !known_call && !call_pending;
    let asked_text = asked.get("title").and_then(Value::as_str).map(|t| t.trim().to_owned()).filter(|t| !t.is_empty());
    let command = match (&asked_text, question) {
        (Some(t), true) => t.clone(),
        _ => command,
    };
    let parsed = (!question && (kind == "execute" || name == "bash")).then(|| super::shell::parse(&command, &s.cwd, &local_bins(&s.cwd)));
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
        None if question => "question".into(),
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
        _ if question => asked_text.as_deref().and_then(|t| t.lines().map(str::trim).find(|l| !l.is_empty())).map(|l| crate::ai::context::clip(l, 120)).unwrap_or_else(|| "The agent asks".into()),
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
        message: if message == command { String::new() } else { message },
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
    // An agent whose checks are off in its sandbox gets its own allow option at once, for a gate (`openshell::auto_allow`).
    let auto = s.sandbox.as_ref().filter(|b| b.allow_all).and_then(|_| super::openshell::auto_allow(&req.options, known_call, call_pending)).map(|o| o.id.clone());
    {
        let mut turn = conn.turn.lock().unwrap_or_else(|e| e.into_inner());
        if auto.is_none() {
            turn.waiting.insert(req_id.clone(), Waiting { rpc: rpc_id.clone(), params: p.clone(), method: "session/request_permission".into() });
        }
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
        if auto.is_some() {
            if let Some(entry) = turn.tools.get_mut(&call) {
                entry.2 = now_ms();
            }
            let hm = app.clock.local_now().strftime("%H:%M").to_string();
            i.update_row(&key, row_id, |r| r.note = format!("allowed in the sandbox at {hm}"));
        } else {
            i.update_row(&key, row_id, |r| {
                r.kind = Kind::Approval;
                r.label = if question { "asks you".into() } else { "needs approval".into() };
                r.request = Some(req_id.clone());
                r.text = if question || kind == "execute" || name == "bash" { command.clone() } else { detail.clone() };
            });
            i.put_request(req);
            i.update_session(&session, |s| s.status = Status::Waiting);
        }
    }
    if let Some(option) = auto {
        if let Err(e) = conn.respond(&rpc_id, json!({ "outcome": { "outcome": "selected", "optionId": option } })) {
            eprintln!("agents: couldn't allow {req_id} in the sandbox: {e}");
        }
        return;
    }
    if !question {
        super::explain::request(app, &req_id);
    }
}

/// `elicitation/create`: the agent asks you something (Claude Code's
/// AskUserQuestion, pi's questions and text input). Shown as an Ask. Form
/// mode only; a URL elicitation is declined.
fn elicit(app: &Arc<App>, conn: &Arc<Conn>, rpc_id: Value, p: &Value) {
    let session = conn.session.clone();
    let schema = p.get("requestedSchema").cloned().unwrap_or(Value::Null);
    if p.get("mode").and_then(Value::as_str) != Some("form") || !schema.get("properties").is_some_and(Value::is_object) {
        let _ = conn.respond(&rpc_id, json!({ "action": "decline" }));
        return;
    }
    let asked_reason = {
        let mut t = conn.turn.lock().unwrap_or_else(|e| e.into_inner());
        match (text_field(&schema), one_choice(&schema), &t.deny_reason) {
            (Some(field), None, Some((_, true))) => t.deny_reason.take().map(|(r, _)| (field, r)),
            _ => None,
        }
    };
    if let Some((field, reason)) = asked_reason {
        let _ = conn.respond(&rpc_id, json!({ "action": "accept", "content": { field: reason } }));
        return;
    }
    let ts = now(app);
    let req_id = format!("{session}:{rpc_id}");
    let question = p.get("message").and_then(Value::as_str).unwrap_or("").trim().to_owned();
    let s = app.agents.store.lock().sessions.get(&session).cloned().unwrap_or_default();
    let req = Request {
        id: req_id.clone(),
        session_id: session.clone(),
        thread: session_thread(&session),
        kind: "ask".into(),
        at: ts.clone(),
        title: question.clone(),
        cwd: s.cwd.clone(),
        question: question.clone(),
        choices: one_choice(&schema).map(|(_, c)| c).unwrap_or_default(),
        tag: "question".into(),
        via: "question".into(),
        form: Some(schema),
        ..Default::default()
    };
    let mut turn = conn.turn.lock().unwrap_or_else(|e| e.into_inner());
    turn.waiting.insert(req_id.clone(), Waiting { rpc: rpc_id, params: p.clone(), method: ELICIT.into() });
    let mut i = app.agents.store.lock();
    let key = session_thread(&session);
    if i.find_row(&key, |r| r.request.as_deref() == Some(req_id.as_str())).is_none() {
        i.push_row(&key, Row { at: ts, kind: Kind::Ask, who: Some(Who::agent(&s.name, Some(session.clone()))), text: question, request: Some(req_id), ..Default::default() });
    }
    i.put_request(req);
    i.update_session(&session, |s| s.status = Status::Waiting);
}

/// The form's one single-choice field and its options, when that's all it
/// needs (other fields optional free text): answered with a click.
fn one_choice(schema: &Value) -> Option<(String, Vec<Choice>)> {
    let props = schema.get("properties")?.as_object()?;
    let required: Vec<&str> = schema.get("required").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
    let mut found = None;
    for (name, f) in props {
        match (field_options(f), f.get("type").and_then(Value::as_str)) {
            (Some(opts), Some("string")) if found.is_none() => found = Some((name.clone(), opts)),
            (None, Some("string")) if !required.contains(&name.as_str()) => {}
            _ => return None,
        }
    }
    let (name, opts) = found?;
    let choices = opts
        .iter()
        .enumerate()
        .map(|(i, o)| {
            let value = o.get("const").and_then(Value::as_str).unwrap_or("").to_owned();
            let title = o.get("title").and_then(Value::as_str).unwrap_or(&value);
            // pi's questionnaire writes "1. Label - what it means" as one label.
            let title = title.trim().trim_start_matches(|c: char| c.is_ascii_digit()).trim_start_matches(['.', ')']).trim_start();
            let title = if title.is_empty() { value.as_str() } else { title };
            let (title, detail) = match o.get("description").and_then(Value::as_str) {
                Some(d) => (title.to_owned(), d.to_owned()),
                None => title.split_once(" - ").or_else(|| title.split_once(" — ")).map(|(a, b)| (a.trim().to_owned(), b.trim().to_owned())).unwrap_or_else(|| (title.to_owned(), String::new())),
            };
            let free = title.trim_end_matches('.').eq_ignore_ascii_case("type something");
            Choice { key: (i + 1).to_string(), title, detail, value, free }
        })
        .collect();
    Some((name, choices))
}

/// A field's options as `{const, title, description}`: JSON Schema `oneOf`/`anyOf`, or a plain `enum`.
fn field_options(f: &Value) -> Option<Vec<Value>> {
    if let Some(a) = f.get("oneOf").or_else(|| f.get("anyOf")).and_then(Value::as_array) {
        return Some(a.clone());
    }
    let e = f.get("enum")?.as_array()?;
    let names = f.get("enumNames").and_then(Value::as_array);
    Some(e.iter().enumerate().map(|(i, v)| json!({ "const": v, "title": names.and_then(|n| n.get(i)).cloned().unwrap_or(v.clone()) })).collect())
}

fn plain(v: &Value) -> String {
    v.as_str().map(str::to_owned).unwrap_or_else(|| v.to_string())
}

/// The first free-text field: where a written reply goes.
fn text_field(schema: &Value) -> Option<String> {
    schema.get("properties")?.as_object()?.iter().find(|(_, f)| f.get("type").and_then(Value::as_str) == Some("string") && field_options(f).is_none()).map(|(k, _)| k.clone())
}

/// Answer an Ask from `elicitation/create`: one of its choices (by key),
/// written text, or the whole form. Text with nowhere to go declines the
/// question and is sent as your next message instead.
pub fn answer(app: &App, req_id: &str, choice: Option<&str>, text: Option<&str>, content: Option<Value>) -> Result<()> {
    let req = app.agents.store.lock().requests.get(req_id).cloned().ok_or_else(|| msg("That question was already answered"))?;
    let conn = app.agents.acp.get(&req.session_id).ok_or_else(|| msg("the agent isn't running anymore"))?;
    let schema = req.form.clone().unwrap_or(Value::Null);
    let text = text.map(str::trim).filter(|t| !t.is_empty());
    let mut follow_up = None;
    let (reply, said, after) = if let Some(c) = content {
        let said = c.as_object().map(|o| o.values().map(|v| v.as_array().map(|a| a.iter().map(plain).collect::<Vec<_>>().join(", ")).unwrap_or_else(|| plain(v))).collect::<Vec<_>>().join("; ")).unwrap_or_default();
        (json!({ "action": "accept", "content": c }), said, None)
    } else if let Some(key) = choice {
        let (field, choices) = one_choice(&schema).ok_or_else(|| msg("That question has no choices"))?;
        let c = choices.iter().find(|c| c.key == key || c.value == key).ok_or_else(|| msg("not one of its choices"))?;
        let said = match text.filter(|_| c.free) {
            Some(t) => {
                follow_up = Some(t.to_owned());
                t.to_owned()
            }
            None => c.title.clone(),
        };
        (json!({ "action": "accept", "content": { field: c.value } }), said, None)
    } else if let Some(t) = text {
        match text_field(&schema) {
            Some(field) => (json!({ "action": "accept", "content": { field: t } }), t.to_owned(), None),
            None => (json!({ "action": "decline" }), "replied below".to_owned(), Some(t.to_owned())),
        }
    } else {
        return Err(msg("Pick a choice or write a reply"));
    };
    let w = {
        let mut t = conn.turn.lock().unwrap_or_else(|e| e.into_inner());
        let w = t.waiting.remove(req_id).ok_or_else(|| msg("That question was already answered"))?;
        if let Some(f) = follow_up {
            t.deny_reason = Some((f, true));
        }
        w
    };
    {
        let mut i = app.agents.store.lock();
        i.remove_request(req_id);
        let key = req.thread.clone();
        if let Some(id) = i.find_row(&key, |r| r.request.as_deref() == Some(req_id)).map(|r| r.id) {
            i.update_row(&key, id, |r| {
                r.request = None;
                r.note = format!("answered: {said}");
            });
        }
        let still_waiting = i.requests.values().any(|r| r.session_id == req.session_id);
        if !still_waiting {
            i.update_session(&req.session_id, |s| s.status = Status::Running);
        }
    }
    // Stored before it's sent, as in `respond`.
    conn.respond(&w.rpc, reply)?;
    if let Some(t) = after {
        send(app, &req.session_id, &t, vec![])?;
    }
    Ok(())
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
    // The note reads in wall-clock time here.
    let hm = app.clock.local_now().strftime("%H:%M").to_string();
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
        if !still_waiting {
            i.update_session(&req.session_id, |s| s.status = Status::Running);
        }
    }
    if let Some(r) = reason.map(str::trim).filter(|r| !r.is_empty()) {
        conn.turn.lock().unwrap_or_else(|e| e.into_inner()).deny_reason = Some((r.to_owned(), opt.name.to_lowercase().contains("reason")));
    }
    // Stored before it's sent: a turn that ends at once must find nothing waiting, or it never goes idle.
    conn.respond(&rpc_id, json!({ "outcome": { "outcome": "selected", "optionId": opt.id } }))?;
    Ok(())
}

fn push_you(app: &App, id: &str, text: &str, mid_turn: bool, images: Vec<RowImage>) -> u64 {
    // Read the transcript first so "prompt" vs "message" sees the earlier rows.
    // Never called under the gate, so taking the database here keeps the lock order.
    let _ = app.agents.store.ensure_loaded(app.store(), &session_thread(id));
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
    conn.turn.lock().unwrap_or_else(|e| e.into_inner()).prompts.insert(rpc, row);
    {
        let ts = now(app);
        let mut i = app.agents.store.lock();
        i.update_row(&key, row, |r| {
            if let Some(d) = r.delivery.as_mut() {
                d.state = "delivered".into();
                d.delivered_at = ts.clone();
                d.count = 1;
            }
        });
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
        // Not under the gate here (it was dropped before writing).
        if let Some(after) = finish_prompt(app, conn, rpc, Err(json!({ "message": e.to_string() })), &now(app)) {
            run_after(app, &id, after);
        }
    }
}

/// A prompt's answer: the turn is over (or failed). What follows (sending a
/// held-back message, the facts box) is returned for the caller to run once
/// the gate is free.
fn finish_prompt(app: &App, conn: &Conn, rpc: u64, res: Reply, ts: &str) -> Option<After> {
    let id = conn.session.clone();
    let key = session_thread(&id);
    let mut t = conn.turn.lock().unwrap_or_else(|e| e.into_inner());
    let row = t.prompts.remove(&rpc)?;
    let idle = t.prompts.is_empty();
    let mut i = app.agents.store.lock();
    for (k, rid) in t.open.drain() {
        let thread = k.split('\u{1f}').next().unwrap_or("").to_owned();
        i.update_row(&thread, rid, |r| r.open = false);
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
    if let Some((r, _)) = t.deny_reason.take() {
        i.update_session(&id, |s| s.queued.push(QueuedMessage { text: r, images: vec![] }));
    }
    // Only truly send the next queued message once this turn (end or cancel) is done.
    let next = if idle { i.sessions.get(&id).and_then(|s| s.queued.first().cloned()) } else { None };
    let drained = next.is_some() && i.sessions.get(&id).map(|s| s.queued.len() == 1).unwrap_or(false);
    let marker = i.sessions.get(&id).and_then(|s| s.queue_marker);
    i.update_session(&id, |s| {
        if next.is_some() {
            s.queued.remove(0);
        }
        if drained {
            s.queue_marker = None;
            s.queue_peak = 0;
        }
        s.seen_turns.get_or_insert(s.stats.turns);
        s.stats.turns += 1;
        s.active_at = ts.to_owned();
        if let Some(p) = &problem {
            s.problem = p.clone();
        }
        if idle && !waiting && next.is_none() {
            s.status = if problem.is_some() { Status::Failed } else { Status::Idle };
        }
    });
    if drained {
        if let Some(rid) = marker {
            i.update_row(&key, rid, |r| {
                r.delivery = Some(Delivery { state: "delivered".into(), at: ts.to_owned(), delivered_at: ts.to_owned(), count: 1, ..Default::default() });
            });
        }
    }
    match next {
        Some(q) => Some(After::Send(q)),
        None if idle => Some(After::Facts),
        None => None,
    }
}

/// Message a local session. One from an earlier run is started again first.
pub fn send(app: &App, id: &str, text: &str, images: Vec<RowImage>) -> Result<()> {
    if id.contains('~') {
        return Err(msg("Subagents take messages through their parent"));
    }
    if let Some(conn) = app.agents.acp.get(id) {
        // Checked and queued under the turn lock: `finish_prompt` takes it to decide what follows a turn, so a turn can't end between the two and leave this message queued with nothing to send it.
        let turn = conn.turn.lock().unwrap_or_else(|e| e.into_inner());
        if !turn.prompts.is_empty() {
            queue_message(app, id, text, images);
            drop(turn);
        } else {
            drop(turn);
            let arc = app.arc().ok_or_else(|| msg("not ready"))?;
            let row = push_you(app, id, text, false, images.clone());
            prompt(&arc, &conn, row, text.to_owned(), images);
        }
        return Ok(());
    }
    let arc = app.arc().ok_or_else(|| msg("not ready"))?;
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
    let waiting: Vec<(String, Waiting)> = conn.turn.lock().unwrap_or_else(|e| e.into_inner()).waiting.drain().collect();
    for (req, w) in waiting {
        let _ = conn.respond(&w.rpc, w.cancelled());
        app.agents.store.lock().remove_request(&req);
    }
    conn.notify("session/cancel", json!({ "sessionId": conn.sid() }))
}

/// ⌘↵ on an empty box: send the held-back message now. During a turn that
/// cancels it, which is what releases the queue (`finish_prompt`); with nothing
/// running the first queued message goes out at once.
pub fn send_queued(app: &App, id: &str) -> Result<()> {
    let conn = app.agents.acp.get(id).ok_or_else(|| msg("It isn't running"))?;
    let turn = conn.turn.lock().unwrap_or_else(|e| e.into_inner());
    if !turn.prompts.is_empty() {
        drop(turn);
        return interrupt(app, id);
    }
    let key = session_thread(id);
    let q = {
        let mut i = app.agents.store.lock();
        let Some(s) = i.sessions.get(id) else { return Ok(()) };
        let (Some(q), drained, marker) = (s.queued.first().cloned(), s.queued.len() == 1, s.queue_marker) else { return Ok(()) };
        i.update_session(id, |s| {
            s.queued.remove(0);
            if drained {
                s.queue_marker = None;
                s.queue_peak = 0;
            }
        });
        if let (true, Some(rid)) = (drained, marker) {
            let ts = now(app);
            i.update_row(&key, rid, |r| r.delivery = Some(Delivery { state: "delivered".into(), at: ts.clone(), delivered_at: ts.clone(), count: 1, ..Default::default() }));
        }
        q
    };
    drop(turn);
    run_after(app, id, After::Send(q));
    Ok(())
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
        // A sandboxed session's directory is inside its sandbox; the one it was uploaded from is this machine's.
        let d = tilde(x.sandbox.as_ref().map(|b| b.upload.as_str()).unwrap_or(&x.cwd));
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

    /// A `Conn` with no real process: enough to make `send` think a turn is in flight.
    fn fake_conn(app: &App, id: &str) -> Arc<Conn> {
        Arc::new(Conn::new(app, id, Turn { prompts: BTreeMap::from([(1, 1)]), ..Default::default() }, true))
    }

    #[test]
    fn a_permission_that_only_names_its_tool_call_is_still_parsed_and_explained() {
        let (app, _d) = app();
        let app = App::shared(app);
        let id = session(&app);
        let conn = fake_conn(&app, &id);
        let key = session_thread(&id);
        let mut turn = Turn::default();
        apply(&app, &id, &mut turn, &json!({ "sessionUpdate": "tool_call", "toolCallId": "t1", "kind": "execute", "title": "ls -la", "status": "pending" }), "2026-10-03T10:41:00");
        conn.turn.lock().unwrap().tools = turn.tools;
        // pi-acp: the request carries the tool call's id and title, no kind or rawInput.
        let p = json!({ "options": [{ "kind": "allow_once", "name": "Yes", "optionId": "yes" }], "toolCall": { "toolCallId": "t1", "title": "ls -la", "status": "in_progress" } });
        permission(&app, &conn, json!(7), &p);
        let r = app.agents.store.lock().requests.values().next().cloned().unwrap();
        assert_eq!(r.command, "ls -la");
        assert!(r.parsed.is_some());
        assert!(r.explained.is_some());
        assert_eq!(rows(&app, &key).len(), 1, "the same row, not a second one");
    }

    #[test]
    fn a_message_sent_mid_turn_is_held_back_not_fired() {
        let (app, _d) = app();
        let id = session(&app);
        let conn = fake_conn(&app, &id);
        app.agents.acp.conns.lock().unwrap_or_else(|e| e.into_inner()).insert(id.clone(), conn);

        send(&app, &id, "are you there?", vec![]).unwrap();

        let s = app.agents.store.lock().sessions[&id].clone();
        assert_eq!(s.queued.len(), 1, "held back while the turn is in flight, not fired");
        assert_eq!(s.queued[0].text, "are you there?");
        let r = rows(&app, &session_thread(&id));
        assert!(r.iter().all(|r| r.kind != Kind::You), "not a real row yet");
        let marker = r.iter().find(|r| r.kind == Kind::State).unwrap();
        assert_eq!(marker.text, "1 message queued");
    }

    #[test]
    fn the_reason_typed_with_a_deny_answers_the_agents_follow_up_question() {
        let (app, _d) = app();
        let app = App::shared(app);
        let id = session(&app);
        let conn = fake_conn(&app, &id);
        let form = json!({ "mode": "form", "message": "Share why", "requestedSchema": { "type": "object", "properties": { "value": { "type": "string" } }, "required": ["value"] } });

        conn.turn.lock().unwrap().deny_reason = Some(("too risky".into(), false));
        elicit(&app, &conn, json!(1), &form);
        assert_eq!(app.agents.store.lock().requests.len(), 1, "a plain No doesn't claim the question");
        assert!(conn.turn.lock().unwrap().deny_reason.is_some());

        conn.turn.lock().unwrap().deny_reason = Some(("too risky".into(), true));
        elicit(&app, &conn, json!(2), &form);
        assert_eq!(app.agents.store.lock().requests.len(), 1, "answered without a card");
        assert!(conn.turn.lock().unwrap().deny_reason.is_none());
    }

    #[test]
    fn words_typed_with_a_free_choice_answer_the_agents_follow_up_question() {
        let (app, _d) = app();
        let app = App::shared(app);
        let id = session(&app);
        let conn = fake_conn(&app, &id);
        app.agents.acp.conns.lock().unwrap().insert(id.clone(), conn.clone());
        let ask = json!({ "mode": "form", "message": "Which?", "requestedSchema": { "type": "object", "properties": { "value": { "type": "string", "oneOf": [
            { "const": "1. Yes", "title": "1. Yes" },
            { "const": "2. Type something.", "title": "2. Type something." }
        ] } }, "required": ["value"] } });
        elicit(&app, &conn, json!(1), &ask);
        let req = app.agents.store.lock().requests.values().next().cloned().unwrap();
        assert_eq!(req.choices.iter().map(|c| c.free).collect::<Vec<_>>(), [false, true]);
        // The agent host isn't running here, so only the send fails; the words are kept first.
        let _ = answer(&app, &req.id, Some("2"), Some("blue"), None);
        assert_eq!(conn.turn.lock().unwrap().deny_reason, Some(("blue".into(), true)));
        let text = json!({ "mode": "form", "message": "Your answer", "requestedSchema": { "type": "object", "properties": { "value": { "type": "string" } }, "required": ["value"] } });
        elicit(&app, &conn, json!(2), &text);
        assert!(app.agents.store.lock().requests.is_empty(), "answered without a second card");
    }

    #[test]
    fn a_projects_direnv_environment_is_read() {
        use std::os::unix::fs::PermissionsExt;
        let bin = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        // Stands in for direnv: prints what `export json` would.
        let fake = bin.path().join("direnv");
        std::fs::write(&fake, "#!/bin/sh\nprintf '{\"GONE\":null,\"PATH\":\"/nix/bin:%s\"}' \"$PATH\"\n").unwrap();
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        let path = format!("{}:/usr/bin:/bin", bin.path().display());
        let env = direnv_env(project.path().to_str().unwrap(), &path);
        assert_eq!(env.get("PATH"), Some(&format!("/nix/bin:{path}")));
        assert!(!env.contains_key("GONE"));
    }

    #[test]
    fn an_up_to_date_environment_keeps_the_apps_watches() {
        let own = Some("app-watches".to_string());
        assert_eq!(direnv_vars(b"", own.clone()).get("DIRENV_WATCHES").map(String::as_str), Some("app-watches"), "nothing changed");
        assert_eq!(direnv_vars(br#"{"DIRENV_WATCHES":"new"}"#, own.clone()).get("DIRENV_WATCHES").map(String::as_str), Some("new"));
        assert!(!direnv_vars(br#"{"DIRENV_WATCHES":null}"#, own).contains_key("DIRENV_WATCHES"), "unloaded: none");
    }

    #[test]
    fn a_change_to_a_watched_file_is_seen() {
        use base64::Engine;
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let envrc = dir.path().join(".envrc");
        std::fs::write(&envrc, "use flake").unwrap();
        let secs = |p: &std::path::Path| std::fs::metadata(p).unwrap().modified().unwrap().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
        let missing = dir.path().join(".env");
        let list = json!([
            { "path": envrc.to_str().unwrap(), "modtime": secs(&envrc), "exists": true },
            { "path": missing.to_str().unwrap(), "modtime": 0, "exists": false }
        ]);
        let mut z = flate2::write::ZlibEncoder::new(vec![], flate2::Compression::default());
        z.write_all(list.to_string().as_bytes()).unwrap();
        let raw = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(z.finish().unwrap());
        let watches = direnv_watches(&BTreeMap::from([("DIRENV_WATCHES".to_string(), raw)]));
        assert_eq!(watches.len(), 2);
        assert!(!env_changed(&watches));
        std::fs::write(&missing, "A=1").unwrap();
        assert!(env_changed(&watches), "a file that didn't exist now does");
        std::fs::remove_file(&missing).unwrap();
        let f = std::fs::File::options().write(true).open(&envrc).unwrap();
        f.set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(secs(&envrc) + 5)).unwrap();
        assert!(env_changed(&watches), "edited");
    }

    #[test]
    fn a_reason_nobody_asked_for_goes_out_when_the_turn_ends() {
        let (app, _d) = app();
        let app = App::shared(app);
        let id = session(&app);
        let conn = fake_conn(&app, &id);
        conn.turn.lock().unwrap().deny_reason = Some(("too risky".into(), false));
        let after = finish_prompt(&app, &conn, 1, Ok(json!({ "stopReason": "end_turn" })), "2026-10-05T10:00:00");
        assert!(matches!(after, Some(After::Send(q)) if q.text == "too risky"));
    }

    #[test]
    fn an_ended_turn_counts_for_the_badge_until_you_look() {
        let (app, _d) = app();
        let app = App::shared(app);
        let id = session(&app);
        let conn = fake_conn(&app, &id);
        assert_eq!(crate::agents::attention(&app), 0);

        finish_prompt(&app, &conn, 1, Ok(json!({ "stopReason": "end_turn" })), "2026-10-05T10:00:00");
        assert_eq!(crate::agents::attention(&app), 1);

        crate::agents::call(&app, "seen", &json!({ "session": id })).unwrap();
        assert_eq!(crate::agents::attention(&app), 0);
    }

    #[test]
    fn send_now_releases_a_queued_message_when_nothing_is_running() {
        let (app, _d) = app();
        let app = App::shared(app);
        let id = session(&app);
        let conn = fake_conn(&app, &id);
        app.agents.acp.conns.lock().unwrap_or_else(|e| e.into_inner()).insert(id.clone(), conn.clone());
        send(&app, &id, "why not", vec![]).unwrap();
        conn.turn.lock().unwrap().prompts.clear();

        send_queued(&app, &id).unwrap();

        assert!(app.agents.store.lock().sessions[&id].queued.is_empty());
        assert!(rows(&app, &session_thread(&id)).iter().any(|r| r.kind == Kind::You && r.text == "why not"));
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
    fn questions_read_as_choices() {
        // pi-acp: the questionnaire's numbered "label - meaning" options.
        let pi = json!({ "type": "object", "properties": { "value": { "type": "string", "oneOf": [
            { "const": "1. Postgres - the one we run", "title": "1. Postgres - the one we run" },
            { "const": "2. SQLite", "title": "2. SQLite" }
        ] } }, "required": ["value"] });
        let (field, c) = one_choice(&pi).unwrap();
        assert_eq!(field, "value");
        assert_eq!((c[0].key.as_str(), c[0].title.as_str(), c[0].detail.as_str()), ("1", "Postgres", "the one we run"));
        assert_eq!((c[1].title.as_str(), c[1].value.as_str()), ("SQLite", "2. SQLite"), "answers with the agent's own value");
        assert_eq!(text_field(&pi), None);
        // Claude Code: a choice and an optional "Other", which a written reply fills.
        let claude = json!({ "type": "object", "properties": {
            "question_0": { "type": "string", "oneOf": [{ "const": "A", "title": "A", "description": "first" }] },
            "question_0_custom": { "type": "string", "title": "Other" }
        } });
        assert_eq!(one_choice(&claude).unwrap().1[0].detail, "first");
        assert_eq!(text_field(&claude).as_deref(), Some("question_0_custom"));
        // Two questions, or a multi-select: the card shows a form instead.
        let two = json!({ "type": "object", "properties": { "a": { "type": "string", "enum": ["x"] }, "b": { "type": "string", "enum": ["y"] } } });
        assert!(one_choice(&two).is_none());
        let many = json!({ "type": "object", "properties": { "a": { "type": "array", "items": { "anyOf": [{ "const": "x" }] } } } });
        assert!(one_choice(&many).is_none());
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
        for looser in ["auto", "bypassPermissions", "dontAsk", "plan"] {
            let started = json!({ "modes": { "currentModeId": looser, "availableModes": [{ "id": "default" }, { "id": "acceptEdits" }, { "id": looser }] } });
            assert_eq!(accept_edits_mode(&started), None, "a session that starts in {looser} stays there");
        }
    }

    #[test]
    fn model_from_session_new() {
        let claude = json!({ "configOptions": [{ "id": "model", "currentValue": "claude-sonnet-4-6" }] });
        assert_eq!(model_of(&claude).as_deref(), Some("sonnet"));
        let pi = json!({ "models": { "currentModelId": "anthropic/claude-haiku-4-5" } });
        assert_eq!(model_of(&pi).as_deref(), Some("haiku"));
    }
}

//! The agent host (`app/agentd`): local agents run there, not in the app, so
//! they keep going through a quit, a crash or a dev rebuild, approvals
//! included. This side finds the daemon binary, runs it from a copy in the
//! data directory (so a rebuild never touches the running one), starts it
//! when needed, and keeps one control connection and one subscription open.
//! Events go to `acp::on_event`; the store keeps how far it has read per
//! agent, so a reconnect asks only for what it missed.

use crate::error::{msg, Result};
use crate::state::App;
use agentd::client::{self, Control};
use agentd::proto::{self, HostInfo, Reply, Request};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

const BIN: &str = "alethiometer-agentd";

#[derive(Default)]
pub struct Daemon {
    ctl: Mutex<Option<Arc<Control>>>,
    status: Mutex<Status>,
    /// Stop following (tests that hand over to a second App).
    pub stop: AtomicBool,
    kick: AtomicBool,
    /// Serializes starting the daemon.
    starting: Mutex<()>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Status {
    /// starting | running | missing | error | unsupported
    pub state: String,
    pub version: String,
    pub pid: u32,
    pub error: String,
    /// A different build of the daemon is available than the one running;
    /// it takes over at the next restart (Restart host, or idle exit).
    pub stale: bool,
    pub binary: String,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Daemon {
    fn set(&self, f: impl FnOnce(&mut Status)) {
        f(&mut lock(&self.status));
    }
}

pub fn status(app: &App) -> Status {
    lock(&app.agents.daemon.status).clone()
}

/// Where a daemon build can come from, best first: `ALETHIOMETER_AGENTD`,
/// next to the app (the bundled copy, `npm run build:mac`), then the repo
/// this app was built from: `app/agentd/target/{debug,release}`, the newer.
pub fn sources() -> Vec<PathBuf> {
    let mut out = vec![];
    if let Some(p) = std::env::var_os("ALETHIOMETER_AGENTD") {
        out.push(PathBuf::from(p));
    }
    if let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)) {
        out.push(dir.join(BIN));
    }
    {
        let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("../agentd/target");
        let mut dev: Vec<PathBuf> = ["debug", "release"].iter().map(|p| base.join(p).join(BIN)).filter(|p| p.is_file()).collect();
        dev.sort_by_key(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok());
        out.extend(dev.into_iter().rev());
    }
    out.into_iter().filter(|p| p.is_file()).collect()
}

/// The copy the app runs: `<data dir>/bin/alethiometer-agentd`.
pub fn installed(app: &App) -> PathBuf {
    app.data_dir.join("bin").join(BIN)
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (std::fs::read(a), std::fs::read(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    }
}

#[derive(Debug, PartialEq)]
enum Installed {
    Same,
    /// A different build was copied in.
    Updated,
    /// A different build waits: agents are running on the current one.
    Waiting,
}

/// `ALETHIOMETER_AGENTD_KEEP=1`: use whatever agent host answers on the
/// socket and never install, swap or start one (tests that run their own).
/// The crate's own unit tests always keep: a stray write or kill there would
/// otherwise start a host in the test's temp dir that nothing stops.
fn keep() -> bool {
    cfg!(test) || std::env::var_os("ALETHIOMETER_AGENTD_KEEP").is_some_and(|v| v == "1")
}

/// Copy the best build into place unless a daemon with agents running is
/// using it.
fn install(app: &App, running_agents: bool) -> Result<(PathBuf, Installed)> {
    let dest = installed(app);
    if keep() {
        return Ok((dest, Installed::Same));
    }
    let Some(src) = sources().into_iter().next() else {
        if dest.is_file() {
            return Ok((dest, Installed::Same));
        }
        return Err(msg("The agent host isn't built: run `npm run agentd` in app/ (see docs/agents.md)"));
    };
    if dest.is_file() && same_file(&src, &dest) {
        return Ok((dest, Installed::Same));
    }
    if dest.is_file() && running_agents {
        return Ok((dest, Installed::Waiting));
    }
    std::fs::create_dir_all(dest.parent().unwrap_or(Path::new(".")))?;
    let tmp = dest.with_extension("new");
    std::fs::copy(&src, &tmp)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))?;
    }
    // A rename leaves a running daemon on its old file.
    std::fs::rename(&tmp, &dest)?;
    Ok((dest, Installed::Updated))
}

fn socket(app: &App) -> PathBuf {
    proto::socket_path(&app.data_dir)
}

/// Start the daemon detached (its own process group, output to its log) and
/// wait for its socket.
fn launch(app: &App, bin: &Path) -> Result<()> {
    let state = proto::state_dir(&app.data_dir);
    std::fs::create_dir_all(&state)?;
    let log = std::fs::OpenOptions::new().create(true).append(true).open(state.join("agentd.log"))?;
    let mut c = std::process::Command::new(bin);
    c.arg("--dir").arg(&app.data_dir).stdin(std::process::Stdio::null()).stdout(log.try_clone()?).stderr(log);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Out of the app's group, so a dev rebuild or a terminal closing doesn't take it down.
        c.process_group(0);
    }
    let mut child = c.spawn().map_err(|e| msg(format!("couldn't start the agent host ({}): {e}", bin.display())))?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    for _ in 0..100 {
        if Control::connect(&socket(app)).is_ok() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Err(msg(format!("the agent host didn't come up; see {}", state.join("agentd.log").display())))
}

/// A control connection, starting the daemon if nothing answers.
fn control(app: &App) -> Result<Arc<Control>> {
    if let Some(c) = lock(&app.agents.daemon.ctl).clone() {
        return Ok(c);
    }
    let _g = lock(&app.agents.daemon.starting);
    if let Some(c) = lock(&app.agents.daemon.ctl).clone() {
        return Ok(c);
    }
    if !cfg!(unix) {
        app.agents.daemon.set(|s| {
            s.state = "unsupported".into();
            s.error = "Local agents need the agent host, which isn't available on this platform yet".into();
        });
        return Err(msg("Local agents aren't available on this platform yet"));
    }
    let c = match Control::connect(&socket(app)) {
        Ok(c) => c,
        Err(e) if keep() => return Err(msg(format!("no agent host on the socket (ALETHIOMETER_AGENTD_KEEP): {e}"))),
        Err(_) => {
            app.agents.daemon.set(|s| s.state = "starting".into());
            let (bin, _) = install(app, false).inspect_err(|e| {
                let e = e.to_string();
                app.agents.daemon.set(|s| {
                    s.state = "missing".into();
                    s.error = e;
                });
            })?;
            launch(app, &bin).inspect_err(|e| {
                let e = e.to_string();
                app.agents.daemon.set(|s| {
                    s.state = "error".into();
                    s.error = e;
                });
            })?;
            Control::connect(&socket(app)).map_err(|e| msg(format!("couldn't reach the agent host: {e}")))?
        }
    };
    let mut c = Arc::new(c);
    // A newer build and nothing running on the old one: swap it in now.
    let running = list_with(&c).map(|h| h.iter().any(|h| h.alive)).unwrap_or(true);
    if !running && install(app, false).map(|(_, i)| i == Installed::Updated).unwrap_or(false) {
        let _ = c.call(&Request::Shutdown { force: false });
        for _ in 0..60 {
            if Control::connect(&socket(app)).is_err() {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        launch(app, &installed(app))?;
        c = Arc::new(Control::connect(&socket(app)).map_err(|e| msg(format!("couldn't reach the agent host: {e}")))?);
    }
    match c.call(&Request::Hello { protocol: proto::PROTOCOL })? {
        Reply::Hello { protocol, version, pid, .. } => {
            let ok = protocol == proto::PROTOCOL;
            let stale = install(app, running).map(|(_, i)| i == Installed::Waiting).unwrap_or(false) || !ok;
            app.agents.daemon.set(|s| {
                s.state = if ok { "running" } else { "error" }.into();
                s.version = version.clone();
                s.pid = pid;
                s.stale = stale;
                s.binary = installed(app).display().to_string();
                s.error = if ok { String::new() } else { format!("The agent host speaks protocol {protocol}; this build needs {}. Restart it.", proto::PROTOCOL) };
            });
        }
        r => return Err(msg(format!("unexpected answer from the agent host: {r:?}"))),
    }
    *lock(&app.agents.daemon.ctl) = Some(c.clone());
    Ok(c)
}

fn list_with(c: &Control) -> Result<Vec<HostInfo>> {
    match c.ok(&Request::List)? {
        Reply::Hosts { hosts } => Ok(hosts),
        r => Err(msg(format!("unexpected answer from the agent host: {r:?}"))),
    }
}

/// One request; a broken connection is reopened (and the daemon started) once.
pub fn call(app: &App, req: &Request) -> Result<Reply> {
    for attempt in 0..2 {
        let c = control(app)?;
        match c.call(req) {
            Ok(Reply::Error { error }) => return Err(msg(error)),
            Ok(r) => return Ok(r),
            Err(e) if attempt == 0 => {
                eprintln!("agents: agent host connection lost ({e}); reconnecting");
                lock(&app.agents.daemon.ctl).take();
            }
            Err(e) => return Err(msg(format!("the agent host isn't answering: {e}"))),
        }
    }
    unreachable!()
}

pub fn list(app: &App) -> Result<Vec<HostInfo>> {
    match call(app, &Request::List)? {
        Reply::Hosts { hosts } => Ok(hosts),
        r => Err(msg(format!("unexpected answer from the agent host: {r:?}"))),
    }
}

/// Wake the follower now (a session was just started).
pub fn kick(app: &App) {
    app.agents.daemon.kick.store(true, Ordering::SeqCst);
}

fn pause(app: &App, d: Duration) {
    let until = std::time::Instant::now() + d;
    while std::time::Instant::now() < until && !app.agents.daemon.kick.swap(false, Ordering::SeqCst) && !app.agents.daemon.stop.load(Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Follow the daemon: restore what's running, then hand every event to
/// `acp::on_event`, reconnecting (and restarting the daemon) when it goes.
/// With no local sessions and no daemon up, it waits rather than start one.
pub fn start(app: Arc<App>) {
    std::thread::Builder::new()
        .name("agentd-follow".into())
        .spawn(move || loop {
            if app.agents.daemon.stop.load(Ordering::SeqCst) {
                return;
            }
            let up = cfg!(unix) && Control::connect(&socket(&app)).is_ok();
            if !up && !super::acp::any_local(&app) {
                app.agents.daemon.set(|s| {
                    if s.state != "missing" {
                        s.state = "idle".into();
                    }
                });
                pause(&app, Duration::from_secs(2));
                continue;
            }
            match list(&app) {
                Ok(hosts) => {
                    super::acp::restore(&app, &hosts);
                    let from = app.agents.store.host_seqs();
                    let a = app.clone();
                    let r = client::subscribe(&socket(&app), from, |ev| {
                        if a.agents.daemon.stop.load(Ordering::SeqCst) {
                            return false;
                        }
                        super::acp::on_event(&a, ev);
                        true
                    });
                    if app.agents.daemon.stop.load(Ordering::SeqCst) {
                        return;
                    }
                    lock(&app.agents.daemon.ctl).take();
                    let why = match r {
                        Ok(()) => "the agent host exited".to_string(),
                        Err(e) => format!("lost the agent host: {e}"),
                    };
                    app.agents.daemon.set(|s| {
                        s.state = "error".into();
                        s.error = why;
                    });
                    super::acp::host_gone(&app);
                }
                Err(e) => {
                    let e = e.to_string();
                    app.agents.daemon.set(|s| {
                        if s.state != "missing" {
                            s.state = "error".into();
                        }
                        s.error = e;
                    });
                }
            }
            pause(&app, Duration::from_secs(2));
        })
        .ok();
}

/// Restart host: stop every local agent, then come back on the newest build.
pub fn restart(app: &App) -> Result<()> {
    let _ = call(app, &Request::Shutdown { force: true });
    lock(&app.agents.daemon.ctl).take();
    for _ in 0..60 {
        if Control::connect(&socket(app)).is_err() {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    control(app).map(|_| ())
}

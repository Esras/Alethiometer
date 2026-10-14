//! The daemon: a Unix socket (`net`), one thread per connection, three per agent
//! (stdout, stderr, wait). Every line an agent writes becomes a numbered
//! event, appended to `<state>/logs/<id>.jsonl` and sent to subscribers.
//! A subscriber first gets what it missed from the log, then live events;
//! the per-agent log lock keeps the two in order.

use crate::net::{UnixListener, UnixStream};
use crate::proto::{self, Event, HostInfo, Kind, Reply, Request};
use crate::tree::{self, Tree};
use std::collections::{BTreeMap, HashMap};
use std::fs::{File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex, MutexGuard};
use std::time::Duration;

pub struct Options {
    /// The app's data directory; the socket and state live under it.
    pub dir: PathBuf,
    /// Exit after this long with no agents running and no app connected.
    pub idle: Duration,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

struct Log {
    seq: u64,
    path: PathBuf,
    file: Option<File>,
}

struct Host {
    id: String,
    pid: u32,
    /// None for an agent from an earlier run of the daemon (`recover`).
    tree: Option<Arc<Tree>>,
    started_ms: u64,
    cmd: String,
    stdin: Mutex<Option<ChildStdin>>,
    log: Mutex<Log>,
    alive: AtomicBool,
    exit: Mutex<Option<i32>>,
}

struct Sub {
    conn: u64,
    tx: mpsc::Sender<String>,
    /// Last event sent per agent; u64::MAX while its backlog is being sent.
    sent: Mutex<HashMap<String, u64>>,
}

struct Daemon {
    state: PathBuf,
    socket: PathBuf,
    started_ms: u64,
    hosts: Mutex<BTreeMap<String, Arc<Host>>>,
    subs: Mutex<Vec<Arc<Sub>>>,
    conns: AtomicUsize,
    next_conn: AtomicU64,
    last_busy: AtomicU64,
    stop: AtomicBool,
}

/// The last event number in a log, and whether its agent's exit is recorded.
fn tail(path: &Path) -> (u64, bool) {
    let Ok(f) = File::open(path) else { return (0, false) };
    let mut last = (0, false);
    for l in BufReader::new(f).lines().map_while(Result::ok) {
        if let Ok(Reply::Ev(e)) = serde_json::from_str::<Reply>(&l) {
            last = (e.seq, e.kind == Kind::Exit);
        }
    }
    last
}

impl Daemon {
    fn emit(&self, h: &Host, kind: Kind, line: Option<String>, code: Option<i32>) {
        let mut log = lock(&h.log);
        log.seq += 1;
        let ev = Reply::Ev(Event {
            id: h.id.clone(),
            seq: log.seq,
            at_ms: proto::now_ms(),
            kind,
            line,
            code,
        });
        let Ok(s) = serde_json::to_string(&ev) else { return };
        if log.file.is_none() {
            log.file = OpenOptions::new().create(true).append(true).open(&log.path).ok();
        }
        if let Some(f) = log.file.as_mut() {
            let _ = f.write_all(format!("{s}\n").as_bytes());
        }
        let seq = log.seq;
        let mut subs = lock(&self.subs);
        subs.retain(|sub| {
            let mut sent = lock(&sub.sent);
            let last = sent.get(&h.id).copied().unwrap_or(0);
            if last != u64::MAX && seq > last {
                sent.insert(h.id.clone(), seq);
                return sub.tx.send(s.clone()).is_ok();
            }
            true
        });
    }

    fn busy(&self) {
        self.last_busy.store(proto::now_ms(), Ordering::SeqCst);
    }

    fn info(&self) -> Vec<HostInfo> {
        lock(&self.hosts)
            .values()
            .map(|h| HostInfo {
                id: h.id.clone(),
                pid: h.pid,
                alive: h.alive.load(Ordering::SeqCst),
                exit_code: *lock(&h.exit),
                seq: lock(&h.log).seq,
                started_ms: h.started_ms,
                cmd: h.cmd.clone(),
            })
            .collect()
    }

    fn spawn(
        self: &Arc<Self>,
        id: &str,
        cmd: &str,
        args: &[String],
        env: &BTreeMap<String, String>,
        cwd: &str,
        after: u64,
    ) -> Result<u32, String> {
        if !proto::valid_id(id) {
            return Err(format!("bad id '{id}'"));
        }
        if lock(&self.hosts)
            .get(id)
            .is_some_and(|h| h.alive.load(Ordering::SeqCst))
        {
            return Err(format!("{id} is already running"));
        }
        let mut c = Command::new(cmd);
        c.args(args)
            .envs(env)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if !cwd.is_empty() {
            c.current_dir(cwd);
        }
        // Its own process group (job object on Windows), so Kill reaches what it starts (npx → node).
        let child = tree::spawn(c).map_err(|e| format!("couldn't start {cmd}: {e}"))?;
        let t = Arc::new(child.tree);
        let pid = t.pid();
        let path = self.state.join("logs").join(format!("{id}.jsonl"));
        // A restarted agent keeps its numbering, so the app's place in the log still holds.
        let seq = tail(&path).0.max(after);
        let h = Arc::new(Host {
            id: id.to_owned(),
            pid,
            tree: Some(t.clone()),
            started_ms: proto::now_ms(),
            cmd: std::iter::once(cmd)
                .chain(args.iter().map(String::as_str))
                .collect::<Vec<_>>()
                .join(" "),
            stdin: Mutex::new(child.stdin),
            log: Mutex::new(Log { seq, path, file: None }),
            alive: AtomicBool::new(true),
            exit: Mutex::new(None),
        });
        lock(&self.hosts).insert(id.to_owned(), h.clone());
        let (out, err) = (child.stdout, child.stderr);
        let (d, hh) = (self.clone(), h.clone());
        let reader = std::thread::spawn(move || {
            if let Some(o) = out {
                for l in BufReader::new(o).lines().map_while(Result::ok) {
                    d.emit(&hh, Kind::Out, Some(l), None);
                }
            }
        });
        let (d, hh) = (self.clone(), h.clone());
        let errs = std::thread::spawn(move || {
            if let Some(e) = err {
                for l in BufReader::new(e).lines().map_while(Result::ok) {
                    d.emit(&hh, Kind::Err, Some(l), None);
                }
            }
        });
        let d = self.clone();
        std::thread::spawn(move || {
            let _ = reader.join();
            let code = t.wait();
            let _ = errs.join();
            h.alive.store(false, Ordering::SeqCst);
            lock(&h.stdin).take();
            *lock(&h.exit) = code;
            d.emit(&h, Kind::Exit, None, code);
            d.busy();
        });
        self.busy();
        Ok(pid)
    }

    fn write(&self, id: &str, line: &str) -> Result<(), String> {
        let h = lock(&self.hosts)
            .get(id)
            .cloned()
            .ok_or_else(|| format!("no agent {id}"))?;
        let mut g = lock(&h.stdin);
        let w = g.as_mut().ok_or_else(|| format!("{id} has exited"))?;
        w.write_all(format!("{line}\n").as_bytes())
            .and_then(|_| w.flush())
            .map_err(|e| format!("couldn't write to {id}: {e}"))
    }

    fn kill(&self, id: &str) -> Result<(), String> {
        let h = lock(&self.hosts)
            .get(id)
            .cloned()
            .ok_or_else(|| format!("no agent {id}"))?;
        if !h.alive.load(Ordering::SeqCst) {
            return Ok(());
        }
        // Closing stdin ends both adapters; TERM (Unix) and, 3 s on, KILL
        // (a job's termination on Windows) catch one that hangs.
        lock(&h.stdin).take();
        let Some(t) = h.tree.clone() else { return Ok(()) };
        t.terminate();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(3));
            if h.alive.load(Ordering::SeqCst) {
                t.kill();
            }
        });
        Ok(())
    }

    fn forget(&self, id: &str) -> Result<(), String> {
        let mut hosts = lock(&self.hosts);
        if hosts.get(id).is_some_and(|h| h.alive.load(Ordering::SeqCst)) {
            return Err(format!("{id} is still running"));
        }
        if let Some(h) = hosts.remove(id) {
            let _ = std::fs::remove_file(&lock(&h.log).path);
        }
        Ok(())
    }

    fn subscribe(&self, conn: u64, tx: &mpsc::Sender<String>, from: &BTreeMap<String, u64>) {
        let hosts: Vec<Arc<Host>> = lock(&self.hosts).values().cloned().collect();
        let sub = Arc::new(Sub {
            conn,
            tx: tx.clone(),
            sent: Mutex::new(hosts.iter().map(|h| (h.id.clone(), u64::MAX)).collect()),
        });
        lock(&self.subs).push(sub.clone());
        let _ = tx.send(serde_json::to_string(&Reply::Ok).unwrap_or_default());
        for h in hosts {
            let log = lock(&h.log);
            let after = from.get(&h.id).copied().unwrap_or(0);
            if after < log.seq {
                if let Ok(f) = File::open(&log.path) {
                    for l in BufReader::new(f).lines().map_while(Result::ok) {
                        if let Ok(Reply::Ev(e)) = serde_json::from_str::<Reply>(&l) {
                            if e.seq > after {
                                let _ = tx.send(l);
                            }
                        }
                    }
                }
            }
            lock(&sub.sent).insert(h.id.clone(), log.seq);
        }
    }

    fn handle(self: &Arc<Self>, conn: u64, req: Request, tx: &mpsc::Sender<String>) -> Option<Reply> {
        self.busy();
        Some(match req {
            Request::Hello { .. } => Reply::Hello {
                protocol: proto::PROTOCOL,
                version: proto::VERSION.into(),
                pid: std::process::id(),
                started_ms: self.started_ms,
            },
            Request::Spawn {
                id,
                cmd,
                args,
                env,
                cwd,
                after,
            } => match self.spawn(&id, &cmd, &args, &env, &cwd, after) {
                Ok(pid) => Reply::Spawned { id, pid },
                Err(error) => Reply::Error { error },
            },
            Request::Write { id, line } => self
                .write(&id, &line)
                .map_or_else(|error| Reply::Error { error }, |_| Reply::Ok),
            Request::Kill { id } => self
                .kill(&id)
                .map_or_else(|error| Reply::Error { error }, |_| Reply::Ok),
            Request::Forget { id } => self
                .forget(&id)
                .map_or_else(|error| Reply::Error { error }, |_| Reply::Ok),
            Request::List => Reply::Hosts { hosts: self.info() },
            Request::Subscribe { from } => {
                self.subscribe(conn, tx, &from);
                return None;
            }
            Request::Shutdown { force } => {
                let running: Vec<String> = self.info().into_iter().filter(|h| h.alive).map(|h| h.id).collect();
                if !running.is_empty() && !force {
                    return Some(Reply::Error {
                        error: format!("{} agent(s) still running", running.len()),
                    });
                }
                for id in running {
                    let _ = self.kill(&id);
                }
                // Stop just after the reply is out: the process ends with `run`.
                let d = self.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_millis(200));
                    d.stop.store(true, Ordering::SeqCst);
                    // Wake the accept loop.
                    let _ = UnixStream::connect(&d.socket);
                });
                Reply::Ok
            }
        })
    }

    fn serve(self: &Arc<Self>, stream: UnixStream) {
        self.conns.fetch_add(1, Ordering::SeqCst);
        let conn = self.next_conn.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = mpsc::channel::<String>();
        let Ok(mut w) = stream.try_clone() else {
            self.conns.fetch_sub(1, Ordering::SeqCst);
            return;
        };
        let writer = std::thread::spawn(move || {
            for s in rx {
                if w.write_all(format!("{s}\n").as_bytes()).is_err() {
                    break;
                }
            }
        });
        for l in BufReader::new(stream).lines().map_while(Result::ok) {
            let reply = match serde_json::from_str::<Request>(&l) {
                Ok(req) => self.handle(conn, req, &tx),
                Err(e) => Some(Reply::Error {
                    error: format!("bad request: {e}"),
                }),
            };
            if let Some(r) = reply {
                let _ = tx.send(serde_json::to_string(&r).unwrap_or_default());
            }
        }
        // A subscriber stays registered until a send fails; drop ours now.
        lock(&self.subs).retain(|s| s.conn != conn);
        drop(tx);
        let _ = writer.join();
        self.conns.fetch_sub(1, Ordering::SeqCst);
        self.busy();
    }
}

/// Agents from an earlier run of the daemon are gone; their logs stay so the
/// app can read what it missed, ending in an exit it may never have seen.
fn recover(d: &Arc<Daemon>) {
    let Ok(dir) = std::fs::read_dir(d.state.join("logs")) else {
        return;
    };
    for e in dir.filter_map(Result::ok) {
        let p = e.path();
        let Some(id) = p
            .file_stem()
            .and_then(|s| s.to_str())
            .filter(|s| p.extension().is_some_and(|x| x == "jsonl") && proto::valid_id(s))
        else {
            continue;
        };
        let (seq, exited) = tail(&p);
        let h = Arc::new(Host {
            id: id.to_owned(),
            pid: 0,
            tree: None,
            started_ms: 0,
            cmd: String::new(),
            stdin: Mutex::new(None),
            log: Mutex::new(Log {
                seq,
                path: p.clone(),
                file: None,
            }),
            alive: AtomicBool::new(false),
            exit: Mutex::new(None),
        });
        if !exited {
            d.emit(
                &h,
                Kind::Exit,
                Some("the agent host stopped while this agent ran".into()),
                None,
            );
        }
        lock(&d.hosts).insert(id.to_owned(), h);
    }
}

/// Bind and serve until Shutdown or idle. Fails with AddrInUse when another
/// daemon already serves this directory.
pub fn run(opts: Options) -> io::Result<()> {
    let state = proto::state_dir(&opts.dir);
    std::fs::create_dir_all(state.join("logs"))?;
    let socket = proto::socket_path(&opts.dir);
    if socket.exists() {
        if UnixStream::connect(&socket).is_ok() {
            return Err(io::Error::new(
                io::ErrorKind::AddrInUse,
                format!("an agent host already serves {}", opts.dir.display()),
            ));
        }
        let _ = std::fs::remove_file(&socket);
    }
    let listener = UnixListener::bind(&socket)?;
    // Only this user. Windows has no file modes: the socket file inherits
    // the data directory's ACL (in the user's profile: the user, SYSTEM and
    // administrators).
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600));
    }
    let _ = std::fs::write(state.join("pid"), std::process::id().to_string());
    let d = Arc::new(Daemon {
        state,
        socket: socket.clone(),
        started_ms: proto::now_ms(),
        hosts: Mutex::new(BTreeMap::new()),
        subs: Mutex::new(vec![]),
        conns: AtomicUsize::new(0),
        next_conn: AtomicU64::new(1),
        last_busy: AtomicU64::new(proto::now_ms()),
        stop: AtomicBool::new(false),
    });
    recover(&d);
    {
        let d = d.clone();
        let idle = opts.idle;
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_secs(5).min(idle));
            if d.stop.load(Ordering::SeqCst) {
                return;
            }
            let running = lock(&d.hosts).values().any(|h| h.alive.load(Ordering::SeqCst));
            let quiet = proto::now_ms().saturating_sub(d.last_busy.load(Ordering::SeqCst)) >= idle.as_millis() as u64;
            if !running && d.conns.load(Ordering::SeqCst) == 0 && quiet {
                eprintln!("agentd: idle, exiting");
                d.stop.store(true, Ordering::SeqCst);
                let _ = UnixStream::connect(&d.socket);
                return;
            }
        });
    }
    eprintln!(
        "agentd {} (protocol {}) on {}",
        proto::VERSION,
        proto::PROTOCOL,
        socket.display()
    );
    for s in listener.incoming() {
        if d.stop.load(Ordering::SeqCst) {
            break;
        }
        if let Ok(s) = s {
            let d = d.clone();
            std::thread::spawn(move || d.serve(s));
        }
    }
    let _ = std::fs::remove_file(&socket);
    let _ = std::fs::remove_file(d.state.join("pid"));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::{subscribe, Control};

    fn start(dir: &Path) -> std::thread::JoinHandle<io::Result<()>> {
        let o = Options {
            dir: dir.to_owned(),
            idle: Duration::from_secs(600),
        };
        let h = std::thread::spawn(move || run(o));
        let sock = proto::socket_path(dir);
        for _ in 0..100 {
            if UnixStream::connect(&sock).is_ok() {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        h
    }

    fn events(dir: &Path, from: BTreeMap<String, u64>, n: usize) -> Vec<Event> {
        let mut out = vec![];
        subscribe(&proto::socket_path(dir), from, |e| {
            out.push(e);
            out.len() < n
        })
        .unwrap();
        out
    }

    #[cfg(unix)]
    #[test]
    fn echoes_numbers_and_replays() {
        let t = tempfile::tempdir().unwrap();
        let dir = t.path();
        let srv = start(dir);
        let c = Control::connect(&proto::socket_path(dir)).unwrap();
        assert!(matches!(
            c.call(&Request::Hello { protocol: 1 }).unwrap(),
            Reply::Hello { protocol: 1, .. }
        ));
        let r = c
            .ok(&Request::Spawn {
                id: "acp-1".into(),
                cmd: "cat".into(),
                args: vec![],
                env: BTreeMap::new(),
                cwd: String::new(),
                after: 0,
            })
            .unwrap();
        assert!(matches!(r, Reply::Spawned { .. }));
        c.ok(&Request::Write {
            id: "acp-1".into(),
            line: "one".into(),
        })
        .unwrap();
        c.ok(&Request::Write {
            id: "acp-1".into(),
            line: "two".into(),
        })
        .unwrap();
        let got = events(dir, BTreeMap::new(), 2);
        assert_eq!(
            got.iter().map(|e| (e.seq, e.line.clone().unwrap())).collect::<Vec<_>>(),
            vec![(1, "one".into()), (2, "two".into())]
        );
        // A reconnect asks only for what it hasn't seen.
        c.ok(&Request::Write {
            id: "acp-1".into(),
            line: "three".into(),
        })
        .unwrap();
        let got = events(dir, BTreeMap::from([("acp-1".to_string(), 2)]), 1);
        assert_eq!((got[0].seq, got[0].line.as_deref()), (3, Some("three")));
        c.ok(&Request::Kill { id: "acp-1".into() }).unwrap();
        let got = events(dir, BTreeMap::from([("acp-1".to_string(), 3)]), 1);
        assert_eq!(got[0].kind, Kind::Exit);
        let Reply::Hosts { hosts } = c.call(&Request::List).unwrap() else {
            panic!()
        };
        assert!(!hosts[0].alive);
        assert!(c.ok(&Request::Shutdown { force: false }).is_ok());
        srv.join().unwrap().unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_restarted_daemon_closes_out_what_ran() {
        let t = tempfile::tempdir().unwrap();
        let dir = t.path();
        let logs = proto::state_dir(dir).join("logs");
        std::fs::create_dir_all(&logs).unwrap();
        let ev = Reply::Ev(Event {
            id: "acp-9".into(),
            seq: 4,
            at_ms: 1,
            kind: Kind::Out,
            line: Some("x".into()),
            code: None,
        });
        std::fs::write(
            logs.join("acp-9.jsonl"),
            format!("{}\n", serde_json::to_string(&ev).unwrap()),
        )
        .unwrap();
        let srv = start(dir);
        let got = events(dir, BTreeMap::from([("acp-9".to_string(), 4)]), 1);
        assert_eq!((got[0].seq, got[0].kind), (5, Kind::Exit));
        // Spawning the same id again keeps counting.
        let c = Control::connect(&proto::socket_path(dir)).unwrap();
        c.ok(&Request::Spawn {
            id: "acp-9".into(),
            cmd: "cat".into(),
            args: vec![],
            env: BTreeMap::new(),
            cwd: String::new(),
            after: 0,
        })
        .unwrap();
        c.ok(&Request::Write {
            id: "acp-9".into(),
            line: "again".into(),
        })
        .unwrap();
        let got = events(dir, BTreeMap::from([("acp-9".to_string(), 5)]), 1);
        assert_eq!((got[0].seq, got[0].line.as_deref()), (6, Some("again")));
        c.ok(&Request::Shutdown { force: true }).unwrap();
        srv.join().unwrap().unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn kill_ends_the_agents_group_and_nothing_else() {
        use crate::tree::signal_group;
        use std::os::unix::process::CommandExt;
        let t = tempfile::tempdir().unwrap();
        let dir = t.path();
        // In a group of its own, like an unrelated process of the user's.
        let mut bystander = Command::new("sleep").arg("30").process_group(0).spawn().unwrap();
        let srv = start(dir);
        let c = Control::connect(&proto::socket_path(dir)).unwrap();
        // Ignores TERM and stdin closing, with a child: only the group KILL ends it.
        let script = "trap '' TERM; sleep 30 & wait";
        c.ok(&Request::Spawn {
            id: "acp-1".into(),
            cmd: "sh".into(),
            args: vec!["-c".into(), script.into()],
            env: BTreeMap::new(),
            cwd: String::new(),
            after: 0,
        })
        .unwrap();
        std::thread::sleep(Duration::from_millis(200));
        c.ok(&Request::Kill { id: "acp-1".into() }).unwrap();
        let got = events(dir, BTreeMap::new(), 1);
        assert_eq!(got[0].kind, Kind::Exit);
        assert!(
            bystander.try_wait().unwrap().is_none(),
            "a process outside the agent's group was signalled"
        );
        assert!(
            !signal_group(0, 0) && !signal_group(1, 0),
            "never the daemon's own group or every process"
        );
        bystander.kill().unwrap();
        let _ = bystander.wait();
        c.ok(&Request::Shutdown { force: true }).unwrap();
        srv.join().unwrap().unwrap();
    }

    /// The Exit event comes only once the agent's stdout closes, so a
    /// grandchild still holding it (the ping here) would keep it from coming.
    #[cfg(windows)]
    #[test]
    fn kill_ends_the_agents_job_and_nothing_else() {
        let t = tempfile::tempdir().unwrap();
        let dir = t.path();
        let mut bystander = Command::new("ping").args(["-n", "30", "127.0.0.1"]).spawn().unwrap();
        let srv = start(dir);
        let c = Control::connect(&proto::socket_path(dir)).unwrap();
        // cmd starts ping and waits; neither cares that stdin closed.
        c.ok(&Request::Spawn {
            id: "acp-1".into(),
            cmd: "cmd".into(),
            args: vec!["/c".into(), "ping -n 30 127.0.0.1".into()],
            env: BTreeMap::new(),
            cwd: String::new(),
            after: 0,
        })
        .unwrap();
        std::thread::sleep(Duration::from_millis(500));
        let asked = std::time::Instant::now();
        c.ok(&Request::Kill { id: "acp-1".into() }).unwrap();
        let got = events(dir, BTreeMap::new(), 1);
        assert_eq!(got[0].kind, Kind::Exit);
        assert!(
            asked.elapsed() < Duration::from_secs(10),
            "ended by the job, not by ping finishing"
        );
        assert!(
            bystander.try_wait().unwrap().is_none(),
            "a process outside the agent's job was ended"
        );
        bystander.kill().unwrap();
        let _ = bystander.wait();
        c.ok(&Request::Shutdown { force: true }).unwrap();
        srv.join().unwrap().unwrap();
    }

    #[test]
    fn one_daemon_per_directory() {
        let t = tempfile::tempdir().unwrap();
        let srv = start(t.path());
        let again = run(Options {
            dir: t.path().to_owned(),
            idle: Duration::from_secs(1),
        });
        assert_eq!(again.unwrap_err().kind(), io::ErrorKind::AddrInUse);
        Control::connect(&proto::socket_path(t.path()))
            .unwrap()
            .ok(&Request::Shutdown { force: true })
            .unwrap();
        srv.join().unwrap().unwrap();
    }
}

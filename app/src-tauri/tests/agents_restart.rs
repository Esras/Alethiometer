//! A local agent outlives the app.
//!
//! - An approval asked in one App is answered in the next one, on the same
//!   agent process and JSON-RPC request (in-process handover).
//! - The app is killed (`kill -9`, the devserver binary as a separate
//!   process) while the agent streams; the agent keeps going, and the next
//!   app shows every chunk exactly once: once after the turn finished while
//!   no app was there, once coming back mid-stream.
//!
//! Runs the agent host in-process and `scripts/fake-acp.mjs` as the agent;
//! skipped when `node` isn't installed.

#![cfg(unix)]

use alethiometer_lib::agents::model::Status;
use alethiometer_lib::settings::{Harness, Settings};
use alethiometer_lib::state::App;
use serde_json::{json, Value};
use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

fn wait<T>(what: &str, secs: u64, mut f: impl FnMut() -> Option<T>) -> T {
    let until = Instant::now() + Duration::from_secs(secs);
    loop {
        if let Some(v) = f() {
            return v;
        }
        assert!(Instant::now() < until, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn open(dir: &Path) -> Arc<App> {
    pin_host();
    let app = App::shared(App::open(dir).unwrap());
    alethiometer_lib::agents::start(app.clone());
    app
}

fn approval(app: &App, session: &str) -> Option<String> {
    let i = app.agents.store.lock();
    i.requests.values().find(|r| r.session_id == session && r.kind == "approval").map(|r| r.id.clone())
}

/// The apps under test use the host this test runs, never one of their own.
fn pin_host() {
    std::env::set_var("ALETHIOMETER_AGENTD_KEEP", "1");
}

fn no_node() -> bool {
    let missing = std::process::Command::new("node").arg("--version").output().is_err();
    if missing {
        eprintln!("skipped: no node");
    }
    missing
}

/// Settings with the fake agent as the only harness, and the agent host running in-process.
fn setup(dir: &Path, env: &[(&str, &str)]) -> std::thread::JoinHandle<std::io::Result<()>> {
    let fake = Path::new(env!("CARGO_MANIFEST_DIR")).join("../scripts/fake-acp.mjs");
    let mut s = Settings { demo_data: false, ..Default::default() };
    s.agents.harnesses = vec![Harness {
        key: "fake".into(),
        label: "Claude Code".into(),
        command: "node".into(),
        args: vec![fake.display().to_string()],
        env: env.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
    }];
    std::fs::write(dir.join("settings.json"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
    // The app finds it on the socket and doesn't start its own.
    let host_dir = dir.to_owned();
    let host = std::thread::spawn(move || agentd::server::run(agentd::server::Options { dir: host_dir, idle: Duration::from_secs(600) }));
    wait("the agent host", 10, || agentd::client::Control::connect(&agentd::proto::socket_path(dir)).ok());
    host
}

fn stop_host(dir: &Path, host: std::thread::JoinHandle<std::io::Result<()>>) {
    let c = agentd::client::Control::connect(&agentd::proto::socket_path(dir)).unwrap();
    c.ok(&agentd::proto::Request::Shutdown { force: true }).unwrap();
    host.join().unwrap().unwrap();
}

#[test]
fn an_agent_waiting_on_you_survives_an_app_restart() {
    if no_node() {
        return;
    }
    let data = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    let dir = data.path();
    let host = setup(dir, &[("FAKE_ACP_FAST", "1")]);

    let first = open(dir);
    let started = alethiometer_lib::agents::call(&first, "start_local", &json!({ "harness": "fake", "cwd": project.path(), "task": "Fix the Settings header" })).unwrap();
    let id = started["id"].as_str().unwrap().to_owned();
    // The fake agent reads, greps, runs a subagent, edits (accept-edits: no ask), then asks to run a shell command.
    let req = wait("the shell command's approval", 30, || approval(&first, &id));

    // Hand over: the first app stops following and writes what it has, as on quit.
    first.agents.daemon.stop.store(true, Ordering::SeqCst);
    first.agents.store.flush(&first.db()).unwrap();
    let second = open(dir);
    let again = wait("the approval to come back after the restart", 20, || approval(&second, &id));
    assert_eq!(again, req, "the same request, still waiting");
    wait("the session to be attached", 10, || second.agents.store.lock().sessions.get(&id).filter(|s| s.live).map(|_| ()));

    alethiometer_lib::agents::call(&second, "respond", &json!({ "request": again, "option": "o" })).unwrap();
    wait("the turn to finish", 30, || {
        let i = second.agents.store.lock();
        let s = i.sessions.get(&id)?;
        (s.status == Status::Idle).then_some(())
    });
    let rows: Vec<Value> = {
        let i = second.agents.store.lock();
        i.threads[&format!("s:{id}")].rows.iter().map(|r| serde_json::to_value(r).unwrap()).collect()
    };
    let text = |kind: &str| rows.iter().filter(|r| r["kind"] == kind).map(|r| r["text"].as_str().unwrap_or("").to_owned()).collect::<Vec<_>>();
    assert!(text("output").iter().any(|t| t.starts_with("Done: Fix the Settings header")), "the turn ended in the second app: {:?}", text("output"));
    assert_eq!(rows.iter().filter(|r| r["kind"] == "you").count(), 1, "nothing replayed twice");
    let tools: Vec<&Value> = rows.iter().filter(|r| r["kind"] == "tool").collect();
    assert!(tools.iter().any(|r| r["tool"]["name"] == "bash" && r["note"].as_str().is_some_and(|n| n.starts_with("allowed once"))), "the approved command is a tool row again");

    stop_host(dir, host);
}

/// The devserver binary as its own process, on a free port.
struct Server {
    child: std::process::Child,
    url: String,
}

impl Server {
    fn start(dir: &Path) -> Server {
        let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let child = std::process::Command::new(env!("CARGO_BIN_EXE_devserver"))
            .env("ALETHIOMETER_DATA_DIR", dir)
            .env("ALETHIOMETER_DEVSERVER_ADDR", format!("127.0.0.1:{port}"))
            .env("ALETHIOMETER_AGENTD_KEEP", "1")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let s = Server { child, url: format!("http://127.0.0.1:{port}/call/") };
        wait("the devserver", 30, || s.call("agents.tree", json!({})).ok());
        s
    }

    fn call(&self, cmd: &str, args: Value) -> Result<Value, String> {
        let mut r = ureq::post(&format!("{}{cmd}", self.url)).send_json(&args).map_err(|e| e.to_string())?;
        r.body_mut().read_json::<Value>().map_err(|e| e.to_string())
    }

    /// No clean shutdown: whatever wasn't flushed is lost, as in a crash.
    fn kill9(mut self) {
        self.child.kill().unwrap();
        let _ = self.child.wait();
    }

    fn output(&self, id: &str) -> String {
        let t = self.call("agents.thread", json!({ "key": format!("s:{id}") })).unwrap();
        t["rows"].as_array().unwrap().iter().filter(|r| r["kind"] == "output").map(|r| r["text"].as_str().unwrap_or("")).collect()
    }

    fn session(&self, id: &str) -> Value {
        let t = self.call("agents.tree", json!({})).unwrap();
        t["sessions"].as_array().unwrap().iter().find(|s| s["id"] == id).cloned().unwrap_or(Value::Null)
    }
}

fn ticks(n: usize) -> String {
    (1..=n).map(|i| format!("t{i} ")).collect()
}

/// Start a streaming turn in one devserver, kill it after `cut` chunks, wait
/// `gap`, start another, and check the transcript.
fn crash_while_streaming(chunks: usize, cut: usize, gap: Duration) {
    let data = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    let dir = data.path();
    let n = chunks.to_string();
    let host = setup(dir, &[("FAKE_ACP_MODE", "stream"), ("FAKE_ACP_CHUNKS", &n), ("FAKE_ACP_DELAY_MS", "40")]);

    let a = Server::start(dir);
    let id = a.call("agents.start_local", json!({ "harness": "fake", "cwd": project.path(), "task": "count" })).unwrap()["id"].as_str().unwrap().to_owned();
    let seen = format!("t{cut} ");
    wait("the first chunks", 30, || a.output(&id).contains(&seen).then_some(()));
    // Let at least one flush (every second) land mid-turn, then crash.
    std::thread::sleep(Duration::from_millis(1100));
    a.kill9();
    std::thread::sleep(gap);

    let b = Server::start(dir);
    wait("the turn to finish", 30, || {
        let s = b.session(&id);
        (s["status"] == "idle" && s["stats"]["turns"] == 1).then_some(())
    });
    assert_eq!(b.output(&id), ticks(chunks), "every chunk exactly once, in order");
    let rows = b.call("agents.thread", json!({ "key": format!("s:{id}") })).unwrap();
    assert_eq!(rows["rows"].as_array().unwrap().iter().filter(|r| r["kind"] == "you").count(), 1);
    assert_eq!(b.session(&id)["live"], true, "still attached to the same agent");
    b.kill9();
    stop_host(dir, host);
}

#[test]
fn a_turn_that_finished_while_the_app_was_dead_shows_whole() {
    if no_node() {
        return;
    }
    // 60 chunks over ~2.4 s; killed early, back after the turn ended.
    crash_while_streaming(60, 5, Duration::from_secs(3));
}

#[test]
fn coming_back_mid_stream_joins_backlog_and_live_cleanly() {
    if no_node() {
        return;
    }
    // 150 chunks over ~6 s; killed early, back while it's still streaming.
    crash_while_streaming(150, 5, Duration::from_millis(300));
}

//! A local agent outlives the app: start a session in one App, let it stop at
//! an approval, hand over to a second App on the same data directory (as a
//! restart or a dev rebuild would), and answer the approval there.
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
    let app = App::shared(App::open(dir).unwrap());
    alethiometer_lib::agents::start(app.clone());
    app
}

fn approval(app: &App, session: &str) -> Option<String> {
    let i = app.agents.store.lock();
    i.requests.values().find(|r| r.session_id == session && r.kind == "approval").map(|r| r.id.clone())
}

#[test]
fn an_agent_waiting_on_you_survives_an_app_restart() {
    if std::process::Command::new("node").arg("--version").output().is_err() {
        eprintln!("skipped: no node");
        return;
    }
    let data = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    let dir = data.path();
    let fake = Path::new(env!("CARGO_MANIFEST_DIR")).join("../scripts/fake-acp.mjs");
    let mut s = Settings { demo_data: false, ..Default::default() };
    s.agents.harnesses = vec![Harness {
        key: "fake".into(),
        label: "Claude Code".into(),
        command: "node".into(),
        args: vec![fake.display().to_string()],
        env: [("FAKE_ACP_FAST".to_string(), "1".to_string())].into(),
    }];
    std::fs::write(dir.join("settings.json"), serde_json::to_string_pretty(&s).unwrap()).unwrap();

    // The agent host, in-process (the app finds it on the socket and doesn't start its own).
    let host_dir = dir.to_owned();
    let host = std::thread::spawn(move || agentd::server::run(agentd::server::Options { dir: host_dir, idle: Duration::from_secs(600) }));
    wait("the agent host", 10, || agentd::client::Control::connect(&agentd::proto::socket_path(dir)).ok());

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

    let c = agentd::client::Control::connect(&agentd::proto::socket_path(dir)).unwrap();
    c.ok(&agentd::proto::Request::Shutdown { force: true }).unwrap();
    host.join().unwrap().unwrap();
}

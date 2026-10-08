//! An agent in an OpenShell sandbox, through `scripts/fake-openshell.mjs`
//! (a sandbox is a directory; `sandbox exec` runs the command here) with
//! `scripts/fake-acp.mjs` as the agent, and the agent host in-process.
//!
//! - A new sandbox is created with the directory uploaded, and the adapter
//!   runs in it through `sandbox exec`, with the harness's sandbox env.
//! - An agent other than Claude Code has its permission requests allowed
//!   with its own allow option; nothing waits on you.
//! - Claude Code is switched to its auto mode, and what it still asks comes to you.
//! - The agent host reattaching while a sandbox is still being made leaves the
//!   new session be (it isn't in the host yet).
//! - An existing sandbox is started (if stopped) and used in the directory
//!   given; nothing is created or uploaded.
//!
//! Skipped when `node` isn't installed.

#![cfg(unix)]

use alethiometer_lib::agents::model::{Kind, Status};
use alethiometer_lib::settings::{Harness, HarnessSandbox, Settings};
use alethiometer_lib::state::App;
use serde_json::{json, Value};
use std::path::Path;
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

fn no_node() -> bool {
    let missing = std::process::Command::new("node").arg("--version").output().is_err();
    if missing {
        eprintln!("skipped: no node");
    }
    missing
}

/// Settings with the fake agent as the only harness, run through the fake
/// CLI (a wrapper that gives it this test's sandbox root and log), and the
/// agent host running in-process.
fn setup(dir: &Path, agent_env: &[(&str, &str)]) -> std::thread::JoinHandle<std::io::Result<()>> {
    setup_slow(dir, agent_env, 0)
}

fn setup_slow(dir: &Path, agent_env: &[(&str, &str)], create_ms: u64) -> std::thread::JoinHandle<std::io::Result<()>> {
    let scripts = Path::new(env!("CARGO_MANIFEST_DIR")).join("../scripts");
    let cli = dir.join("openshell");
    std::fs::write(
        &cli,
        format!(
            "#!/bin/sh\nFAKE_OPENSHELL_CREATE_MS={create_ms} FAKE_OPENSHELL_ROOT='{}' FAKE_OPENSHELL_LOG='{}' exec node '{}' \"$@\"\n",
            dir.join("sandboxes").display(),
            dir.join("openshell.log").display(),
            scripts.join("fake-openshell.mjs").display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&cli, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();

    let mut s = Settings { demo_data: false, ..Default::default() };
    s.agents.openshell.command = cli.display().to_string();
    s.agents.harnesses = vec![Harness {
        key: "fake".into(),
        label: "Fake".into(),
        // On this machine it isn't even there: only the sandbox's command runs.
        command: "fake-acp-not-installed-here".into(),
        args: vec![],
        env: [("FAKE_ACP_MODE".to_string(), "stream".to_string())].into(),
        cache_ttl_min: None,
        openshell: Some(HarnessSandbox {
            from: "ghcr.io/example/fake-acp:1".into(),
            providers: vec!["anthropic".into()],
            command: "node".into(),
            args: vec![scripts.join("fake-acp.mjs").display().to_string()],
            env: agent_env.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
            ..Default::default()
        }),
    }];
    std::fs::write(dir.join("settings.json"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
    let host_dir = dir.to_owned();
    let host = std::thread::spawn(move || agentd::server::run(agentd::server::Options { dir: host_dir, idle: Duration::from_secs(600) }));
    wait("the agent host", 10, || agentd::client::Control::connect(&agentd::proto::socket_path(dir)).ok());
    host
}

fn open(dir: &Path) -> Arc<App> {
    std::env::set_var("ALETHIOMETER_AGENTD_KEEP", "1");
    let app = App::shared(App::open(dir).unwrap());
    alethiometer_lib::agents::start(app.clone());
    app
}

fn stop_host(dir: &Path, host: std::thread::JoinHandle<std::io::Result<()>>) {
    let c = agentd::client::Control::connect(&agentd::proto::socket_path(dir)).unwrap();
    c.ok(&agentd::proto::Request::Shutdown { force: true }).unwrap();
    host.join().unwrap().unwrap();
}

fn output(app: &App, id: &str) -> String {
    app.agents.store.lock().threads[&format!("s:{id}")].rows.iter().filter(|r| r.kind == Kind::Output).map(|r| r.text.clone()).collect()
}

fn calls(dir: &Path) -> Vec<Vec<String>> {
    std::fs::read_to_string(dir.join("openshell.log")).unwrap_or_default().lines().map(|l| serde_json::from_str(l).unwrap()).collect()
}

#[test]
fn a_sandboxed_agent_asks_nothing() {
    if no_node() {
        return;
    }
    let data = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    std::fs::write(project.path().join("README.md"), "hello").unwrap();
    let dir = data.path();
    let host = setup(dir, &[("FAKE_ACP_MODE", "gate")]);
    let app = open(dir);

    let id = alethiometer_lib::agents::call(&app, "start_local", &json!({ "harness": "fake", "cwd": project.path(), "task": "Clean up", "sandbox": { "gateway": "remote" } })).unwrap()["id"].as_str().unwrap().to_owned();
    wait("the turn to finish", 30, || app.agents.store.lock().sessions.get(&id).filter(|s| s.status == Status::Idle).map(|_| ()));
    assert_eq!(output(&app, &id), "gate: choice-0", "allowed with the agent's own allow option");
    assert!(app.agents.store.lock().requests.is_empty(), "nothing waited on you");

    let s = app.agents.store.lock().sessions[&id].clone();
    let sb = s.sandbox.clone().unwrap();
    assert_eq!((sb.gateway.as_str(), sb.allow_all), ("remote", true));
    let base = project.path().file_name().unwrap().to_string_lossy().into_owned();
    let workdir = dir.join("sandboxes/remote").join(&sb.name).join(&base);
    assert_eq!(s.cwd, workdir.display().to_string(), "it runs where the upload landed");
    assert!(workdir.join("README.md").is_file(), "the directory was uploaded");
    assert_eq!(s.context, format!("openshell · remote · {}", sb.name));
    let dirs = alethiometer_lib::agents::call(&app, "dirs", &json!({})).unwrap();
    assert_eq!(dirs["dirs"], json!([project.path().display().to_string()]), "the directory it came from, not the one in the sandbox");
    let tool = app.agents.store.lock().threads[&format!("s:{id}")].rows.iter().find(|r| r.kind == Kind::Tool).cloned().unwrap();
    assert!(tool.note.starts_with("allowed in the sandbox at"), "{:?}", tool.note);

    let calls = calls(dir);
    let create = calls.iter().find(|c| c.windows(2).any(|w| w == ["sandbox", "create"])).expect("a sandbox create");
    assert_eq!(&create[..5], ["-g", "remote", "sandbox", "create", "--name"]);
    for pair in [["--from", "ghcr.io/example/fake-acp:1"], ["--provider", "anthropic"], ["--upload", project.path().to_str().unwrap()]] {
        assert!(create.windows(2).any(|w| w == pair), "{pair:?} in {create:?}");
    }
    let exec = calls.iter().find(|c| c.contains(&"node".to_string())).expect("the adapter's exec");
    assert!(exec.windows(2).any(|w| w == ["--workdir", s.cwd.as_str()]), "{exec:?}");
    assert!(exec.windows(2).any(|w| w == ["--env", "FAKE_ACP_MODE=gate"]), "the sandbox env, not the harness's own: {exec:?}");

    stop_host(dir, host);
}

#[test]
fn claude_runs_in_auto_mode_and_still_asks_you() {
    if no_node() {
        return;
    }
    let data = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    let dir = data.path();
    let host = setup(dir, &[("FAKE_ACP_MODE", "gate"), ("FAKE_ACP_SAY_MODE", "1"), ("FAKE_ACP_NAME", "@agentclientprotocol/claude-agent-acp")]);
    let app = open(dir);

    let id = alethiometer_lib::agents::call(&app, "start_local", &json!({ "harness": "fake", "cwd": project.path(), "task": "Clean up", "sandbox": {} })).unwrap()["id"].as_str().unwrap().to_owned();
    let req = wait("the gate's approval", 30, || app.agents.store.lock().requests.values().find(|r| r.session_id == id).map(|r| r.id.clone()));
    assert!(!app.agents.store.lock().sessions[&id].sandbox.as_ref().unwrap().allow_all);
    alethiometer_lib::agents::call(&app, "respond", &json!({ "request": req, "option": "1" })).unwrap();
    wait("the turn to finish", 30, || app.agents.store.lock().sessions.get(&id).filter(|s| s.status == Status::Idle).map(|_| ()));
    assert_eq!(output(&app, &id), "gate: choice-0 · auto");
    let create = calls(dir).into_iter().find(|c| c.get(1).map(String::as_str) == Some("create")).expect("a sandbox create");
    assert_eq!(create[0], "sandbox", "the active gateway takes no -g");

    stop_host(dir, host);
}

#[test]
fn the_cli_lists_gateways_and_sandboxes() {
    if no_node() {
        return;
    }
    let data = tempfile::tempdir().unwrap();
    let dir = data.path();
    let host = setup(dir, &[]);
    let app = open(dir);
    let g = alethiometer_lib::agents::call(&app, "openshell_gateways", &json!({})).unwrap();
    let names: Vec<(&str, bool, bool)> = g.as_array().unwrap().iter().map(|g| (g["name"].as_str().unwrap(), g["remote"].as_bool().unwrap(), g["active"].as_bool().unwrap())).collect();
    assert_eq!(names, [("local", false, true), ("remote", true, false)]);
    std::fs::create_dir_all(dir.join("sandboxes/remote/old-one")).unwrap();
    let s = alethiometer_lib::agents::call(&app, "openshell_sandboxes", &json!({ "gateway": "remote" })).unwrap();
    assert_eq!(s, json!([{ "name": "old-one", "phase": "Ready" }]));
    let relative = alethiometer_lib::agents::call(&app, "start_local", &json!({ "harness": "fake", "cwd": "project", "sandbox": { "gateway": "remote", "existing": "old-one", "workdir": "project" } }));
    assert!(relative.unwrap_err().to_string().contains("inside the sandbox"), "ACP needs an absolute directory");
    let tree = alethiometer_lib::agents::call(&app, "tree", &json!({})).unwrap();
    assert_eq!(tree["openshell"], Value::Bool(true));
    stop_host(dir, host);
}

#[test]
fn a_reattach_while_the_sandbox_is_made_leaves_the_session_be() {
    if no_node() {
        return;
    }
    let data = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    let dir = data.path();
    let host = setup_slow(dir, &[("FAKE_ACP_MODE", "gate")], 1500);
    let app = open(dir);
    let id = alethiometer_lib::agents::call(&app, "start_local", &json!({ "harness": "fake", "cwd": project.path(), "task": "Clean up", "sandbox": {} })).unwrap()["id"].as_str().unwrap().to_owned();
    let thread = format!("s:{id}");
    wait("the sandbox to be in the making", 10, || app.agents.store.lock().threads.get(&thread).filter(|t| t.rows.iter().any(|r| r.text.starts_with("Creating sandbox"))).map(|_| ()));
    // What the agent host's (re)connection does: nothing of this session is in the host yet.
    alethiometer_lib::agents::acp::restore(&app, &[]);
    assert_eq!(app.agents.store.lock().sessions[&id].status, Status::Running);
    wait("the turn to finish", 30, || app.agents.store.lock().sessions.get(&id).filter(|s| s.status == Status::Idle).map(|_| ()));
    assert_eq!(output(&app, &id), "gate: choice-0");
    stop_host(dir, host);
}

#[test]
fn an_existing_sandbox_is_used_as_it_is() {
    if no_node() {
        return;
    }
    let data = tempfile::tempdir().unwrap();
    let dir = data.path();
    let host = setup(dir, &[("FAKE_ACP_MODE", "gate")]);
    let work = dir.join("sandboxes/remote/old-one/work");
    std::fs::create_dir_all(&work).unwrap();
    let app = open(dir);
    let id = alethiometer_lib::agents::call(&app, "start_local", &json!({ "harness": "fake", "cwd": work, "task": "Clean up", "sandbox": { "gateway": "remote", "existing": "old-one", "workdir": work } })).unwrap()["id"].as_str().unwrap().to_owned();
    wait("the turn to finish", 30, || app.agents.store.lock().sessions.get(&id).filter(|s| s.status == Status::Idle).map(|_| ()));
    assert_eq!(output(&app, &id), "gate: choice-0");
    let s = app.agents.store.lock().sessions[&id].clone();
    assert_eq!((s.cwd.as_str(), s.sandbox.unwrap().upload.as_str()), (work.to_str().unwrap(), ""), "nothing uploaded");
    let calls = calls(dir);
    assert!(!calls.iter().any(|c| c.contains(&"create".to_string())), "no sandbox made: {calls:?}");
    assert!(calls.iter().any(|c| c.windows(2).any(|w| w == ["start", "old-one"])), "started in case it was stopped: {calls:?}");
    stop_host(dir, host);
}

fn start(app: &App, project: &Path, task: &str) -> String {
    alethiometer_lib::agents::call(app, "start_local", &json!({ "harness": "fake", "cwd": project, "task": task, "sandbox": {} })).unwrap()["id"].as_str().unwrap().to_owned()
}

fn idle(app: &App, id: &str) {
    wait("the turn to finish", 30, || app.agents.store.lock().sessions.get(id).filter(|s| s.status == Status::Idle).map(|_| ()));
}

#[test]
fn a_pi_permission_gate_is_allowed_but_its_question_comes_to_you() {
    if no_node() {
        return;
    }
    let data = tempfile::tempdir().unwrap();
    let dir = data.path();
    let host = setup(dir, &[("FAKE_ACP_MODE", "piui")]);
    let app = open(dir);
    let project = tempfile::tempdir().unwrap();
    let gate = alethiometer_lib::agents::call(&app, "start_local", &json!({ "harness": "fake", "cwd": project.path(), "task": "Clean", "sandbox": {} })).unwrap()["id"].as_str().unwrap().to_owned();
    idle(&app, &gate);
    assert_eq!(output(&app, &gate), "piui: choice-0", "the extension's gate on a pending call, allowed");
    stop_host(dir, host);

    let data = tempfile::tempdir().unwrap();
    let dir = data.path();
    let host = setup(dir, &[("FAKE_ACP_MODE", "piquestion")]);
    let app = open(dir);
    let q = alethiometer_lib::agents::call(&app, "start_local", &json!({ "harness": "fake", "cwd": project.path(), "task": "Clean", "sandbox": {} })).unwrap()["id"].as_str().unwrap().to_owned();
    let req = wait("the question to come to you", 30, || app.agents.store.lock().requests.values().find(|r| r.session_id == q).cloned());
    assert_eq!(req.title, "Also update the docs?");
    alethiometer_lib::agents::call(&app, "respond", &json!({ "request": req.id, "option": "2" })).unwrap();
    idle(&app, &q);
    assert_eq!(output(&app, &q), "piquestion: choice-1", "your answer, not an automatic yes");
    stop_host(dir, host);
}

#[test]
fn an_agents_questions_in_a_sandbox_come_to_you() {
    if no_node() {
        return;
    }
    let data = tempfile::tempdir().unwrap();
    let dir = data.path();
    let host = setup(dir, &[("FAKE_ACP_MODE", "ask")]);
    let app = open(dir);
    let project = tempfile::tempdir().unwrap();
    let id = alethiometer_lib::agents::call(&app, "start_local", &json!({ "harness": "fake", "cwd": project.path(), "task": "Add a cache", "sandbox": {} })).unwrap()["id"].as_str().unwrap().to_owned();
    let ask = |app: &App| app.agents.store.lock().requests.values().find(|r| r.session_id == id && r.kind == "ask").cloned();
    let q = wait("the first question", 30, || ask(&app));
    alethiometer_lib::agents::call(&app, "answer", &json!({ "request": q.id, "choice": "2" })).unwrap();
    let next = wait("the text question", 20, || ask(&app).filter(|r| r.id != q.id));
    alethiometer_lib::agents::call(&app, "answer", &json!({ "request": next.id, "text": "cache_entries" })).unwrap();
    idle(&app, &id);
    assert_eq!(output(&app, &id), r#"Got {"question_0":"SQLite"} then {"value":"cache_entries"}"#);
    stop_host(dir, host);
}

#[test]
fn the_sandboxs_drafted_network_rules_are_approvals() {
    if no_node() {
        return;
    }
    let data = tempfile::tempdir().unwrap();
    let dir = data.path();
    let host = setup(dir, &[("FAKE_ACP_MODE", "gate")]);
    let app = open(dir);
    let project = tempfile::tempdir().unwrap();
    let id = start(&app, project.path(), "Fetch deps");
    idle(&app, &id);
    let sb = app.agents.store.lock().sessions[&id].sandbox.clone().unwrap();
    let rules_file = dir.join("sandboxes/.rules").join(format!("local--{}.json", sb.name));
    std::fs::create_dir_all(rules_file.parent().unwrap()).unwrap();
    let rules = json!([
        { "chunk": "c1", "status": "pending", "rule": "allow-github", "rationale": "git fetch was denied", "endpoints": "github.com:443 [L4]" },
        { "chunk": "c2", "status": "pending", "rule": "allow-pypi", "rationale": "pip install", "endpoints": "pypi.org:443 [L7 rest]", "security": "new host with credentials" },
        { "chunk": "c3", "status": "pending", "rule": "allow-npm", "rationale": "npm install", "endpoints": "registry.npmjs.org:443 [L4]" }
    ]);
    std::fs::write(&rules_file, rules.to_string()).unwrap();
    // What the poller does every 10 s, once.
    let cli = app.settings().agents.openshell.command.clone();
    let poll = || alethiometer_lib::agents::openshell::reconcile_rules(&app, &id, &alethiometer_lib::agents::openshell::pending_rules(&cli, &sb).unwrap());
    poll();
    poll();
    let reqs: Vec<String> = { let mut v: Vec<String> = app.agents.store.lock().requests.keys().cloned().collect(); v.sort(); v };
    assert_eq!(reqs, [format!("osr:{id}:c1"), format!("osr:{id}:c2"), format!("osr:{id}:c3")]);
    let rows = |app: &App| app.agents.store.lock().threads[&format!("s:{id}")].rows.iter().filter(|r| r.ext.starts_with("osr:")).cloned().collect::<Vec<_>>();
    assert_eq!(rows(&app).len(), 3, "one row each, not one per poll");
    assert_eq!(app.agents.store.lock().requests[&format!("osr:{id}:c2")].tag, "network · flagged");

    alethiometer_lib::agents::call(&app, "respond", &json!({ "request": format!("osr:{id}:c1"), "option": "1" })).unwrap();
    alethiometer_lib::agents::call(&app, "respond", &json!({ "request": format!("osr:{id}:c2"), "option": "2", "reason": "not needed" })).unwrap();
    let after: Value = serde_json::from_str(&std::fs::read_to_string(&rules_file).unwrap()).unwrap();
    assert_eq!((after[0]["status"].as_str(), after[1]["status"].as_str(), after[1]["reason"].as_str()), (Some("approved"), Some("rejected"), Some("not needed")));
    // The app restarts (requests aren't stored, the rows are), and c3 is approved from a terminal meanwhile.
    app.agents.store.lock().remove_request(&format!("osr:{id}:c3"));
    let mut edited = after.clone();
    edited[2]["status"] = json!("approved");
    std::fs::write(&rules_file, edited.to_string()).unwrap();
    poll();
    assert!(app.agents.store.lock().requests.is_empty(), "answered here or elsewhere, none left");
    let notes: Vec<String> = rows(&app).iter().map(|r| r.note.clone()).collect();
    assert!(notes[0].starts_with("approved at"), "{notes:?}");
    assert!(notes[1].starts_with("rejected at"), "{notes:?}");
    assert_eq!(notes[2], "decided outside Alethiometer");
    stop_host(dir, host);
}

#[test]
fn changes_come_back_then_archiving_deletes_the_sandbox() {
    if no_node() {
        return;
    }
    let data = tempfile::tempdir().unwrap();
    let dir = data.path();
    let host = setup(dir, &[("FAKE_ACP_MODE", "write")]);
    let app = open(dir);
    let project = tempfile::tempdir().unwrap();
    let p = project.path();
    for (f, t) in [("README.md", "# demo\n"), ("OLD.txt", "old\n"), ("KEEP.txt", "mine\n")] {
        std::fs::write(p.join(f), t).unwrap();
    }
    let id = alethiometer_lib::agents::call(&app, "start_local", &json!({ "harness": "fake", "cwd": p, "task": "Write notes", "sandbox": {} })).unwrap()["id"].as_str().unwrap().to_owned();
    idle(&app, &id);
    std::fs::write(p.join("KEEP.txt"), "mine, edited here\n").unwrap();

    let plan = alethiometer_lib::agents::call(&app, "archive_plan", &json!({ "target": format!("s:{id}") })).unwrap();
    assert_eq!(plan["state"], "not_done", "its work is only in the sandbox: {plan}");
    assert!(plan["reasons"][0]["text"].as_str().unwrap().contains("haven't been brought back"));
    assert_eq!(plan["sandboxes"], 1);

    let look = alethiometer_lib::agents::call(&app, "sandbox_changes", &json!({ "session": id })).unwrap();
    let list = |v: &Value| v.as_array().unwrap().iter().map(|c| format!("{} {}", c["what"].as_str().unwrap(), c["path"].as_str().unwrap())).collect::<Vec<_>>();
    assert_eq!(list(&look["changes"]), ["added NOTES.md", "deleted OLD.txt", "modified README.md"]);
    assert_eq!(list(&look["conflicts"]), ["modified KEEP.txt"]);
    assert!(p.join("OLD.txt").exists(), "a look touches nothing");
    let done = alethiometer_lib::agents::call(&app, "sandbox_copy_back", &json!({ "session": id })).unwrap();
    assert_eq!(done["applied"].as_array().unwrap().len(), 3);
    assert_eq!(std::fs::read_to_string(p.join("NOTES.md")).unwrap(), "notes from the agent\n");
    assert_eq!(std::fs::read_to_string(p.join("README.md")).unwrap(), "# demo, by the agent\n");
    assert!(!p.join("OLD.txt").exists());
    assert_eq!(std::fs::read_to_string(p.join("KEEP.txt")).unwrap(), "mine, edited here\n", "a conflict stays as it is here");
    let note = app.agents.store.lock().threads[&format!("s:{id}")].rows.iter().rev().find(|r| r.kind == Kind::State).unwrap().text.clone();
    assert!(note.starts_with("Brought back 3 files") && note.contains("KEEP.txt"), "{note}");
    assert_eq!(alethiometer_lib::agents::call(&app, "archive_plan", &json!({ "target": format!("s:{id}") })).unwrap()["state"], "not_done", "the conflict is still only in the sandbox");

    // The sandbox wins this time.
    let again = alethiometer_lib::agents::call(&app, "sandbox_changes", &json!({ "session": id })).unwrap();
    assert!(again["changes"].as_array().unwrap().is_empty() && again["ready"] == false, "nothing new: {again}");
    assert!(alethiometer_lib::agents::call(&app, "sandbox_copy_back", &json!({ "session": id, "force": true })).is_err(), "nothing to copy without a preview");
    std::fs::write(p.join("NOTES.md"), "notes from the agent\n").unwrap();
    // Make the conflict copyable: a fresh change in the sandbox too, then force it.
    let sb = app.agents.store.lock().sessions[&id].sandbox.clone().unwrap();
    let inside = app.agents.store.lock().sessions[&id].cwd.clone();
    std::fs::write(Path::new(&inside).join("EXTRA.md"), "more\n").unwrap();
    alethiometer_lib::agents::call(&app, "sandbox_changes", &json!({ "session": id })).unwrap();
    alethiometer_lib::agents::call(&app, "sandbox_copy_back", &json!({ "session": id, "force": true })).unwrap();
    assert_eq!(std::fs::read_to_string(p.join("KEEP.txt")).unwrap(), "the agent's version\n");
    assert!(p.join("EXTRA.md").exists());

    let plan = alethiometer_lib::agents::call(&app, "archive_plan", &json!({ "target": format!("s:{id}") })).unwrap();
    assert_eq!(plan["state"], "ok", "{plan}");
    alethiometer_lib::agents::call(&app, "archive", &json!({ "target": format!("s:{id}") })).unwrap();
    let http = alethiometer_lib::sync::http::FakeHttp::default();
    while alethiometer_lib::outbox::process_one(&app, &http).unwrap() {}
    let gone = dir.join("sandboxes/local").join(&sb.name);
    assert!(!gone.exists(), "the sandbox was deleted");
    assert!(calls(dir).iter().any(|c| c.windows(3).any(|w| w == ["sandbox", "delete", sb.name.as_str()])));
    stop_host(dir, host);
}

#[test]
fn a_sandbox_that_was_already_there_is_never_deleted() {
    if no_node() {
        return;
    }
    let data = tempfile::tempdir().unwrap();
    let dir = data.path();
    let host = setup(dir, &[("FAKE_ACP_MODE", "gate")]);
    let work = dir.join("sandboxes/local/theirs/work");
    std::fs::create_dir_all(&work).unwrap();
    let app = open(dir);
    let id = alethiometer_lib::agents::call(&app, "start_local", &json!({ "harness": "fake", "cwd": work, "task": "x", "sandbox": { "existing": "theirs", "workdir": work } })).unwrap()["id"].as_str().unwrap().to_owned();
    idle(&app, &id);
    let plan = alethiometer_lib::agents::call(&app, "archive_plan", &json!({ "target": format!("s:{id}") })).unwrap();
    assert_eq!((plan["state"].as_str(), plan["sandboxes"].as_u64()), (Some("ok"), Some(0)));
    assert!(alethiometer_lib::agents::call(&app, "sandbox_changes", &json!({ "session": id })).unwrap_err().to_string().contains("already there"));
    stop_host(dir, host);
}
